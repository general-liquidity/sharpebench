# Contributing to SharpeBench

SharpeBench aims to be the neutral, reproducible standard for evaluating AI
trading agents. Contributions that strengthen its rigor, realism, or
verifiability are very welcome.

## Ground rules

- **Determinism is sacred.** `sb-core` must stay pure: no I/O, no system clock,
  no ambient randomness (pass an explicit seed). A given input must produce a
  byte-identical score on every platform, forever. Changes that alter a published
  score must be deliberate, documented, and versioned.
- **`#![forbid(unsafe_code)]`** in `sb-core`, `sb-sim`, and `sb-protocol`.
- **Tests with the math.** New scoring logic ships with unit tests, including a
  case that demonstrates it resists gaming (see `composite.rs` for the pattern).

## Before you push

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

## Scope

See [docs/PLAN.md](docs/PLAN.md) for the phased roadmap. Good first areas:
significance tests (Hansen SPA), cost/slippage models in `sb-sim`, additional
process-discipline checks, and reference agents implementing `sb-protocol`.

## License

By contributing you agree your work is dual-licensed under MIT OR Apache-2.0.
