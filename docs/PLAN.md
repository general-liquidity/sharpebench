# SharpeBench — Build Plan (Rust, Day 1)

> The industry standard for **trustworthy-with-capital** AI trading agents.
> Ranks on luck-robust, risk-adjusted skill — not raw return — with a forward-attested,
> independently-verifiable, single-binary harness.
>
> Separate public repo: **`github.com/general-liquidity/sharpebench`** (Apache-2.0).
> NOT inside Gordon. Gordon is a *competitor on* the board + a *consumer of* the scoring kernel (via WASM).

---

## 0. Why Rust, why a separate repo (the two settled decisions)

- **Separate repo:** a benchmark inside the product it grades = vendor-conflict. Gordon must compete, not host.
- **Rust from Day 1:** a benchmark's #1 property is **reproducibility/verifiability** — the same submission yields the identical score, forever, for anyone. Rust wins that axis (determinism, no GC, reproducible/hermetic builds à la Codex, single static binary). The cross-language drift worry is dissolved by compiling the scoring kernel to **WASM**, which Gordon (Bun) consumes natively → one canonical scorer, zero drift.
- **Agent submissions stay language-agnostic** (container/HTTP) — any agent (Python/TS/Rust/hosted) competes. Only the harness/core/sim are Rust.

## 1. The wedge (what makes it a standard, not another leaderboard)

Every prior benchmark ranks on RAW risk-adjusted metrics over a short window / few runs → "return-rank = luck":
FinBen (SR CIs ±1.08 → ties), QuantBench (mean±std, no deflation, no agent loop), StockBench (CR/MDD/Sortino, single window/run), FinBench (static QA, no trading metrics).

SharpeBench adds, as ranking gates, what none of them have:
1. **Deflated Sharpe / PSR** (Bailey & López de Prado) — deflate by #agents × track length × skew/kurtosis.
2. **pass^k reliability** — skill on *every* seed×window (mode `All`), not on average.
3. **Field-wide significance** — Hansen SPA / White Reality Check across all agents (stronger than per-agent deflation).
4. **Process score** over the decision trace — risk-gate-before-order, concentration cap, drawdown-halt, no deny-list bypass.
5. **Decay / Correlation / Calibration** — edge half-life, crowdedness vs the field, confidence-vs-outcome calibration.
6. **Forward-attestation** — agents commit before the data exists; the un-gameable spine.

Raw return is reported but is **not** the rank key.

## 2. Workspace layout (Cargo workspace, multi-crate — Codex `codex-rs` shape)

```
sharpebench/
├── Cargo.toml                  # [workspace]
├── rust-toolchain.toml         # pinned channel (reproducibility)
├── flake.nix                   # hermetic toolchain (optional but recommended)
├── crates/
│   ├── sharpebench-core/                # THE SCORING KERNEL — pure, deterministic, #![forbid(unsafe_code)]
│   │   ├── types.rs            # Returns, Trajectory, Trace, ProcessEvent, Score, Ranking
│   │   ├── deflated_sharpe.rs  # DSR + PSR
│   │   ├── pass_k.rs           # pass^k (mode All/Any/Threshold)
│   │   ├── significance.rs     # Hansen SPA / White Reality Check (seeded stationary bootstrap)
│   │   ├── process.rs          # process-discipline scoring over a Trace
│   │   ├── decay.rs            # IC/edge half-life
│   │   ├── correlation.rs      # cross-agent correlation / crowdedness
│   │   ├── calibration.rs      # Brier / reliability of stated confidence
│   │   └── composite.rs        # gated composite score + leaderboard ranking
│   ├── sharpebench-wasm/                # wasm-bindgen façade over sharpebench-core (Gordon/Bun consumes this)
│   ├── sharpebench-protocol/            # language-agnostic agent I/O: MarketObservation -> Decision (JSON schema)
│   ├── sharpebench-sim/                 # point-in-time market sim / backtest
│   │   ├── data.rs             # PIT store: exposes only t <= decision_time (look-ahead impossible)
│   │   ├── engine.rs           # bar loop, order matching, fills
│   │   ├── costs.rs            # slippage + market-impact (own-order) + fees
│   │   └── windows.rs          # multi-window OOS, walk-forward, regime tags (bull/bear/chop)
│   ├── sharpebench-harness/             # orchestrate: drive agent over seeds×windows -> capture Trajectories -> score
│   ├── sharpebench-attest/              # pre-registration, time-locked commits, signed verifiable results
│   ├── sharpebench-leaderboard/         # storage + rendering + signed result chain (reuse Gordon's HMAC pattern)
│   └── sharpebench-cli/                 # `sharpebench` single static binary
├── suites/                     # benchmark suite definitions (universe × windows × baselines)
├── data/                       # curated PIT datasets (or checksummed pointers)
├── docs/                       # methodology, protocol spec, governance
└── .github/workflows/          # reproducible CI: pinned toolchain, golden-score tests, SLSA-signed binary
```

## 3. The scoring kernel (`sharpebench-core`) — the crown jewel

**Invariants (this is what makes scores reproducible forever):**
- Pure: no I/O, no system clock, no ambient RNG. Any randomness (bootstrap) takes an explicit seed argument.
- `#![forbid(unsafe_code)]`. Deterministic f64 (document reduction order; avoid parallel float-sum nondeterminism).
- Golden-value tests checked in: fixed input trajectories → fixed scores. If a number ever changes, CI fails.

