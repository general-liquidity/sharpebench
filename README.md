<!-- prettier-ignore -->
<div align="center">

# SharpeBench

### The luck-robust benchmark for AI trading agents

*Other leaderboards rank the luckiest run over one quarter. SharpeBench ranks the skill that survives deflation, and proves it forward.*

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

Every existing financial-agent benchmark ranks on **raw** risk-adjusted metrics over a single short window and a handful of runs, so the leaderboard mostly measures noise. FinBen reports Sharpe confidence intervals of ±1.08, which makes its rankings statistically indistinguishable. StockBench runs one window, once. QuantBench reports Sharpe across 40 seeds but never deflates it.

**In an AI trading benchmark, the hard part is not measuring return. It is separating skill from luck.** A model that posts a great Sharpe over one quarter has told you almost nothing: the number is dominated by sampling noise, by the number of strategies that were tried, and by hidden risk the linear return series can't see.

SharpeBench adds, as **ranking gates**, the things none of the others have:

1. **Deflated Sharpe / PSR**: deflate the Sharpe by how many agents were tested × track length × return skew/kurtosis (Bailey & López de Prado), plus each agent's *own* declared in-sample trials, so a strategy mined from a thousand private backtests is deflated for that search too.
2. **pass^k reliability**: the agent must clear the bar on *every* seed × window, not on average.
3. **Significance**: a deterministic stationary bootstrap is the gate: each agent's edge must beat the bootstrap null at `alpha`, not just noise. The field-wide data-snooping tests (White's Reality Check, Hansen's studentized & consistent SPA, Romano–Wolf step-down) are computed over the whole field and reported on every row, so a reader can see whether the edge also survives the search across agents.
4. **Process discipline**: placing an order that never passed the risk gate, ignoring a drawdown halt, bypassing a deny-list, or **selling tail risk with a naked short-gamma book zeroes the entry**, however good the P&L looks. The edge must also survive a realistic execution-cost profile (the `typical` or `stressed` fees / slippage / impact / financing), not just a `frictionless` fill.
5. **Forward-attestation**: agents commit before the data exists, so there's nothing to overfit, and result chains are signed so a board is tamper-evident: the HMAC chain lets any key-holder detect a modified row, and Ed25519 public-key signing lets anyone with the host's public key verify the board independently of the host, without sharing a secret. Every run can be captured as a raw-decision **trajectory** and replayed by a separate verifier that recomputes a byte-identical score; a forged trajectory recomputes differently.

Raw return is reported but is **never** the rank key. The composite also *reports* (without gating) alpha/beta attribution, calibration, edge half-life, OOS decay, turnover, Pareto-optimality, conviction-weighted return, cost-efficiency (cost-normalized DSR), rolling-window worst-case Sharpe, selection robustness, and the **Sortino ratio** (downside-only risk), so a high score is legible, not a black box.

**Contamination & input defenses.** Comparison is restricted to the **shared** instruments a field actually traded, a rediscovery check flags a "novel" strategy that is a cosine-near copy of a known one, held-out datasets can be **sealed** (committed, opened only at scoring), a **canary** tripwire detects post-hoc that a model trained on the scenarios, and a **briefing-neutrality** audit lints the shared information packet for the salience bias that would tilt every agent at once.

> An agent does not rank on raw return. It ranks only if its edge survives deflation, $\text{pass}^{k}$, process discipline, the bootstrap null and its configured mandate. Backtest results for pretrained models are advisory; forward certification requires an opened, immutable-artifact window.

## What the evidence shows

We ran the benchmark against the market itself: nine frozen datasets, four asset classes, four bar sizes, a field of reference agents plus a luck floor of random agents on each, 4,608 scored cells. The methodology paper, *SharpeBench: A Luck-Robust Benchmark for Trading Agents* ([`paper/`](paper/)), reports what came out of that run. One of its four numbered findings **corrected the benchmark**, and three further defects found while writing are recorded in its limitations section:

