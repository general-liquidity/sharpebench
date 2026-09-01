<!-- prettier-ignore -->
<div align="center">

# SharpeBench

### The luck-robust benchmark for trading agents

Rank skill only after it survives deflation, repeated runs, significance, and
process checks, using one deterministic Rust kernel across every surface.

[![Crates.io](https://img.shields.io/crates/v/sharpebench-core?style=flat-square&logo=rust&color=DEA584&label=crates.io)](https://crates.io/crates/sharpebench-core)
[![npm](https://img.shields.io/npm/v/@general-liquidity/sharpebench?style=flat-square&logo=npm&color=CB3837)](https://www.npmjs.com/package/@general-liquidity/sharpebench)
[![PyPI](https://img.shields.io/pypi/v/sharpebench?style=flat-square&logo=pypi&logoColor=white&color=3776AB)](https://pypi.org/project/sharpebench/)
[![docs.rs](https://img.shields.io/docsrs/sharpebench-core?style=flat-square&logo=docsdotrs&label=docs.rs)](https://docs.rs/sharpebench-core)
[![CI](https://img.shields.io/github/actions/workflow/status/general-liquidity/sharpebench/ci.yml?style=flat-square&label=CI)](https://github.com/general-liquidity/sharpebench/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Unsafe](https://img.shields.io/badge/workspace%20packages-unsafe%20forbidden-success?style=flat-square)](docs/book/src/introduction.md)

**[Quick start](#quick-start) · [Gates](#what-makes-an-agent-rank-eligible) · [Bring an agent](#bring-your-own-agent) · [Verify](#capture-and-verify) · [Paper](paper/main.pdf) · [Documentation](#documentation)**

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

## Benchmark at a glance

| | |
|:--|:--|
| Question | Does an agent's apparent edge survive selection, repeated execution, significance, process, and mandate checks? |
| Judge | One deterministic Rust kernel. No LLM judge. |
| Inputs | Frozen returns and process traces, or point-in-time decisions from a live agent. |
| Eligibility | Five conjunctive hard gates. One failed gate makes the submission ineligible. |
| Data | Eight tradable-price datasets across four asset classes, plus one rates-yield stress series, at four bar sizes. |
| Outputs | A diagnostic board, replayable trajectories, and optional signed forward records. |

> [!CAUTION]
> The code license does not license the frozen market observations. The
> [series-by-series rights audit](data/RIGHTS.md) records unresolved
> redistribution restrictions for the current data bundle. Historical evidence
> remains bound to the exact committed bytes, but those CSVs must not be treated
> as MIT- or Apache-licensed data.

## Quick start

```bash
cargo install sharpebench
sharpebench run
```

`sharpebench run` drives buy-and-hold, momentum, and three zero-skill luck-floor
agents across two windows and eight execution seeds. To see the headline
failure directly, score the committed teaching submissions from a repository
checkout:

```bash
sharpebench score suites/example_submissions.json
```

The resulting board makes the rank rule concrete:

| Entrant | Raw mean return | pass^k | Process | Eligible |
|:--|--:|:--:|:--:|:--:|
| `skilled-momentum` | 0.2020% | yes | pass | **yes** |
| `lucky-yolo` | **0.4111%** | no | pass | no |
| `ungated-bot` | 0.2020% | yes | fail | no |

The lucky entrant earns roughly twice the raw mean return and still cannot
rank. The process violator reproduces the skilled return but is floored. The
table is pinned to the
[committed golden board](crates/sharpebench-core/golden/example_submissions.scores.json).

> [!NOTE]
> This is a deterministic teaching field, not a model leaderboard, and
> `sharpebench run` does not reproduce the paper's complete evidence sweep. The
> current paper evaluates author-written entrants and externally specified rules;
> no LLM trading agent has competed. See the
> [paper](paper/main.pdf) and its [exact reproduction commands](paper/sections/A-commands.tex).

Common next steps:

```bash
sharpebench check returns.csv --trials 200
sharpebench run --data data/crypto-majors-1d.csv
sharpebench audit
sharpebench arena init league
sharpebench arena verify league
```

Commands that render reports accept `--json` for structured output. Commands
that create an artifact already write the documented JSON form. The full
reference is in the [CLI chapter](docs/book/src/cli.md).

## What makes an agent rank-eligible

All hard gates are conjunctive:

| Gate | Requirement | What it resists |
|:--|:--|:--|
| Deflated Sharpe | Edge survives trial count, sample length, skew, and kurtosis. | Lucky search and backtest selection. |
| pass^k | The bar clears on every required seed and window. | One-lucky-seed winners. |
| Significance | The stationary-bootstrap null is beaten at the configured alpha. | Data-snooping false positives. |
| Process | No block-severity lifecycle or trace violation occurs. | Risk-gate bypass and invalid execution behavior. |
| Host mandate | Every run respects the configured drawdown bound. | Taking uncontrolled drawdown to buy return. |

`pass^k` means every required execution seed and evaluation window passes. It
is intentionally stricter than `pass@k`: one successful attempt demonstrates
possible capability, not reliable trading edge. Seeds test execution stability;
windows test robustness across market regimes.

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

For a resumable command or HTTP sweep, bind the entrant artifact explicitly:

```bash
sharpebench run --cmd "./trusted-local-agent" \
  --checkpoint sweep.json \
  --entrant-sha256 <64-lowercase-hex-digest>
```

| Transport | Boundary |
|:--|:--|
| `--image` | Fail-closed Docker containment: digest-pinned local image, no network or IPC, non-root, read-only root, dropped capabilities, `no-new-privileges`, bounded memory, CPU, PIDs, and files, plus explicit timeouts. |
| `--cmd` | Trusted host process. The harness clears its environment and passes only platform essentials plus variables named in `SHARPEBENCH_AGENT_ENV`. |
| `--http` | Remote or local endpoint; the operator owns its isolation. |

> [!WARNING]
> `--cmd` and `--http` are not sandboxed. For code you do not control, use the
> digest-pinned `--image` path. It refuses missing Docker, mutable tags, absent
> images, readiness failures, and indeterminate cleanup or OOM state; it never
> falls through to host execution.

The Docker-enabled CI suite verifies user, capability, and no-new-privilege
state; read-only and `noexec` mounts; seven egress-denial classes with timeout
discrimination; a real production spawn; a live cgroup OOM classification; and
cleanup. That is evidence for one runner and one benign fixture, not proof
against a Docker or kernel escape, and not evidence for a hosted multi-tenant
service. Details and exact limits are in [The arena](docs/book/src/arena.md).

A runnable stdio agent and Dockerfile live in
[`examples/reference-agent/`](examples/reference-agent/).

Entrant faults remain in the pass^k denominator as failing sentinels. Exhausted
runtime or transport failures make the sweep noncertifying: the CLI reports the
missing cells, emits no board, and exits unsuccessfully.

## Capture and verify

For a built-in agent, preserve raw decisions instead of trusting a reported
score:

```bash
sharpebench capture momentum trajectory.json
sharpebench verify-trajectory trajectory.json
```

The strict verifier binds the dataset, costs, engine, runner, ordered windows,
and ordered seeds. It requires every declared cell and every decision step,
then replays the decisions through the frozen simulator and recomputes the
score with the original replicate grouping. Missing, duplicated, reordered,
shortened, or cross-environment evidence is refused. An explicit
`--allow-unbound-trajectory` option exists only for a legacy or cross-version
diagnostic regrade. See [Evidence contracts](docs/book/src/evidence-contracts.md)
and [Submitting an agent](docs/book/src/submitting.md).

SharpeBench can also pre-register strategy digests before a forward window,
commit to held-out data, sign boards with publicly verifiable Ed25519 chains,
and link consecutive windows so replacing an earlier board breaks a later
anchor. HMAC chains are shared-secret checks whose keyholders can also forge;
they are not a substitute for public verification. The forward arena is
file-backed and clock-free. Cryptography binds bytes and history, not chronology:
the operator owns epoch advancement, participant identity, held-out-data custody,
reveal timing, the signing key, and the public verifying-key channel. A forward
interpretation assumes an auditable wall-time mapping and no pre-commitment
observation of the target data.

### Evaluation contract

| Part | Contract |
|:--|:--|
| Held fixed | Exact dataset bytes and hash, cost profile, score configuration, execution seeds, and evaluation windows. |
| Entrant varies | Decisions, resulting return streams, declared search history, and process traces. |
| Field context | The submitted field sets an observable trial-count floor and can supply the measured cross-strategy dispersion. |
| Judge | Every entrant in the field is scored by the same deterministic Rust kernel. |

Rows are directly rank-comparable within one signed board. Cross-board claims
also require the same run specification, entrant field, trial footprint,
schema, and scorer artifact. Matching `RunSpec` alone is not sufficient: field
composition changes the observed trial floor and can change measured
dispersion. The full comparability rule lives in
[Evidence contracts](docs/book/src/evidence-contracts.md) and
[The arena](docs/book/src/arena.md).

See [Integrity](docs/book/src/integrity.md) and
[The arena](docs/book/src/arena.md) for the signed fields and verification
model.

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

## Choose a surface

| Surface | Install | Best for |
|:--|:--|:--|
| CLI | `cargo install sharpebench` | Running fields, scoring submissions, checking one return series, stress and audit, attestation, and forward leagues. |
| Rust kernel | `cargo add sharpebench-core` | Deterministic rank, eligibility, process, attribution, and diagnostic APIs. |
| Statistics | `cargo add sharpebench-stats` | PSR, Deflated Sharpe, stationary bootstrap, Reality Check, SPA, step-down, and selection primitives without the benchmark. |
| Python | `pip install sharpebench` | The statistics kernel and JSON-compatible board or return rankers. |
| npm | `npm i @general-liquidity/sharpebench` | Typed JavaScript and TypeScript calls over the WASM kernel. |
| MCP | `npx -y @general-liquidity/sharpebench-mcp` | The scoring kernel exposed as agent tools. |

See the [package map](docs/book/src/introduction.md#layout) before choosing a
lower-level dependency.

## Data and evidence

Scoring uses frozen, checksummed, point-in-time datasets rather than a live API.
The historical evidence uses eight tradable-price datasets across four asset
classes, plus one rates-yield stress series, at four bar sizes. Deterministic
synthetic and stress generators are separate. Two bundled
artifacts do not clear the stylized-facts realism gate, and the rates dataset
contains yields rather than tradable prices; both facts are recorded in the
data inventory and paper limitations. Two committed golden fields reproduce
byte-for-byte on the Linux, macOS, and Windows CI hosts.

Provenance scopes are code-owned, reject empty matches, and verify source blobs
and result artifacts. Releases build in an isolated checkout, bind the final
version tree, tag the provenance commit, publish through OIDC, and verify every
registry surface. Reproducibility means pinned inputs, not that an unpinned data
source remains unchanged forever.

The companion [paper](paper/main.pdf) reports calibration, determinism checks,
the adversarial self-audit, frozen-data results, and explicit limitations. Its
scripts, artifacts, figures, and provenance manifest live under
[`paper/`](paper/). The engineering status and remaining external decisions are
in [`docs/PLAN.md`](docs/PLAN.md).

## Architecture

The codebase is a Rust workspace with a pure scoring center and explicit I/O at
the edges. All twelve workspace packages under `crates/` forbid `unsafe`. The
published PyO3 binding is excluded from that workspace and is the disclosed
exception because generated FFI glue expands to unsafe operations.

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

### Additional package

`sharpebench-memory` scores caller-supplied memory and retrieval ablations. It
does not run agents or own a store. See the
[memory and retrieval benchmark](docs/book/src/memory.md).

## Documentation

| I want to... | Read |
|:--|:--|
| Understand the benchmark and packages | [Introduction](docs/book/src/introduction.md) · [Book contents](docs/book/src/SUMMARY.md) |
| Use the CLI or submit an agent | [CLI reference](docs/book/src/cli.md) · [Submitting](docs/book/src/submitting.md) |
| Understand scoring | [Methodology](docs/book/src/methodology.md) · [Process discipline](docs/book/src/methodology-process.md) |
| Operate the forward league or sandbox | [Arena](docs/book/src/arena.md) · [Attestation](docs/book/src/attestation.md) |
| Audit integrity and provenance | [Integrity](docs/book/src/integrity.md) · [Evidence contracts](docs/book/src/evidence-contracts.md) · [65-benchmark architecture audit](docs/BENCHMARK_ARCHITECTURE_AUDIT.md) |
| Reproduce the paper | [Paper PDF](paper/main.pdf) · [Commands](paper/sections/A-commands.tex) |
| Contribute or propose a change | [`CONTRIBUTING.md`](CONTRIBUTING.md) · [Governance](docs/GOVERNANCE.md) |
| Review releases and licensing | [`CHANGELOG.md`](CHANGELOG.md) · [MIT](LICENSE-MIT) · [Apache-2.0](LICENSE-APACHE) |
| Publish a release | [`RELEASING.md`](RELEASING.md) · [Publishing model](docs/PUBLISHING.md) |
| Browse everything | [Documentation map](docs/README.md) |

---

<div align="center">
<sub>Produce the trajectory in SharpeArena. Prove the edge here.</sub>
</div>
