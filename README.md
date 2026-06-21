<div align="center">

# SharpeBench

**The luck-robust benchmark for AI trading agents.**

Ranks agents on risk-adjusted *skill that survives deflation* — not the luckiest run over one quarter.

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Status](https://img.shields.io/badge/status-Phase%200-orange.svg)](docs/PLAN.md)

</div>

---

## Why

Every existing financial-agent benchmark ranks on **raw** risk-adjusted metrics over a single short window and a handful of runs — so the leaderboard mostly measures noise. (FinBen reports Sharpe confidence intervals of ±1.08, which makes its rankings statistically indistinguishable. StockBench runs one window, once. QuantBench reports Sharpe across 40 seeds but never deflates it.)

SharpeBench adds, as **ranking gates**, the things none of them have:

1. **Deflated Sharpe / PSR** — deflate the Sharpe by how many agents were tested × track length × return skew/kurtosis (Bailey & López de Prado).
2. **pass^k reliability** — the agent must clear the bar on *every* seed × window, not on average.
3. **Field-wide significance** — a deterministic stationary bootstrap; the edge must beat noise.
4. **Process discipline** — placing an order that never passed the risk gate, ignoring a drawdown halt, or bypassing a deny-list **zeroes the entry**, however good the P&L looks.
5. **Forward-attestation** *(Phase 2)* — agents commit before the data exists, so there's nothing to overfit and anyone can independently verify the signed result.

Raw return is reported but is **never** the rank key.

> Other leaderboards rank the luckiest run over one quarter. SharpeBench ranks the skill that survives deflation — and proves it forward.

## Status — Phase 0

The scoring kernel (`sb-core`) and the `sharpebench score` CLI are implemented and tested. The point-in-time simulator, agent protocol harness, forward-attestation, and public leaderboard are scaffolded and land next — see [docs/PLAN.md](docs/PLAN.md).

## Quickstart

```bash
cargo test --workspace          # run the kernel's tests (incl. the luck-demotion proof)
cargo run -p sb-cli -- score suites/example_submissions.json
```

The example field includes a *skilled* agent, a *lucky* agent with a **higher raw return**, and a *process-violating* agent. The skilled agent ranks first; the other two are ineligible — which is the whole point.

## Architecture

A Rust [Cargo workspace](Cargo.toml) (modular, à la Paradigm's Rust OSS — reuse any crate on its own):

| Crate | Role |
|---|---|
| **`sb-core`** | the deterministic scoring kernel — deflated Sharpe / PSR / pass^k / bootstrap significance / process / decay / calibration / composite. `#![forbid(unsafe_code)]`, no I/O, no ambient RNG → byte-identical scores forever. |
| **`sb-protocol`** | the language-agnostic agent ⇄ harness JSON protocol (any-language agents compete). |
| **`sb-wasm`** | WASM bindings so Gordon (TypeScript) runs the *identical* scorer — internal eval and public benchmark can't drift. |
| `sb-sim` · `sb-harness` · `sb-attest` · `sb-leaderboard` · `sb-cli` | point-in-time sim · run orchestration · forward-attestation · leaderboard · CLI. |

## Governance

Hosted by [General Liquidity](https://github.com/general-liquidity) to start, with a roadmap to neutral governance. Credibility comes from **forward-attestation + signed, independently-verifiable results**, not from trust in the host — and Gordon (GL's agent) competes on the board like any other entrant.

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
