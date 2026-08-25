# Final-integrity addendum — 2026-08-25 post-repair

**Status: PASS for the frozen v0.9.0 release candidate, subject to the CI and
registry checks recorded after commit.** This addendum supersedes the numerical
and statistical verdicts in the earlier 2026-08-25 integrity report. It retains
the earlier report only as an audit-history record.

## Statistical repairs verified

| Surface | Repair | Frozen result |
|---|---|---|
| Pooled inference | The scorer averages aligned execution-seed returns within each frozen market window before concatenating windows for PSR, DSR, the stationary bootstrap and pooled diagnostics. The seeds remain separate only for per-run $\mathrm{pass}^{k}$. Incomplete or unequal blocks fail closed. | Eight executions of a 409-bar market path contribute 409 temporal observations, not 3,272 pseudo-independent copies. Every score stamps the seed width and pooled-observation count. |
| Deflation units and null | Every configured annualized dispersion is divided by `sqrt(periods_per_year)` exactly once. Measured per-period dispersion is never reconverted. The expected-maximum formula carries an explicit zero-Sharpe null mean; the field mean is an attribution device, not the null. | Daily FX, rates and hourly crypto use measured annualized dispersions 2.9985, 1.5095 and 12.0067, corresponding to annualized deflation bars 6.8254, 3.4362 and 27.3309. Other default cells use the configured or measured-and-floored 0.5 annualized value, with bar 1.1382. |
| Witness circularity | A separate five-agent zero-edge calibration field fixes the witness's deflation dispersion before the candidate edge sweep. The witness cannot vote on its own bar. | Eligibility first appears at injected per-period Sharpe 0.35 on the weekly geometry and 0.20 on the daily geometry. DSR and $\mathrm{pass}^{k}$ co-bind at the sampled weekly onset; $\mathrm{pass}^{k}$ binds daily. |
| Field-wide tests | Reality Check, SPA and Romano--Wolf use a fixed aligned buy-and-hold benchmark when present and a stamped cash null otherwise. They do not use the cost-bleeding field mean as their hypothesis benchmark and do not gate rank. | The serialized values are diagnostics with a declared null; no eligibility statement relies on them. |
| Mandate declaration | The typed declaration changes the question the verdict answers without relaxing DSR, bootstrap, process or applicable drawdown gates. Field-relative declarations fail closed when verified without their aligned benchmark field. | Zero of 36 declared reference rows meets its declared mandate. Daily-crypto risk-managed is the nearest case: it satisfies one declared reliability question but remains ineligible. |
| Execution noise | An opt-in profile adds deterministic fill delays, partial fills with carry and queue-position slippage; the default path is byte-identical. | Mean across-seed annualized-Sharpe dispersion rises by roughly two orders of magnitude on three daily panels, but every reference-agent refusal remains a window-leg refusal on this grid. |

## Frozen evidence checked

- The nine default evidence files contain 512 records each, 4,608 in total.
  No reference agent is eligible in any recorded configuration.
- Default leading DSRs are 0.178150 (weekly-US buy-and-hold), 0.240632
  (weekly-crypto momentum), 0.250456 (daily-crypto buy-and-hold), 0.155789
  (daily-US buy-and-hold) and 0.323041 (4-hour-crypto buy-and-hold); all remain
  below 0.95 and fail the applicable regime verdict.
- Weekly-US risk-managed has DSR 0.026158, PSR 0.895755, stationary-bootstrap
  p-value 0.071464 and worst-run drawdown 0.11325. It fails multiple gates; no
  current text calls it a deflation-only refusal.
- The 1,000-agent daily-crypto floor reaches DSR 0.250029 only in the expressly
  unfloored diagnostic. Under the shipped dispersion floor its maximum is
  0.001190; all 2,000 zero-skill agent--dataset rows are ineligible.
- Clone collapse uses the dedicated 0.995 threshold. A regression reconstructs
  every committed field and finds no honest merge; the adversarial near-clone
  field still reproduces and the audit reports all nine attacks demoted.
- Window records 001 and 002 are explicitly superseded historical pre-entry
  records. No forward window or result is claimed as current.

## Mathematical and manuscript checks

1. The displayed PSR and deflated-Sharpe formulas match the kernel term by term,
   including non-excess fourth moment, the $\sqrt{n-1}$ factor and the recorded
   null mean. Bailey and López de Prado's worked example is cited at the pages
   where annualized trial Sharpes are converted to the daily scale before PSR.
2. Every table value and symbol is explicit; there are no ditto cells. Dataset
   bar counts are per symbol, and the table reports 2,513 daily-US and 1,000
   daily-crypto bars rather than mixed total-row counts.
3. The manuscript defines bull/bear/chop as equal-weight window returns above
   +3 percent, below -3 percent and otherwise. It states that the fixed symbol
   universes do not close survivorship bias and that the raw Treasury-yield
   panel is a simulator diagnostic, not a tradable bond-return series.
4. All local labels, equation references, citations, figures and section inputs
   resolve. No literal-tab TeX damage, em dash or ditto-style table cell remains.
5. Paid-model failures are excluded from result evidence. The paper reports no
   LLM or learned-agent result.

## Verification record

- `cargo fmt --all --check`: clean.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`: clean.
- `cargo test --workspace`: zero failures across all workspace suites and
  doctests; native/WebAssembly parity and golden fixtures pass.
- Excluded Python crate: Rust fmt/clippy clean; a fresh editable binding build
  passes 50/50 pytest tests.
- npm wrapper: build succeeds and 9/9 tests pass.
- `cargo deny check` and the excluded-Python dependency audit: clean.
- `sharpebench audit`: 9/9 attacks demoted.
- `sharpebench arena verify arena --pubkey <pinned key>`: valid chain with zero
  published windows.
- Native LaTeX build: zero errors, zero undefined references and zero overfull
  boxes; PDF text extraction confirms the title and current headline values.

## Deliberate limitations, not integrity failures

- All reported simulations use the typical cost profile; no frictionless or
  stressed-profile sensitivity table is claimed.
- The fixed universes carry survivorship selection, and the rates row is a raw
  yield-level diagnostic.
- No real or learned agent has attained eligibility; the paid LLM field remains
  unrun and excluded.
- No forward window is currently open, and identity governance remains outside
  the implemented statistical defenses.
