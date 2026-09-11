#!/usr/bin/env python3
"""SharpeBench LLM stdio agent.

Speaks the ExternalAgent protocol: one MarketObservation JSON per line on
stdin, one Decision JSON per line on stdout. Each decision point the agent
summarizes the observation (last 20 bars of returns per symbol, rounded, plus
coarse portfolio weights) and asks Claude for target weights as strict JSON.

Model selection: first CLI argument, else LLM_MODEL env, else the Haiku
default. Per-model request shape:
  - claude-haiku-4-5*: temperature 0, max_tokens 300 (no thinking).
  - claude-fable-5 / claude-opus-5: sampling parameters are rejected by the
    API (adaptive thinking is on), so temperature is omitted; max_tokens 4000
    with effort "low" so thinking has room without runaway spend. No fallback
    models are configured: the benchmark pins policy identity to the named
    model, so a refusal is recorded and scored as a hold, never silently
    answered by a different model.

Model identity: the requested id is the policy identity, and substitution is
refused rather than absorbed. An unknown-model error is a failure of this
run, not a cue to retry a different model, and the id the API reports back is
checked against the request: the same id, or the requested alias followed by a
hyphen and a dated snapshot of exactly eight digits (`claude-haiku-4-5` served
as `claude-haiku-4-5-20251001`), is the requested policy; anything else is a
different policy answering under the requested name and fails the subprocess.
A reply that names no model at all fails too, because `Message.model` is a
required field of this API and an identity that was never stated cannot be
published as the requested one. Both identities are recorded on every cached
decision, so a replay states which model actually answered, and the served one
is screened on the way back in by the same rule that admitted it: a cached
record naming a model this scaffold would refuse today is dropped rather than
replayed.

Pricing: the rate card is matched by that same identity rule, exactly or as a
dated snapshot of a priced alias, and a model the table does not name refuses
the run before the first observation is read. A prefix walk returning
`(0.0, 0.0)` for an unknown model reported every call of such a run as free,
and a plausible wrong number is worse than an absence for a benchmark that
publishes what an agent spent. It also let a model whose name extends a priced
one be billed at the other model's card.

Determinism and cost controls:
  - temperature 0 where the API accepts it; the summarization is a pure
    function of the observation. On the frontier tier the API offers no
    sampling control, so bitwise determinism is not guaranteed; the response
    cache is what makes the recorded run replayable.
  - Decisions are cached to a per-model JSONL keyed by SHA-256 of the whole
    effective request (model, system prompt, message, temperature, max_tokens,
    thinking settings) plus SCAFFOLD_VERSION, which names the summarizer,
    system prompt and parser that produced and will interpret it. Keying on
    (model, prompt) alone let a changed system prompt, token budget or parser
    reuse decisions taken under a different policy configuration, so a rerun
    could report the current scaffold while replaying an older one. Reruns and
    cross-seed repeats under an unchanged configuration remain free.
  - A decision stride (default 5): the model is consulted every Nth bar and
    the book is left untouched in between (empty orders = hold).
  - A hard per-model cap on fresh API calls (default 800), counted as provider
    requests rather than as cached results. Every fresh call reserves one unit
    of the allowance in a per-model ledger file before the request is sent, so
    a call that fails, times out or returns something uncacheable still spends
    its unit, and the harness retrying the subprocess cannot re-spend it. The
    ledger lives beside the response cache and survives the process the same
    way, which is what makes the cap hold across the many subprocesses the
    harness spawns. The count is re-read from the ledger inside an exclusive
    lock at each reservation rather than advanced from a value read at startup,
    so a second shim sharing the ledger cannot spend a unit another already
    took, whatever order the harness starts them in. The client is
    built with the SDK's own automatic retries off, and the effective setting
    is read back off the constructed client before the run starts, so one
    reservation is exactly one HTTP request to the provider and the ceiling
    counts what the provider is actually asked to do. That guarantee is
    conditional on the check, not on any SDK version: if the client reports a
    non-zero retry setting, or none that can be read, the run refuses to start
    rather than proceed on the assumption.
  - No in-process retry of a transient failure. A rate limit, a timeout or a
    connection fault raises on the first attempt, having spent its unit, and
    fails the subprocess. The harness respawns it (EXTERNAL_MAX_RETRIES) and
    the next attempt takes a fresh unit from the same ledger, so a retried
    window is bounded by the same allowance as any other call. Retrying inside
    this process, or leaving the SDK to do it, would issue several billable
    requests under one reservation, which is the thing the ledger exists to
    prevent.
  - Malformed model output is emitted as an invalid wire decision so the Rust
    transport classifies the affected run as an agent-protocol failure. It is
    never flattened into a hold. Explicit refusals remain deliberate holds.
    Infrastructure failures (missing credit, authentication, rate limits,
    network faults, or an exhausted call budget) fail the subprocess.

Environment:
  ANTHROPIC_API_KEY   required (the SDK reads it)
  LLM_MODEL           model id (overridden by argv[1])
  LLM_CACHE_DIR       directory for llm-cache-<model>.jsonl response caches
  LLM_STATS_DIR       directory for per-process stats files (summed afterwards)
  LLM_STRIDE          decision stride in bars (default 5)
  LLM_MAX_CALLS       fresh-API-call cap per model (default 800), counted as
                      provider requests reserved, not as results cached
"""

