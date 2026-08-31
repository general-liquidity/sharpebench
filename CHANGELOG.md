# Changelog

All notable changes to SharpeBench are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). One workspace
version covers every crate, the npm packages and the PyPI package. Sharing a
version is not the same as being published: the crates that reach crates.io
are the twelve listed in [RELEASING.md](RELEASING.md), and `xtask` and
`examples/reference-agent` are `publish = false`. Each section is one `v*` tag
and links the commits it was built from.

[Unreleased]: https://github.com/general-liquidity/sharpebench/compare/v0.15.0...HEAD

## [Unreleased]

### Changed
- sim: a subprocess agent that dies during the decide wait is reported immediately as the new retryable `DecideError::Exited` carrying its exit status, instead of burning the full 30s decide budget and reporting `Timeout` — a container or process that crashes at startup was misclassified as a slow one. The wait now watches for the child's exit on three paths: the reader thread's EOF (with a short grace, because the pipe closes a beat before the exit is observable), a stdin write failure against a dead child, and a poll during the timeout wait for the case where a grandchild holds the stdout pipe open past its parent's death. The race is handled in the agent's favor: a fast agent that answers and exits in the same instant is drained and scored as the SUCCESS it is, never as an exit fault; the drain semantics are pinned by a deterministic unit test. The held-open-pipe fixture is unix-only (shell job control) and runs on the ubuntu/macos CI legs.
- sim/cli: **behavior change on `--cmd` and every hermetic external-agent spawn** — `ExternalAgent::spawn` now clears the child's environment instead of inheriting the harness's full environment, API keys included, into a process the user was only warned is unsandboxed. The agent receives an explicit allowlist of what a plain subprocess needs to run at all (`PATH`, temp dirs, the user-profile path; on Windows also `SystemRoot`, `windir`, `ComSpec`, `PATHEXT`, `SystemDrive`, `APPDATA`, `LOCALAPPDATA`; on Unix also `LANG`, `LC_ALL`, `TZ`). A legitimately env-dependent agent passes named variables through with `SHARPEBENCH_AGENT_ENV=NAME1,NAME2`, or a driver names them programmatically via the new `ExternalAgent::spawn_with_env`; refusing silently would be the wrong failure mode, so the `--cmd` warning text and the README name the escape hatch. `ExternalAgent::spawn_inheriting` exists for trusted transport tooling only: the sandbox's `docker` client keeps its `DOCKER_HOST` / `DOCKER_CONFIG` context, while the entrant inside the container gets the container's own fresh environment either way. The `llm_field_eval` and `local_open_weight_field_eval` examples now pass exactly the variables their shims need (`ANTHROPIC_API_KEY`; `PYTHONPATH` and `OLLAMA_HOST`).

### Fixed
- arena: the hardened `docker run` builder holds its flags and the trailing image positional separately until assembly (`HardenedLaunch`), instead of one flat list ending `"-i", image` that callers `extend`ed after. The flat shape was correct only because the image happened to be last: appending a hardening flag to the list would silently turn it into a *container command* argument, handed to the entrant instead of to Docker. The ordering is now structural — a flag appended at any point lands before the image, an explicit container command can only land after it — and a regression test appends a flag late and asserts where it lands.

### Added
- harness/arena/cli: a kill by the sandbox's published `--memory` budget is a first-class failure. An OOM-killed entrant exits 137 like any other SIGKILL, so exceeding a published resource budget, a scoring-relevant fact, was invisible to the failure taxonomy: the dead pipe it leaves behind was filed as a retryable transport blip, and the harness respawned an agent guaranteed to blow the same budget again. The sandbox now launches the agent container named and without `--rm` (readiness probes keep `--rm`), reads `State.OOMKilled` via `docker inspect` after the wait, and removes the container explicitly, preserving the no-leak property on both the `finish` path and `Drop`. The verdict folds into the taxonomy as `FailureKind::ResourceLimitExceeded` through `apply_oom_verdict`: an agent fault, never retried, counted against pass^k, and it overrides even a clean run because the budget breach is a fact about the run regardless of what made it onto the wire before the kill. `run_external_sandboxed` now returns a `SandboxedAgent` (transport plus container handle) instead of a bare `ExternalAgent`. The classification is unit-tested through an injected `ContainerInspector`; the live `docker inspect` leg runs only in the Docker-enabled CI job.

## [0.15.0] - 2026-08-30

### Added
- cli: `sharpebench run --image <repository@sha256:...>` runs an external entrant inside the container boundary. The hardened launch in `sharpebench-arena` had no production consumer: its only non-test reference was the crate re-export, while `run --cmd` spawned the agent through a bare `std::process::Command` on the host. Every property the sandbox configures (network and IPC isolation, read-only root, non-root user, dropped capabilities, no-new-privileges, cpu / memory / pid / fd limits, digest-pinned image) was therefore written and unreachable from the CLI. `--image` is the missing consumer, and a refusal on that path (no daemon, a mutable tag, an image absent locally) ends the run rather than degrading to host execution.
- arena: `check_sandbox_readiness` asserts egress by attempting one outbound connect from inside the boundary rather than inferring it from the launch flags, and separates a policy refusal from the three outcomes that resemble one: a connect that hung until its client timeout (which a broken network also produces), a fixture image with no HTTP client installed, and a connect that succeeded. The attempt is timed net of a do-nothing run of the same image, so the budget bounds the connect and not container startup. `sandbox-check` reports it as an eighth passed check. Like every other live leg it runs only in the Docker-enabled CI job.
- arena: `require_local_image` refuses a digest-pinned reference that Docker cannot resolve to a locally present artifact. `docker run --pull never` spawns successfully whether or not the image exists, so without this an absent image reached the harness as a dead agent mid-sweep instead of as a refusal before anything started.

### Changed
- cli: `run --cmd` now prints an unsandboxed-execution warning to stderr on every run, in both text and `--json` mode. Its behaviour is otherwise unchanged: it still executes the named program directly on the host, which is what it has always done, and existing invocations keep working. What changed is that the choice is now recorded instead of implicit, so an unsandboxed run cannot be mistaken for a sandboxed one after the fact.
- sim: the stdio transport caps one decision line at 8 MiB, the same budget the HTTP transport already applied to a response, and reports a line past it as the new non-retryable `DecideError::Oversized`. The reader was doing an unbounded `read_line` on untrusted subprocess output, so an entrant that writes without ever sending a newline grew the buffer until the harness died, losing every other agent's results in the same sweep. An oversized line counts with the protocol faults rather than the transport faults, because it is the entrant's own violation of the one-decision-per-line contract.
- sim: an external agent is spawned into its own process group on Unix, and teardown signals the group (TERM, a 500 ms grace, then KILL) instead of only the direct child. An entrant wrapped in `sh -c` leaves a grandchild holding the inherited stdout pipe, so the reader thread never saw EOF and every later decision spent its full wall-clock budget waiting on a process whose parent was already dead.

