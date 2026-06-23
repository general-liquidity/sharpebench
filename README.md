<div align="center">

# SharpeBench

**The luck-robust benchmark for AI trading agents.**

Ranks agents on risk-adjusted *skill that survives deflation* — not the luckiest run over one quarter.

[![Crates.io](https://img.shields.io/crates/v/sharpebench.svg)](https://crates.io/crates/sharpebench)
[![docs.rs](https://img.shields.io/docsrs/sharpebench-core)](https://docs.rs/sharpebench-core)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Status](https://img.shields.io/badge/status-active%20(pre--1.0)-brightgreen.svg)](docs/PLAN.md)
[![Unsafe](https://img.shields.io/badge/unsafe-forbidden-success.svg)](#architecture)

</div>

---

## Why

Every existing financial-agent benchmark ranks on **raw** risk-adjusted metrics over a single short window and a handful of runs — so the leaderboard mostly measures noise. (FinBen reports Sharpe confidence intervals of ±1.08, which makes its rankings statistically indistinguishable. StockBench runs one window, once. QuantBench reports Sharpe across 40 seeds but never deflates it.)

SharpeBench adds, as **ranking gates**, the things none of them have:

1. **Deflated Sharpe / PSR** — deflate the Sharpe by how many agents were tested × track length × return skew/kurtosis (Bailey & López de Prado), plus each agent's *own* declared in-sample trials, so a strategy mined from a thousand private backtests is deflated for that search too.
2. **pass^k reliability** — the agent must clear the bar on *every* seed × window, not on average.
3. **Field-wide significance** — a deterministic stationary bootstrap, White's Reality Check, Hansen's studentized & consistent SPA, and Romano–Wolf step-down; the edge must beat data-snooping, not just noise.
4. **Process discipline** — placing an order that never passed the risk gate, ignoring a drawdown halt, or bypassing a deny-list **zeroes the entry**, however good the P&L looks. The edge must also survive a **realistic execution cost** profile (typical or worst-case fees / slippage / impact / financing), not just a frictionless fill.
5. **Forward-attestation** — agents commit before the data exists, so there's nothing to overfit, and signed, tamper-evident result chains let anyone verify the board independently of the host. Every run can be captured as a raw-decision **trajectory** and replayed by a separate verifier that recomputes a byte-identical score — a forged trajectory recomputes differently.

Raw return is reported but is **never** the rank key. It also *reports* (without gating) alpha/beta attribution, confidence calibration, edge half-life, out-of-sample decay, turnover, Pareto-optimality, conviction-weighted return, cost-efficiency (cost-normalized DSR), rolling-window worst-case Sharpe, selection robustness (best-vs-median DSR across an agent's candidate set), and economic-rationality — so a high score is legible, not a black box.

**Contamination defenses.** Comparison is restricted to the **shared** instruments a set of agents actually traded (no winning by picking an easier universe), a rediscovery check flags a "novel" strategy that is really a cosine-near copy of a known one, and held-out datasets can be **sealed** (cryptographically committed, opened only at scoring) so they can't be trained on in advance.

> Other leaderboards rank the luckiest run over one quarter. SharpeBench ranks the skill that survives deflation — and proves it forward.

## Status — active (pre-1.0)

All eight crates are implemented, tested, and CI-green (fmt · clippy `-D warnings` · workspace tests · a determinism check · the self-audit · a docs build). The scoring kernel, point-in-time simulator, run harness, forward-attestation, leaderboard, WASM bridge, and CLI all work end-to-end — on synthetic data and on **real frozen datasets** (crypto majors + US equity indices; see [Data](#data)).

**Not yet built** (need external infra or a decision): single-name equity data (index + crypto data already ship in [`data/`](data/); individual constituents need a keyed feed), a live / forward public arena with hosting, and the public data-curation protocol. See [docs/PLAN.md](docs/PLAN.md).

## Quickstart

```bash
cargo test --workspace                                  # all tests, incl. the luck-demotion proof
cargo run -p sharpebench -- run                              # run reference agents + the luck floor through the sim
cargo run -p sharpebench -- score suites/example_submissions.json   # rank a JSON field of submissions
cargo run -p sharpebench -- audit                           # prove the scorer resists 6 known gaming attacks
cargo run -p sharpebench -- run --data data/crypto-majors-1d.csv   # run on real crypto-majors daily bars
```

The example field includes a *skilled* agent, a *lucky* agent with a **higher raw return**, and a *process-violating* agent. The skilled agent ranks first; the other two are ineligible — which is the whole point. `run` adds a **luck floor** of random "monkey" agents so you can see the zero-skill distribution a real edge must clear.

### CLI commands

| Command | What it does |
|---|---|
| `run` (+ `--data <csv>`, `--http <addr>`/`--cmd "<prog>"`) | Run agents through the point-in-time sim and rank them. `--data` runs on a frozen real-data CSV (else synthetic); `--http`/`--cmd` drives **your** external agent (an HTTP `POST /decide` endpoint, or a stdio subprocess) into the field too. |
| `score <subs.json>` | Rank a JSON field of pre-computed submissions. |
| `stress` | Run the adversarial stress suite (flash-crash / whipsaw), contamination-masked. |
| `audit` | Self-audit: fire 6 known gaming attacks at the scorer; non-zero exit if any is not demoted. |
| `commit <agent> <window> <digest> <salt>` | Forward-attestation pre-registration commitment. |
| `sign <subs.json> <key> <out.json>` | Score + sign a board to a tamper-evident file. |
| `verify <board.json> <key>` | Verify a signed board's chain. |
| `capture <agent> <out.json> [--data <csv>]` | Run an agent and capture its raw per-seed×window **decision trajectory** to JSON. |
| `verify-trajectory <traj.json> [--data <csv>]` | Replay a captured trajectory through the sim and recompute its score from the raw decisions — a forged trajectory recomputes to a different number. |

Add `--json` to any command for machine-readable output (structured JSON instead of the human table) — for agents, CI, or a leaderboard front-end.

### Bring your own agent

Agents are external and language-agnostic — implement the tiny JSON contract (`MarketObservation` → `Decision`) over either transport, then rank yourself into the field alongside the references:

```bash
cargo run -p sharpebench -- run --cmd "cargo run -q -p reference-agent"             # stdio subprocess
cargo run -p sharpebench -- run --http 127.0.0.1:8080                               # HTTP POST /decide
```

A runnable reference agent (stdio + a Dockerfile) and the full wire format live in [`examples/reference-agent/`](examples/reference-agent/).

> **Security — running untrusted agents.** The harness executes whatever agent you point it at (a subprocess, or an HTTP endpoint) **without sandboxing**. Only run agents you trust. Hosting third-party submissions safely — container isolation, CPU / memory / time limits, no network egress — is a Phase-2 item and is **not yet built**.

## Data

The benchmark runs on **frozen, checksummed, point-in-time** datasets — no live API in the scoring path, so a score reproduces forever. A real **crypto-majors** set ships in [`data/`](data/) (BTC/ETH/SOL/BNB/XRP daily closes from Binance's public API), fetched and frozen by the offline Rust ingester (`xtask`, `publish = false` — its deps never reach the CLI):

```bash
cargo run -p xtask -- crypto                                   # re-fetch + re-checksum the dataset
cargo run -p sharpebench -- run --data data/crypto-majors-1d.csv
```

A real **US equity-index** set ships too — SPX / DJI / IXIC daily closes from FRED (public domain):

```bash
cargo run -p sharpebench -- run --data data/us-indices-1d.csv
```

The format is long `date,symbol,close[,dividend]`; any aligned dataset works. Next source: single-name equities (a keyed feed). See [`data/README.md`](data/README.md).

## Architecture

A Rust [Cargo workspace](Cargo.toml) (modular, à la Paradigm's Rust OSS — reuse any crate on its own). The whole tree is `#![forbid(unsafe_code)]`.

| Crate | Role |
|---|---|
| **`sharpebench-core`** | the deterministic scoring kernel — deflated Sharpe / PSR (incl. in-sample-trial deflation) / pass^k / bootstrap + Reality Check + SPA + step-down significance / process + cost-normalized floor / rolling-Sharpe / decay / calibration / attribution / roles / OOS-decay / economic-rationality / selection-robustness / benchmark-comparison-sets / rediscovery / self-audit / composite. No I/O, no ambient RNG, fixed float reduction → byte-identical scores forever. |
| **`sharpebench-protocol`** | the language-agnostic agent ⇄ harness JSON protocol (any-language agents compete), including the captured-trajectory artifact and per-order decision rationale. |
| **`sharpebench-sim`** | point-in-time simulator (look-ahead is structurally impossible) with fees, seeded slippage, square-root market impact, financing, liquidity/partial-fill caps, dividends, selectable execution-cost profiles (none / typical / worst-case) + decision delay, adversarial stress paths, trajectory capture/replay, and reference + team + random agents. |
| **`sharpebench-harness`** | run orchestration across seeds × windows; team harness + role attribution; luck-floor and economic-rationality producers; a runtime-vs-agent **failure taxonomy** (a crashed container is retried, never charged against pass^k; an agent fault becomes a failing sentinel run). |
| **`sharpebench-attest`** | forward-attestation: SHA-256 pre-registration commitments + HMAC tamper-evident signed result chains + an integer-epoch time-lock registry + sealed held-out datasets (commit / seal / open / verify). |
| **`sharpebench-wasm`** | WASM bindings so Gordon (TypeScript) runs the *identical* scorer — internal eval and public benchmark can't drift. |
| `sharpebench-leaderboard` · `sharpebench-cli` | leaderboard render / sign / persist, incl. **self-describing** boards (the run-spec — dataset hash, costs, config, seeds — is bound into the signed chain) · the `sharpebench` CLI. |

> Crates publish to crates.io as **`sharpebench-*`**; the binary crate is **`sharpebench`** (`cargo install sharpebench`).

## Governance

Hosted by [General Liquidity](https://github.com/general-liquidity) to start, with a roadmap to neutral governance. Credibility comes from **forward-attestation + signed, independently-verifiable results** (`sharpebench-attest`), not from trust in the host — and Gordon (GL's agent) competes on the board like any other entrant.

The neutral home may already exist: the FINOS-governed [Open FinLLM Leaderboard](https://huggingface.co/spaces/finosfoundation/Open-Financial-LLM-Leaderboard) covers the financial-*knowledge* axis (NLP, sentiment, QA, compliance) but has **no trading-performance axis**. SharpeBench is positioned to be the skill-vs-luck *trading* track it lacks — complementary, not competing. See **[docs/GOVERNANCE.md](docs/GOVERNANCE.md)**.

## Documentation

Full methodology — the gates, each significance test, process discipline, the
submission formats, forward-attestation, and the integrity model — is in the
mdBook under [`docs/book/`](docs/book/) (`mdbook serve docs/book`). Design and
governance live in [docs/PLAN.md](docs/PLAN.md) and [docs/GOVERNANCE.md](docs/GOVERNANCE.md); crates.io publishing is in [docs/PUBLISHING.md](docs/PUBLISHING.md).

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
