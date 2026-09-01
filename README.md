<!-- prettier-ignore -->
<div align="center">

# SharpeBench

### A luck-robust benchmark and forward-attestation stack for trading agents

Rank skill only after it survives deflation, repeated runs, significance, and
process checks—using one deterministic Rust kernel across every surface.

[![Crates.io](https://img.shields.io/crates/v/sharpebench-core?style=flat-square&logo=rust&color=DEA584&label=crates.io)](https://crates.io/crates/sharpebench-core)
[![npm](https://img.shields.io/npm/v/@general-liquidity/sharpebench?style=flat-square&logo=npm&color=CB3837)](https://www.npmjs.com/package/@general-liquidity/sharpebench)
[![PyPI](https://img.shields.io/pypi/v/sharpebench?style=flat-square&logo=pypi&logoColor=white&color=3776AB)](https://pypi.org/project/sharpebench/)
[![docs.rs](https://img.shields.io/docsrs/sharpebench-core?style=flat-square&logo=docsdotrs&label=docs.rs)](https://docs.rs/sharpebench-core)
[![CI](https://img.shields.io/github/actions/workflow/status/general-liquidity/sharpebench/ci.yml?style=flat-square&label=CI)](https://github.com/general-liquidity/sharpebench/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Unsafe](https://img.shields.io/badge/unsafe-forbidden-success?style=flat-square)](docs/book/src/introduction.md)

**[Quick start](#quick-start) · [Surfaces](#choose-a-surface) · [Gates](#what-makes-an-agent-rank-eligible) · [Agent transports](#bring-your-own-agent) · [Documentation](#documentation)**

</div>

---

SharpeBench owns the judging half of the Sharpe suite. It consumes frozen
returns and process traces, asks whether apparent edge survives the ways trading
systems fool themselves, and emits an auditable board. The sibling
[SharpeArena](https://github.com/general-liquidity/sharpearena) product produces
point-in-time trajectories using the same protocol and execution model.

> [!IMPORTANT]
> Raw return is reported, never used as the rank key. An agent ranks only after
> every hard gate passes; an impressive but unreliable or process-invalid run
> remains ineligible.

## Quick start

```bash
cargo install sharpebench
sharpebench run
```

The built-in field includes a steady agent, a lucky agent with higher raw
return, a process violator, and random-agent luck floors. The steady agent ranks;
the others show why headline return is insufficient.

Common next steps:

```bash
sharpebench score suites/example_submissions.json
sharpebench check returns.csv --trials 200
sharpebench run --data data/crypto-majors-1d.csv
sharpebench audit
sharpebench arena init league
sharpebench arena verify league
```

Every command supports `--json` for machine-readable output. The full reference
is in the [CLI chapter](docs/book/src/cli.md).

## Choose a surface

| Surface | Install | Best for |
|:--|:--|:--|
| CLI | `cargo install sharpebench` | Running fields, scoring submissions, checking one return series, stress/audit, attestation, and forward leagues. |
| Rust kernel | `cargo add sharpebench-core` | Deterministic rank, eligibility, process, attribution, and diagnostic APIs. |
| Statistics | `cargo add sharpebench-stats` | PSR, Deflated Sharpe, stationary bootstrap, Reality Check, SPA, step-down, and selection primitives without the benchmark. |
| Python | `pip install sharpebench` | The statistics kernel and JSON-compatible board/return rankers. |
| npm | `npm i @general-liquidity/sharpebench` | Typed JS/TS calls over the WASM kernel. |
| MCP | `npx -y @general-liquidity/sharpebench-mcp` | The scoring kernel exposed as agent tools. |

The workspace contains 12 published Rust crates, two private Rust helpers
(`xtask` and the reference agent), and the separate maturin-built Python crate.
See the [package map](docs/book/src/introduction.md#layout) before choosing a
lower-level dependency.

## How the Sharpe suite fits

```text
agent (any language)
        │ Observation → Decision
        ▼
SharpeArena
  point-in-time environment · execution · process trace
        │ validated field artifact
        ▼
SharpeBench
  deflation · pass^k · significance · process and mandate gates
        │
        ▼
signed board · forward window · reproducible report
```

SharpeArena depends on the small SharpeBench protocol, simulator, and kernel
crates so the two products cannot invent competing execution semantics.
SharpeBench does not import the full Arena package. The field compiler at their
boundary refuses incomplete or internally inconsistent artifacts before scoring.

## What makes an agent rank-eligible

All hard gates are conjunctive:

| Gate | Requirement | What it resists |
|:--|:--|:--|
| Deflated Sharpe / PSR | Edge survives trial count, sample length, skew, and kurtosis. | Lucky search and backtest selection. |
| pass^k | The bar clears on every required seed and window. | One-lucky-seed winners. |
| Significance | The stationary-bootstrap null is beaten at the configured alpha. | Data-snooping false positives. |
| Process | No block-severity lifecycle or trace violation occurs. | Risk-gate bypass and invalid execution behavior. |
| Mandate | The submitted run respects its drawdown/risk mandate. | Taking uncontrolled risk to buy return. |

Reality Check, SPA, step-down families, downside metrics, rolling stability,
calibration, decay, turnover, attribution, and cost-normalized measures remain
visible diagnostics. They do not silently replace the published rank key.

See [Methodology](docs/book/src/methodology.md) and
[process discipline](docs/book/src/methodology-process.md) for definitions and
derivations.

## Bring your own agent

Implement the closed `MarketObservation` → `Decision` JSON contract, then choose
the trust boundary explicitly:

```bash
sharpebench run --image ghcr.io/you/agent@sha256:<digest>
sharpebench run --cmd "./trusted-local-agent"
sharpebench run --http 127.0.0.1:8080
```

| Transport | Boundary |
|:--|:--|
| `--image` | Fail-closed Docker containment: digest-pinned local image, no network/IPC, non-root, read-only root, dropped capabilities, `no-new-privileges`, bounded memory/CPU/PIDs/files, and explicit timeouts. |
| `--cmd` | Trusted host process. The harness clears its environment and passes only platform essentials plus variables named in `SHARPEBENCH_AGENT_ENV`. |
| `--http` | Remote/local endpoint; the operator owns its isolation. |

> [!WARNING]
> `--cmd` and `--http` are not sandboxed. For code you do not control, use the
> digest-pinned `--image` path. It refuses missing Docker, mutable tags, absent
> images, readiness failures, and indeterminate cleanup/OOM state; it never
> falls through to host execution.

The Docker-enabled CI suite verifies user/capability/no-new-privilege state,
read-only and `noexec` mounts, seven egress-denial classes with timeout
discrimination, a real production spawn, a live cgroup OOM classification, and
cleanup. That is evidence for one runner and one benign fixture—not a proof
against Docker/kernel escape or a hosted multi-tenant service. Details and exact
limits are in [The arena](docs/book/src/arena.md).

A runnable stdio agent and Dockerfile live in
[`examples/reference-agent/`](examples/reference-agent/).

## Forward verification

SharpeBench can preserve more than a score:

- capture raw decisions and replay them through the frozen simulator;
- pre-register strategy digests before a forward window opens;
- seal held-out data and publish canary markers;
- sign boards with publicly verifiable Ed25519 chains;
- link consecutive windows so replacing an earlier board breaks a later anchor;
- declare exactly which fields each digest covers and why exclusions exist.

The forward arena is file-backed and clock-free. The operator supplies explicit
integer epochs and owns scheduling, participant identity, data publication, and
the public verifying-key channel. See
[Attestation](docs/book/src/attestation.md), [Integrity](docs/book/src/integrity.md),
and [The arena](docs/book/src/arena.md).

## Data and reproducibility

Scoring uses frozen, checksummed, point-in-time datasets rather than a live API.
The repository includes nine curated datasets spanning multiple asset classes
and bar sizes, plus deterministic synthetic/stress generators. For a pinned
release, configuration, and input artifact, the same trajectories produce the
same score across supported platforms.

Provenance scopes are code-owned, reject empty matches, and verify source blobs
and result artifacts. Releases build in an isolated checkout, bind the final
version tree, tag the provenance commit, publish through OIDC, and verify every
registry surface. Reproducibility means pinned inputs—not that an unpinned data
source remains unchanged forever.

## Memory and retrieval

`sharpebench-memory` applies the same discipline to a caller-supplied memory or
retrieval system. It scores baseline/retrieval/oracle ablations, significance,
fraction of oracle ceiling, poisoning, cross-session dependency, point-in-time
correctness, and confabulation. It is a pure 40-test library over outcome
vectors; it does not run agents or own a store. See
[Memory and retrieval benchmark](docs/book/src/memory.md).

## Current evidence

The companion paper reports the benchmark calibration, determinism checks,
adversarial self-audit, frozen datasets, and baseline/reference-agent results.
No local open-weight model field is part of the admitted evidence. SharpeArena's
runner can produce that field, but the current paper does not claim model
performance that has not been measured.

Exact scripts, artifacts, figures, and the provenance manifest live under
[`paper/`](paper/). The full engineering status and remaining external decisions
are in [`docs/PLAN.md`](docs/PLAN.md).

## Architecture

The codebase is a Rust workspace with a pure scoring center and explicit I/O at
the edges. All production crates forbid `unsafe`.

```text
sharpebench-stats
   ├── sharpebench-edge
   └── sharpebench-core
          ├── protocol · sim · harness
          ├── attest · leaderboard · arena
          └── CLI · WASM/npm · Python · MCP
```

See the [package layout](docs/book/src/introduction.md#layout) and the
[architecture chapters](docs/book/src/SUMMARY.md) for the detailed dependency
and methodology map.

## Documentation

| I want to… | Read |
|:--|:--|
| Understand the benchmark and packages | [Introduction](docs/book/src/introduction.md) · [Book contents](docs/book/src/SUMMARY.md) |
| Use the CLI or submit an agent | [CLI reference](docs/book/src/cli.md) · [Submitting](docs/book/src/submitting.md) |
| Understand scoring | [Methodology](docs/book/src/methodology.md) · [Process discipline](docs/book/src/methodology-process.md) |
| Operate the forward league or sandbox | [Arena](docs/book/src/arena.md) · [Attestation](docs/book/src/attestation.md) |
| Audit integrity and provenance | [Integrity](docs/book/src/integrity.md) |
| Publish a release | [`RELEASING.md`](RELEASING.md) · [Publishing model](docs/PUBLISHING.md) |
| See what was extracted from smolvm | [smolvm assessment](docs/SMOLVM_ASSESSMENT.md) |
| Browse everything | [Documentation map](docs/README.md) |

---

<div align="center">
<sub>Produce the trajectory in SharpeArena. Prove the edge here.</sub>
</div>