import contextlib
import hashlib
import json
import math
import os
import sys
import time
from pathlib import Path

import anthropic

_START_NS = time.time_ns()

MODEL = sys.argv[1] if len(sys.argv) > 1 else os.environ.get(
    "LLM_MODEL", "claude-haiku-4-5-20251001"
)
# The requested id is the policy identity and is never rebound: a run that
# cannot use it has failed, and a different model answering under it would be
# a different policy published under this name.
REQUESTED_MODEL = MODEL
# Names the parts of the scaffold that are not in the request itself: the
# observation summarizer, the reply parser, and the order validation between
# them. Bump it whenever any of those change meaning, so cached decisions taken
# under the old ones are not replayed as if this one produced them.
SCAFFOLD_VERSION = "summarize-v1/parse-v1"
STRIDE = int(os.environ.get("LLM_STRIDE", "5"))
MAX_CALLS = int(os.environ.get("LLM_MAX_CALLS", "800"))
# The SDK retries some failures itself (anthropic.DEFAULT_MAX_RETRIES is 2).
# Those extra requests are billable and invisible to the ledger, which would
# make the allowance bound dispatches from this process rather than provider
# requests. Zero makes one reservation exactly one request.
PROVIDER_MAX_RETRIES = 0
# The SDK the retry reading was taken from, and the one the regression asserts
# against. Recorded so a reader can tell which version the ceiling's
# one-reservation-is-one-request property was established on. It is not a
# runtime requirement: `assert_no_provider_retries` checks the knob on the
# constructed client, so a compatible upgrade passes on behaviour rather than
# on a version string.
EVIDENCED_SDK_VERSION = "0.112.0"
HERE = Path(__file__).resolve().parent
CACHE_DIR = Path(os.environ.get("LLM_CACHE_DIR", HERE))
CACHE_PATH = CACHE_DIR / f"llm-cache-{MODEL}.jsonl"
# The spend ledger: one appended line per dispatch this scaffold sends, written
# before the request. Separate from the cache because a call that fails leaves
# no cache record and must still count against the allowance.
ATTEMPTS_PATH = CACHE_DIR / f"llm-attempts-{MODEL}.jsonl"
# Exclusive ownership of the ledger for the length of one reservation, so the
# read of the count and the append that spends against it are one step.
LEDGER_LOCK_PATH = CACHE_DIR / f"llm-attempts-{MODEL}.jsonl.lock"
STATS_DIR = Path(os.environ.get("LLM_STATS_DIR", HERE / "stats"))
# The separator and the width of a dated snapshot the provider expands a
# requested alias into. Every alias/pinned pair the SDK's own `Message.model`
# literal enumerates has this shape: claude-haiku-4-5-20251001,
# claude-opus-4-5-20251101, claude-sonnet-4-5-20250929, claude-opus-4-1-20250805.
SNAPSHOT_SEPARATOR = "-"
SNAPSHOT_DIGITS = 8

# First-party API pricing, USD per token (input, output).
PRICING = {
    "claude-fable-5": (10.00e-6, 50.00e-6),
    "claude-opus-5": (5.00e-6, 25.00e-6),
    "claude-haiku-4-5": (1.00e-6, 5.00e-6),
}


class UnpricedModel(RuntimeError):
    """No rate card names this model, so what its calls cost is not known.

    Raised rather than answered with a zero. The benchmark publishes what an
    agent spent, and a zero for a model the table does not price is a plausible
    wrong number where the project records an unavailability: the Rust side
    answers an unknowable cost with `MonetarySummary::Unavailable` and a reason,
    never with an amount (`crates/sharpebench-harness/src/accounting.rs`). This
    shim has no such channel. Its statistics file carries a single `cost_usd`
    that `paper/evidence/assemble_llm_field.py` sums, so the only way it can
    decline to state a number is to not produce the run. The model is the
    operator's choice and the table is in this file, so a missing rate card is
    an operator error, knowable before any money moves.
    """