**Core API (sketch):**
```rust
pub fn deflated_sharpe_ratio(returns: &[f64], n_trials: u32, track_len: usize) -> f64;
pub fn probabilistic_sharpe_ratio(sr: f64, sr_star: f64, n: usize, skew: f64, kurt: f64) -> f64;
pub fn pass_k(per_run: &[RunScore], k: usize, mode: PassMode) -> bool;          // mode = All for safety suites
pub fn spa_pvalue(agent: &[f64], field: &[&[f64]], seed: u64, boot: usize) -> f64;
pub fn process_score(trace: &Trace) -> ProcessScore;                            // gate: block-severity violations zero the entry
pub fn edge_half_life(ic_series: &[f64]) -> Option<f64>;
pub fn composite(inputs: &CompositeInputs) -> CompositeScore;                   // gated; raw return reported, not ranked
pub fn rank(entries: &[CompositeScore]) -> Ranking;
```

**Gating logic (composite):** an agent ranks only if it (a) clears the deflated-Sharpe / SPA significance bar, (b) passes pass^k on every seed×window, and (c) has zero block-severity process violations. A **random-agent "luck floor"** baseline runs on every board so the noise distribution is visible.

## 4. `sharpebench-wasm` — eat-our-own-dogfood bridge to Gordon

`wasm-bindgen` exports `sharpebench-core` scoring to JS. Gordon (Bun) imports the WASM and uses the **identical** scorer inside its RULER harness → internal eval and the public benchmark cannot drift. This is the resolution to "TS product vs Rust benchmark": one kernel, two consumers.

## 5. `sharpebench-protocol` — the adoption surface (must be dead-simple)

Agents are external — a container or HTTP endpoint, not Rust code. Per step:
- Harness → agent: `MarketObservation` (PIT prices/fundamentals/news + portfolio state) as JSON.
- Agent → harness: `Decision` { per-symbol action, target size, confidence, reasoning } as JSON.
- Transports: stdio (container, `Dockerfile` contract) and HTTP. Ship a reference agent + a thin Gordon adapter.

## 6. `sharpebench-sim` — realistic, leak-proof backtest

- **Look-ahead impossible by construction:** the PIT data store only ever returns rows with `t <= decision_time` (don't police leakage after the fact — make it unrepresentable).
- **Costs in:** slippage + market impact (own order moves price) + fees (fixes FinBen's frictionless fills).
- **Multi-window OOS + walk-forward + regime tagging** (fixes StockBench's single window).
- Deterministic given (data, seed, decisions).

## 7. `sharpebench-attest` — forward-attestation (the un-gameable spine, blunts vendor-conflict)

- **Pre-register:** agent commits a hash of its binary/config before the eval window's data exists.
- **Time-lock:** submission targets a future window; data revealed after the lock.
- **Verify:** anyone re-runs the pinned submission against revealed data and reproduces the **signed** score (HMAC chain, same pattern as Gordon's audit log). Independent verifiability is what makes GL-hosting acceptable to start.

## 8. Reproducibility & CI (the standard-hood layer)

- `rust-toolchain.toml` pinned; `flake.nix` hermetic build; `cargo build --locked`.
- Golden-score regression tests (numbers frozen). `#![forbid(unsafe_code)]` in `sharpebench-core`/`sharpebench-sim`.
- Release the `sharpebench` binary as a **single static musl binary**, **cosign/SLSA-signed** (reuse the supply-chain discipline just built for Gordon).
- Determinism CI: run the same suite twice, assert byte-identical scores.

## 9. Phased roadmap

- **Phase 0 (wk 1–2) — `sharpebench-core` + `sharpebench-cli score` + `sharpebench-wasm`.** The pure kernel + golden tests + composite ranking, validated on **synthetic agents** (skilled / lucky-noisy / process-violating / crowded-factor) proving deflation demotes luck. Ship the WASM so Gordon adopts the identical scorer immediately. *Crown jewel, all-math, lowest risk — the credible first artifact.*
- **Phase 1 (wk 3–6) — `sharpebench-sim` + `sharpebench-protocol` + `sharpebench-harness`.** PIT sim, costs, multi-window; the agent protocol + reference container; first real end-to-end run (Gordon as the agent).
- **Phase 2 (wk 7–9) — `sharpebench-attest` + `sharpebench-leaderboard`.** Forward-attestation, signed verifiable results, public leaderboard. Gordon becomes competitor #1 (publish its rank — far stronger than a self-graded number).
- **Phase 3 (ongoing) — data curation + methodology paper + public release + governance roadmap.** Data-prep may be a polyglot *offline* pipeline; harness/core/sim stay Rust. Roadmap to neutral governance / third-party verification.

## 10. Positioning

GL-org public, Apache-2.0. The pitch competitors can't own: *other leaderboards rank the luckiest run over one quarter; SharpeBench ranks the skill that survives deflation — and proves it forward.* Forward-attestation + signed verifiable results carry the credibility despite GL hosting; explicit roadmap to neutral governance once it has traction.

## 11. Open inputs (need from operator)

1. Confirm repo name/visibility: `general-liquidity/sharpebench`, public, Apache-2.0.
2. Universe for v1 suite: DJIA-20 (StockBench parity) + crypto majors (Gordon's domain)?
3. Team Rust capacity / whether to bring in help — Rust-from-day-1 was chosen with eyes open on the velocity cost.
4. Governance stance to state publicly on day 1 (GL-hosted-now → neutral-later, with forward-attestation as the trust mechanism).
