# Versioned stressed decision delay

Status: implementation candidate on `feat/stressed-execution-delay`, based on
main `8484a76`. Owner approved actual delay on new stressed runs, preserving
historical evidence. Delivery and final committed-tree verification are pending.

## Contract

- New `CostProfile::WorstCase` models carry `fixed_delay` tagged
  `sharpebench.fixed-decision-delay.v1`, with `bars: 2`.
- A submission at dataset bar `t` becomes eligible at `t + 2`. FIFO means a
  later submission does not cancel a waiting decision. Prices, NAV, costs and
  existing execution-noise draws are taken at eligibility, not submission.
- Existing noise can defer or partially fill an eligible order. Its carried
  remainder remains subject to existing cancel/replace behavior when a later
  decision becomes eligible, not when that decision is submitted.
- Beyond-window eligibility never fills. Raw trajectory and compute accounting
  still retain those submissions. Invalid hard targets and duplicate-symbol
  orders emit violations at submission, even at the tail.
- Checkpoint state contains queued decisions; reset clears them. Fixed timing
  enters the execution digest. Strict replay refuses a different delay setting.
- An absent setting retains immediate execution, legacy serialized bytes and
  legacy digests. Unknown timing versions are refused. Default, frictionless,
  typical and realistic profiles acquire no fixed delay.
- Replay diagnostic lag is additional to the model's fixed timing. The
  diagnostic's undelayed row means no extra replay lag.

## Verification receipts so far

`crates/sharpebench-sim/tests/fixed_delay.rs` exercises FIFO and eligibility
prices, nonzero window start, tail capture/billing, replay/environment parity,
JSON checkpoint/restore and reset, submission guards, additional noise delay,
legacy serialization and version refusal, actual stressed-profile execution,
and additive diagnostic lag. All seven pass from a fresh candidate-only build.
The complete simulator and protocol suites pass, including the synthetic input
golden; the harness suite passes with its documented slow/installed-shim cases
left ignored. The historical stressed digest fixture explicitly disables fixed
timing, preserving its old pin rather than repinning it to a new profile.

`fixed_delay_is_bound_to_capture_identity_without_changing_legacy_digest` in
the harness reconstructs the legacy digest preimage and verifies both same-model
acceptance and different-delay refusal.

Isolated source mutations in a detached checkout based on `8484a76`:

| Mutation | Observed result |
| --- | --- |
| Force the engine delay to zero | Four timing/replay/checkpoint/noise tests failed |
| Remove submission-time violation events | Invalid-tail test failed with an empty event list |
| Omit timing from cost identity | Harness identity test failed because the digests matched |
| Omit fixed timing from resolved stressed model | Version/profile test failed: `None` versus `Some(Fifo { bars: 2 })` |
| Force immediate execution for the additive-lag diagnostic | Opening exclusion was three bars instead of five; test failed |

The production source was never mutated. The detached source was restored from
the candidate. Sharing a Cargo output directory initially caused a Windows
executable-lock failure and later reused a mutant executable. These runs are
not final candidate evidence. Final checks use a fresh candidate-only target
directory. The restored isolated seven timing tests and harness identity test
also pass after forcing recompilation. Copying source back had retained old
timestamps, which let Cargo reuse a newer mutant artifact; the restored source
files were touched before this control rebuild.

Local commands with successful exit codes:

```text
cargo test --locked -p sharpebench-sim -p sharpebench-protocol
cargo test --locked -p sharpebench-harness
cargo clippy --locked -p sharpebench-sim -p sharpebench-harness -p sharpebench-protocol -p sharpebench --all-targets --all-features -- -D warnings
cargo fmt --all --check
python scripts/check-paired-boundaries.py
```

The final CLI suite, provenance, exact pushed-head CI and main-tree verification
are still pending. These working-tree receipts do not claim delivery.

## Distribution and evidence boundaries

The changed simulator is a Rust execution surface. The Python and WASM crates
depend on scoring/statistics crates, not `sharpebench-sim`, and expose no affected
cost-profile route. No rebuilt scoring bundle or Python wheel is claimed for
this execution change. A separate debug CLI install from this candidate, under
an owned temporary prefix, scored `suites/example_submissions.json --json` and
matched the committed teaching golden after JSON normalization. This checks CLI
compatibility, not an empirical execution result. The CLI has no `--version`
command; an attempted version smoke check was refused and was not counted as a
passing check.

The paper's repair appendix states the current-versus-frozen timing distinction.
Frozen empirical records are not regenerated, and no delay effect on a measured
entrant is asserted. No new experiment, model call, release, tag or Arena
registry-pin update is included.
