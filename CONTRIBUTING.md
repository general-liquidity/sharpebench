# Contributing to SharpeBench

SharpeBench aims to be the neutral, reproducible standard for evaluating AI
trading agents. Contributions that strengthen its rigor, realism, or
verifiability are very welcome.

## Ground rules

- **Determinism is sacred.** `sharpebench-core` must stay pure: no I/O, no system clock,
  no ambient randomness (pass an explicit seed). A given input must produce a
  byte-identical score on every platform, forever. Changes that alter a published
  score must be deliberate, documented, and versioned.
- **`#![forbid(unsafe_code)]`** in `sharpebench-core`, `sharpebench-sim`, and `sharpebench-protocol`.
- **Tests with the math.** New scoring logic ships with unit tests, including a
  case that demonstrates it resists gaming (see `composite.rs` for the pattern).

## Before you push

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

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
