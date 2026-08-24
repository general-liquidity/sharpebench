# SharpeBench final-integrity audit — 2026-08-24

**Stage:** 4.5 final integrity, fresh working-tree audit before release commit.
**Inputs:** `paper/main.tex` SHA-256 `24132e379ef8f6d9476d5ae9c9a49c27ba37f52b5b8ea930947c4bb7d3ede19a`; `paper/refs.bib` SHA-256 `75bdebe43d280bc75d809fcce9a328e9f9263b4947ec598840f6a8616a252ac2`; base commit `95a80e2831289b346afb687462fb0c473b1f711f`.
**Scope:** no paid-model/API call was made. The incomplete LLM run was inspected only to verify that it is excluded.

## Verdict: PASS — zero registered integrity issues

All registered scientific claim surfaces are internally consistent with their declared records, all citation commands resolve, and the seven failure-mode checks are CLEAR. The current version-controlled working tree, including its pending normal-release additions, is the audited artifact.

The current `git status --short` is preserved below as release-readiness provenance: it records pending manuscript, source, evidence and arena files that form the candidate artifact for the next ordinary commit. It is not a manuscript-integrity failure under the agreed working-tree scope. `git diff --check` completed with no whitespace errors.

```text
M  .gitignore; README.md; crates/sharpebench-cli/src/main.rs
M  crates/sharpebench-core/src/selfaudit.rs; crates/sharpebench-py/Cargo.lock
M  docs/book/src/{cli,integrity}.md; paper/main.pdf; paper/refs.bib
M  paper/sections/{00-abstract,01-introduction,03-benchmark,04-integrity,
   05-experiments,06-related,07-limitations,08-reproducibility,A-commands}.tex
?? arena/; crates/sharpebench-harness/examples/{llm_field_eval,luck_floor_1000}.rs
?? examples/llm-agent/; paper/evidence/assemble_llm_field.py
?? paper/evidence/final/luck-floor-1000.jsonl
?? paper/sections/hardening-fragment.tex; paper/review/final-integrity-2026-08-24.md
```

This PASS does not certify global correctness of external sources or raw market provenance beyond the bounded checks below. Any change to the audited bytes requires a new integrity pass.

## Coverage and citation inventory

| Population | Denominator | Check | Result |
|---|---:|---|---|
| Bibliography entries | 35/35 | Parsed every `@...{key,}` in `refs.bib`; each has title, author, year, and DOI/URL/arXiv locator. | Local metadata complete. |
| Citation commands | 66/66 | Parsed every `\\cite*{...}` in `main.tex` and `sections/*.tex`: 35 unique cited keys, 0 undefined keys and 0 uncited entries. | PASS. |
| Citation contexts | 66/66 | Read every citing paragraph/caption. Method references occur at the stated method; benchmark references occur at the comparison claims. | PASS, bounded to manuscript/source-file evidence. |
| Table cells | 88/88 | `paper/evidence/table-provenance.md` records each mark and deliberate blank. It explicitly labels body-level gaps `unchecked` rather than promoting them to claims. | PASS. |
| Fresh external primary-source fetch | 0/35 | Not run in this audit. Existing primary locators and the prior verification ledger remain in `refs.bib` and `table-provenance.md`. | Out of scope / unknown; not counted as positive fresh re-verification. |

The local inventory is therefore 35/35 bibliography entries with a citation context and 66/66 resolving citation commands. The `refs.bib:1-4` historical primary-source-check statement was not treated as a fresh network verification.

## Numerical/statistical claim surfaces

| Surface | Denominator / evidence | Fresh check | Result |
|---|---|---|---|
| Nine-dataset sweep | 9 files x 512 = 4,608 rows in `paper/evidence/final/{commodities,crypto*,fx,rates,us-indices*}.jsonl` | Counted 512 rows in every file, matching `sections/05-experiments.tex:3`. | PASS. |
| Risk-managed sensitivity | `risk-managed.jsonl`, 100 rows including 35 `n_sensitivity`; SHA-256 `fda3df0b5b92f549d1120de689197a7e51ae0d9d8b7f0ec86d1b9399249f9563` | Weekly risk-managed DSRs at N=7/10/25/50/100 are 0.8589/0.7699/0.4923/0.3015/0.1645; all are ineligible under both verdicts, matching `sections/05-experiments.tex:97-99`. | PASS. |
| Synthetic pass witness | `pass-witness.jsonl`, 156 rows; SHA-256 `52bc7a057ea59f97c745454c58eec5f7e5d42cfd0dfd54a929fcfac545912765` | 13 eligible rows; first injected Sharpe is 0.45 per period / 3.245 annualized weekly and 0.20 / 3.175 daily, matching `sections/05-experiments.tex:109`. | PASS. |
| Thousand-agent luck floor | `luck-floor-1000.jsonl`, 2,002 rows: 2 x 1,000 agents plus 2 summaries | Zero eligible on both configured/measured paths. Crypto measured max is 0.04681445, first-five max 0.01285744; US-indices max is 0.0. Neither floor beats the best reference raw return. This supports the rounded 0.047, 0.013, 2,000, and about-3.6x claims in `hardening-fragment.tex:7`. | PASS. |
| Sybil audit | `crates/sharpebench-core/src/selfaudit.rs:446-599` | The code constructs 200 puppets, marks `expected_vulnerable: true`, verifies an eligibility flip, and requires exactly one known gap. Its emitted values are 0.335 -> 0.057, 0.000 -> 0.973, 199/199, matching `sections/03-benchmark.tex:37` and `sections/04-integrity.tex:9`. | PASS. |
| Forward window | `arena/windows/window-001/window.json:1-32`; `arena/state.json:1-7` | `status` is `open`; epochs 20710/20719; commitments and scores are empty. Text dates and “zero commitments/no result” match `sections/04-integrity.tex:33`. | PASS. |
| LLM result exclusion | ignored partial records, run log and stats; `paper/evidence/assemble_llm_field.py:80-105` | The run log records a provider credit-balance failure; no assembled `llm-field.jsonl` exists; assembler rejects `api_errors` or exhausted budget. The paper explicitly treats partial provider failures as inadmissible (`sections/07-limitations.tex:3-5`). | PASS: no incomplete result is reported. |