- **The shipped deflation prior was in the wrong units.** Annualized, applied per period, it demanded an annualized Sharpe of **18 on daily bars and 106 on hourly bars** before anything could rank. Fixed in v0.3.0: thresholds are stated annualized and converted once via `periods_per_year`.
- **No reference agent is eligible anywhere.** Every dataset contains a bear window and pass^k demands profitability in every window, so the benchmark declines to certify that owning the index is safe in a downturn. A weaker per-window drawdown gate refuses the same agents: unhedged exposures lose **up to 99 percent** in their worst windows and breach the 20 percent cap everywhere except daily FX buy-and-hold at **0.199**, while lower-drawdown cases fail the statistical gates.
- **Risk controls do not manufacture an eligible edge.** A risk-managed agent (trend filter, vol targeting, drawdown halt, no tuning) is process-clean and respects its per-run drawdown bound on weekly US indices, but fails deflation, the stationary bootstrap and the applicable reliability verdict. The diagnosis is more informative than a blanket refusal, but it is not a deflation-only result.
- **The acceptance region is nonempty.** A synthetic agent family with a controlled injected edge, scored beside a zero-edge field under the shipped defaults and common random numbers across the edge grid, becomes rank-eligible at an injected per-period Sharpe of 0.35 on weekly-shaped tracks and 0.20 on daily-shaped ones, annualized Sharpes of 2.52 and 3.17. pass^k is the binding gate on both geometries: under the exogenously calibrated bar DSR clears from the first nonzero sampled edge (0.05) on each. The predicate is strict, not vacuous.
- **The luck floor behaves.** No random agent ever beats a reference agent on raw return. A host cannot turn selection correction off by declaring `N=1`: ranking observes the submitted field size and uses the larger count.
- **The thousand-agent tail stays below the gate.** Across 1,000 random agents on each of two daily datasets, none is eligible. With the observable trial footprint set to 1,000 and execution seeds de-duplicated as market observations, a deliberately unfloored daily-crypto diagnostic reaches DSR **0.2500**; the shipped dispersion floor lowers the operational maximum to **0.0012** against the 0.95 bar.
- **The field-level Sybil attack is defended.** With clone collapse off, two hundred near-clone entries shrink measured trial dispersion from **0.3258 to 0.0559** and lift a borderline agent's DSR from **0.0000 to 0.9522**, admitting it. Ranking now collapses near-clone clusters (absolute cosine at or above the dedicated 0.995 threshold) to one dispersion vote before it measures: the same field measures **0.3018**, the agent stays at **0.0000** and refused, 199 of 199 puppets are flagged, and all 200 stay on the board. The visible submissions still count toward the observed trial floor. The ninth audit case asserts both halves on every commit; see [the integrity chapter](docs/book/src/integrity.md).
- **A benchmark-relative verdict refuses everything too.** Opt-in `--pass-mode relative-to-benchmark` judges each window on excess return over same-window buy-and-hold instead of raw profitability. No agent becomes eligible or passes pass^k under it on any of the nine datasets (best: momentum on weekly crypto, 2 of 6 windows); every window gained is a bear window gained by a flat agent, every window lost is a bull window where the agent did not beat its beta. The refusal is not an artifact of the all-weather mandate.
- **Declarations improve diagnosis, not eligibility.** Of 36 named agent--dataset declarations, none meets the full declared predicate. Daily-crypto risk-managed is the only declared reliability pass; it remains ineligible at DSR **0.2118** with bootstrap **0.1949**. Relative declarations require the aligned field benchmark and fail closed when it is absent.
- **Execution seeds now exercise a richer failure model.** The opt-in realistic profile adds deterministic one-bar delays, partial fills with carry, and queue-slippage proxies. Across-seed annualized-Sharpe dispersion rises by roughly two orders of magnitude on three daily panels, but all 18 named rows are still refused by the window leg rather than a mixed-seed window. The result shows the seed leg is live, not that it binds on this field.

The forward arena's first live window, `window-003`, is open in [`arena/`](arena/README.md): commit deadline epoch 20711, data reveal epoch 20720, bound to the SHA-256 `263fadcd4376ca2d1c63435b885f9809fea3748a9091e30c9d72a577463c2fa1` of the attested v0.9.0 release binary. It holds no commitment yet, so no forward result is claimed. The two empty pre-entry records that preceded it were explicitly superseded when the scoring schema and provenance requirements changed; their historical bytes and linkage remain digest-checked. The paid frontier-model evaluation is also pending: the stdio adapter and three-model runner in [`examples/llm-agent/`](examples/llm-agent/README.md) exist, need the Anthropic Python SDK and an API key with credit, and fail closed on provider, credit or budget errors, so a partial API run cannot become evidence. No result from it is claimed here.

Every number reproduces from committed data via the commands in the paper's appendix; the synthetic pass witness and the thousand-agent floor are harness examples (`pass_witness.rs`, `luck_floor_1000.rs`) under [`crates/sharpebench-harness/examples/`](crates/sharpebench-harness/examples/). A benchmark that publishes what its own evidence found against itself is the credibility claim.

## Status: active, evidence-tested

