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
checked against the request: the same id, or a versioned expansion of a
requested alias (`claude-haiku-4-5` served as `claude-haiku-4-5-20251001`),
is the requested policy; anything else is a different policy answering under
the requested name and fails the subprocess. Both identities are recorded on
every cached decision, so a replay states which model actually answered.

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
    harness spawns, which the producer starts one at a time. The client is
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
STATS_DIR = Path(os.environ.get("LLM_STATS_DIR", HERE / "stats"))

# First-party API pricing, USD per token (input, output).
PRICING = {
    "claude-fable-5": (10.00e-6, 50.00e-6),
    "claude-opus-5": (5.00e-6, 25.00e-6),
    "claude-haiku-4-5": (1.00e-6, 5.00e-6),
}


def price_for(model):
    for prefix, p in PRICING.items():
        if model.startswith(prefix):
            return p
    return (0.0, 0.0)


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

    One line per reservation, so the count is the line count. Read at startup
    the way the cache is, because the harness runs each window in its own
    subprocess and the allowance is per model, not per process.
    """
    if not ATTEMPTS_PATH.exists():
        return 0
    with ATTEMPTS_PATH.open("r", encoding="utf-8") as f:
        return sum(1 for line in f if line.strip())


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

    Reservations are serialized by the producer, which runs one model against
    one window at a time. The count is read at startup and advanced in memory,
    so concurrent processes sharing one ledger would each start from the same
    base; the ceiling assumes the sequential spawn the harness performs.
    """
    ATTEMPTS_PATH.parent.mkdir(parents=True, exist_ok=True)
    record = {
        "key": key,
        "model_requested": REQUESTED_MODEL,
        "scaffold_version": SCAFFOLD_VERSION,
        "pid": os.getpid(),
        "started_ns": _START_NS,
    }
    with ATTEMPTS_PATH.open("a", encoding="utf-8") as f:
        f.write(json.dumps(record, sort_keys=True) + "\n")
        f.flush()
        os.fsync(f.fileno())


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


def effective_model(response):
    """The id the API says answered, and whether it is the policy requested.

    A provider may expand a requested alias into the pinned version it served;
    that names the same policy more precisely and is recorded. Any other id is
    a different policy answering under the requested name, which is the one
    thing this benchmark must not publish, so it fails the subprocess.
    """
    served = getattr(response, "model", None)
    if served is None:
        return REQUESTED_MODEL
    if served == REQUESTED_MODEL or served.startswith(REQUESTED_MODEL):
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
    client = client if client is not None else build_client()
    cache = load_cache()
    attempts = load_attempt_count()
    STATS["calls_reserved"] = attempts
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
                    # must not resurrect the old masked-hold behavior.
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
            elif attempts >= MAX_CALLS:
                # Reserved dispatches, not cached results: a failed call spent
                # the money and must count. An incomplete model run is not
                # evidence either way. Failing the subprocess makes the harness
                # record a transport failure and the Rust driver refuses to
                # publish the field.
                STATS["budget_exhausted"] += 1
                write_stats()
                raise RuntimeError(
                    f"LLM call budget exhausted for {MODEL} "
                    f"({attempts} of {MAX_CALLS} dispatches reserved); field incomplete"
                )
            else:
                valid = {s["symbol"] for s in obs.get("symbols", [])}
                try:
                    reserve_call(key)
                    attempts += 1
                    STATS["calls_reserved"] = attempts
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
                            record_decision(
                                cache, key, effective,
                                {"orders": [], "malformed": True,
                                 "tokens_in": tin, "tokens_out": tout},
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