Other direct anchors: historical units table `sections/05-experiments.tex:31-46`; bootstrap specification `sections/03-benchmark.tex:19-27`; eight-plus-one audit inventory `sections/04-integrity.tex:9-28`; producing commands `sections/A-commands.tex:1-77`.

## Headline-claim audit

| Claim | Bounded evidence | Result |
|---|---|---|
| Eight defended / one Sybil gap | `selfaudit.rs:544-599` tests the eight non-gap cases and exactly one expected-vulnerable gap. | PASS. |
| 1,000 zero-skill agents, max DSR 0.047 | The exact 2,002-row record supports two daily datasets only; the manuscript does not generalize it to all nine. | PASS. |
| First forward window open, no result | Open state with empty commitments/scores supports lifecycle-start only, not neutral governance or performance. The paper says this. | PASS. |
| No paid LLM result | The attempted field is incomplete and fail-closed; no valid assembled field result is cited. | PASS. |
| Protocol/reference-agent scope | Abstract, introduction, and limitations consistently state that all reported entrants are author-written controls/reference/synthetic agents. | PASS. |

## Seven AI-research failure modes

| Mode | Status | Evidence / boundary |
|---|---|---|
| 1. Implementation bug passing self-review | CLEAR | New result claims trace to code plus JSONL records; units and single-symbol-floor defects are disclosed. This is not proof against future bugs. |
| 2. Hallucinated citation | CLEAR, bounded | 35/35 local entries and 66/66 commands inventory clean. Fresh external re-fetch is explicitly out of scope. |
| 3. Hallucinated result | CLEAR | Every new headline has a named record/source and Appendix-A producing command; incomplete LLM output is excluded. |
| 4. Shortcut reliance | CLEAR with disclosed residual | Single-symbol random-floor degeneracy is fixed; Sybil gaming is a known gap; pretrained-model contamination is restricted to advisory backtests (`sections/03-benchmark.tex:39`). |
| 5. Bug reframed as insight | CLEAR | The paper explicitly reports units, documentation, public-verifiability, and floor corrections (`sections/07-limitations.tex:15`). |
| 6. Methodology fabrication | CLEAR, bounded | Counts/configuration are in JSONL records and Appendix A; the arena state agrees with prose; no unrun field/future result is described as completed. |
| 7. Frame-lock | CLEAR with limitations | Contribution is scoped to protocol/kernel; eligibility is separated from deployability; effective panel count and open experiments are disclosed. |

## Deterministic rerun inventory

```bash
python3 - <<'PY'
import re
from pathlib import Path
bib = Path('paper/refs.bib').read_text()
keys = re.findall(r'^@\\w+\\{([^,]+),', bib, re.M)
tex = '\\n'.join(p.read_text() for p in list(Path('paper/sections').glob('*.tex')) + [Path('paper/main.tex')])
contexts = re.findall(r'\\\\cite\\w*\\{([^}]*)\\}', tex)
used = [k.strip() for c in contexts for k in c.split(',')]
print(len(keys), len(contexts), sorted(set(used)-set(keys)), sorted(set(keys)-set(used)))
PY
python3 - <<'PY'
import json
rows = [json.loads(x) for x in open('paper/evidence/final/luck-floor-1000.jsonl') if x.strip()]
for dataset in sorted({x['dataset'] for x in rows if x['record'] == 'agent'}):
    r = [x for x in rows if x['record'] == 'agent' and x['dataset'] == dataset]
    print(dataset, len(r), max(x['dsr_field'] for x in r), sum(x['rank_eligible_field'] for x in r))
PY
```

No manuscript or source was edited by this audit; this report is the sole write.
