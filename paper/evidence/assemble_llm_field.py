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
    raise SystemExit(
        f"refusing to assemble: no rate card for {model}; PRICING names "
        f"{sorted(PRICING)}. A model absent from the table has no cost this "
        "field can state, and reporting zero would publish billed calls as free"
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
                  "budget_exhausted", "api_errors"]
for f in sorted(STATS_DIR.glob("stats-*.json")):
    try:
        rec = json.loads(f.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"refusing to assemble: corrupt stats file {f}: {exc}") from exc
    m = rec.get("model")
    if m not in per_model:
        continue
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
observed_models = {
    r.get("model") for r in records if r.get("agent_id", "").startswith("llm-")
}
observed_datasets = {r.get("dataset") for r in records}
if observed_models != required_models:
    raise SystemExit(
        f"refusing to assemble: models {sorted(observed_models)}; "
        f"required {sorted(required_models)}"
    )
if observed_datasets != required_datasets:
    raise SystemExit(
        f"refusing to assemble: datasets {sorted(observed_datasets)}; "
        f"required {sorted(required_datasets)}"
    )
if any(m.get("api_errors", 0) or m.get("budget_exhausted", 0)
       for m in per_model.values()):
    raise SystemExit(
        "refusing to assemble: API errors or exhausted budgets make the field incomplete"
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
