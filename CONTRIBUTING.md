# Contributing to SharpeBench

SharpeBench aims to be the neutral, reproducible standard for evaluating AI
trading agents. Contributions that strengthen its rigor, realism, or
verifiability are very welcome.

## Ground rules

- **Determinism is sacred.** `sharpebench-core` must stay pure: no I/O, no system clock,
  no ambient randomness (pass an explicit seed). Changes that alter a published
  score must be deliberate, documented, and versioned. Two committed Rust
  goldens are checked on Linux, macOS, and Windows; do not generalize that
  evidence to every platform and toolchain.
- **`#![forbid(unsafe_code)]`** at all twelve workspace package roots under
  `crates/`. The published PyO3 binding is excluded from the workspace and is
  the disclosed exception because its generated FFI glue expands to unsafe
  operations.
- **Tests with the math.** New scoring logic ships with unit tests, including a
  case that demonstrates it resists gaming (see `composite.rs` for the pattern).

## Before you push

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

## Standing CI legs

[`ci.yml`](.github/workflows/ci.yml) is the release gate: fmt, clippy, rustdoc,
the three OS test matrices, the Lean model, byte-identical scores, the live
container boundary, the self-audit, stylized facts, `cargo deny`, the mdBook,
paper provenance and the packaged consumers. Everything below runs on every
pull request as well and is deliberately bounded so it never slows the gate.

[`mutation.yml`](.github/workflows/mutation.yml) carries the two gates the
2026-09-07 audit deferred as "full mutation and paired-boundary gates":

- **cargo-mutants, PR diff only.** `cargo mutants --in-diff` over the diff
  between the pull request and its merge base, restricted to the four pure
  crates (`sharpebench-core`, `sharpebench-stats`, `sharpebench-protocol`,
  `sharpebench-attest`), 30 minute job budget, 3x the baseline test time per
  mutant. A missed mutant fails the job; timeouts are warnings with the
  `mutants-out` artifact naming them. A PR that changes no Rust in those crates
  is a no-op. Exclusions live in [`.cargo/mutants.toml`](.cargo/mutants.toml),
  each with its reason. Reproduce locally with
  `git diff --merge-base main HEAD > /tmp/pr.diff && cargo mutants --in-diff /tmp/pr.diff -p sharpebench-core -p sharpebench-stats -p sharpebench-protocol -p sharpebench-attest`.
- **Paired boundaries.** `python scripts/check-paired-boundaries.py` scans the
  public functions of `sharpebench-core` and `sharpebench-stats` whose doc
  comment states a domain for a numeric input (finite, non-negative, in (0, 1),
  alpha, confidence, probability) and fails unless a boundary test names each
  one. A boundary test is a `#[test]` whose name contains `boundary` or
  `boundaries`, or any test in a file whose name does
  (`tests/greeks_boundaries.rs`, `tests/r02_statistical_boundaries.rs`). New
  domain-documented functions need such a test. The `ALLOWLIST` in the script
  holds the functions that were uncovered when the gate landed and only
  shrinks: the gate fails on a row whose function is now covered or gone.

## Scope

See [docs/PLAN.md](docs/PLAN.md) for the current roadmap. Before proposing a
new kernel primitive, search the workspace and the documentation map: the
statistics kernel already includes Reality Check, Hansen SPA, step-down tests,
stationary bootstrap, and cost-aware simulation. Useful contributions include
adversarial regression fixtures, additional reference entrants implemented
against `sharpebench-protocol`, clearer diagnostics, and source-backed dataset
or documentation corrections. Dataset additions also need provenance,
licensing, and a deprecation policy.

## License

By contributing you agree your work is dual-licensed under MIT OR Apache-2.0.