All thirteen crates (twelve Rust workspace members plus the maturin-built Python binding crate, which is excluded from the Rust workspace) are implemented, tested, and CI-green on Linux, macOS and Windows (fmt · clippy `-D warnings` · workspace tests · cross-platform golden-score fixtures · WASM-native parity · the self-audit's 9 defended attacks, the ninth a Sybil case closed by clone collapse · a docs build · an npm build/test · a maturin build + pytest for the Python bindings). The statistics kernel, the backtest-honesty verdict, scoring kernel, point-in-time simulator, run harness, forward arena, leaderboard, WASM bridge, npm package, MCP server, Python ranker, and CLI all work end-to-end on synthetic data and on **nine real frozen datasets** across four asset classes and four bar sizes. The version on crates.io, npm and PyPI is always the latest `v*` tag; every registry is checked by the release pipeline's verify job.

**Not yet built** (need external infra or a decision): single-name equity data (a keyed feed), hosted arena intake and a scheduler (the arena lifecycle itself is built and test-driven, see below), and the public data-curation protocol. See [docs/PLAN.md](docs/PLAN.md).

## Quickstart

```bash
cargo install sharpebench                                    # the CLI
sharpebench run                                              # reference agents + a luck floor, ranked
sharpebench score suites/example_submissions.json           # rank a JSON field of submissions
sharpebench audit                                           # 9 defended attacks, no known gaps
sharpebench run --data data/crypto-majors-1d.csv            # run on real crypto-majors daily bars
sharpebench run --pass-mode relative-to-benchmark           # per-window verdict on excess over buy-and-hold
sharpebench uncertainty returns.csv --confidences conf.csv  # aleatoric / epistemic / distributional split
sharpebench import csv my_field/ --out subs.json            # re-score a rival board's return series
sharpebench arena init league && sharpebench arena verify league   # the forward league, file-backed
```

Prefer a prebuilt binary? Each release attaches a static Linux binary (`sharpebench-x86_64-linux-musl`) with SLSA build provenance; verify it with `gh attestation verify sharpebench-x86_64-linux-musl --repo general-liquidity/sharpebench`.

The example field includes a *skilled* agent, a *lucky* agent with a **higher raw return**, and a *process-violating* agent. The skilled agent ranks first; the other two are ineligible, which is the whole point. `run` adds a **luck floor** of random "monkey" agents so you can see the zero-skill distribution a real edge must clear. On a one-symbol dataset the random agent draws its gross exposure per seed instead of a normalized weight, so the floor does not collapse into buy-and-hold.

## Use it from anywhere

One kernel, scored identically across every surface: the internal eval and the public benchmark can't drift.

| Surface | Get it | What it is |
|:--|:--|:--|
| <img height="14" align="top" src="https://cdn.simpleicons.org/rust/DEA584" />&nbsp; **Rust crate** | `cargo add sharpebench-core` | The pure scoring kernel: deterministic, `#![forbid(unsafe_code)]`. |
| <img height="14" align="top" src="https://cdn.simpleicons.org/rust/DEA584" />&nbsp; **Rust (just the stats)** | `cargo add sharpebench-stats` | The standalone statistics kernel: PSR, deflated Sharpe, the data-snooping tests, selection. The same math the board ranks on, with no benchmark attached. |
| <img height="14" align="top" src="https://cdn.simpleicons.org/gnubash/4EAA25" />&nbsp; **CLI** | `cargo install sharpebench` | `run` / `score` / `check` / `regime` / `audit` / `sign` / `verify` / `greeks` / … |
| <img height="14" align="top" src="https://cdn.simpleicons.org/npm/CB3837" />&nbsp; **npm** | `npm i @general-liquidity/sharpebench` | Typed JS/TS API over the WASM kernel: `score`, `greeks`, `selfAudit`. |
| <img height="14" align="top" src="https://cdn.simpleicons.org/python/3776AB" />&nbsp; **Python** | `pip install sharpebench` | The stats kernel **plus the ranker**: `rank_board` / `rank_returns` take and return the same wire JSON as the CLI. |
| <img height="14" align="top" src="https://cdn.simpleicons.org/modelcontextprotocol" />&nbsp; **MCP** | `npx -y @general-liquidity/sharpebench-mcp` | An [MCP](https://modelcontextprotocol.io) server; agents call the kernel as tools. |
| <img height="14" align="top" src="https://cdn.simpleicons.org/webassembly/654FF0" />&nbsp; **WASM** | `sharpebench-wasm` | The wasm-bindgen bridge the npm package and Gordon (Bun) embed. |

```ts
import { score, greeks } from "@general-liquidity/sharpebench";

const board = score(submissions);   // ranked CompositeScore[]; raw return never buys rank
greeks({ spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0.2, is_call: true }).price; // 10.45
```

### From Rust

```rust
// Is this Sharpe real, or an artifact of luck and multiple testing?
use sharpebench_stats::{deflated_sharpe_ratio, probabilistic_sharpe_ratio, sharpe_ratio};

let returns = [0.012, -0.004, 0.009, 0.011, -0.002, 0.008, 0.010, -0.001];
let sr = sharpe_ratio(&returns);                     // observed, per-period
let psr = probabilistic_sharpe_ratio(&returns, 0.0); // P(true Sharpe > 0)
let dsr = deflated_sharpe_ratio(&returns, 200, 0.5); // deflated for 200 trials searched

// Rank a field of agents. The deflated Sharpe sorts the board; raw return never does.
use sharpebench_core::{rank, AgentSubmission, Run, ScoreConfig, Trace};
let mk = |id: &str, returns: Vec<f64>, trials: u32| AgentSubmission {
    agent_id: id.into(),
    runs: vec![Run { returns, trace: Trace::default(), confidences: vec![], outcomes: vec![], cost: 0.0 }],
    in_sample_trials: trials,
    candidates: vec![],
};
let board = rank(&[
    mk("skilled", vec![0.012, 0.008, 0.011, 0.009, 0.010], 1),
    mk("lucky",   vec![0.090, -0.02, 0.001, -0.03, 0.05], 500), // bigger raw return, 500 trials
], &ScoreConfig::default());
for s in &board {
    println!("{}  deflated={:.3}  eligible={}", s.agent_id, s.deflated_sharpe, s.rank_eligible);
}
```

Both halves are compile-and-run-checked as doctests in `sharpebench-stats` and `sharpebench-core`, so they can't silently drift from the API.

### CLI commands

| Command | What it does |
|:--|:--|
| `run` (+ `--data <csv>`, `--http`/`--cmd`) | Run agents through the point-in-time sim and rank them; `--http`/`--cmd` drives **your** external agent into the field. |
| `score <subs.json>` | Rank a JSON field of pre-computed submissions. `--pass-mode relative-to-benchmark [--benchmark-agent <id>]` (also on `run`) judges each run on its excess return over the same-window run of the named benchmark agent, `buy-and-hold` by default; the benchmark's own zero-excess series fails. |
| `check <returns.csv> --trials N` | "Is my Sharpe real?" Prints deflated Sharpe / haircut / MinTRL / verdict for your own return series; `--trials` is required (no silent default). |
| `regime <a.csv> <b.csv> <regimes.csv>` | Compare two return series *within* each market regime (zero-mass / continuous split, KS statistic, sign reversals the pooled mean hides). Regime labels are an input, one per period; nothing is inferred. |
| `audit` | Self-audit: require all 9 claimed defenses to demote their attacks, the ninth being the field-level Sybil case closed by clone collapse (its test also asserts the attack still reproduces with the collapse off); non-zero exit if a claimed defense regresses. |
| `stress` | Run the adversarial stress suite (flash-crash / whipsaw), contamination-masked. |
| `commit` · `sign` · `verify` | Forward-attestation: pre-register a digest, sign a board, verify its chain. |
| `capture` · `verify-trajectory` | Capture an agent's raw-decision trajectory, then replay it to recompute the score. |
| `audit-briefing` · `canary` | Audit a shared briefing for salience bias; derive a do-not-train contamination tripwire. |
| `score-allocation` · `greeks` | Score a weight-vector trajectory (turnover); price an option + Greeks + tail-risk. |
| `arena <init·open·commit·advance·score·publish·verify>` | Drive a forward window end to end: rules fixed before entries exist, commitments refused after the deadline, Ed25519 boards that **chain across windows** into one verifiable history. |
| `import <csv·stockbench> --out subs.json` | Convert a rival board's per-period return series into a scoreable field (caveats embedded: no trace, unknown trials understate deflation, so demotions are a lower bound). |
| `select <candidates.csv...>` | Pick a candidate on a bootstrap **percentile** instead of the observed best; reports the optimism gap that separates lucky from robust. |
| `disqualify` · `rediscover` | Classify each agent's hard gates vs advisory flags; flag a submission cosine-near a known strategy. |
| `uncertainty` · `decay-prior` | Split uncertainty into aleatoric / epistemic / distributional legs; compare measured edge decay to the crowding prior (model prior, reported never gating). |

Add `--json` to any command for machine-readable output.

### Bring your own agent

Agents are external and language-agnostic: implement the tiny JSON contract (`MarketObservation` → `Decision`) over either transport, then rank yourself into the field:

```bash
sharpebench run --cmd "cargo run -q -p reference-agent"   # stdio subprocess
sharpebench run --http 127.0.0.1:8080                     # HTTP POST /decide
```

A runnable reference agent (stdio + Dockerfile) and the wire format live in [`examples/reference-agent/`](examples/reference-agent/).

`sharpebench_core::entrants` publishes rules from the literature as specified, deterministic, hidden-state-free transforms you can build an entrant on: Donchian channel breakout, the Brock-Lakonishok-LeBaron variable moving average, Faber's ten-month filter and Wilder's RSI, plus seven more added in 0.12.0 (a regime-conditioned RSI, a bounce counter, a signal gate, a max-exposure timeout, an ATR breakout, a distribution-day count and a follow-through day). Each names its source and its parameters, and thresholds are supplied by the caller rather than baked in. They are **entrants to be scored, not infrastructure to score with**: they are unit-tested, no field evaluation has been run on them, and no result for any of them is claimed anywhere in this repository.

The contract is published as JSON Schema (draft 2020-12), and it is authoritative:

| Message | Schema |
|:--|:--|
| `MarketObservation` (with `SymbolSnapshot`, `PositionState`) | [`crates/sharpebench-protocol/schema/observation.schema.json`](crates/sharpebench-protocol/schema/observation.schema.json) |
| `Decision` (with `Order`, `DecisionCost`) | [`crates/sharpebench-protocol/schema/decision.schema.json`](crates/sharpebench-protocol/schema/decision.schema.json) |

A bidirectional drift guard (`crates/sharpebench-protocol/tests/schema_drift.rs`) fails the build if a schema and the Rust type it describes disagree in either direction, so the published document cannot rot away from the code that enforces it.

> **Breaking for entrants as of 0.11.0: the wire contract is closed.** Both schemas set `additionalProperties: false`, mirroring `#[serde(deny_unknown_fields)]` on `MarketObservation`, `SymbolSnapshot`, `PositionState`, `Decision`, `Order` and `DecisionCost`. Through 0.10.x an agent could emit extra keys and they were ignored; from 0.11.0 an extra key is rejected at the transport boundary and scored as a **non-retryable agent protocol fault**, which materializes as a failing sentinel run and counts against pass^k. This is deliberate: an attested benchmark cannot let an unread field carry meaning the scorer never saw.
>
> **Migration.** Validate one decision against `decision.schema.json` before submitting. If you emitted diagnostics alongside the orders (`latency_ms`, `model`, `notes`, and the like), put free text in `reasoning` and structured spend in `cost` (`cost_usd`, `tokens_in`, `tokens_out`, `reasoning_tokens`); drop the rest. A rejected decision now prints a diagnostic naming the offending field and the accepted set, so a failing run tells you which key to remove rather than reporting an opaque parse failure. Note also that `target_weight` is documented as `[-1, 1]`, negative meaning a short, correcting a contradictory `[0, 1] (signed for shorts)` in earlier docs.

> **Security: running untrusted agents.** `sharpebench run` executes whatever agent you point it at **without sandboxing**; only run agents you trust. The arena's containment path (`sharpebench-arena`) launches an external agent under Docker with the network and IPC namespaces removed, all capabilities dropped, `no-new-privileges`, a non-root user, a read-only root, bounded `noexec` temporary filesystems, memory / CPU / PID / file-descriptor limits, an image pinned by digest, and explicit startup and execution timeouts; a missing image or a missing daemon is a refusal, never a silent unsandboxed fallback.
>
> **That boundary has never been observed to hold.** Its smoke test skipped invisibly for its whole life, returning green when Docker was absent, and its assertion held for every failure mode including a container that never started. Both the smoke test and the new hostile probe are now `#[ignore]`d and wired to a Docker-enabled CI job, and neither has ever been executed, because the development machine's Docker daemon is not running. The hardening is written and reviewable; it is not yet evidence. Multi-tenant hosting of untrusted submissions is **not built**.

### SharpeArena composition and local open-weight fields

[SharpeArena](https://github.com/general-liquidity/sharpearena) is the agentic point-in-time environment and the wire contract; SharpeBench is the evaluator, and the containment boundary for untrusted entrants lives here in `sharpebench-arena`, not there. The composition is directed, not a mutual package dependency:

```text
SharpeArena observation/decision loop, leak-free by construction
                         ↓
        validated, append-only field artifact
                         ↓
           SharpeBench score / audit / board
```

SharpeArena depends on the small published SharpeBench protocol, simulator and kernel crates so both sides use one wire contract and execution model. SharpeBench does not import the full SharpeArena package. `sharpearena-compile-bench` closes the boundary by refusing incomplete grids, failed cells, coordinate collisions, conflicting completions and invalid return hashes before emitting one ordinary SharpeBench submissions file per dataset.

The compatibility example below can also drive exact locally installed Ollama tags through SharpeArena's fail-closed stdio shim and this repository's frozen historical panels. It records exact model/server identity, cadence and thinking settings. A protocol-invalid model decision is an agent fault; an infrastructure failure aborts the field and leaves only the partial artifact. No open-weight model result is part of the current paper.

**Prerequisite:** the shim is the sibling repository's code, not this one's. The example spawns `python -m sharpearena.ollama_shim`, so it needs SharpeArena installed in the interpreter it will use (`pip install sharpearena`, or `pip install -e` against a checkout) as well as a running Ollama. It preflights that import and exits with an actionable message if it is missing, so the dependency is announced before any dataset is loaded rather than surfacing as a spawn failure mid-field.

`SHARPEBENCH_LOCAL_MODELS` takes **exact Ollama tags**, verbatim as `ollama list` prints them; the identity recording is only worth anything if the tag is real. Size the choice to your VRAM, using the pulled size `ollama list` reports rather than the parameter count: a 9B tag at 4-bit is around 5 GB of weights and fits a 16 GB card with room for the KV cache, while a 27B tag is around 16 GB and a 35B tag 20 to 23 GB, so both exceed a 16 GB card once the cache is added and will spill to CPU or fail to load. A model that spills still produces decisions, so the run will not fail; it will just be slow enough to change what you can afford to sweep.

```bash
ollama list                                    # copy a tag from this, verbatim
export SHARPEBENCH_LOCAL_MODELS='ornith:9b'    # 5.24 GB pulled; fits a 16 GB card
cargo run --release -p sharpebench-harness \
  --example local_open_weight_field_eval -- \
  local-open-weight.jsonl us-indices-1d
```

The output path is a required positional argument for every evidence-producing example, not a default: a producer that is not told where the evidence goes exits 2 instead of writing a copy of a paper artifact into whatever directory it happened to be run from.

For the canonical batched/sharded field, the strategy-generation trial ledger and the paper-only forward arm, use SharpeArena's [`LOCAL_AGENT_ARCHITECTURE.md`](https://github.com/general-liquidity/sharpearena/blob/main/docs/LOCAL_AGENT_ARCHITECTURE.md). Historical model scores remain advisory for pretrained policies because public-market contamination cannot be excluded; certification still requires a committed forward window.

## What it measures

An agent is **rank-eligible only if every gate holds**; eligible agents then sort by the rank key (Deflated Sharpe).

| Gate | Demands | Defeats |
|:--|:--|:--|
| **Deflated Sharpe / PSR** | edge survives deflation for trials × length × skew/kurtosis | data-snooping, lucky search |
| **pass^k** | clears the bar on *every* seed × window | one-lucky-seed wins |
| **Significance** | beats the stationary-bootstrap null at `alpha` (Reality Check, SPA, and step-down are reported per row, not gating) | multiple-testing false positives |
| **Process** | zero block-severity trace violations | gate-bypass, naked tail-selling, manipulation |
| **Mandate** | respected the drawdown cap | blowing risk to chase return |

Reported but never gating: Sortino + downside deviation, rolling worst-case Sharpe, selection robustness, alpha/beta, calibration, edge half-life, OOS decay, turnover, Pareto-optimality, cost-normalized DSR. Full methodology: the [mdBook](docs/book/) (`mdbook serve docs/book`).

**Lifecycle ordering.** The process gate above reads events, and until 0.12.0 it read them as an unordered set: it could say an order bypassed the risk gate, but it could not say a risk evaluation must *precede* the order it authorizes. `check_lifecycle` adds that ordering as a typed lifecycle over observation, decision, risk evaluation, submission, acknowledgment, fill and reconciliation. Every step carries the subject it concerns, and an authorization satisfies a requirement only when the subjects match, so a risk check on one instrument cannot legitimize an order in another. The checks are typed over the event representation rather than matched on tool names, which are scaffold-specific and would reward naming conventions instead of behavior. It is additive: `process_score` is byte-unchanged and the ordering leg is read through `process_score_with_ordering`, so no committed board moves. See [process discipline](docs/book/src/methodology-process.md).

**Agreement and dissent.** Two deterministic statistics with no model in them, which is why they belong in a judge-free kernel. `agreement` asks whether the automated gate agrees with the human who triaged the gold set (Cohen's kappa with the standard chance correction, Spearman rho with tie handling). `dissent` splits a disagreement into whether the *ranking* differs or the *levels* differ, which a single number collapses (Kendall tau-b, tie-corrected in the denominator). Neither is judge-specific: the same split applies to seeds, windows and scorer configurations.

**Evidence coverage.** `evidence_coverage` is a machine-readable inventory of which fields each digest covers and which are excluded with a stated reason, so a consumer can tell signed fields from unsigned ones by inspection instead of by reading the hashing code. A test fails when a new field is neither covered nor excluded, and secrets are redacted before hashing, so verification never depends on secret material.

## Data

The benchmark runs on **frozen, checksummed, point-in-time** datasets: no live API in the scoring path, so a score reproduces forever.

| Source | Set | Provides |
|:--|:--|:--|
| <img height="14" align="top" src="https://cdn.simpleicons.org/binance/F0B90B" />&nbsp; Binance | `crypto-majors-1d.csv` | BTC/ETH/SOL/BNB/XRP daily closes (public API, no key) |
| 🏛️ [FRED](https://fred.stlouisfed.org) | `us-indices-1d.csv` | SPX / DJI / IXIC daily closes (public domain) |

Both are fetched and frozen by the offline Rust ingester (`xtask`, `publish = false`, so its deps never reach the CLI). The format is long `date,symbol,close[,dividend]`; any aligned dataset works.

```bash
cargo run -p xtask -- crypto                              # re-fetch + re-checksum
sharpebench run --data data/us-indices-1d.csv
```

## Architecture

A Rust [Cargo workspace](Cargo.toml) (modular, à la Paradigm's Rust OSS: reuse any crate on its own). The whole tree is `#![forbid(unsafe_code)]`.

```
sharpebench-stats ── the statistics kernel: PSR, expected-max-Sharpe, deflated Sharpe, the
                     data-snooping family (bootstrap / White RC / Hansen SPA / Romano–Wolf),
                     Sortino + moments, selection
      ├── sharpebench-core ── the deterministic scoring kernel (no I/O, no ambient RNG); re-exports -stats
      │     ├── sharpebench-protocol   language-agnostic agent ⇄ harness JSON
      │     ├── sharpebench-sim        point-in-time simulator (look-ahead unrepresentable at the agent boundary)
      │     ├── sharpebench-harness    orchestration across seeds × windows
      │     ├── sharpebench-attest     SHA-256 commitments + signed chains + sealed data + canary
      │     ├── sharpebench-leaderboard render / sign / self-describing boards
      │     ├── sharpebench-arena      the forward league: windowed commitments, sandboxed runs, chained Ed25519 boards
      │     ├── sharpebench-wasm       the identical kernel for JS/TS (npm, Gordon, MCP)
      │     └── sharpebench-cli        the `sharpebench` binary
      ├── sharpebench-edge ── the "is my Sharpe real?" verdict: MinTRL + PBO + the two-tier honesty check
      └── sharpebench-memory ── the memory/retrieval benchmark: 3-arm ablation + poisoning + PIT + multi-session + confabulation, significance via -stats
```

| Crate | Role |
|:--|:--|
| **`sharpebench-core`** | the scoring layer over `sharpebench-stats`: pass^k / process + cost floor / rolling / decay / calibration / attribution / comparison-sets / rediscovery / briefing-audit / allocation / regime-conditional comparison / options-Greeks / self-audit / composite, plus a **disqualification-reason taxonomy** (a typed `FailReason` rollup over the signals the scorer already computes, so a suite of submissions is legible as "X failed on luck/pass^k, Y on process, Z on deflation/overfit-decay"), the typed **lifecycle-ordering** check over the process trace, the **evidence-coverage** inventory that declares which fields each digest binds, a library of **reference entrants** (pure signal transforms and exposure gates from the published literature, scored like any other entrant and never used to score), and a re-export of the whole `-stats` kernel so existing `sharpebench_core::…` paths are unchanged. Byte-identical scores forever. |
| **`sharpebench-stats`** | the deterministic statistics kernel, split out so any project can depend on just the math: PSR, expected-max-Sharpe, deflated Sharpe (Bailey & López de Prado), the data-snooping family (stationary bootstrap, White's Reality Check, Hansen SPA liberal + consistent, Romano–Wolf step-down), Sortino + moments + normal primitives, selection robustness, **agreement** (Cohen's kappa, Spearman rho) and **dissent** (Kendall tau-b, split into ranking-differs versus levels-differ) for comparing one verdict against another without a judge in the loop, and a **stylized-facts realism validator** (Cont battery: fat tails, volatility clustering, gain/loss skew, aggregational Gaussianity, Zumbach time-reversal asymmetry) that certifies a frozen dataset is market-realistic, wired into a `sharpebench realism` CLI + CI gate so a drifted generator fails the build. No I/O, no ambient RNG, fixed reduction order. |
| **`sharpebench-edge`** | the "is my Sharpe real?" honesty layer over `-stats`: Minimum Track Record Length, Probability of Backtest Overfitting (CSCV), and the two-tier `is_my_sharpe_real` verdict (PSR / deflated Sharpe / MinTRL + haircut + Pass/Borderline/Fail; the full tier adds the data-snooping family + PBO). Powers `sharpebench check`. |
| **`sharpebench-memory`** | the memory/retrieval benchmark over `-stats`: the three-arm ablation (baseline / retrieval / oracle) with retrieval lift, stationary-bootstrap significance, and fraction-of-ceiling, plus four legs a SOTA memory benchmark also has to answer - **E1** poisoning (integrity delta / attack-success rate / degradation significance), **E2** interdependent multi-session (per-session conditioned lift + dependency-satisfaction rate + pooled significance), **E3** point-in-time correctness (per-arm no-lookahead compliance + leak flag), and **E6** confabulation (regret over reinforced-but-never-re-tested false beliefs). Pure and deterministic; significance delegated to `-stats`; it scores caller-supplied outcome vectors, with no live agent runner. |
| **`sharpebench-sim`** | fees, seeded slippage, square-root impact, financing, turnover (TRF) cost, liquidity caps, dividends, execution-cost profiles, a parameterized synthetic generator (volatility + jumps), adversarial stress paths, trajectory capture/replay, and O(1) `clone_state` / `restore_state` snapshots. |
| **`sharpebench-attest`** | SHA-256 pre-registration commitments + signed result chains (HMAC for key-holders, Ed25519 for public verification) + time-lock registry + sealed held-out datasets + canary contamination tripwire. |
| **`sharpebench-harness`** | seeds × windows orchestration; luck-floor producers; a runtime-vs-agent failure taxonomy. |
| `protocol` · `leaderboard` · `wasm` · `cli` | the JSON contract · render/sign/self-describing boards · the WASM bridge · the CLI. |

### Memory benchmark (`sharpebench-memory`)

The same skill-vs-luck discipline, pointed at a memory or retrieval layer. Proving a memory layer helps (rather than just adding tokens and latency) is an ablation, so the crate scores the three-arm ablation - baseline (no memory, the floor), retrieval (the layer under test), and oracle (gold records only, the ceiling) - and reports the retrieval lift, its stationary-bootstrap significance (delegated to `sharpebench-stats`, not reinvented), and the fraction of the oracle ceiling captured.

Around that floor and ceiling it adds the four legs a memory benchmark also has to answer, each pure and deterministic:

- **E1 poisoning.** Inject corrupted records and measure the behavior-integrity delta, the attack-success rate, and the bootstrap significance of the degradation. Money-memory (a forged limit, a wrong balance, a spoofed venue) is the high-severity case.
- **E2 multi-session.** Model sessions as a dependency graph; a later session's lift is credited only when the memory an earlier session wrote was actually retained. Reports per-session conditioned lift, the cross-session dependency-satisfaction rate, and pooled significance.
- **E3 point-in-time correctness.** Score no-lookahead compliance per arm from recall-audit counts and flag whether the retrieval arm leaked future data. It takes counts only, so it stays decoupled from any enforcement layer - a bi-temporal store, a replay harness, or a hand audit can feed it.
- **E6 confabulation.** The "honest lying" metric: among beliefs that were reinforced but never re-tested and have since resolved, the fraction that proved wrong.

Together these cover the union a SOTA memory benchmark has to prove in one place: statistical significance (stationary bootstrap via `-stats`), point-in-time no-lookahead, poison-resistance, cross-session dependency, and confabulation. Like the scoring kernel it consumes caller-supplied outcome vectors and has **no live agent runner** in this crate; 36 unit tests, `#![forbid(unsafe_code)]`, and a fixed bootstrap seed so a verdict never moves when re-run.

## Tech stack

| Technology | Role |
|:--|:--|
| <img height="14" align="top" src="https://cdn.simpleicons.org/rust/DEA584" />&nbsp; [Rust](https://www.rust-lang.org) | The whole kernel: pure `f64`, fixed reduction order, no `unsafe` |
| <img height="14" align="top" src="https://cdn.simpleicons.org/webassembly/654FF0" />&nbsp; [WebAssembly](https://webassembly.org) | The kernel for non-Rust hosts (`wasm-bindgen`) |
| <img height="14" align="top" src="https://cdn.simpleicons.org/typescript/3178C6" />&nbsp; [TypeScript](https://www.typescriptlang.org) | The typed npm package + MCP server |
| <img height="14" align="top" src="https://cdn.simpleicons.org/npm/CB3837" />&nbsp; [npm](https://www.npmjs.com/package/@general-liquidity/sharpebench) | JS/TS distribution of the scoring kernel |
| <img height="14" align="top" src="https://github.com/serde-rs.png" />&nbsp; serde | Deterministic JSON for every submission, board, and config |
| <img height="14" align="top" src="https://cdn.simpleicons.org/githubactions/2088FF" />&nbsp; GitHub Actions | CI: fmt · clippy · tests · determinism · self-audit · docs · npm |
| <img height="14" align="top" src="https://cdn.simpleicons.org/modelcontextprotocol" />&nbsp; [MCP](https://modelcontextprotocol.io) | Agents call the kernel as tools |
| cargo-deny | Supply-chain gate (advisories · bans · licenses · sources) |

## Methodology & references

The gates are not invented; they are the published, peer-reviewed controls for skill-vs-luck, assembled into one ranking.

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

Hosted by [General Liquidity](https://github.com/general-liquidity) to start, with a roadmap to neutral governance. Credibility comes from **forward-attestation + signed, independently-verifiable results**, not from trust in the host, and Gordon (GL's agent) competes on the board like any other entrant. The neutral home may already exist: the FINOS-governed [Open FinLLM Leaderboard](https://huggingface.co/spaces/finosfoundation/Open-Financial-LLM-Leaderboard) covers the financial-*knowledge* axis but has **no trading-performance axis**; SharpeBench is positioned to be the skill-vs-luck *trading* track it lacks. See **[docs/GOVERNANCE.md](docs/GOVERNANCE.md)**.

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

---

<div align="center">
<sub><em>Skill that survives deflation, and proves it forward.</em></sub>
</div>
