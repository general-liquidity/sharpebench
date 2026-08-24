# Stage 4.5 Final Integrity Report — 2026-08-25

## Verdict

**PASS for the current, explicitly bounded claims.** This supersedes the
2026-08-24 report. It does not certify an unrun LLM field, a future forward
result, private search-budget declarations, or external trading performance.

## Corrections re-audited

| Finding | Resolution verified |
|---|---|
| Deflation units and domain | The annualized cross-trial dispersion is divided by `sqrt(periods_per_year)` exactly once. The paper identifies this as an IID annualization approximation. The expected-maximum-Sharpe implementation defines the `N <= 1` convention and floors the finite-sample radicand before its square root. |
| PSR parameterization | The kernel and manuscript use the same normal approximation, per-period Sharpe, sample skewness, and non-excess kurtosis. The paper does not call a marginal 0.90 per-run PSR conjunction a simultaneous 90% confidence statement. |
| Observable trial count | Ranking uses `N_eff = max(N_host, N_field) + N_declared`. A regression test proves that a host configured with `N=1` cannot score an eight-entry field below eight observable trials; direct single-agent scoring is explicitly field-blind. |
| Measured dispersion | Near-clone clusters cast one dispersion vote; the annualized 0.5 lower bound prevents distinct low-dispersion submissions from relaxing the prior. All visible entries still count toward the observed trial floor and remain on the board. |
| Sybil audit | With clone collapse off, 200 puppets move measured dispersion 0.3258 -> 0.0559 and the real agent's DSR 0.0000 -> 0.9522, flipping eligibility. With collapse on, dispersion is 0.3018, DSR is 0.0000, effective `N` is 207, and the agent is refused; 199/199 duplicates are flagged and 200/200 remain visible. |
| Nine-dataset sweep | Every final file has 512 rows: 4,608 records total. The default table was recomputed from the current source. Its only nonzero leading DSRs are 0.004596 weekly US, 0.023492 weekly crypto, and 0.028517 daily crypto; no reference agent is eligible. |
| Risk-managed control | The weekly-US DSR at effective `N=7/10/25/50/100` is 0.025488/0.003481/5.966e-6/2.024e-8/4.024e-11. The record serializes the effective count and the measured/floored dispersion source. Under the default verdict it also fails pass^k; under the never-catastrophic verdict deflation is the remaining refusal. |
| Synthetic witness | One noise realization is reused across the edge grid. The producer aborts if witness DSR decreases or eligibility closes after opening. Eligibility begins at per-period Sharpe 0.35 weekly (annualized 2.52) and 0.20 daily (3.17). DSR and pass^k co-bind on the sampled weekly grid; pass^k binds on the daily grid, where DSR clears at 0.10. |
| Thousand-agent floor | Both daily fields are scored at the observable 1,000-trial footprint. The deliberately unfloored daily-crypto diagnostic reaches DSR 0.030192 (0.007510 among the first five streams in the same context); the shipped 0.5 annualized floor makes both operational maxima exactly zero. All 2,000 agent-dataset cells are ineligible, and no random agent beats the best reference raw return. |
| Forward configuration | Window 001 carries a schema version, every score-config field explicitly, and a digest over the typed score configuration. Loading rejects missing or extra score fields instead of filling later defaults. The window is open with zero published results; the scheduler does not claim to perform the future reveal or scoring operation. |
| Evidence provenance | `paper/evidence/provenance.json` hashes the source/data/arena snapshot and every admitted JSONL and figure artifact. Incomplete paid-model caches and records are explicitly excluded; no provider call was made in this correction pass. |

## Verification record

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo test --workspace`: every Rust, native/Wasm parity, golden, integration,
  and doctest passed; zero failed.
- `sharpebench audit`: all 9 attacks defended.
- `sharpebench arena verify arena --pubkey <pinned key>`: valid open chain with
  zero published windows.
- Nine evidence sweeps: 9 x 512 rows; risk control: 100 rows; pass witness: 156
  rows; thousand-agent floor: 2,002 rows.
- `latexmk -pdf -jobname=finalcheck -interaction=nonstopmode -halt-on-error
  main.tex`: 30 pages, zero TeX errors, zero undefined references, zero overfull
  boxes.
- PDF text check: corrected title and the values 0.0302, 0.9522, and the 0.35
  witness onset render; stale values 0.047, 0.9756, and 0.45 do not.
- TeX source sweep: no literal-tab-stripped command pattern and no ditto-style
  table cells.

## Remaining named research, not defects in this candidate

The paid LLM field remains unrun for lack of API credits, and the first forward
window has no result before its reveal. Private in-sample trial declarations
remain unverifiable without a preregistered search ledger. These limits are
stated as limits, not silently promoted to protocol guarantees.
