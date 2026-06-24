<!-- prettier-ignore -->
<div align="center">

# SharpeBench

### The luck-robust benchmark for AI trading agents

*Other leaderboards rank the luckiest run over one quarter. SharpeBench ranks the skill that survives deflation — and proves it forward.*

[![Crates.io](https://img.shields.io/crates/v/sharpebench?style=flat-square&logo=rust&color=DEA584&label=crates.io)](https://crates.io/crates/sharpebench)
[![npm](https://img.shields.io/npm/v/@general-liquidity/sharpebench?style=flat-square&logo=npm&color=CB3837)](https://www.npmjs.com/package/@general-liquidity/sharpebench)
[![docs.rs](https://img.shields.io/docsrs/sharpebench-core?style=flat-square&logo=docsdotrs&label=docs.rs)](https://docs.rs/sharpebench-core)
[![CI](https://img.shields.io/github/actions/workflow/status/general-liquidity/sharpebench/ci.yml?style=flat-square&label=CI)](https://github.com/general-liquidity/sharpebench/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](#license)
[![Unsafe](https://img.shields.io/badge/unsafe-forbidden-success?style=flat-square)](#architecture)

**[Why](#why) · [Quickstart](#quickstart) · [Surfaces](#use-it-from-anywhere) · [What it measures](#what-it-measures) · [Architecture](#architecture) · [Tech stack](#tech-stack) · [References](#methodology--references)**

</div>

---

## Why

Every existing financial-agent benchmark ranks on **raw** risk-adjusted metrics over a single short window and a handful of runs — so the leaderboard mostly measures noise. FinBen reports Sharpe confidence intervals of ±1.08, which makes its rankings statistically indistinguishable. StockBench runs one window, once. QuantBench reports Sharpe across 40 seeds but never deflates it.

**In an AI trading benchmark, the hard part is not measuring return. It is separating skill from luck.** A model that posts a great Sharpe over one quarter has told you almost nothing — the number is dominated by sampling noise, by the number of strategies that were tried, and by hidden risk the linear return series can't see.

SharpeBench adds, as **ranking gates**, the things none of the others have:

1. **Deflated Sharpe / PSR** — deflate the Sharpe by how many agents were tested × track length × return skew/kurtosis (Bailey & López de Prado), plus each agent's *own* declared in-sample trials, so a strategy mined from a thousand private backtests is deflated for that search too.
2. **pass^k reliability** — the agent must clear the bar on *every* seed × window, not on average.
3. **Field-wide significance** — a deterministic stationary bootstrap, White's Reality Check, Hansen's studentized & consistent SPA, and Romano–Wolf step-down; the edge must beat data-snooping, not just noise.
4. **Process discipline** — placing an order that never passed the risk gate, ignoring a drawdown halt, bypassing a deny-list, or **selling tail risk with a naked short-gamma book zeroes the entry**, however good the P&L looks. The edge must also survive a realistic execution-cost profile (typical or worst-case fees / slippage / impact / financing), not just a frictionless fill.
5. **Forward-attestation** — agents commit before the data exists, so there's nothing to overfit, and signed, tamper-evident result chains let anyone verify the board independently of the host. Every run can be captured as a raw-decision **trajectory** and replayed by a separate verifier that recomputes a byte-identical score — a forged trajectory recomputes differently.

Raw return is reported but is **never** the rank key. The composite also *reports* (without gating) alpha/beta attribution, calibration, edge half-life, OOS decay, turnover, Pareto-optimality, conviction-weighted return, cost-efficiency (cost-normalized DSR), rolling-window worst-case Sharpe, selection robustness, and the **Sortino ratio** (downside-only risk) — so a high score is legible, not a black box.

**Contamination & input defenses.** Comparison is restricted to the **shared** instruments a field actually traded, a rediscovery check flags a "novel" strategy that is a cosine-near copy of a known one, held-out datasets can be **sealed** (committed, opened only at scoring), a **canary** tripwire detects post-hoc that a model trained on the scenarios, and a **briefing-neutrality** audit lints the shared information packet for the salience bias that would tilt every agent at once.

> An agent does not rank on raw return. It ranks only if its edge survives deflation, reliability, significance, and process discipline — and it proves all of it forward.

## Status — active (pre-1.0)

All eight crates are implemented, tested, and CI-green (fmt · clippy `-D warnings` · workspace tests · a determinism check · the 7-attack self-audit · a docs build · an npm build/test). The scoring kernel, point-in-time simulator, run harness, forward-attestation, leaderboard, WASM bridge, npm package, MCP server, and CLI all work end-to-end — on synthetic data and on **real frozen datasets** (crypto majors + US equity indices).

**Not yet built** (need external infra or a decision): single-name equity data (a keyed feed), a live / forward public arena with hosting, and the public data-curation protocol. See [docs/PLAN.md](docs/PLAN.md).

## Quickstart

```bash
cargo install sharpebench                                    # the CLI
sharpebench run                                              # reference agents + a luck floor, ranked
sharpebench score suites/example_submissions.json           # rank a JSON field of submissions
sharpebench audit                                           # prove the scorer resists 7 known gaming attacks
sharpebench run --data data/crypto-majors-1d.csv            # run on real crypto-majors daily bars
```

The example field includes a *skilled* agent, a *lucky* agent with a **higher raw return**, and a *process-violating* agent. The skilled agent ranks first; the other two are ineligible — which is the whole point. `run` adds a **luck floor** of random "monkey" agents so you can see the zero-skill distribution a real edge must clear.

## Use it from anywhere

One kernel, scored identically across every surface — the internal eval and the public benchmark can't drift.

| Surface | Get it | What it is |
|:--|:--|:--|
| <img height="14" align="top" src="https://cdn.simpleicons.org/rust/DEA584" />&nbsp; **Rust crate** | `cargo add sharpebench-core` | The pure scoring kernel — deterministic, `#![forbid(unsafe_code)]`. |
| <img height="14" align="top" src="https://cdn.simpleicons.org/gnubash/4EAA25" />&nbsp; **CLI** | `cargo install sharpebench` | `run` / `score` / `audit` / `sign` / `verify` / `greeks` / … |
| <img height="14" align="top" src="https://cdn.simpleicons.org/npm/CB3837" />&nbsp; **npm** | `npm i @general-liquidity/sharpebench` | Typed JS/TS API over the WASM kernel — `score`, `greeks`, `selfAudit`. |
| <img height="14" align="top" src="https://cdn.simpleicons.org/modelcontextprotocol" />&nbsp; **MCP** | `npx -y @general-liquidity/sharpebench-mcp` | An [MCP](https://modelcontextprotocol.io) server — agents call the kernel as tools. |
| <img height="14" align="top" src="https://cdn.simpleicons.org/webassembly/654FF0" />&nbsp; **WASM** | `sharpebench-wasm` | The wasm-bindgen bridge the npm package and Gordon (Bun) embed. |

```ts
import { score, greeks } from "@general-liquidity/sharpebench";

const board = score(submissions);   // ranked CompositeScore[] — raw return never buys rank
greeks({ spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0.2, is_call: true }).price; // 10.45
```

### CLI commands

| Command | What it does |
|:--|:--|
| `run` (+ `--data <csv>`, `--http`/`--cmd`) | Run agents through the point-in-time sim and rank them; `--http`/`--cmd` drives **your** external agent into the field. |
| `score <subs.json>` | Rank a JSON field of pre-computed submissions. |
| `audit` | Self-audit: fire 7 known gaming attacks at the scorer; non-zero exit if any survives. |
| `stress` | Run the adversarial stress suite (flash-crash / whipsaw), contamination-masked. |
| `commit` · `sign` · `verify` | Forward-attestation: pre-register a digest, sign a board, verify its chain. |
| `capture` · `verify-trajectory` | Capture an agent's raw-decision trajectory, then replay it to recompute the score. |
| `audit-briefing` · `canary` | Audit a shared briefing for salience bias; derive a do-not-train contamination tripwire. |
| `score-allocation` · `greeks` | Score a weight-vector trajectory (turnover); price an option + Greeks + tail-risk. |

Add `--json` to any command for machine-readable output.

### Bring your own agent

Agents are external and language-agnostic — implement the tiny JSON contract (`MarketObservation` → `Decision`) over either transport, then rank yourself into the field:

```bash
sharpebench run --cmd "cargo run -q -p reference-agent"   # stdio subprocess
sharpebench run --http 127.0.0.1:8080                     # HTTP POST /decide
```

A runnable reference agent (stdio + Dockerfile) and the wire format live in [`examples/reference-agent/`](examples/reference-agent/).

> **Security — running untrusted agents.** The harness executes whatever agent you point it at **without sandboxing**. Only run agents you trust. Safe hosting of third-party submissions is a Phase-2 item and is **not yet built**.

## What it measures

An agent is **rank-eligible only if every gate holds**; eligible agents then sort by the rank key (Deflated Sharpe).

| Gate | Demands | Defeats |
|:--|:--|:--|
| **Deflated Sharpe / PSR** | edge survives deflation for trials × length × skew/kurtosis | data-snooping, lucky search |
| **pass^k** | clears the bar on *every* seed × window | one-lucky-seed wins |
| **Significance** | beats bootstrap + Reality Check + SPA + step-down | multiple-testing false positives |
| **Process** | zero block-severity trace violations | gate-bypass, naked tail-selling, manipulation |
| **Mandate** | respected the drawdown cap | blowing risk to chase return |

Reported but never gating: Sortino + downside deviation, rolling worst-case Sharpe, selection robustness, alpha/beta, calibration, edge half-life, OOS decay, turnover, Pareto-optimality, cost-normalized DSR. Full methodology: the [mdBook](docs/book/) (`mdbook serve docs/book`).

## Data

The benchmark runs on **frozen, checksummed, point-in-time** datasets — no live API in the scoring path, so a score reproduces forever.

| Source | Set | Provides |
|:--|:--|:--|
| <img height="14" align="top" src="https://cdn.simpleicons.org/binance/F0B90B" />&nbsp; Binance | `crypto-majors-1d.csv` | BTC/ETH/SOL/BNB/XRP daily closes (public API, no key) |
| 🏛️ [FRED](https://fred.stlouisfed.org) | `us-indices-1d.csv` | SPX / DJI / IXIC daily closes (public domain) |

Both are fetched and frozen by the offline Rust ingester (`xtask`, `publish = false` — its deps never reach the CLI). The format is long `date,symbol,close[,dividend]`; any aligned dataset works.

```bash
cargo run -p xtask -- crypto                              # re-fetch + re-checksum
sharpebench run --data data/us-indices-1d.csv
```

## Architecture

A Rust [Cargo workspace](Cargo.toml) (modular, à la Paradigm's Rust OSS — reuse any crate on its own). The whole tree is `#![forbid(unsafe_code)]`.

```
sharpebench-core ── the deterministic scoring kernel (no I/O, no ambient RNG)
      │
      ├── sharpebench-protocol   language-agnostic agent ⇄ harness JSON
      ├── sharpebench-sim        point-in-time simulator (look-ahead impossible)
      ├── sharpebench-harness    orchestration across seeds × windows
      ├── sharpebench-attest     SHA-256 commitments + signed chains + sealed data + canary
      ├── sharpebench-leaderboard render / sign / self-describing boards
      ├── sharpebench-wasm       the identical kernel for JS/TS (npm, Gordon, MCP)
      └── sharpebench-cli        the `sharpebench` binary
```

| Crate | Role |
|:--|:--|
| **`sharpebench-core`** | deflated Sharpe / PSR / pass^k / bootstrap + Reality Check + SPA + step-down / process + cost floor / rolling + Sortino / decay / calibration / attribution / selection / comparison-sets / rediscovery / briefing-audit / allocation / options-Greeks / self-audit / composite. Byte-identical scores forever. |
| **`sharpebench-sim`** | fees, seeded slippage, square-root impact, financing, liquidity caps, dividends, execution-cost profiles, adversarial stress paths, trajectory capture/replay. |
| **`sharpebench-attest`** | SHA-256 pre-registration commitments + HMAC signed result chains + time-lock registry + sealed held-out datasets + canary contamination tripwire. |
| **`sharpebench-harness`** | seeds × windows orchestration; luck-floor producers; a runtime-vs-agent failure taxonomy. |
| `protocol` · `leaderboard` · `wasm` · `cli` | the JSON contract · render/sign/self-describing boards · the WASM bridge · the CLI. |

## Tech stack

| Technology | Role |
|:--|:--|
| <img height="14" align="top" src="https://cdn.simpleicons.org/rust/DEA584" />&nbsp; [Rust](https://www.rust-lang.org) | The whole kernel — pure `f64`, fixed reduction order, no `unsafe` |
| <img height="14" align="top" src="https://cdn.simpleicons.org/webassembly/654FF0" />&nbsp; [WebAssembly](https://webassembly.org) | The kernel for non-Rust hosts (`wasm-bindgen`) |
| <img height="14" align="top" src="https://cdn.simpleicons.org/typescript/3178C6" />&nbsp; [TypeScript](https://www.typescriptlang.org) | The typed npm package + MCP server |
| <img height="14" align="top" src="https://cdn.simpleicons.org/npm/CB3837" />&nbsp; [npm](https://www.npmjs.com/package/@general-liquidity/sharpebench) | JS/TS distribution of the scoring kernel |
| <img height="14" align="top" src="https://cdn.simpleicons.org/serde/000000" />&nbsp; serde | Deterministic JSON for every submission, board, and config |
| <img height="14" align="top" src="https://cdn.simpleicons.org/githubactions/2088FF" />&nbsp; GitHub Actions | CI: fmt · clippy · tests · determinism · self-audit · docs · npm |
| <img height="14" align="top" src="https://cdn.simpleicons.org/modelcontextprotocol" />&nbsp; [MCP](https://modelcontextprotocol.io) | Agents call the kernel as tools |
| cargo-deny | Supply-chain gate (advisories · bans · licenses · sources) |

## Methodology & references

The gates are not invented — they are the published, peer-reviewed controls for skill-vs-luck, assembled into one ranking.

| Control | Reference |
|:--|:--|
| Deflated Sharpe & PSR | Bailey & López de Prado, *The Deflated Sharpe Ratio* (2014) |
| Reality Check | White, *A Reality Check for Data Snooping* (2000) |
| Superior Predictive Ability | Hansen, *A Test for Superior Predictive Ability* (2005) |
| Step-down multiple testing | Romano & Wolf (2005) |
| Reliability across runs (pass^k) | Sierra τ²-bench reliability metric |
| Downside risk (Sortino) | Sortino & van der Meer (1991) |
| Options Greeks | Black–Scholes–Merton (1973) |

Full derivations in the [mdBook](docs/book/): [methodology](docs/book/src/methodology.md) · [integrity](docs/book/src/integrity.md) · [attestation](docs/book/src/attestation.md).

## Governance

Hosted by [General Liquidity](https://github.com/general-liquidity) to start, with a roadmap to neutral governance. Credibility comes from **forward-attestation + signed, independently-verifiable results**, not from trust in the host — and Gordon (GL's agent) competes on the board like any other entrant. The neutral home may already exist: the FINOS-governed [Open FinLLM Leaderboard](https://huggingface.co/spaces/finosfoundation/Open-Financial-LLM-Leaderboard) covers the financial-*knowledge* axis but has **no trading-performance axis** — SharpeBench is positioned to be the skill-vs-luck *trading* track it lacks. See **[docs/GOVERNANCE.md](docs/GOVERNANCE.md)**.

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

---

<div align="center">
<sub><em>Skill that survives deflation — and proves it forward.</em></sub>
</div>
