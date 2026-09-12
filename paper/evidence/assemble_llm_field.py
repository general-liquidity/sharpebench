#!/usr/bin/env python3
"""Assemble paper/evidence/final/llm-field.jsonl.

Line 1 is a command/provenance record (how the field was produced, per-model
LLM call accounting, malformed-output and refusal rates); the rest are
per-(dataset, agent) score records in the same shape as the other final
evidence files, produced by
crates/sharpebench-harness/examples/llm_field_eval.rs.

Call accounting is derived from the per-model response caches
(llm-cache-<model>.jsonl): one line per fresh API call, flagged when the
response was malformed or a refusal. That makes the accounting restart-proof
(the harness may be resumed; cached decisions cost nothing and are not
recounted). The per-process stats files supply the secondary counters
(observations, stride holds, cache hits).

A run leaves three kinds of evidence and this file reconciles all three. The
attempt ledger (llm-attempts-<model>.jsonl) takes one line under a lock before
each request leaves, so it is the ceiling and the only record written before the
money is spent. The response cache records the requests that returned an answer,
so it is a subset of the ledger. The per-process statistics count each request in
the process that issued it, so they partition the ledger. In a field this file
will publish the three counts are equal, and a disagreement is calls at least one
of them cannot see.

What it refuses to publish: a field whose (model, dataset) cells are not all
present, one recording a (dataset, agent_id) submission twice or a (model,
dataset) cell twice, a model whose calls it cannot cost -- because no rate card
names it, because no accounting row was built for it, because one of its cache
records states no usage, or because its cache records no call at all -- a scored
model with no response cache, an accounting row with no score row, a model whose
attempt ledger is absent or empty, a model whose three evidence kinds disagree
about how many calls it made, a statistics file it cannot parse, and a run
reporting API errors, an exhausted budget or a refused model identity. Each is an
incompleteness whose only plausible alternative is a number nobody measured.

Run from the repo root after the field run:
  python paper/evidence/assemble_llm_field.py
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
FINAL = HERE / "final"
RECORDS = FINAL / "llm-field-records-all.jsonl"
STATS_DIR = FINAL / "llm-stats"
OUT = FINAL / "llm-field.jsonl"

# The rate card, the alias-expansion rule and the acceptance decision, shared
# with `examples/llm-agent/llm_agent.py`, which meters the run this file
# publishes. Both were restated here once, because importing the shim would pull
# the Anthropic SDK into an assembler that reads only files, and the two copies
# could then be edited apart: a model priced by one side and refused by the
# other, or priced differently by each. `llm_pricing` imports nothing at all, so
# sharing it carries nothing into either side. `paper/src/test_llm_pricing.py`
# fails if this file or the shim grows a second table or a second rule.
sys.path.insert(0, str(HERE))
from llm_pricing import PRICING, lookup_price  # noqa: E402

if not RECORDS.exists() or not RECORDS.read_text(encoding="utf-8").strip():
    raise SystemExit("refusing to assemble: score record file is empty")


def refuse_unaccountable(model, cause, detail):
    """Refuse the field: `model`'s spend cannot be stated, so it is not published.

    The single statement of that rule. Every cause reaches it, and they are the
    same unavailability seen from several sides: no rate card names the model,
    so its calls cannot be costed; no per-model accounting row was built for it,
    so there are no calls to cost; one of its cache records carries no usage, so
    that call's tokens are unknown; its cache records no call at all, or no cache
    exists for a model the field scores, or no ledger records the requests, so
    the score rows it published rest on calls this field has no evidence of; an
    accounting row carries no score row, so its spend is summed into totals no
    published row explains; and the three evidence kinds disagree, so at least
    one of them cannot see calls the provider was asked to make. Zero is the
    tempting answer to each and is a plausible wrong number where the project
    records an unavailability.

    Stated once because the four are one rule. Restated, a later edit could
    repair the refusal on one path and leave the other reporting a billed model
    as free, which is the shape this file exists to refuse.
    """
    raise SystemExit(
        f"refusing to assemble: {cause} for {model}; {detail}. A model whose "
        "spend this field cannot state has no cost it can publish, and "
        "reporting zero would publish calls that were billed as free"
    )


def price_for(model):
    """The rate card for `model`, or a refusal to assemble the field.

    A cache file may name a priced alias or the dated snapshot the provider
    expanded it into, and nothing else: the shim refuses to record a decision
    under any other served id, and `lookup_price` admits exactly those two.

    Two fail-open behaviours are gone. A prefix walk billed a model whose name
    extends a priced one at the other model's card, and an unknown model fell
    back to `(0.0, 0.0)`, so the assembled field published a `cost_usd` of 0 for
    calls that were billed. This file's whole job is to publish what the field
    spent, and a plausible wrong number is worse there than no field at all:
    every other incompleteness here refuses the same way.
    """
    rate = lookup_price(model)
    if rate is not None:
        return rate
    refuse_unaccountable(
        model, "no rate card", f"PRICING names {sorted(PRICING)}"
    )


def usage(cache, lineno, rec, field):
    """One call's count of `field`, or a refusal to assemble the field.

    `rec.get(field, 0)` is the same fail-open one level below `price_for`: a
    record carrying no usage was costed at zero, so a call that was billed was
    published as free. A model whose rate card is unknown has no cost this field
    can state, and neither has a call whose token counts are unknown, so both
    refuse through the one rule.

    Present is not measured. A null, a string, a negative count or a boolean is
    not a number of tokens, and a count that cannot be read is not a count of
    zero.
    """
    n = rec.get(field)
    if isinstance(n, int) and not isinstance(n, bool) and n >= 0:
        return n
    refuse_unaccountable(
        cache.stem.removeprefix("llm-cache-"),
        "no usage evidence",
        f"{cache.name} line {lineno} states {field}={n!r}, which is not a count "
        "of tokens",
    )


per_model = {}
for cache in sorted(FINAL.glob("llm-cache-*.jsonl")):
    model = cache.stem.removeprefix("llm-cache-")
    calls = malformed = refusals = tin = tout = 0
    for lineno, line in enumerate(cache.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        rec = json.loads(line)
        calls += 1
        malformed += 1 if rec.get("malformed") else 0
        refusals += 1 if rec.get("refusal") else 0
        tin += usage(cache, lineno, rec, "tokens_in")
        tout += usage(cache, lineno, rec, "tokens_out")
    # Decided here rather than left to the loop, because an empty cache is a
    # different input from a record with no usage and the two deserve different
    # causes. A cache accumulates one line per fresh call across resumes, and
    # every model this field publishes carries score rows the harness produced
    # by calling it, so a cache with no lines is not a measurement that a model
    # spent nothing: it is the absence of any evidence about a model that
    # certainly called. Publishing `cost_usd: 0.0` from it states a number
    # nobody measured, which is the same zero the rule below refuses.
    if not calls:
        refuse_unaccountable(
            model,
            "no calls recorded",
            f"{cache.name} carries no records, so the score rows this field "
            "publishes for the model rest on calls it has no evidence of",
        )
    pin, pout = price_for(model)
    per_model[model] = {
        "llm_calls": calls,
        "malformed_outputs": malformed,
        "malformed_rate": (malformed / calls) if calls else 0.0,
        "refusals": refusals,
        "tokens_in": tin,
        "tokens_out": tout,
        "cost_usd": round(tin * pin + tout * pout, 4),
    }

secondary_keys = ["observations", "stride_holds", "cache_hits",
                  "budget_exhausted", "api_errors", "identity_refusals"]
stats_files_read = 0
unaccounted = {}
# The third evidence kind, kept out of `per_model` because it is not published:
# how many dispatches the per-process statistics counted for each model, and
# over how many files. `llm_calls` in a statistics file counts the dispatches
# that process made; `llm_calls` in `per_model` counts the records its response
# cache holds. They are different measurements of the same calls and are
# reconciled below rather than summed into one another.
stats_calls = {}
stats_files_naming = {}
for f in sorted(STATS_DIR.glob("stats-*.json")):
    stats_files_read += 1
    try:
        rec = json.loads(f.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"refusing to assemble: corrupt stats file {f}: {exc}") from exc
    m = rec.get("model")
    # A model a run reported statistics for and the per-model table does not
    # name is collected here and refused below, not skipped. `continue` alone
    # treated it as costing nothing: its calls, tokens and spend were dropped
    # and the totals were published as if that model had never run, which is
    # the free-by-omission form of the zero `price_for` refuses. Collected
    # rather than refused on the first file so the refusal can say how much of
    # the run is unaccounted for, which is what tells an operator whether a
    # stray file or an entire model's spend is missing.
    if m not in per_model:
        unaccounted[m] = unaccounted.get(m, 0) + 1
        continue
    stats_calls[m] = stats_calls.get(m, 0) + rec.get("llm_calls", 0)
    stats_files_naming[m] = stats_files_naming.get(m, 0) + 1
    for k in secondary_keys:
        per_model[m][k] = per_model[m].get(k, 0) + rec.get(k, 0)

if unaccounted:
    counted = ", ".join(f"{m} ({n})" for m, n in sorted(unaccounted.items()))
    refuse_unaccountable(
        ", ".join(sorted(unaccounted)),
        "no accounting row",
        f"{sum(unaccounted.values())} of {stats_files_read} statistics files "
        f"report a model the per-model table does not name: {counted}. The "
        f"table is built from the response caches and names {sorted(per_model)}, "
        "so no llm-cache file was read for these. Their calls, tokens and spend "
        "would be absent from per_model and from llm_calls_total and "
        "cost_usd_total while the score records still carry the model, so the "
        "field would name more models than it accounts for",
    )

records = [
    json.loads(line)
    for line in RECORDS.read_text(encoding="utf-8").splitlines()
    if line.strip()
]

required_models = {
    "claude-fable-5",
    "claude-opus-5",
    "claude-haiku-4-5-20251001",
}
required_datasets = {"us-indices-1d", "crypto-majors-1d"}

# The field is one submission per model per dataset, so what has to be complete
# is the product of the two sets and not each set on its own. Two separate
# memberships were checked, `observed_models` against the required models and
# `observed_datasets` against the required datasets, and a field missing a
# specific cell passed both as long as every model appeared against some dataset
# and every dataset appeared against some model: dropping
# (claude-opus-5, crypto-majors-1d) alone left opus-5 present on us-indices-1d
# and crypto-majors-1d present under the other two models. The published field
# would then have named three models and two datasets while carrying five of the
# six cells, and every total it states would have been over five.
required_cells = {(m, d) for m in required_models for d in required_datasets}
observed_cells = {
    (r.get("model"), r.get("dataset"))
    for r in records
    if r.get("agent_id", "").startswith("llm-")
}
missing = sorted(f"{m}/{d}" for m, d in required_cells - observed_cells)
unexpected = sorted(f"{m}/{d}" for m, d in observed_cells - required_cells)
if missing or unexpected:
    raise SystemExit(
        "refusing to assemble: the LLM field is one submission per model per "
        f"dataset, so {len(required_cells)} (model, dataset) cells are "
        f"required; missing {missing}; unexpected {unexpected}"
    )

# Completeness of the product says every cell is present at least once; it does
# not say any cell is present once. A repeated (dataset, agent_id) is two score
# rows for one submission, which every per-cell reader would count twice and no
# gate here would have noticed. Checked over all records rather than the LLM
# rows alone: the reference-field and luck-floor rows the file carries are
# submissions under the same pairing.
pairs = [(r.get("dataset"), r.get("agent_id")) for r in records]
duplicates = sorted(f"{d}/{a}" for d, a in set(pairs) if pairs.count((d, a)) > 1)
if duplicates:
    raise SystemExit(
        "refusing to assemble: one (dataset, agent_id) is one submission and "
        f"these are recorded more than once: {duplicates}"
    )

# The cell is the submission; the agent id is not. Neither gate above can see a
# cell recorded twice under two agent ids. `observed_cells` is a set, so the
# second row collapses into the member already there and the product stays
# complete; the pairing just checked is keyed on a different identity, so a
# rerun filed under a second agent id satisfies it. Both passed, and two score
# rows for one cell is what every per-cell reader of the published field counts
# twice. Kept beside the pairing rather than replacing it: the two are different
# invariants, and the pairing is the one that covers the reference-field and
# luck-floor rows, which carry no model. The refusal names the cell and the
# agent ids that filed it, because which row to withdraw is the next question.
cell_agents = {}
for r in records:
    if not r.get("agent_id", "").startswith("llm-"):
        continue
    cell_agents.setdefault((r.get("model"), r.get("dataset")), []).append(
        r.get("agent_id")
    )
repeated = sorted(
    f"{m}/{d} filed by {sorted(agents)}"
    for (m, d), agents in cell_agents.items()
    if len(agents) > 1
)
if repeated:
    raise SystemExit(
        "refusing to assemble: the LLM field is one submission per model per "
        f"dataset and these cells carry more than one score row: {repeated}"
    )

observed_datasets = {r.get("dataset") for r in records}
if observed_datasets != required_datasets:
    raise SystemExit(
        f"refusing to assemble: datasets {sorted(observed_datasets)}; "
        f"required {sorted(required_datasets)}"
    )
# Which models must be accounted for, decided by the field rather than by the
# directory. Every gate above this point reads a roster off the files that
# happen to be on disk: `per_model` is whatever `llm-cache-*.jsonl` globbed, and
# the missing-model refusal fires only from the loop over `stats-*.json`, so a
# run with no cache files and no statistics files had nothing to check anything
# against. A complete six-cell score grid then published `per_model: {}`,
# `llm_calls_total: 0` and `cost_usd_total: 0`, and every gate passed: the
# score rows were never reconciled against the accounting table at all. The
# roster is the models the field publishes scores for, taken from the score rows
# and from the cells this file requires, and it exists whether or not a single
# evidence file does.
accounting_roster = sorted(
    required_models
    | {r.get("model") for r in records if r.get("agent_id", "").startswith("llm-")}
)

unpublished = [m for m in accounting_roster if m not in per_model]
if unpublished:
    refuse_unaccountable(
        ", ".join(unpublished),
        "no response cache",
        f"the field publishes score rows for {accounting_roster} and the "
        f"per-model accounting table names {sorted(per_model)}. The table is "
        "built by globbing llm-cache-*.jsonl, so a model with no cache file is "
        "absent from per_model and from llm_calls_total and cost_usd_total "
        "while its score rows are published regardless. Absent the statistics "
        "file that would otherwise have named it, nothing reconciled the two, "
        "and the field would state a complete score grid it reports no calls "
        "and no dollars for",
    )

# The other direction of the same reconciliation. A cache file for a model the
# field does not score builds an accounting row nothing publishes a score for,
# and its calls and dollars are summed into the totals: the field would then
# report spend on a model no reader can find a row for.
unscored = [m for m in sorted(per_model) if m not in accounting_roster]
if unscored:
    refuse_unaccountable(
        ", ".join(unscored),
        "no score row",
        f"the per-model accounting table names {sorted(per_model)} and the "
        f"field scores {accounting_roster}; the calls, tokens and spend of a "
        "model the field does not publish would still be summed into "
        "llm_calls_total and cost_usd_total",
    )


def dispatches_reserved(model):
    """Provider requests `model` reserved, or a refusal to assemble the field.

    The attempt ledger is the first of the three evidence kinds and the only one
    written before the money is spent: the shim appends one line under an
    exclusive lock before each request leaves, so a call that fails, times out
    or returns something uncacheable has a ledger line and no cache record. It
    is therefore the ceiling on what the field can have spent, and the response
    cache alone cannot establish it -- a cache is what survived, not what was
    dispatched.

    Absent and empty are separate causes for the reason the empty cache is:
    a ledger with no lines is not a measurement that a model reserved nothing,
    because the score rows this field publishes were produced by calling it.
    """
    path = FINAL / f"llm-attempts-{model}.jsonl"
    if not path.exists():
        refuse_unaccountable(
            model,
            "no attempt ledger",
            f"{path.name} does not exist, so the field has no record of the "
            "provider requests the model's score rows were produced by; the "
            "response cache records only the calls that came back",
        )
    reserved = sum(
        1 for line in path.read_text(encoding="utf-8").splitlines() if line.strip()
    )
    if not reserved:
        refuse_unaccountable(
            model,
            "no dispatch reserved",
            f"{path.name} carries no records, and one line is appended before "
            "every request, so the score rows this field publishes rest on "
            "dispatches it has no evidence of",
        )
    return reserved


# The three evidence kinds, reconciled as three measurements of one set of
# calls rather than one of them taken for the whole run. The ledger reserves a
# line before each request; the response cache records the answer to each
# request that returned one; the per-process statistics count each request in
# the process that issued it. So the ledger is the ceiling, the cache is a
# subset of it, and the statistics are a partition of it by process. A field
# this file will publish has no API errors, no exhausted budget and no refused
# identity -- each is refused below -- so every reserved dispatch of a
# publishable field returned a recorded answer inside a process whose statistics
# survived, and the three counts are equal. They disagree in three ways, all of
# them the finding this gate exists for: fewer cache records than reservations
# is spend with no answer recorded, fewer counted dispatches than reservations
# is a process whose statistics are missing, and more of either than the ledger
# reserved is a count of calls nothing reserved.
for model in accounting_roster:
    reserved = dispatches_reserved(model)
    cached = per_model[model]["llm_calls"]
    counted = stats_calls.get(model, 0)
    if not reserved == cached == counted:
        refuse_unaccountable(
            model,
            "evidence disagrees",
            f"the attempt ledger reserves {reserved} dispatches, the response "
            f"cache records {cached} and the {stats_files_naming.get(model, 0)} "
            f"statistics files naming the model count {counted}. These are "
            "three measurements of one set of calls and a publishable field "
            "has them equal; where they differ, at least one of the three is "
            "missing calls the provider was asked to make",
        )

# An identity refusal is a call the provider answered under a model this field
# does not name. It fails the shim, so it is the same kind of incompleteness as
# an API error or an exhausted budget, and it is refused with them.
if any(m.get("api_errors", 0) or m.get("budget_exhausted", 0)
       or m.get("identity_refusals", 0)
       for m in per_model.values()):
    raise SystemExit(
        "refusing to assemble: API errors, exhausted budgets or refused model "
        "identities make the field incomplete"
    )

meta = {
    "kind": "command",
    "description": (
        "First LLM-agent field: examples/llm-agent/llm_agent.py (stdio "
        "ExternalAgent protocol; temperature 0 where the API accepts it, "
        "decision stride 5 bars, per-model response caches "
        "llm-cache-<model>.jsonl) driven through the standard walk-forward "
        "harness and scoring kernel on us-indices-1d and crypto-majors-1d, "
        "against the reference field and luck floor; one submission per "
        "model. Produced by: cargo run --release -p sharpebench-harness "
        "--example llm_field_eval -- <out.jsonl> [dataset]"
    ),
    "stride_bars": 5,
    # The denominator for the secondary counters below: how many per-process
    # statistics files were read into them. Every file found is either read into
    # a model's row or refuses the assembly, so this is also how many were
    # found, and a reader can tell that none was dropped on the way.
    "stats_files_read": stats_files_read,
    "datasets": sorted({r["dataset"] for r in records}),
    "per_model": per_model,
    "llm_calls_total": sum(m["llm_calls"] for m in per_model.values()),
    "cost_usd_total": round(sum(m["cost_usd"] for m in per_model.values()), 4),
}

# Result artifacts are hashed byte-exact, so the writer must not translate newlines.
with OUT.open("w", encoding="utf-8", newline="") as f:
    f.write(json.dumps(meta, sort_keys=True) + "\n")
    for r in records:
        f.write(json.dumps(r) + "\n")

print(f"wrote {OUT} ({1 + len(records)} lines)")
for r in records:
    if not r["agent_id"].startswith("llm-"):
        continue
    print(
        f"  {r['dataset']} {r['agent_id']}: DSR={r['deflated_sharpe']:.4g} "
        f"passed_k={r['passed_k']} bootstrap_p={r['bootstrap_p']:.4g} "
        f"eligible={r['rank_eligible']} maxDD={r['worst_run_drawdown']:.3f} "
        f"mean_ret={r['raw_mean_return']:.3g}"
    )
for m, s in sorted(per_model.items()):
    print(f"meta {m}: calls={s['llm_calls']} malformed={s['malformed_outputs']} "
          f"rate={s['malformed_rate']:.4f} refusals={s['refusals']} "
          f"cost_usd={s['cost_usd']}")
print(f"total calls={meta['llm_calls_total']} cost_usd={meta['cost_usd_total']}")
