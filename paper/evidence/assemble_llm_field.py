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

What it refuses to publish: a field whose (model, dataset) cells are not all
present, one recording a (dataset, agent_id) submission twice, a model whose
calls it cannot cost -- because no rate card names it, or because no accounting
row was built for it -- a statistics file it cannot parse, and a run reporting
API errors, an exhausted budget or a refused model identity. Each is an
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

    The single statement of that rule. Two causes reach it, and they are the
    same unavailability seen from two sides: no rate card names the model, so
    its calls cannot be costed, and no per-model accounting row was built for
    it, so there are no calls to cost. Zero is the tempting answer to each and
    is a plausible wrong number where the project records an unavailability.

    Stated once because the two are one rule. Restated, a later edit could
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


per_model = {}
for cache in sorted(FINAL.glob("llm-cache-*.jsonl")):
    model = cache.stem.removeprefix("llm-cache-")
    calls = malformed = refusals = tin = tout = 0
    for line in cache.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        rec = json.loads(line)
        calls += 1
        malformed += 1 if rec.get("malformed") else 0
        refusals += 1 if rec.get("refusal") else 0
        tin += rec.get("tokens_in", 0)
        tout += rec.get("tokens_out", 0)
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
for f in sorted(STATS_DIR.glob("stats-*.json")):
    stats_files_read += 1
    try:
        rec = json.loads(f.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"refusing to assemble: corrupt stats file {f}: {exc}") from exc
    m = rec.get("model")
    # A model a run reported statistics for and the per-model table does not
    # name is refused, not skipped. `continue` treated it as costing nothing:
    # its calls, tokens and spend were dropped from the field and the totals
    # were published as if that model had never run, which is the free-by-
    # omission form of the zero `price_for` refuses. The table is built from the
    # response caches, so the cause is a run whose cache file is missing or
    # named otherwise, and what its calls cost is exactly what is unknown.
    if m not in per_model:
        refuse_unaccountable(
            m,
            "no accounting row",
            f"{f.name} reports statistics for it and no llm-cache-{m}.jsonl "
            f"was read; the table names {sorted(per_model)}",
        )
    for k in secondary_keys:
        per_model[m][k] = per_model[m].get(k, 0) + rec.get(k, 0)

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

observed_datasets = {r.get("dataset") for r in records}
if observed_datasets != required_datasets:
    raise SystemExit(
        f"refusing to assemble: datasets {sorted(observed_datasets)}; "
        f"required {sorted(required_datasets)}"
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