def price_for(model):
    """The rate card for `model`, by the rule that decides model identity.

    Matched exactly, or as a dated snapshot of a priced alias, which is the one
    expansion the provider makes and the same rule `is_dated_snapshot_of`
    states. A prefix walk took any continuation, so a model whose name extends a
    priced one was billed at the other model's card: `claude-opus-5-1` would
    have been priced as `claude-opus-5`, and a table gaining a `claude-haiku-4`
    would price every `claude-haiku-4-5` at whichever key the walk reached
    first. That is the model-identity defect in the accounting, and it is
    repaired the same way.
    """
    for alias, p in PRICING.items():
        if model == alias or is_dated_snapshot_of(alias, model):
            return p
    raise UnpricedModel(
        f"no rate card for {model}: PRICING names {sorted(PRICING)}, and a "
        "model absent from it has no cost this run can state. Reporting zero "
        "would publish a call that was billed as free. Add the model's rate "
        "card or run a model the table prices"
    )


def assert_model_is_priced():
    """Refuse the run unless the requested policy has a rate card.

    Checked before the first observation is read, so a missing rate card costs
    nothing rather than being discovered after a field's worth of calls has been
    billed and cannot be priced. The served id is bound to the requested one by
    `effective_model`, which admits only the requested id or a dated snapshot of
    it, and `price_for` matches on exactly that rule, so a requested model this
    prices is a served model it prices too. `price_for` still refuses on its own
    path rather than trusting that argument.
    """
    price_for(REQUESTED_MODEL)


SYSTEM = (
    "You are a portfolio allocation agent in a daily-bar trading benchmark. "
    "Costs, slippage, and market impact are charged on every trade, so trade "
    "only on conviction. You are long-only and unleveraged: each target_weight "
    "is in [0, 1] and the sum of target weights must be at most 1. Reply with "
    "STRICT JSON only, no prose, no code fences, exactly this shape: "
    '{"orders":[{"symbol":"X","action":"buy|sell|hold","target_weight":0.0}]}. '
    "Omit a symbol to leave its position untouched."
)

STATS = {
    "observations": 0,
    "llm_calls": 0,
    "cache_hits": 0,
    "stride_holds": 0,
    "malformed": 0,
    "refusals": 0,
    "budget_exhausted": 0,
    # Provider requests reserved against the cap, this process and every
    # earlier one sharing the ledger. Equal to llm_calls within one process,
    # which counts the same dispatches; larger by whatever earlier processes
    # sharing this model's ledger already spent.
    "calls_reserved": 0,
    "api_errors": 0,
    "tokens_in": 0,
    "tokens_out": 0,
    "cost_usd": 0.0,
    "model": MODEL,
    "model_requested": REQUESTED_MODEL,
    # Filled in from the first response the API returns. None until then, so an
    # unanswered run never claims an effective identity it did not observe.
    "model_effective": None,
    "scaffold_version": SCAFFOLD_VERSION,
    "cache_records_ignored": 0,
    "stride": STRIDE,
}


def request_kwargs(model, prompt):
    kw = {
        "model": model,
        "system": SYSTEM,
        "messages": [{"role": "user", "content": prompt}],
    }
    if model.startswith("claude-haiku"):
        kw["temperature"] = 0
        kw["max_tokens"] = 300
    else:
        # Frontier tier: sampling params rejected, adaptive thinking on.
        kw["max_tokens"] = 4000
        kw["extra_body"] = {"output_config": {"effort": "low"}}
    return kw


def request_identity(model, prompt):
    """The full configuration a cached decision was taken under.

    Everything the provider is sent, plus the scaffold that built the prompt
    and will interpret the reply. A cached decision is only reusable for a
    request identical in all of it.
    """
    return {
        "scaffold_version": SCAFFOLD_VERSION,
        "requested_model": REQUESTED_MODEL,
        "request": request_kwargs(model, prompt),
    }