### Fixed
- paper: `make-provenance.py` refuses a scope pattern that matches no file, and `check-provenance.py` fails a manifest that records one. A dead pattern removes files from both the digest leg and the unrecorded-file leg while the run still reports OK, so one renamed directory could empty a whole scope and the validator would pass on a tree it had checked nothing in. `arena/**/*.toml`, which matches nothing, is dropped from the source scope.
- paper: `make-provenance.py` writes the manifest atomically (temp file, fsync, rename, directory fsync), so an interrupted or out-of-space run leaves the previous manifest intact instead of a truncated prefix of the new one.

## [0.14.1] - 2026-08-28

### Fixed
- paper: the line breaker gets an emergency pass, taking underfull boxes from 14 to 13. Long typewriter identifiers cannot hyphenate, so a column forced to hold one is set loose. Inserting break points inside the identifiers clears two more, but the PDF then copies `OutperformBuyAndHold` as two words, and a paper whose readers copy these strings should not trade a loose line for a broken identifier, so the rest stay loose.

## [0.14.0] - 2026-08-27

### Fixed
- release: the MCP package installs its just-published dependency without consulting the committed lock ([61c479a](https://github.com/general-liquidity/sharpebench/commit/61c479a)). `@general-liquidity/sharpebench-mcp` publishes in the same run as `@general-liquidity/sharpebench`, and before that version exists on the registry the committed lock cannot hold the integrity hash of the tarball npm will serve for it; npm accepted the fresh metadata, rejected the tarball against the pre-release integrity, and the MCP package never published. `npm install --package-lock=false` in the publish step is the fix, and it now has a regression test (`npm/mcp/test/release-install.test.js`) asserting the flag and the ordering of the propagation wait, which the original change shipped without.
- release: `sharpebench-memory` is published. It carried the workspace version, had no `publish = false`, and was on no registry, so the version it advertised existed nowhere; nothing in the workspace depends on it, so nothing caught that. It is a caller-facing library with a documented API and 40 tests, so it is now in the crates.io publish order after `sharpebench-stats`, its only dependency. The name is unclaimed, so the first publish must be by hand before the next tag (RELEASING.md).

## [0.13.0] - 2026-08-26

### Added
- paper/ci: `check-provenance.py` recomputes every source and artifact digest, re-expands both committed scopes to detect additions or removals, and fails CI on any mismatch.

### Fixed
- paper: the source snapshot excludes build products, virtual environments, bytecode caches and dependency trees. The previous manifest admitted machine-generated Rust files from `crates/sharpebench-py/target/`, so its identity depended on the development machine.
- paper: source hashes canonicalize CRLF to LF while result artifacts stay byte-exact. The previous raw-byte source hashes gave four tracked files different identities on the Windows development worktree and the Linux CI checkout.
- paper: the commands appendix no longer quotes a digest over a source set containing the appendix itself. The manifest is the sole machine-readable authority, the externally specified rule field is covered as an ordinary artifact, and generation records whether its source tree was clean.

## [0.12.0] - 2026-08-26

### Added
- core: lifecycle ordering in the process gate, linked by subject. The gate scored events but had no ordering semantics at all, so it could not express that a risk evaluation must precede the order it authorizes. `check_lifecycle` reads a typed lifecycle over observation, decision, risk evaluation, submission, acknowledgment, fill and reconciliation; every step carries the `Subject` it concerns, and an authorization satisfies a requirement only when the subjects match, so a risk check on one instrument cannot legitimize an order in another. Checks are typed over the event representation rather than matched on tool names, which are scaffold-specific and would reward naming conventions instead of behavior. `process_score` is unchanged and the ordering leg is additive, exposed separately as `process_score_with_ordering` ([f31cd72](https://github.com/general-liquidity/sharpebench/commit/f31cd72)).
- stats: two judge-free modules. `agreement` answers a question the benchmark could not previously ask, whether the automated gate agrees with the human who triaged the gold set: Cohen's kappa with the standard chance correction, Spearman rho with tie handling, and a `gate_vs_human` rollup. `dissent` splits a disagreement into whether the ranking differs or the levels differ, which the benchmark previously collapsed into one number, on Kendall tau-b with tie correction in the denominator; it generalizes off judges entirely, to seeds, windows and scorer configurations. Both are deterministic with no model in them, implemented from the standard definitions cited in their docstrings and property-tested against known values ([577b3ad](https://github.com/general-liquidity/sharpebench/commit/577b3ad)).
- core: `evidence_coverage`, a machine-readable inventory of which fields each digest covers and which are excluded with a stated reason, so a consumer can tell signed fields from unsigned ones by inspection rather than by reading the hashing code. A test fails when a new field is neither covered nor excluded. Secrets are redacted before hashing, so verification never depends on secret material ([46e5a0d](https://github.com/general-liquidity/sharpebench/commit/46e5a0d)).
- core: seven reference entrants in `entrants`, pure signal transforms and exposure gates with caller-supplied thresholds, published in the same style as the four rules already scored from the literature (Donchian channel breakout, the Brock-Lakonishok-LeBaron variable moving average, Faber's ten-month filter, Wilder's RSI). Each is specified, deterministic, carries no hidden state, and names its source and parameters. They are entrants to be scored, not infrastructure to score with; no field evaluation has been run on them and no result is claimed ([46e5a0d](https://github.com/general-liquidity/sharpebench/commit/46e5a0d)).
- protocol: the wire contract is published as draft 2020-12 JSON Schema covering all six wire types across two documents, `observation.schema.json` (`MarketObservation`, `SymbolSnapshot`, `PositionState`) and `decision.schema.json` (`Decision`, `Order`, `DecisionCost`), every object closed with `additionalProperties: false` to mirror `deny_unknown_fields`. A bidirectional drift guard (`tests/schema_drift.rs`) compares serialized keys against declared properties in both directions and asserts each separately, so a failure names which side is missing what. It is verified non-vacuous in both directions: removing `Order.rationale` from the schema and adding a schema-only property each fail with the offending key named ([1c70ce6](https://github.com/general-liquidity/sharpebench/commit/1c70ce6)).
- ci: a job that runs the arena's `#[ignore]`d sandbox tests on a Docker-enabled runner, with alpine pinned by RepoDigest. The job has never been executed and is unvalidated ([aeb30d3](https://github.com/general-liquidity/sharpebench/commit/aeb30d3)).

### Changed
- provenance: the source snapshot is re-pinned at 157 sources and 22 artifacts with none stale; 43 of the previous 144 pinned hashes had drifted at HEAD, including the engine, the protocol and the transport. The source scope now includes the published JSON schemas, because a contract entrants are told to validate against that the integrity hash does not cover is only half published ([d62f60b](https://github.com/general-liquidity/sharpebench/commit/d62f60b)).
- docs: `SHARPEBENCH_LOCAL_MODELS` had quoted `qwen3.8:27b`, which is not a real Ollama tag. The example now uses a tag that exists, states its footprint at 4-bit, and says plainly what a 16 GB card can and cannot hold ([fe3369e](https://github.com/general-liquidity/sharpebench/commit/fe3369e)).

### Fixed
- protocol/sim: a rejected decision now names the offending field. `deny_unknown_fields` on six wire types is a breaking change for entrants, and an entrant sending an extra key previously got an opaque deserialization failure at the transport boundary. `decision_from_wire` keeps serde's message, which already names the offending field and lists the accepted set, and appends the schema path; `parse_decision` routes through it and prints the diagnostic at the fault site. The breaking-change framing and the entrant migration are documented in the crate header ([93dde01](https://github.com/general-liquidity/sharpebench/commit/93dde01)).
- harness: every evidence-producing example now takes its output path as a required positional argument and exits 2 with a usage line when it is missing. Each had defaulted to a bare CWD-relative filename, so running one from the repository root wrote a stray artifact beside the committed copy; `luck_floor_1000` defaulted straight into `paper/evidence/final/`. `local_open_weight_field_eval` spawns a shim that lives in the sibling SharpeArena package, which is not a dependency of this repository: it now declares that dependency in its header and probes for the module before loading any dataset, exiting with the module name, the interpreter it tried and both remedies. The example gains a test target, so the preflight has the tests the changelog already claimed it had ([4c13c42](https://github.com/general-liquidity/sharpebench/commit/4c13c42)).
- arena: `docker_spawn_smoke` returned early with an `eprintln` when Docker was absent, so it counted as one of the passing tests and cargo swallowed the message; its assertion was `orders.is_empty()`, which holds for every failure mode including a container that never started, so the boundary could not have been observed even when the test did run. It is now `#[ignore]`d, so a skip reads as a skip, and the assertion moved to transport health: a container that ran and stayed silent yields a timeout, while one that never started closes stdout and yields a transport error, so the two are no longer the same result. A new test, `live_hostile_probe_passes_inside_the_hardened_boundary`, is the first in the repository that executes `HOSTILE_PROBE`. **Neither test has ever been executed.** Docker Desktop is installed on the development machine but its daemon is not running, so the hostile probe has never been observed to pass and the CI job that would run it is unvalidated. What changed is that the absence of evidence is now visible rather than counted as evidence ([aeb30d3](https://github.com/general-liquidity/sharpebench/commit/aeb30d3)).

## [0.11.0] - 2026-08-26

### Added
- harness: `local_open_weight_field_eval`, a local frontier-model compatibility field that drives exact Ollama tags through SharpeArena's canonical fail-closed stdio shim, records model/server identity plus cadence/thinking configuration, and withholds the final artifact if any infrastructure cell fails. The path is built and tested; no model performance result is admitted yet.
- docs: the directed SharpeArena-to-SharpeBench artifact boundary is documented. SharpeArena owns the environment and sandbox, while SharpeBench remains the independent field evaluator; there is no reverse dependency on the full SharpeArena package.

### Fixed
- protocol/sim: the closed `Decision` contract rejects unknown fields, duplicate or unobserved symbols, non-finite or out-of-range targets, confidence, and spend at every transport boundary. Signed target weights now open real short positions, and financing uses gross exposure after a deliberate short target without moving the frozen long-only golden path.
- arena: the local OCI sandbox is digest-pinned and fail-closed, runs as a non-root user with no capabilities or network, a read-only root, bounded `noexec` temporary filesystems, explicit startup/execution timeouts, and live hostile readiness probes. A missing image is refused rather than pulled implicitly.
- examples: the legacy LLM adapter emits the canonical `Decision` wire shape and treats generation or parsing faults as faults instead of silent holds.
- paper: Figures 1 and 4 use the full text width; the 48-page PDF passed a complete 180-DPI rendered inspection with no clipped panels or dropped floats.

## [0.10.0] - 2026-08-25

### Changed
- paper: the verification re-review reports the seed-averaging interaction with clone collapse, corrects the pass-witness binding gate, thousand-agent corners, perturbation spread and drawdown range, and groups the findings without renumbering them ([ed14597](https://github.com/general-liquidity/sharpebench/commit/ed14597)).
- arena: window 003 is opened against the attested v0.9.0 scorer after the two pre-entry windows were superseded ([9e0b016](https://github.com/general-liquidity/sharpebench/commit/9e0b016)).

### Fixed
- release: publish `sharpebench-protocol` before `sharpebench-core`, which depends on it ([06cb970](https://github.com/general-liquidity/sharpebench/commit/06cb970)).

## [0.9.0] - 2026-08-25

### Added
- core/protocol: `DeclaredMandate`, an opt-in mandate declared on the submission (`absolute_return`, `relative_to`, `drawdown_capped`, `outperform_buy_and_hold`; the prior `long_only_beta` wire spelling remains a compatibility alias), scored by `rank_declared` / `score_agent_declared` as a labeled second verdict. A declaration selects which reliability question pass^k asks and never relaxes the DSR, bootstrap, process or host-mandate gates; none of the 36 declared evidence rows is eligible ([f49d93c](https://github.com/general-liquidity/sharpebench/commit/f49d93c)).
- sim/harness: a disclosed execution-noise profile and seed-leg evaluation make execution-seed variability material while preserving the deterministic default ([f49d93c](https://github.com/general-liquidity/sharpebench/commit/f49d93c)).

### Fixed
- statistics: pooled PSR, DSR and bootstrap inputs now average aligned execution-seed returns within a window before concatenating market time; malformed or unequal seed blocks fail closed. Scores stamp execution-seed topology, pooled observations, the fixed null and configured-versus-measured dispersion source.
- paper/docs: regenerated final evidence replaces pre-repair values. The default grid has no eligible agent in 4,608 cells; the risk-managed control is not a deflation-only refusal; the thousand-agent daily-crypto diagnostic peaks at DSR 0.2500 while the operational path peaks at 0.0012; two empty forward-window records are superseded and no window is open.
- release: recovery runs dispatched with `release_tag` now run the npm and Python manifest guards and the registry verify job instead of skipping them, so a recovery cannot report success without proving every registry serves the tag; a failed crate publish prints the exact trusted-publisher configuration to add on crates.io (this commit).

## [0.8.0] - 2026-08-24

### Added
- core: `ScoreConfig.min_measured_trials_sr_std`, a precommitted annualized floor (default 0.5) under the field-measured deflation dispersion, recorded on every score as `TrialsSrStdSource::MeasuredFloored` when it binds; distinct low-dispersion submissions can no longer relax the prior below it ([a5d59e3](https://github.com/general-liquidity/sharpebench/commit/a5d59e3)).
- core: the effective trial count is `max(N_host, n_field) + N_declared`, so a host floor cannot fall below the number of submitted strategies actually observed; a regression test proves a host configured with `N = 1` cannot score an eight-entry field below eight trials ([a5d59e3](https://github.com/general-liquidity/sharpebench/commit/a5d59e3)).
- arena: windows carry an optional SHA-256 commitment to a SharpeArena sealed-evaluation salt (`open_window_with_sealed_eval_commitment`, CLI flag), pairing the sealed-seed commit-reveal protocol with forward attestation ([a5d59e3](https://github.com/general-liquidity/sharpebench/commit/a5d59e3)).
- ci: `Forward arena clock`, a daily scheduled workflow that advances the committed arena epoch only when a recorded commit deadline or reveal epoch has arrived; scoring and publishing stay manual because they need the sealed bundle and the signing key ([67f0dd4](https://github.com/general-liquidity/sharpebench/commit/67f0dd4)).
- paper: `paper/evidence/provenance.json` hashes the source snapshot and every admitted evidence and figure artifact ([44e845c](https://github.com/general-liquidity/sharpebench/commit/44e845c)).

### Changed
- paper: re-derived every number under the floored, effective-N kernel. The Sybil audit's undefended lift is now 0.0000 to 0.9522 at effective `N = 207`; the pass witness opens at per-period Sharpe 0.35 weekly (annualized 2.52) and 0.20 daily (3.17); the thousand-agent floor's operational maxima are exactly zero under the floor (0.0302 unfloored diagnostic); the default table's only nonzero leading DSRs are 0.004596 weekly US, 0.023492 weekly crypto and 0.028517 daily crypto; Finding 2 now states that deflation and regime robustness refuse independently ([44e845c](https://github.com/general-liquidity/sharpebench/commit/44e845c)).
- paper: the PSR text names the normal approximation, per-period Sharpe and non-excess kurtosis, and no longer calls the per-run 0.90 conjunction a simultaneous 90 percent confidence statement ([44e845c](https://github.com/general-liquidity/sharpebench/commit/44e845c)).

### Fixed
- cli, core: the sealed arena option compiles under the release profile ([f589629](https://github.com/general-liquidity/sharpebench/commit/f589629)).
- paper: the relative-mandate evidence log is committed beside its records ([c667b5e](https://github.com/general-liquidity/sharpebench/commit/c667b5e)).

## [0.7.0] - 2026-08-24
### Added
- core: `rank` collapses near-clone entries before measuring `trials_sr_std` (`dedup_clones_for_measured_sr_std`, default on): pooled streams whose absolute cosine meets a dedicated clone-collapse constant (0.995, distinct from the rediscovery screen's 0.97; sock puppets sit at 0.99999 or above while honest long-only agents on small universes reach 0.990) form connected-component clusters that vote once with their median Sharpe, both in the dispersion estimate and in the five-agent measurement floor. Clones stay scored and on the board. The ninth self-audit case is now a defense: with the collapse off, 200 sock puppets shrink a seven-agent honest field's dispersion from 0.3258 to 0.0559 and lift a borderline agent's DSR from 0.0000 to 0.9756; with it on the field measures 0.3018, the agent stays at 0.0000 and refused, the regression test asserts both halves, and a harness test rebuilds every committed evidence field and asserts zero merges at the collapse constant. `sharpebench audit` reports nine defended attacks and no known gaps ([338a68c](https://github.com/general-liquidity/sharpebench/commit/338a68c)).
- core, cli, py: opt-in `PassMode::RelativeToBenchmark`, a per-run reliability verdict on the excess return over the same-window, same-seed run of a named benchmark agent in the same field (`benchmark_agent_id`, default `buy-and-hold`). A zero-excess series fails, a missing or misaligned benchmark cell fails; every other gate is unchanged and the golden fixtures are byte-identical under the default. Selected with `sharpebench run|score --pass-mode relative-to-benchmark [--benchmark-agent <id>]`, `ScoreConfig::relative_to_benchmark(id)`, or `relative_to_benchmark_config(id)`; `per_run_passes` exposes the per-run vector ([338a68c](https://github.com/general-liquidity/sharpebench/commit/338a68c)).
- harness: `relative_mandate_eval` example scores the risk-managed field on all nine datasets under the default and the relative verdict; evidence committed as `paper/evidence/final/relative-mandate.jsonl` (81 records). No agent is eligible or passes pass^k under the relative verdict on any dataset ([338a68c](https://github.com/general-liquidity/sharpebench/commit/338a68c)).
- paper: two evidence figures, the pass-witness boundary (`evidence-pass-witness.pdf`) and the thousand-agent luck-floor distribution (`evidence-luck-floor-1000.pdf`), both reductions over the committed records by `paper/src/make-evidence-figures.py` ([338a68c](https://github.com/general-liquidity/sharpebench/commit/338a68c)).

### Changed
- paper: the Sybil case is described as defended everywhere it was described as a known gap (abstract, introduction, benchmark, integrity, limitations, conclusion, reproducibility, commands appendix), and the relative-mandate verdict is added as an experiments subsection with its table; `docs/book` integrity and pass-k chapters updated to match ([338a68c](https://github.com/general-liquidity/sharpebench/commit/338a68c)).
- paper: restore the two `\texttt` commands a heredoc had stripped from the thousand-agent floor paragraph, and rebuild the PDF ([e331244](https://github.com/general-liquidity/sharpebench/commit/e331244), [2e0e648](https://github.com/general-liquidity/sharpebench/commit/2e0e648)).

### Fixed
- release: the workflow accepts a `release_tag` input so a recovery or verify run targets one existing tag instead of whatever ref triggered it; every job checks out that tag and the crates job asserts the workspace manifest matches it ([19a7a54](https://github.com/general-liquidity/sharpebench/commit/19a7a54)).

## [0.6.0] - 2026-08-24

### Added
- core: self-audit cases carry `expected_vulnerable`, the report carries `known_gaps`, and a ninth case reproduces the field-level Sybil attack on the measured-deflation path (200 near-clone entries shrink the measured trial dispersion and lift a borderline agent past the bar). It is recorded as a known gap, not a defense; `sharpebench audit` prints it as `KNOWN GAP` ([8e56523](https://github.com/general-liquidity/sharpebench/commit/8e56523)).
- harness: `luck_floor_1000` example scores 1,000 seeded random agents on `us-indices-1d` and `crypto-majors-1d` under both the configured and the field-measured deflation path; evidence committed as `paper/evidence/final/luck-floor-1000.jsonl` ([8e56523](https://github.com/general-liquidity/sharpebench/commit/8e56523)).
- harness: `pass_witness` example scores a synthetic agent family with a controlled injected edge beside a zero-edge field to locate the eligibility boundary and prove the acceptance region is nonempty ([95a80e2](https://github.com/general-liquidity/sharpebench/commit/95a80e2)).
- harness: `llm_field_eval` example and `examples/llm-agent/` stdio adapter for the pending frontier-model field; fails closed on provider, credit or budget errors and publishes a score file only after every model and dataset completes ([8e56523](https://github.com/general-liquidity/sharpebench/commit/8e56523)).
- arena: window `window-001` opened in the committed `arena/` state directory with its scoring config and the host Ed25519 verifying key recorded before any entry exists ([8e56523](https://github.com/general-liquidity/sharpebench/commit/8e56523)).
- harness: N-sensitivity sweep in the risk-managed evaluation ([95a80e2](https://github.com/general-liquidity/sharpebench/commit/95a80e2)).

### Changed
- paper: five-seat panel revision. Title becomes "SharpeBench: A Luck-Robust Benchmark for Trading Agents"; the risk-managed result is conditioned on the never-catastrophic verdict; forward-evaluation prior art, FinRL-Meta and InvestorBench join related work; per-cell table provenance recorded ([95a80e2](https://github.com/general-liquidity/sharpebench/commit/95a80e2)).
- docs: `integrity.md` and `cli.md` describe the nine-case battery (eight defended, one expected-vulnerable) ([8e56523](https://github.com/general-liquidity/sharpebench/commit/8e56523)).

### Fixed
- sim: `RandomAgent` no longer collapses into buy-and-hold on a one-symbol universe; it draws gross exposure per seed instead of a normalized weight, so the luck floor varies on single-symbol datasets (`rates-1d` evidence regenerated) ([95a80e2](https://github.com/general-liquidity/sharpebench/commit/95a80e2)).
- release: `sharpebench-arena` is published (the CLI depends on it, so the v0.5.0 crates leg failed on the CLI); added to the publish and verify loops in dependency order ([829efe4](https://github.com/general-liquidity/sharpebench/commit/829efe4)).

## [0.5.0] - 2026-08-24

### Added
- arena: `sharpebench-arena`, the forward league driver. File-backed window lifecycle that fixes scoring rules before entries exist, refuses commitments after the deadline epoch, verifies reveals against commitments, scores with the recorded config, and publishes Ed25519 boards that chain across windows. External agents run in a network-isolated Docker container with bounded CPU and memory; no Docker is a hard refusal ([5790089](https://github.com/general-liquidity/sharpebench/commit/5790089)).
- cli: `import csv|stockbench` converts a rival board's per-period return series into a scoreable field, embedding the no-trace and unknown-trials caveats; `import stockbench` is a documented refusal because no per-agent series exist publicly ([81430f8](https://github.com/general-liquidity/sharpebench/commit/81430f8)).
- sim, harness: `RiskManaged` reference agent (trend filter, inverse-vol target, 10 percent drawdown halt) and perturbed-window generation that stays inside the original series' bar-to-bar range ([e4196cc](https://github.com/general-liquidity/sharpebench/commit/e4196cc)).
- core: every score reports `process_score` and `process_warnings`; within a DSR tie band the cleaner process orders first. Economic-rationality and role attribution are elicited from what a frozen trace supports ([71018ae](https://github.com/general-liquidity/sharpebench/commit/71018ae)).
- py: `rank_board`, `score_one`, `rank_returns`, `default_score_config`, `never_catastrophic_config` take and return the CLI's wire JSON ([6ef1a54](https://github.com/general-liquidity/sharpebench/commit/6ef1a54)).
- cli, wasm, npm, mcp: `select`, `disqualify`, `rediscover`, `uncertainty` and `decay-prior` subcommands, with WASM, npm and MCP surfaces for the four that make sense in a browser ([f6ad5e3](https://github.com/general-liquidity/sharpebench/commit/f6ad5e3), [5abbf14](https://github.com/general-liquidity/sharpebench/commit/5abbf14)).
- release: SLSA build provenance on the musl release binary, verifiable with `gh attestation verify` ([d203825](https://github.com/general-liquidity/sharpebench/commit/d203825)).
- paper: methodology paper draft with committed evidence records, the full nine-dataset grid (4,608 cells), evidence figures and the inverse comparison table. Later regenerated evidence supersedes the early risk-managed ``deflation only'' description ([d48295b](https://github.com/general-liquidity/sharpebench/commit/d48295b), [8cb43db](https://github.com/general-liquidity/sharpebench/commit/8cb43db), [442cd3d](https://github.com/general-liquidity/sharpebench/commit/442cd3d), [73a054f](https://github.com/general-liquidity/sharpebench/commit/73a054f), [25431ee](https://github.com/general-liquidity/sharpebench/commit/25431ee)).

### Changed
- paper: title aligned with the product tagline ([930e41d](https://github.com/general-liquidity/sharpebench/commit/930e41d), [a459f78](https://github.com/general-liquidity/sharpebench/commit/a459f78)).
- readme: evidence section, arena and analysis commands, Python ranker row, accurate sandbox note; version references removed from prose that is not a fact about a release ([73a054f](https://github.com/general-liquidity/sharpebench/commit/73a054f), [0ee09c2](https://github.com/general-liquidity/sharpebench/commit/0ee09c2), [c0fa02d](https://github.com/general-liquidity/sharpebench/commit/c0fa02d)).
- repo: LaTeX build artifacts untracked ([d8ad245](https://github.com/general-liquidity/sharpebench/commit/d8ad245)).

### Fixed
- sim: wall-clock timeout on the stdio agent transport (dedicated reader thread, 30s default); a silent agent is recorded as a `Timeout` fault. CI gains per-job timeouts, rust-cache and per-ref concurrency ([9729b2e](https://github.com/general-liquidity/sharpebench/commit/9729b2e)).

## [0.4.0] - 2026-08-23

### Added
- core: `ScoreConfig.pass_mode` (default `All`, byte-identical) and `Mandate.max_run_drawdown`; the `reliability_never_catastrophic(max_run_dd)` preset sets `Any` mode with a per-run drawdown bound so both reliability verdicts can be compared from config ([01a3224](https://github.com/general-liquidity/sharpebench/commit/01a3224)).
- harness: the evidence sweep scores every cell under both verdicts (`rank_eligible` and `eligible_never_catastrophic`) on identical trajectories, and a dataset's grid can be split across processes by `dsr_bar` ([15ff9d4](https://github.com/general-liquidity/sharpebench/commit/15ff9d4), [0925876](https://github.com/general-liquidity/sharpebench/commit/0925876)).

## [0.3.0] - 2026-08-23

### Fixed
- core: the deflation thresholds are stated annualized and converted per period once. `trials_sr_std = 0.5` had been applied per period, which demanded an annualized Sharpe of 18 on daily bars and 106 on hourly bars. `ScoreConfig` gains `periods_per_year` (default 252) and `per_run_min_annual_sharpe`; a `Deflation` type makes the measured path structurally unable to be converted twice; `run --periods-per-year` prints the value in the header. Golden fixtures regenerated ([f41acb7](https://github.com/general-liquidity/sharpebench/commit/f41acb7)).

### Changed
- harness: the evidence sweep pins `periods_per_year` per dataset and gains a dataset filter ([c3eb4b6](https://github.com/general-liquidity/sharpebench/commit/c3eb4b6)).

## [0.2.1] - 2026-08-23

### Added
- harness: `evidence_sweep` example scores every frozen dataset with the reference agents and a luck floor across a grid of `dsr_bar`, `n_trials` and `trials_sr_std`, one JSON record per cell ([9b5ad80](https://github.com/general-liquidity/sharpebench/commit/9b5ad80)).

### Fixed
- test: the simulator golden test moved to `sharpebench-sim` so `sharpebench-core` has no dev-dependency cycle; the v0.2.0 crates publish had failed on it, leaving crates.io without 0.2.0 ([d0ae4f2](https://github.com/general-liquidity/sharpebench/commit/d0ae4f2)).

## [0.2.0] - 2026-08-23

### Added
- attest: Ed25519 public signing of the board chain alongside HMAC; `PublicChain` carries its verifying key and `verify_public_chain_with` pins one. CLI `sign --ed25519`, `verify --pubkey|--public`, and a `regime` subcommand ([37f0090](https://github.com/general-liquidity/sharpebench/commit/37f0090)).
- test: golden-score fixtures at full `f64` precision, a WASM-native parity test, and a three-OS CI matrix ([f0884f6](https://github.com/general-liquidity/sharpebench/commit/f0884f6)).
- data: seven more frozen datasets (crypto majors 1h/4h/1w, US indices weekly, FX majors, WTI and Brent, 10y Treasury yield), each with a sha256 sidecar and a `--check` fetch mode ([0de7272](https://github.com/general-liquidity/sharpebench/commit/0de7272)).

### Changed
- docs: every headline claim matched to the code (eight attacks, twelve crates, the exact eligibility predicate, look-ahead wording, HMAC versus Ed25519); MCP reports its version from `package.json` and registers a `regime_compare` tool ([106db21](https://github.com/general-liquidity/sharpebench/commit/106db21)).

### Fixed
- core: `rank` restricts every submission to the shared run set before scoring (`shared_run_set`, default on); `trials_sr_std` is measured from the field when at least five agents qualify and falls back to the configured value otherwise; `METHODOLOGY_VERSION` reads the workspace version ([1ba8bac](https://github.com/general-liquidity/sharpebench/commit/1ba8bac)).

## [0.1.0] - 2026-08-20

### Added
- core: eighth self-audit case (adversarial input inside the in-sample range) and a three-leg uncertainty decomposition (aleatoric, epistemic, distributional) ([fe78603](https://github.com/general-liquidity/sharpebench/commit/fe78603)).
- core: `compare_by_regime` regime-conditional distributional comparison with a `pooled_hides_reversal` flag, and a crowding-implied decay half-life prior (reported, never gating) ([9f56910](https://github.com/general-liquidity/sharpebench/commit/9f56910)).
- stats: `percentile_selection` picks a candidate on a bootstrap percentile rather than the observed best ([cd3cd44](https://github.com/general-liquidity/sharpebench/commit/cd3cd44)).

### Changed
- stats: one shared SplitMix64 across `significance` and `selection` ([cd3cd44](https://github.com/general-liquidity/sharpebench/commit/cd3cd44)).

## [0.0.14] - 2026-07-20

### Added
- core, py: `budget_curve`, a luck-robust performance-versus-budget analyzer (per-point OOS deflated Sharpe, marginal DSR per budget, overfit onset, selection-deflated peak) ([4254bd0](https://github.com/general-liquidity/sharpebench/commit/4254bd0), [f079957](https://github.com/general-liquidity/sharpebench/commit/f079957), [5ff3f7d](https://github.com/general-liquidity/sharpebench/commit/5ff3f7d)).

### Changed
- ci: the workspace-excluded `sharpebench-py` crate is linted, built and tested; every job pins the `rust-toolchain.toml` channel ([a97b0aa](https://github.com/general-liquidity/sharpebench/commit/a97b0aa), [03c25dd](https://github.com/general-liquidity/sharpebench/commit/03c25dd), [4c8fca8](https://github.com/general-liquidity/sharpebench/commit/4c8fca8)).

## [0.0.13] - 2026-07-20

### Added
- py: `sharpebench-py`, a maturin-built pyo3 binding on PyPI exposing deflated Sharpe, PSR, PBO and the data-snooping family to Python ([4f91a3a](https://github.com/general-liquidity/sharpebench/commit/4f91a3a), [e16044a](https://github.com/general-liquidity/sharpebench/commit/e16044a)).

## [0.0.12] - 2026-07-20

### Added
- stats, core, leaderboard: bootstrap DSR confidence interval and `runs_for_power`; `rank` groups agents whose DSR CIs overlap into tie bands and the leaderboard renders the band and interval instead of a bare rank; a skilled-human Sharpe band as a DSR reference ([ad3025f](https://github.com/general-liquidity/sharpebench/commit/ad3025f), [f1e953d](https://github.com/general-liquidity/sharpebench/commit/f1e953d), [914bcf1](https://github.com/general-liquidity/sharpebench/commit/914bcf1), [78c141a](https://github.com/general-liquidity/sharpebench/commit/78c141a)).
- memory: per-arm `ArmCost` and cost-normalized lift (`lift_per_token`, `lift_per_latency`) ([b512d76](https://github.com/general-liquidity/sharpebench/commit/b512d76)).
- protocol, sim: optional `DecisionCost` on `Decision` accumulates into `Run.cost`, making `return_per_cost` and `dsr_per_cost` live for external agents ([1e028a6](https://github.com/general-liquidity/sharpebench/commit/1e028a6)).
- sim, harness, cli: transport faults are typed (`DecideError`), retried, and tripped through a circuit breaker instead of degrading silently to a hold; the harness maps them to `FailureKind` ([a6e5e26](https://github.com/general-liquidity/sharpebench/commit/a6e5e26)).
- harness, cli: resumable checkpointed sweep for the external-agent path (`run --checkpoint <path>`), byte-identical to an uninterrupted run ([4d08690](https://github.com/general-liquidity/sharpebench/commit/4d08690)).

### Fixed
- release: cargo-release rewrites the npm manifests, the npm job asserts its manifests match the tag, and a verify job queries every registry afterwards; publishing runs from CI only, since the crates are trusted-publishing-only ([00de78b](https://github.com/general-liquidity/sharpebench/commit/00de78b), [94ea2a3](https://github.com/general-liquidity/sharpebench/commit/94ea2a3), [4a35e96](https://github.com/general-liquidity/sharpebench/commit/4a35e96)).

## [0.0.11] - 2026-07-03

### Added
- stats: Cont stylized-facts realism validator; `sharpebench realism` subcommand and a CI gate over both frozen datasets ([1021566](https://github.com/general-liquidity/sharpebench/commit/1021566), [57641a3](https://github.com/general-liquidity/sharpebench/commit/57641a3)).
- core: `FailReason` disqualification taxonomy with `classify_disqualification` and a suite `rollup` ([bb5dddb](https://github.com/general-liquidity/sharpebench/commit/bb5dddb)).
- stats, edge: Benjamini-Hochberg FDR control and a Harvey-Liu-Zhu t >= 3.0 factor gate in the full verdict ([274589d](https://github.com/general-liquidity/sharpebench/commit/274589d), [45aafc6](https://github.com/general-liquidity/sharpebench/commit/45aafc6)).
- memory: `sharpebench-memory` crate with the three-arm ablation and the E1 poisoning, E2 multi-session, E3 point-in-time and E6 confabulation legs ([3caae8d](https://github.com/general-liquidity/sharpebench/commit/3caae8d), [7cd3152](https://github.com/general-liquidity/sharpebench/commit/7cd3152), [1331303](https://github.com/general-liquidity/sharpebench/commit/1331303), [67c5905](https://github.com/general-liquidity/sharpebench/commit/67c5905), [7fb014c](https://github.com/general-liquidity/sharpebench/commit/7fb014c)).

### Fixed
- release: publish `sharpebench-edge` before `sharpebench-wasm` ([137fef2](https://github.com/general-liquidity/sharpebench/commit/137fef2)).

## [0.0.10] - 2026-06-27

### Added
- wasm, npm, mcp: `is_my_sharpe_real` and its full variant reach JS/TS and the MCP server; npm packages synced to the workspace version ([6ed0593](https://github.com/general-liquidity/sharpebench/commit/6ed0593)).

### Fixed
- ci: the MCP package tests against the locally built sibling before it is published ([9e35bdd](https://github.com/general-liquidity/sharpebench/commit/9e35bdd)).

## [0.0.9] - 2026-06-27

### Added
- stats: `sharpebench-stats` extracted from core (PSR, deflated Sharpe, the data-snooping family, selection); core re-exports it ([820528d](https://github.com/general-liquidity/sharpebench/commit/820528d)).
- edge: `sharpebench-edge` with MinTRL, PBO via CSCV and the two-tier `is_my_sharpe_real` verdict; `sharpebench check <returns.csv> --trials N` ([758ae11](https://github.com/general-liquidity/sharpebench/commit/758ae11)).
- docs: simulator chapter in the mdBook and doctest-checked Rust examples in the README ([073f9ee](https://github.com/general-liquidity/sharpebench/commit/073f9ee), [3a4a13f](https://github.com/general-liquidity/sharpebench/commit/3a4a13f)).

## [0.0.8] - 2026-06-27

### Added
- sim: TRF turnover cost (`CostModel.trf_cost`), `synthetic_parameterized` volatility/jump generator, and O(1) `clone_state` / `restore_state` snapshots ([c641929](https://github.com/general-liquidity/sharpebench/commit/c641929)).

## [0.0.7] - 2026-06-26

### Added
- sim: Gym-style `TradingEnv` (`reset` / `step`) sharing one `step_once` body with `run_backtest`, plus the `Scenario` bundle and `crisis_suite` ([207e026](https://github.com/general-liquidity/sharpebench/commit/207e026)).

## [0.0.6] - 2026-06-25

### Changed
- release: crates.io and npm publishing over OIDC trusted publishing on a `v*` tag; CI on Node 24 and Actions v6 ([06a7359](https://github.com/general-liquidity/sharpebench/commit/06a7359), [7a4d7ea](https://github.com/general-liquidity/sharpebench/commit/7a4d7ea), [74dc1ac](https://github.com/general-liquidity/sharpebench/commit/74dc1ac), [4b814a0](https://github.com/general-liquidity/sharpebench/commit/4b814a0)).

### Fixed
- release: per-crate idempotent publish and a wait for npm dependency propagation ([0ceac17](https://github.com/general-liquidity/sharpebench/commit/0ceac17)).

## [0.0.5] - 2026-06-24

### Fixed
- npm: 0.0.4 shipped without the wasm because wasm-pack regenerated `pkg/.gitignore`; stripped at publish ([8456fd9](https://github.com/general-liquidity/sharpebench/commit/8456fd9)).

## [0.0.4] - 2026-06-24

### Added
- npm: `@general-liquidity/sharpebench` typed package over the full WASM kernel surface, and `@general-liquidity/sharpebench-mcp` exposing it as MCP tools ([1949ce5](https://github.com/general-liquidity/sharpebench/commit/1949ce5), [f3a4cbb](https://github.com/general-liquidity/sharpebench/commit/f3a4cbb)).
- core: Sortino and downside deviation reported; `TailSellingExposure` is a block-severity process event and the seventh self-audit attack ([33d5da0](https://github.com/general-liquidity/sharpebench/commit/33d5da0)).

### Changed
- docs: README rewritten with surface, data and tech-stack tables; npm build/test CI ([ba175f6](https://github.com/general-liquidity/sharpebench/commit/ba175f6)).

### Fixed
- npm: packages scoped to `@general-liquidity`; test discovery scoped per package ([b944225](https://github.com/general-liquidity/sharpebench/commit/b944225), [2867724](https://github.com/general-liquidity/sharpebench/commit/2867724)).

## [0.0.3] - 2026-06-23

### Added
- core, attest: briefing-neutrality audit, allocation scoring with turnover penalty, Black-Scholes Greeks with a tail-selling classifier, and the canary contamination tripwire, each with a CLI subcommand ([a8e915e](https://github.com/general-liquidity/sharpebench/commit/a8e915e), [6fe5d3d](https://github.com/general-liquidity/sharpebench/commit/6fe5d3d)).
- cli: opt-in `self-update` behind a feature flag ([9c45bdf](https://github.com/general-liquidity/sharpebench/commit/9c45bdf)).

## [0.0.2] - 2026-06-23

### Added
- core: `in_sample_trials` folded into deflation, best-vs-median selection, percentile against a reference population, ordinal rank ([c09c79f](https://github.com/general-liquidity/sharpebench/commit/c09c79f)).
- core, sim: rolling worst-window Sharpe, `dsr_per_cost`, the process floor, and swappable execution-cost profiles ([9875356](https://github.com/general-liquidity/sharpebench/commit/9875356)).
- core, attest: comparison sets, the rediscovery classifier, and sealed held-out datasets ([e22a185](https://github.com/general-liquidity/sharpebench/commit/e22a185)).
- protocol, sim, harness, cli: the decisions-only trajectory artifact with `capture` and `verify-trajectory` replay ([98593dd](https://github.com/general-liquidity/sharpebench/commit/98593dd)).
- harness: per-order rationale, the runtime-versus-agent failure taxonomy with bounded retries, a cheat-agent demotion case, and self-describing signed boards ([9d45b22](https://github.com/general-liquidity/sharpebench/commit/9d45b22)).
- release: cargo-release pipeline ([21bd488](https://github.com/general-liquidity/sharpebench/commit/21bd488)).

## [0.0.1] - 2026-06-22

First published release.

### Added
- The scoring kernel: deflated Sharpe and PSR, pass^k, stationary-bootstrap significance, process discipline, edge decay, calibration, and the gated composite ([55da087](https://github.com/general-liquidity/sharpebench/commit/55da087)); alpha/beta attribution, White's Reality Check, Hansen SPA and consistent SPA, Romano-Wolf step-down, mandate drawdown cap, turnover and Pareto-optimality, selectable rank key, role attribution, crowdedness, OOS decay, economic-rationality tests ([572b9eb](https://github.com/general-liquidity/sharpebench/commit/572b9eb), [10e6ca6](https://github.com/general-liquidity/sharpebench/commit/10e6ca6), [769b729](https://github.com/general-liquidity/sharpebench/commit/769b729), [2af49fd](https://github.com/general-liquidity/sharpebench/commit/2af49fd), [382c8e7](https://github.com/general-liquidity/sharpebench/commit/382c8e7), [d80cee8](https://github.com/general-liquidity/sharpebench/commit/d80cee8), [c4c5993](https://github.com/general-liquidity/sharpebench/commit/c4c5993), [60bfa52](https://github.com/general-liquidity/sharpebench/commit/60bfa52), [fc9caa8](https://github.com/general-liquidity/sharpebench/commit/fc9caa8), [d76d613](https://github.com/general-liquidity/sharpebench/commit/d76d613), [15915ba](https://github.com/general-liquidity/sharpebench/commit/15915ba), [e44c789](https://github.com/general-liquidity/sharpebench/commit/e44c789)).
- The point-in-time simulator: reference agents, walk-forward windows with regime tagging, flash-crash and whipsaw stress, contamination masking, manipulative-order detection, square-root impact, financing and liquidity caps, dividends, the random-agent luck floor, and CSV loading with frozen Binance and FRED datasets ([3a8fc72](https://github.com/general-liquidity/sharpebench/commit/3a8fc72), [d627823](https://github.com/general-liquidity/sharpebench/commit/d627823), [c14b630](https://github.com/general-liquidity/sharpebench/commit/c14b630), [1df8f2d](https://github.com/general-liquidity/sharpebench/commit/1df8f2d), [0218be6](https://github.com/general-liquidity/sharpebench/commit/0218be6), [3c04209](https://github.com/general-liquidity/sharpebench/commit/3c04209), [ad32315](https://github.com/general-liquidity/sharpebench/commit/ad32315), [2f56b47](https://github.com/general-liquidity/sharpebench/commit/2f56b47), [7c073ae](https://github.com/general-liquidity/sharpebench/commit/7c073ae), [d3b9409](https://github.com/general-liquidity/sharpebench/commit/d3b9409), [f6f3000](https://github.com/general-liquidity/sharpebench/commit/f6f3000)).
- Forward attestation: SHA-256 commitments, the HMAC-signed result chain, the epoch registry with time-lock, and the signed leaderboard with `sign` / `verify` ([111eab9](https://github.com/general-liquidity/sharpebench/commit/111eab9), [54c9c96](https://github.com/general-liquidity/sharpebench/commit/54c9c96), [38635e6](https://github.com/general-liquidity/sharpebench/commit/38635e6), [ff2e831](https://github.com/general-liquidity/sharpebench/commit/ff2e831)).
- External agents over stdio and HTTP, `run --http|--cmd`, and a Rust reference agent with a Dockerfile ([895e42e](https://github.com/general-liquidity/sharpebench/commit/895e42e), [cd363f3](https://github.com/general-liquidity/sharpebench/commit/cd363f3), [28866c1](https://github.com/general-liquidity/sharpebench/commit/28866c1), [48a1b9c](https://github.com/general-liquidity/sharpebench/commit/48a1b9c)).
- The five-attack self-audit, the `--json` flag, the WASM build, the mdBook, cargo-deny, and a Nix flake ([6b749b9](https://github.com/general-liquidity/sharpebench/commit/6b749b9), [cb0140b](https://github.com/general-liquidity/sharpebench/commit/cb0140b), [7f766c4](https://github.com/general-liquidity/sharpebench/commit/7f766c4), [e4f8c3b](https://github.com/general-liquidity/sharpebench/commit/e4f8c3b), [426f140](https://github.com/general-liquidity/sharpebench/commit/426f140), [9f12535](https://github.com/general-liquidity/sharpebench/commit/9f12535)).

### Changed
- Crates renamed from `sb-*` to `sharpebench-*` with the binary `sharpebench`; Python data fetchers replaced by a Rust `xtask` ([0fef64b](https://github.com/general-liquidity/sharpebench/commit/0fef64b), [2af52f0](https://github.com/general-liquidity/sharpebench/commit/2af52f0), [533a92d](https://github.com/general-liquidity/sharpebench/commit/533a92d)).

### Fixed
- Constant-time HMAC verification and bounded, timed agent HTTP reads ([6c3d174](https://github.com/general-liquidity/sharpebench/commit/6c3d174)).

[0.15.0]: https://github.com/general-liquidity/sharpebench/compare/v0.14.1...v0.15.0
[0.14.1]: https://github.com/general-liquidity/sharpebench/compare/v0.14.0...v0.14.1
[0.14.0]: https://github.com/general-liquidity/sharpebench/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/general-liquidity/sharpebench/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/general-liquidity/sharpebench/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/general-liquidity/sharpebench/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/general-liquidity/sharpebench/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/general-liquidity/sharpebench/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/general-liquidity/sharpebench/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/general-liquidity/sharpebench/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/general-liquidity/sharpebench/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/general-liquidity/sharpebench/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/general-liquidity/sharpebench/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/general-liquidity/sharpebench/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/general-liquidity/sharpebench/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/general-liquidity/sharpebench/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/general-liquidity/sharpebench/compare/v0.0.14...v0.1.0
[0.0.14]: https://github.com/general-liquidity/sharpebench/compare/v0.0.13...v0.0.14
[0.0.13]: https://github.com/general-liquidity/sharpebench/compare/v0.0.12...v0.0.13
[0.0.12]: https://github.com/general-liquidity/sharpebench/compare/v0.0.11...v0.0.12
[0.0.11]: https://github.com/general-liquidity/sharpebench/compare/v0.0.10...v0.0.11
[0.0.10]: https://github.com/general-liquidity/sharpebench/compare/v0.0.9...v0.0.10
[0.0.9]: https://github.com/general-liquidity/sharpebench/compare/v0.0.8...v0.0.9
[0.0.8]: https://github.com/general-liquidity/sharpebench/compare/v0.0.7...v0.0.8
[0.0.7]: https://github.com/general-liquidity/sharpebench/compare/v0.0.6...v0.0.7
[0.0.6]: https://github.com/general-liquidity/sharpebench/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/general-liquidity/sharpebench/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/general-liquidity/sharpebench/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/general-liquidity/sharpebench/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/general-liquidity/sharpebench/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/general-liquidity/sharpebench/releases/tag/v0.0.1