def cache_key(model, prompt):
    canonical = json.dumps(
        request_identity(model, prompt), sort_keys=True, separators=(",", ":")
    )
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def load_cache():
    """Cached decisions whose stored identity matches this configuration.

    A record that names a different scaffold version, or whose stored request
    digest is not its own key, was taken under a configuration this process is
    not running. It is dropped rather than replayed, and counted so the drop is
    reported instead of silent.

    The served identity is screened here by `is_requested_policy`, the same
    function `effective_model` accepts a fresh reply with, so a record naming a
    model this scaffold would refuse today is not resurrected by replaying it.
    This scaffold cannot write such a record, which bounds the case to a cache
    file from somewhere else, and the request digest still has to match a
    request for the requested model; but a replay is a published decision, and
    it is screened by the rule that governs a fresh one rather than by a shorter
    one.
    """
    cache = {}
    if CACHE_PATH.exists():
        with CACHE_PATH.open("r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    rec = json.loads(line)
                    key = rec["key"]
                except (json.JSONDecodeError, KeyError):
                    STATS["cache_records_ignored"] += 1
                    continue
                if (
                    rec.get("scaffold_version") != SCAFFOLD_VERSION
                    or rec.get("request_sha256") != key
                    or rec.get("model_requested") != REQUESTED_MODEL
                    or not is_requested_policy(rec.get("model_effective"))
                ):
                    STATS["cache_records_ignored"] += 1
                    continue
                cache[key] = rec
    return cache


def record_decision(cache, key, effective, fields):
    """Stamp a decision with the identity it was taken under, store and return it.

    The request digest, the scaffold version and both model identities travel
    with every cached decision, so a replay can state which configuration and
    which model actually produced it instead of inferring it from the file name.
    """
    rec = dict(fields)
    rec["key"] = key
    rec["request_sha256"] = key
    rec["scaffold_version"] = SCAFFOLD_VERSION
    rec["model_requested"] = REQUESTED_MODEL
    rec["model_effective"] = effective
    CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
    with CACHE_PATH.open("a", encoding="utf-8") as f:
        f.write(json.dumps(rec, sort_keys=True) + "\n")
    cache[key] = rec
    return rec


def load_attempt_count():
    """Dispatches already reserved against this model's allowance.

    One line per reservation, so the count is the line count. Read from disk at
    every reservation, and again at startup for the statistics, because the
    harness runs each window in its own subprocess and the allowance is per
    model, not per process.
    """
    if not ATTEMPTS_PATH.exists():
        return 0
    with ATTEMPTS_PATH.open("r", encoding="utf-8") as f:
        return sum(1 for line in f if line.strip())


class BudgetExhausted(RuntimeError):
    """The allowance is spent. Raised where that can be established, which is
    inside the reservation: only a count read under the lock is current."""


class LedgerBusy(RuntimeError):
    """The ledger is owned by someone else, so this process cannot reserve.

    Either another shim is reserving right now, or one died holding the lock.
    Neither is broken automatically: a lock nobody can prove is dead puts two
    writers back on one allowance, which is what it exists to prevent.
    """


def describe_ledger_holder():
    """An operator-facing description of whoever holds the lock. Never raises:
    an unreadable or foreign document still has to produce a refusal message."""
    try:
        document = json.loads(LEDGER_LOCK_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return "a holder whose lock document could not be read"
    if not isinstance(document, dict) or "pid" not in document:
        return "a holder whose lock document is not a ledger lock"
    return f"pid {document['pid']} started at ns {document.get('started_ns')}"


class LedgerLockStranded(RuntimeError):
    """The lock was taken and could not be released, so it is still on disk."""


# Releasing races with a refused shim describing the holder: that read has the
# lock file open for a moment, and Windows refuses to delete a file another
# handle holds (WinError 32), which eight contending threads reproduced here.
# The holder retries briefly rather than stranding a lock that would refuse
# every later reservation, and says so if it cannot, because a lock left behind
# is an operator action rather than something to discover a run later.
LOCK_RELEASE_ATTEMPTS = 100
LOCK_RELEASE_PAUSE_S = 0.01


def release_ledger_lock():
    for remaining in range(LOCK_RELEASE_ATTEMPTS - 1, -1, -1):
        try:
            LEDGER_LOCK_PATH.unlink()
            return
        except FileNotFoundError:
            return
        except PermissionError:
            if remaining == 0:
                raise LedgerLockStranded(
                    f"the call ledger lock {LEDGER_LOCK_PATH} was taken by this "
                    "process and could not be released; every later reservation "
                    "will be refused until an operator removes that file"
                ) from None
            time.sleep(LOCK_RELEASE_PAUSE_S)


@contextlib.contextmanager
def ledger_lock():
    """Exclusive ownership of the ledger while one unit is reserved.

    A sibling file named after the ledger with `.lock` appended, created with
    `O_CREAT | O_EXCL` so the file system picks one winner and a second holder
    is refused rather than queued. The same shape the money journal uses
    (`crates/sharpebench-harness/src/gateway_journal.rs`), for the same reason:
    read-then-append is a check-then-act race, and two shims that each read the
    same count each believe the same unit is theirs.

    Held only across the read and the append, not for the run, so a killed shim
    can strand it for one reservation rather than for a whole field. A stranded
    lock is still refused rather than removed. Two hosts sharing one directory
    over a network file system are not separated by this: `O_EXCL` is only as
    exclusive as the remote server makes it.
    """
    LEDGER_LOCK_PATH.parent.mkdir(parents=True, exist_ok=True)
    try:
        fd = os.open(LEDGER_LOCK_PATH, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except (FileExistsError, PermissionError):
        # `PermissionError` is the same answer on Windows, where a lock whose
        # holder is deleting it is refused as access denied rather than as
        # already existing. Either way this process does not own the ledger.
        raise LedgerBusy(
            f"the call ledger for {REQUESTED_MODEL} is held by "
            f"{describe_ledger_holder()}; its lock file is {LEDGER_LOCK_PATH}. "
            "Another shim is reserving a call, or one died holding the lock. A "
            "lock is never broken automatically, because two writers on one "
            "allowance is the defect it prevents; if no such process is "
            "running, an operator must remove that file."
        ) from None
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(
                json.dumps(
                    {"pid": os.getpid(), "started_ns": _START_NS}, sort_keys=True
                )
            )
        yield
    finally:
        release_ledger_lock()


def reserve_call(key):
    """Spend one unit of the allowance, durably, before the request is sent.

    Written and flushed ahead of the call so that a provider failure, a timeout
    or a killed process still consumes the unit: the alternative counts only
    calls that came back, which lets a retried subprocess dispatch again under
    the same allowance. The reservation names the request it was taken for, so
    the ledger can be read against the cache afterwards.

    One reservation is one provider request. The client disables the SDK's own
    automatic retries (`PROVIDER_MAX_RETRIES`), so nothing issues a second
    billable request under a unit already spent; a transient failure is raised
    to the harness, whose respawn takes a fresh unit from this ledger.

    The ceiling is enforced here, on a count re-read from the ledger under the
    lock, rather than on a number carried from process start. A count read at
    startup is only current while nothing else writes, which was true of the
    one caller and asserted of every future one; a second shim reading the same
    base would have spent units a first had already taken, and nothing would
    have said so. Returns the number of units spent including this one.
    """
    ATTEMPTS_PATH.parent.mkdir(parents=True, exist_ok=True)
    record = {
        "key": key,
        "model_requested": REQUESTED_MODEL,
        "scaffold_version": SCAFFOLD_VERSION,
        "pid": os.getpid(),
        "started_ns": _START_NS,
    }
    with ledger_lock():
        spent = load_attempt_count()
        if spent >= MAX_CALLS:
            raise BudgetExhausted(
                f"LLM call budget exhausted for {MODEL} "
                f"({spent} of {MAX_CALLS} dispatches reserved); field incomplete"
            )
        with ATTEMPTS_PATH.open("a", encoding="utf-8") as f:
            f.write(json.dumps(record, sort_keys=True) + "\n")
            f.flush()
            os.fsync(f.fileno())
        return spent + 1


def write_stats():
    # The harness kills the subprocess when a run ends, so stats are rewritten
    # after every decision rather than at exit. One file per process (the
    # timestamp guards against OS PID reuse); sum them afterwards.
    STATS_DIR.mkdir(parents=True, exist_ok=True)
    path = STATS_DIR / f"stats-{os.getpid()}-{_START_NS}.json"
    path.write_text(json.dumps(STATS, sort_keys=True), encoding="utf-8")


def summarize(obs):
    """Deterministic compact prompt: last 20 bar-over-bar returns per symbol
    (4 dp) and portfolio weights (2 dp). Rounding makes near-identical states
    across execution seeds hash to the same cache key."""
    lines = []
    prices = {}
    for s in obs.get("symbols", []):
        hist = s.get("close_history", [])
        prices[s["symbol"]] = hist[-1] if hist else 0.0
        rets = []
        tail = hist[-21:]
        for a, b in zip(tail, tail[1:]):
            rets.append(round(b / a - 1.0, 4) if a else 0.0)
        lines.append(f"{s['symbol']} last-{len(rets)}-bar returns: {rets}")
    nav = obs.get("cash", 0.0)
    for p in obs.get("portfolio", []):
        nav += p.get("shares", 0.0) * prices.get(p["symbol"], 0.0)
    weights = {}
    for p in obs.get("portfolio", []):
        w = 0.0
        if nav > 1e-12:
            w = p.get("shares", 0.0) * prices.get(p["symbol"], 0.0) / nav
        weights[p["symbol"]] = round(w, 2)
    cash_frac = round(obs.get("cash", 0.0) / nav, 2) if nav > 1e-12 else 1.0
    lines.append(f"current weights: {json.dumps(weights, sort_keys=True)}")
    lines.append(f"cash fraction: {cash_frac}")
    lines.append(
        "Choose target portfolio weights for the next bars. STRICT JSON only."
    )
    return "\n".join(lines)


def parse_decision(text, valid_symbols):
    """Parse the model's reply into a validated order list, or None."""
    t = text.strip()
    if t.startswith("```"):
        t = t.strip("`")
        if t.startswith("json"):
            t = t[4:]
    start, end = t.find("{"), t.rfind("}")
    if start < 0 or end <= start:
        return None
    try:
        payload = json.loads(t[start : end + 1])
    except json.JSONDecodeError:
        return None
    raw = payload.get("orders")
    if not isinstance(raw, list):
        return None
    orders = []
    total = 0.0
    for o in raw:
        if not isinstance(o, dict):
            return None
        sym = o.get("symbol")
        action = o.get("action")
        try:
            w = float(o.get("target_weight", 0.0))
        except (TypeError, ValueError):
            return None
        if sym not in valid_symbols or action not in ("buy", "sell", "hold"):
            return None
        if not math.isfinite(w) or not 0.0 <= w <= 1.0:
            return None
        total += w
        orders.append({"symbol": sym, "action": action, "target_weight": w})
    if total > 1.0 + 1e-12:
        return None
    return orders


def hold(reason, cost=None):
    d = {"orders": [], "reasoning": reason}
    if cost:
        d["cost"] = cost
    return d


def is_dated_snapshot_of(requested, served):
    """Whether `served` is `requested` pinned to a dated snapshot of itself.

    The rule is taken from what the provider returns, not from what a served id
    happens to start with. An alias expands into the same alias followed by one
    hyphen and an eight-digit date, and that is the only remainder the API
    appends: `claude-haiku-4-5` -> `claude-haiku-4-5-20251001`,
    `claude-opus-4-5` -> `claude-opus-4-5-20251101`, `claude-sonnet-4-5` ->
    `claude-sonnet-4-5-20250929`, `claude-opus-4-1` -> `claude-opus-4-1-20250805`.
    Those four pairs are the alias/pinned pairs the installed SDK's own
    `Message.model` literal enumerates.

    So any other continuation is a different model, not a more precise name for
    the requested one: `-mini` is not a date, and neither is a truncated or
    padded one. The rule is deliberately narrower than the provider's whole
    namespace. Two deprecated aliases rebind rather than expand
    (`claude-sonnet-4-0` is served as `claude-sonnet-4-20250514`), and this
    refuses those; refusing a policy that is arguably the requested one costs a
    run, while accepting one that is not publishes the wrong identity.
    """
    prefix = requested + SNAPSHOT_SEPARATOR
    if not served.startswith(prefix):
        return False
    snapshot = served[len(prefix):]
    return (
        len(snapshot) == SNAPSHOT_DIGITS and snapshot.isascii() and snapshot.isdigit()
    )


def is_requested_policy(served):
    """Whether `served` names the policy this run publishes.

    The single statement of the rule. `effective_model` applies it to the id an
    API reply carries and `load_cache` to the id a stored decision carries, so a
    served model refused on the writing path cannot be admitted on the replaying
    one. Two copies of the rule would be free to drift apart, and a replay
    screened by a stale copy resurrects exactly what the fresh path refuses.

    `None` is not the requested policy. `Message.model` is required on this API,
    and a cached record whose `model_effective` is absent or null states no
    identity at all, which is the same absence `effective_model` refuses.
    """
    if not isinstance(served, str):
        return False
    return served == REQUESTED_MODEL or is_dated_snapshot_of(REQUESTED_MODEL, served)


def effective_model(response):
    """The id the API says answered, and whether it is the policy requested.

    A provider may expand a requested alias into the dated snapshot it served;
    that names the same policy more precisely and is recorded. Any other id is
    a different policy answering under the requested name, which is the one
    thing this benchmark must not publish, so it fails the subprocess.

    A reply that names no model fails the same way. `Message.model` is a
    required field of this API, so absence is not a normal answer this run
    should absorb; it leaves the identity unverifiable, and returning the
    requested id would state that the requested policy answered on the strength
    of the API not having said so. That is the accepting-on-absence shape the
    reservation ledger and the retry guard both refuse.
    """
    served = getattr(response, "model", None)
    if served is None:
        raise RuntimeError(
            f"model identity unverifiable: requested {REQUESTED_MODEL} and the "
            "response names no model. This API always states the model that "
            "answered, so an absent id is not evidence that the requested "
            "policy did, and the field pins policy identity to the requested model"
        )
    if is_requested_policy(served):
        return served
    raise RuntimeError(
        f"model substitution: requested {REQUESTED_MODEL}, served {served}; "
        "the field pins policy identity to the requested model"
    )


def call_model(client, prompt):
    """One request under the requested model. No substitution on any path.

    An unknown or unavailable model is a failure of this run: retrying under a
    different model would evaluate a policy the field does not name, and the
    result would be recorded and replayed as the requested one.
    """
    try:
        return client.messages.create(**request_kwargs(REQUESTED_MODEL, prompt))
    except anthropic.NotFoundError as e:
        raise RuntimeError(
            f"model {REQUESTED_MODEL} is not available to this account; "
            "no fallback model is configured, so the field is incomplete"
        ) from e


def assert_no_provider_retries(client):
    """Refuse the run unless this client really will not retry.

    Passing `max_retries=0` is a request, not a guarantee. The shim imports
    whatever `anthropic` the operator installed, and a later major version that
    renames, moves or ignores the knob would accept the keyword and drop it,
    restoring silently the exact defect the ledger exists to prevent: several
    billable requests under one reserved unit. So the effective setting is read
    back off the object the run will use, which lets a compatible upgrade pass
    on its behaviour instead of on an allowlist of version strings.

    Fails closed in both directions. A setting that is present and not zero
    refuses, and so does one that cannot be determined, because a money ceiling
    that cannot be shown to hold is worse than a run that does not start.
    """
    effective = getattr(client, "max_retries", None)
    readable = isinstance(effective, int) and not isinstance(effective, bool)
    if readable and effective == PROVIDER_MAX_RETRIES:
        return
    installed = getattr(anthropic, "__version__", "unknown")
    detail = (
        f"reports max_retries={effective!r}"
        if readable
        else f"does not report a readable max_retries (got {effective!r})"
    )
    raise RuntimeError(
        f"the provider client {detail}, but the call ceiling requires "
        f"{PROVIDER_MAX_RETRIES}: one reserved unit must be exactly one "
        "provider request, and a client that retries bills several against "
        f"one unit. anthropic {installed} is installed; the ceiling's "
        f"guarantee was established against {EVIDENCED_SDK_VERSION}. Refusing "
        "to start rather than risk overspending the declared allowance."
    )


def build_client():
    """The provider client the call ceiling is defined against.

    `max_retries=0` is load-bearing rather than a tuning choice: the allowance
    is reserved once per `messages.create`, so the SDK must not expand that
    into several HTTP requests. With retries off the SDK sends exactly one.
    The setting is then verified on the constructed client, because the ceiling
    depends on it binding and not merely on it having been asked for.
    """
    client = anthropic.Anthropic(max_retries=PROVIDER_MAX_RETRIES)
    assert_no_provider_retries(client)
    return client


def main(client=None):
    # The client is a parameter so the decision loop can be exercised against a
    # stand-in; nothing but a test passes one.
    #
    # The retry check runs here and not only in `build_client`, because this is
    # the narrowest point every provider request passes through. A caller that
    # supplies a client would otherwise reach `client.messages.create` with its
    # retry policy never read, and the ceiling's published guarantee, that
    # either the setting binds on the client the run uses or the run does not
    # start, would hold on one path and not on the other.
    #
    # A supplied client is checked rather than refused, because the guarantee is
    # about how the client behaves and not about who constructed it, and the
    # case that observes the SDK's own retry policy has to hand in a client
    # built over a stand-in transport. Refusing every supplied client would push
    # that case back onto a path with no check on it, which is the shape this
    # removes. `build_client` keeps its own check: it is reachable on its own
    # and states the property of what it returns, and reading a compliant
    # client's setting a second time costs one attribute lookup.
    client = client if client is not None else build_client()
    assert_no_provider_retries(client)
    # Beside the retry guard for the same reason: a run whose calls cannot be
    # priced must not start, rather than bill a field and then report it as
    # free. Both are established before the first observation is read, when
    # refusing is still free.
    assert_model_is_priced()
    cache = load_cache()
    STATS["calls_reserved"] = load_attempt_count()
    step = 0
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        obs = json.loads(line)
        STATS["observations"] += 1
        decision = None
        if step % STRIDE != 0:
            STATS["stride_holds"] += 1
            decision = hold("stride hold (rebalance cadence)")
        else:
            prompt = summarize(obs)
            key = cache_key(REQUESTED_MODEL, prompt)
            if key in cache:
                STATS["cache_hits"] += 1
                if cache[key].get("malformed"):
                    # Deliberately violate the wire schema: ExternalAgent records
                    # an agent protocol fault and the resilient harness inserts a
                    # failing sentinel run. Replaying a cached malformed response
                    # must not resurrect the old masked-hold behavior. The cost
                    # of the call that produced it rides on the cache record,
                    # not here: this decision has to stay unparseable, so it can
                    # carry no accounting a reader would trust.
                    decision = {"protocol_error": "cached malformed model output"}
                else:
                    decision = {
                        "orders": cache[key]["orders"],
                        "reasoning": "cached decision",
                    }
                    if cache[key].get("cost"):
                        # The cache makes reruns free to the operator, but the
                        # benchmark's efficiency column describes the model call
                        # that produced this frozen decision, not the replay cost.
                        decision["cost"] = cache[key]["cost"]
            else:
                valid = {s["symbol"] for s in obs.get("symbols", [])}
                try:
                    # Reserved dispatches, not cached results: a failed call
                    # spent the money and must count. The reservation refuses
                    # when the ledger is out, which is the only place the
                    # current count is known. An incomplete model run is not
                    # evidence either way. Failing the subprocess makes the
                    # harness record a transport failure and the Rust driver
                    # refuses to publish the field.
                    STATS["calls_reserved"] = reserve_call(key)
                except BudgetExhausted:
                    STATS["budget_exhausted"] += 1
                    write_stats()
                    raise
                try:
                    STATS["llm_calls"] += 1
                    resp = call_model(client, prompt)
                    effective = effective_model(resp)
                    STATS["model_effective"] = effective
                    text = "".join(
                        b.text for b in resp.content if b.type == "text"
                    )
                    tin = resp.usage.input_tokens
                    tout = resp.usage.output_tokens
                    STATS["tokens_in"] += tin
                    STATS["tokens_out"] += tout
                    pin, pout = price_for(effective)
                    usd = tin * pin + tout * pout
                    STATS["cost_usd"] += usd
                    cost = {
                        "cost_usd": usd,
                        "tokens_in": tin,
                        "tokens_out": tout,
                    }
                    if getattr(resp, "stop_reason", None) == "refusal":
                        STATS["refusals"] += 1
                        decision = hold("model refusal -> hold", cost)
                        record_decision(
                            cache, key, effective,
                            {"orders": [], "refusal": True,
                             "tokens_in": tin, "tokens_out": tout, "cost": cost},
                        )
                    else:
                        orders = parse_decision(text, valid)
                        if orders is None:
                            STATS["malformed"] += 1
                            decision = {"protocol_error": "malformed model output"}
                            # The call is billed whether or not its reply parsed,
                            # so the record carries what it cost, as the refusal
                            # and success records do. Without it a replayed
                            # malformed decision reported no cost at all, and a
                            # reader adding up the cache read a real call as free.
                            record_decision(
                                cache, key, effective,
                                {"orders": [], "malformed": True,
                                 "tokens_in": tin, "tokens_out": tout,
                                 "cost": cost},
                            )
                        else:
                            decision = {
                                "orders": orders,
                                "reasoning": "llm allocation",
                                "cost": cost,
                            }
                            record_decision(
                                cache, key, effective,
                                {"orders": orders, "tokens_in": tin,
                                 "tokens_out": tout, "cost": cost},
                            )
                except anthropic.APIError as e:
                    STATS["api_errors"] += 1
                    write_stats()
                    raise RuntimeError(
                        f"Anthropic API failure for {REQUESTED_MODEL}: {type(e).__name__}"
                    ) from e
        step += 1
        sys.stdout.write(json.dumps(decision) + "\n")
        sys.stdout.flush()
        write_stats()


if __name__ == "__main__":
    main()
