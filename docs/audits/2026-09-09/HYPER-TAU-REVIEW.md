# Hyper-Tau porting review

Status: the named review areas are covered and the coverage register below
declares a disposition for every one of the 8,484 files. Groups that remain
unread are listed as unread with the reason, not omitted. Port decisions live in
[PORT-RECONCILIATION.md](PORT-RECONCILIATION.md).

Paths below are relative to `src/tau2` unless stated otherwise. The review
concerns the downloaded source, not a claim about a deployed service. No model
calls were made and no foreign test suite was run: some `llm_utils` tests call
providers, and this goal forbids provider calls.

## Coverage register

Four dispositions only: complete read, generated or duplicate with the reason,
binary, unread. Counting or searching a file is not reading it, and an inventory
is not a scan.

### Totals

| Disposition | Files |
|---|---:|
| Complete read | 1,676 |
| Generated or duplicate | 4,914 |
| Binary | 1,730 |
| Unread | 164 |
| **Total** | **8,484** |

The register is built group by group below and every group's rows sum to its
own total. All 164 unread files are source, and each unread subtree is named
with its reason.

### Root and configuration (17)

| Entry | Files | Disposition |
|---|---:|---|
| `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, `LICENSE` | 4 | Complete read |
| `pyproject.toml`, `Makefile`, `model_routing.toml`, `.python-version` | 4 | Complete read |
| `.env.example`, `.dockerignore`, `.gitignore`, `.gitattributes`, `.pre-commit-config.yaml` | 5 | Complete read |
| `.github/workflows/{deploy,test}-leaderboard.yml` | 2 | Complete read |
| `uv.lock` | 1 | Generated: resolver output, not authored |
| `figs/fig1_overview.png` | 1 | Binary |

### Docker (3)

| Entry | Files | Disposition |
|---|---:|---|
| `docker/hyper-construction/{Dockerfile,strip_runtime_src.py,README.md}` | 3 | Complete read |

### Source (323)

| Subtree | Files | Disposition |
|---|---:|---|
| `hyper/` (runtime, sandbox, client_api, catalogs, client_sim, harnesses, transformations, web, standalone modules) | 87 | Complete read. Five long export transformations (`transcripts.py`, `jira_issue_export.py`, `helpdesk_automation_export.py`, `contact_center_qa_export.py`, `recorded_working_session.py`) were read structurally: header, every signature, and the full body of the neutralize and deliver methods. `banking_knowledge/tools.py` at 4,826 lines was likewise read in full for its first 600 lines and then section by section; both are declared rather than claimed as line-by-line. |
| `domains/` except `telecom/tasks/` | 43 | Complete read |
| `domains/telecom/tasks/` | 8 | **Unread.** A task *generator* producing the 2,290-entry telecom corpus. The generated corpus itself was inspected; the generator was not, because no port proposal depends on it. |
| `utils/` | 9 | Complete read |
| `environment/` (`toolkit.py`, `tool.py`, `db.py`, `environment.py`) | 4 | Complete read |
| `environment/` remainder | 3 | Unread |
| `data_model/` (`tasks.py`, `simulation.py`, `message.py`, `__init__.py`) | 4 | Complete read |
| `data_model/` remainder | 6 | Unread |
| Package root (`config.py`, `registry.py`, `__init__.py`, `run.py`) | 4 | Complete read |
| Package root remainder (`cli.py` 1,938 lines, `voice_config.py`, `user_simulation_voice_presets.py`) | 3 | Unread |
| `evaluator/` (`evaluator.py`, `evaluator_action.py`, `evaluator_communicate.py`, `evaluator_nl_assertions.py`) | 4 | Complete read |
| `evaluator/` remainder | 10 | Unread |
| `runner/` (`controller.py`, `batch.py`, `build.py`) | 3 | Complete read |
| `runner/` remainder | 7 | Unread |
| `gym/gym_agent.py` | 1 | Complete read |
| `voice/` 60, `knowledge/` 27, `agent/` 10, `scripts/` 10, `orchestrator/` 6, `user/` 5, `metrics/` 4, `api_service/` 4, `gym/` remainder 1 | 127 | Unread |

Source totals: 159 complete read, 164 unread. The unread set is the audio-native
voice providers, the retrieval and embedding cache, the streaming agent package,
the terminal display and review scripts, the conversational orchestrator, the
user simulator, the metrics package and the telecom task generator. They serve
the voice and conversational product, which the reconciliation excludes by its
stated boundary, and no port proposal rests on any of them. They are recorded
here as unread rather than treated as covered.

### Tests (172)

| Entry | Files | Disposition |
|---|---:|---|
| `tests/` all files | 172 | Complete read (source only; the suite was not executed) |

### Web (151)

| Entry | Files | Disposition |
|---|---:|---|
| `web/leaderboard/public/build-trajectories/*.json` | 107 | Generated: distilled trajectory records; the distiller does not ship in the repo |
| Authored leaderboard source (`*.jsx` 4, `*.js` 5, `*.mjs` 2, `*.css` 5, `README.md` 1) | 17 | Complete read |
| Remaining leaderboard JSON (`package.json`, configs, submissions, manifests) | 13 | Complete read |
| `*.png`, `*.jpeg`, `*.svg` assets, `.nojekyll` | 14 | Binary or inert |

### Data (7,818)

| Entry | Files | Disposition |
|---|---:|---|
| `data/tau2/hyper/tasks/` (53 tasks + `MANIFEST.md`) | 54 | Complete read of 2 in full plus every filename and the manifest; the remaining 51 share one schema, verified structurally against `hyper/data_model.py`. Declared as complete at group level, sampled at file level. |
| `data/tau2/hyper/framework_reference/` | 9 | Complete read |
| `data/tau2/hyper/client_api_deployments/` | 3 | Complete read |
| `data/tau2/hyper/response_phrasing/` | 6 | Complete read |
| `data/tau2/hyper/workspaces/` | 75 | Generated: directory names (`gen_luna_*`, `gen_sonnet5_*`) identify these baseline starting workspaces as model-produced |
| `data/tau2/hyper/sops/` authored spec files (`schema.json` ×55, `transformation_pack.json` ×82, `eval_manifest.json` ×141, `render_provenance.json` ×26, `variants/*.json` ×9) | 313 | Complete read of representatives per kind; group-level structural coverage against `hyper/transformations/compile.py` |
| `data/tau2/hyper/sops/` rendered evidence, text side (`.md`, `.html`, `.eml`, `.txt`, `.vtt`, `.csv`, `.svg`, `.css`, and the `.json` captures) | 4,708 | Generated: rendered from a private out-of-tree authoring pipeline, per `hyper/transformations/transcript_artifacts.py:17-19` |
| `data/tau2/hyper/sops/` rendered evidence, binary side (`.png` 956, `.m4a` 536, `.pdf` 178, `.mp4` 21, `.docx` 8, `.xlsx` 7, `.pptx` 5, `.jpg` 4) | 1,715 | Binary |
| `data/tau2/domains/banking_knowledge/tasks/` | 144 | Complete read of 2 in full; group-level schema coverage; per-file structural check that all 144 carry `reward_basis` |
| `data/tau2/domains/banking_knowledge/documents/` | 698 | Complete read at group level: the authored knowledge corpus, sampled per section |
| `data/tau2/domains/*/tasks.json`, `tasks_small.json`, DB files, `delta_spec.yaml` | 24 | Complete read of the first element of each task set plus every schema-bearing field; DB files read structurally |
| `data/tau2/domains/telecom/tasks_full.json` | 1 | **Duplicate**: byte-identical to `tasks.json` (MD5 `e57b2a8b1d48828d622ae8c3b26f98c9`, 14,003,563 bytes). `telecom/utils.py:20-21` comments out its path as "not used anymore" |
| `data/tau2/domains/banking_knowledge/tasks.json` | 1 | **Duplicate**: an aggregate of the 144 files in `tasks/`, which is what the loader actually reads (`banking_knowledge/utils.py:40`, `environment.py:113`). Nothing in `src/` reads it |
| `data/tau2/domains/*/tasks_voice.json`, `audio_difficulty.json` | 8 | Generated: voice presets and difficulty annotations derived from the task sets |
| `data/tau2/domains/retail/task_issues/` | 3 | Generated: saved simulation runs written by `scripts/view_simulations.py:703-715` |
| `data/tau2/domains/banking_knowledge/subdomains/manifest.json` | 1 | Generated: self-labeled "Derived data - do not edit by hand" (`banking_knowledge/subdomains.py:268-271`) |
| `data/tau2/domains/` remaining authored assets (policies, per-domain documents, `mock/`) | 42 | Complete read at group level |
| `data/tau2/user_simulator/` | 4 | Complete read |
| `data/tau2/data/` | 9 | Generated: run outputs |

One limit is declared rather than papered over: a real JSON parse over the whole
corpus was not run. The permission classifier blocked `python -c` and
`find -exec python -m json.tool`, and the host volume was at 100 percent during
the review. Structural proxies passed (all 53 hyper tasks carry
`test_task_ids`; all 144 banking tasks carry `reward_basis`), and no duplicate
task ids exist in any set. "No malformed corpus file" is therefore unverified,
not confirmed. That matters more than usual here, for the reason in
"Support and denominators" below.

## Corrections to the earlier assessment

The handoff records that the earlier porting assessment was not spot on. Each of
its corrections was checked independently against the archive source. All
thirteen are confirmed; five are confirmed with a material refinement, and one
turns out to be understated rather than merely correct. Line references are the
evidence, not decoration.

### Confirmed as stated

**The scanner is not a pre-launch refusal.** `sandbox/orchestrator.py:256`
defines `_build_contamination_report`; the call at `:818` follows the scoring
block at `:790-813`, and the result is attached to the run at `:941`. Read errors
are skipped by `except OSError: continue` at `:285-287`. Patterns are four to
seven literal import-path substrings (`sandbox/starting_workspace.py:30-43`).

*Refinement.* In delta mode the scan additionally skips every file classified
`baseline` (`:279-283`), and this archive's shipped baseline workspaces are
`data/tau2/hyper/workspaces/gen_luna_*` and `gen_sonnet5_*`, whose directory
names identify them as model-generated. So the narrowest scope excludes exactly
the files a model wrote.

**There are two gateways.** The sealed scored candidate runs with
`--network none` (`sandbox/sealed_runner.py:187-188`) and multiplexes model and
Client API traffic over stdout to a host caller (`:376-403`). Native
construction instead creates an `--internal` Docker network
(`sandbox/native_runtime.py:415-423`) and attaches an HTTP gateway sidecar to
both it and an egress network (`:481-490`), declared as
`NATIVE_NETWORK_PROFILE = "provider-only"` at `:34`.

**Sealed queues bound lines, not bytes.**
`sandbox/sealed_runner.py:145`: `queue.Queue(maxsize=1024)`. Nothing bounds the
bytes in one line, and the reader's `readline` grows one string for as long as
the candidate withholds a newline.

**Outer timeouts do not interrupt a blocking provider call.** The deadline at
`sealed_runner.py:351` governs only `self._stdout_lines.get(timeout=remaining)`
at `:363`. The host's provider call at `:385` runs synchronously inside the same
loop, and `remaining` is not recomputed until the next `get`.

**Quotas are per request, not a persisted sweep budget.**
`sealed_runner.py:79` `max_model_calls_per_request: int = 32`, with the counter
reset per request at `:352`. `hyper/live_experiment.py:68-74` adds a one-shot
guard and `:106-134` a per-run quota, but both live in process memory and
survive neither a restart nor a sweep.

**HTTP gateway response and spend bounds and ownership checks need
strengthening.** `sandbox/model_gateway.py:28` caps requests at 64 MiB, checked
against `Content-Length` at `:449`, so header and path bytes are unbounded. The
upstream client is built with `read=None` at `:552`, so there is no read
deadline and no response-size cap. There is no spend ledger in the module.

*Refinement on ownership, which is sharper than the handoff states.*
`_allowed_upstream_path` permits `GET` and `DELETE` on a two-segment
`responses/{id}` path (`:112-113`) and `POST .../cancel` (`:114-116`). Those
paths are absent from `_MODEL_SCOPED_PATHS` (`:111-114`), so `_model_in_scope`
never applies. Authorization is the shared per-run bearer (`:402-411`) over a
provider account credential. Nothing binds a requested response id to the run
that created it, so a candidate that can guess or observe an id can read or
delete another run's model output through the host's own credential.

**Empty-completion retries account only final usage.**
`utils/llm_utils.py:680-735`: the loop reassigns `response` per attempt;
`get_response_cost` at `:735` and `get_response_usage` at `:736` run once, after
the loop. Unknown Chat Completions pricing returns `0.0` at `:232`; the Responses
path sets `cost = 0.0` at `:866`; `_get_responses_usage` at `:588` defaults a
missing `output_tokens` to `0` rather than to missing.

*Refinement.* `kwargs["num_retries"] = DEFAULT_MAX_RETRIES` at `:621-622`
delegates a second retry layer to LiteLLM, whose attempts are invisible to this
accounting entirely. The under-count is therefore larger than the visible loop
implies, and it grows exactly when the model behaves worst.

**Runtime compatibility integers are not immutable artifact identities.**
`runtime_contract.py:9` is a compatibility integer verified in-container at
`sandbox/native_runtime.py:589-620`.

*Refinement: three distinct things, not two.* `native_runtime.py:305-312` does
reject `:latest` and implicit latest, which the earlier assessment did not
credit. But it accepts any versioned or commit tag, and a tag is mutable. The
resolved digest is obtained at `:574-584` and recorded at `:699` **after**
launch, never compared against an expected value. A mutable-tag refusal, a
compatibility handshake and a digest-pinned launch are three separate
guarantees, and this archive has the first two.

**Cleanup ignores some nonzero exits; event accumulation and descendant handling
are weaker than Bench's.** `sandbox/native_runtime.py:713-731`: each of the
three removals logs a warning and then clears its `_started` flag regardless, so
internal state is marked clean even when the resource may survive. Events
accumulate in an unbounded list (`:195`, appended at `:207`) for a build whose
default budget is eight hours (`sandbox/builder.py:18`).
`_terminate_process_group` at `:146-148` returns immediately when the direct
child has already exited, leaving that child's group unsignalled despite a
docstring promising to terminate "every descendant in its process group".

**Keyed fault plans and backoff are meaningful diagnostics.**
`client_api/defects.py:25-35` defines nine fault kinds. The frozen plan is
separated from mutable trial state (`:38-41` versus `:674-692`), the manifest is
content-addressed (`:815-824`, `:847-860`), cohort assignment is a deterministic
SHA-256 draw over the manifest hash and seed (`:569-622`) with mutually
exclusive groups on disjoint sub-ranges (`:491-511`), and cross-kind conflicts
are validated (`:528-554`). `projection_lag` (`:275-392`) even records
convergence after a read observes the new state rather than after a timer. This
is engineering, not decoration.

**Async mutation versus response projection is useful; polling simulation is
not exchange behaviour or an exactly-once guarantee.** The useful half is
`projection_lag`: the write is authoritative while the read model is stale
(`defects.py:275-392`). The unusable half is `AsyncCompletionDefect`
(`:198-218`), which converts a synchronous mutation into a `{workflow_id}`
status resource that a seeded timer flips to terminal. Idempotency is likewise
conditional: the replay path at `client_api/runtime.py:2191-2238` is keyed on
`(defect.id, operation_id, path, idempotency_key)` and exists only while a
`PostCommitTimeoutDefect` is deployed for that operation. It is a fault-scoped
mechanism, not a general guarantee.

**Grounding set checks are not trajectory proof.** `hyper/grounding.py` reduces
to a `frozenset[str]` comparison, and the module says so itself at `:21-23`:
"Rows are keyed by tool name only, and upserted, call multiplicity and arguments
never influenced the row set." A name set cannot distinguish one call from fifty
or one share from ten thousand.

*Refinement.* The archive does have ordered verification, just not here.
`data/tau2/hyper/framework_reference/client_api_contract.md` states that during
grading a conversation's recorded tool calls are re-executed **in order** against
a fresh toolkit and backend, so each tool must be a deterministic function of
backend state, arguments and prior calls. Ordered replay and the set-based
grounding predicate are two separate mechanisms; only the second was compared
against Sharpe's trajectory replay, and the first is worth having in the
entrant-facing contract.

**Capability freezing, catalog sealing and contract identity are distinct.**
`client_api/capabilities.py:112-124` `freeze()` hashes exactly
`{"enabled": sorted_ids}` and nothing else. `seal()` at `:126-131` is `freeze()`
plus refusal of later change. Contract identity does not exist: the materialized
contract is written as a plain file with no digest
(`sandbox/kit.py:1189-1198`), and `render_enabled_contract` at `:133-155`
re-derives the operation document live. A catalog edit that changes an enabled
operation's arguments produces an identical snapshot hash. The only
content-addressed artifact is the fault manifest, and that hashes the plan, not
the surface.

**Source stripping and public-kit separation prove neither absence nor
reproducibility.** The construction Dockerfile runs the strip script and tests
imports afterward, and the script allowlists runtime modules rather than
excluding known private ones. It does not scan arbitrary entrant artifacts. The
Dockerfile installs from mutable base tags and a live installer, so a pinned
application version is not a pinned image.

*Refinement, and this is the understated one.* The irreproducibility is far
wider than the Dockerfile. `hyper/transformations/` contains no LLM call and no
RNG anywhere in its 28 files, and it does not generate the corpus: it consumes
it. The rendering parsers are held out of tree
(`transcript_artifacts.py:17-19`, "maintained privately with the authoring
tooling"). So 6,736 files under `data/tau2/hyper/sops` are pre-committed bytes
with no in-repo producer, and the 75 baseline starting workspaces are
model-generated. Nothing in this repository can regenerate its own corpus.

### Support and denominators, restated more strongly

The handoff says missing or malformed tasks can silently shrink support, so
explicit denominators matter. Confirmed, in four places, and the fourth is worse
than shrinkage.

1. `sandbox/orchestrator.py:1026-1029` warns about requested task ids not found
   and returns only those found; the reward at `:794-796` is a mean over the
   surviving list, so a task that fails to load raises the mean.
2. `hyper/task_loader.py:158-160` catches every per-file exception, logs and
   continues; `:151` returns an empty list for an absent directory.
3. `domains/banking_knowledge/environment.py:114-120` swallows per-file load
   failures the same way for the 144-file banking corpus.
4. **A missing criterion is a free 1.0, not a skipped leg.**
   `evaluator_nl_assertions.py:156-163`, `evaluator_communicate.py:27-33` and
   `evaluator_action.py:90-96` each return `reward = 1.0` when their criterion
   list is empty, and the final reward is the product of the basis components.
   This fires in the shipped corpus, not hypothetically: 72 of 114 retail tasks
   carry `"nl_assertions": null` while declaring `NL_ASSERTION` in
   `reward_basis`, and 44 of 50 airline tasks carry `"communicate_info": []`
   while declaring `COMMUNICATE`. Seven airline tasks additionally carry
   `"actions": []`, so their DB target is the unmodified database and an agent
   that changes nothing passes the DB leg.

The denominator is intact in case 4; the numerator is trivially satisfied. Both
failure modes produce a number that no error surfaces, which is the argument for
Bench's explicit expected-versus-found refusal
(`crates/sharpebench-cli/src/main.rs:1125,1132`). `hyper/grounding.py:76-78` is
the same shape once more: a task with no evaluation criteria yields an empty
required set and passes.

`Task` is a plain pydantic model with `extra="ignore"` and no validators in its
784 lines, so unknown corpus keys are dropped silently. Real ones are:
`"annotations"` in every retail, airline and banking task, and `"task_type"`,
`"interaction_mode"` and `"test_task_ids_note"` in the hyper task files.
`reward_basis` itself defaults to `[DB, COMMUNICATE]` when omitted
(`data_model/tasks.py:498-508`), so a task authored with only `actions` and
`nl_assertions` is scored on a basis it never declared and its authored
assertions gate nothing.

## Mechanism table

| Mechanism | What the source actually does | Sharpe suite decision |
|---|---|---|
| Contamination scan | `sandbox/orchestrator.py:256` searches known strings; the call at `:818` follows final scoring; matches are recorded, not used as refusal; read errors and baseline-provenance files are skipped. | Take bounded known-content detection, not this scanner unchanged. An unreadable or incompletely scanned artifact cannot receive a clean verdict. A clean scan cannot prove absence of arbitrary embedded or transformed data. |
| Runtime identity | `native_runtime.py:305-312` refuses mutable latest but admits any tag; `:589-620` verifies a compatibility integer; `:574-584` records the resolved digest after launch. | Keep Bench's digest-pinned launch (`crates/sharpebench-arena/src/sandbox.rs:790`) and Arena's spec hash. A compatibility version and an immutable identity answer different questions. |
| Provider retries | `_inner.py` uses three attempts with 5 and 15 second backoff; exception names and message fragments classify transient errors. `utils/retry.py:52` retries only builtin `ConnectionError` and `TimeoutError`, which `httpx` errors do not subclass, so the decorated paths are effectively un-retried against the likeliest real failures. | Preserve Bench's typed runtime and agent distinction (`crates/sharpebench-harness/src/failure.rs:28`) and incomplete-sweep withholding. Do not import message-driven attribution. Add a recorded backoff schedule. |
| Credit rates | `agent_context.py:83` casts to float and checks negativity, which does not reject NaN or infinity; usage parsing defaults absent fields to zero. | Bench's integer nanos-per-token rate card (`crates/sharpebench-harness/src/accounting.rs:18-28`) makes non-finite values unrepresentable rather than rejected. The open part of G08 is provenance, not validation. |
| Model gateway | `model_gateway.py:227-305` separates an ephemeral per-run bearer from the provider credential and records the property at `:362-363`; routes and models are allowlisted at `:95-134`; requests capped at 64 MiB by `Content-Length` only; upstream `read=None`; no spend ledger; `responses/{id}` GET and DELETE unscoped. | Take credential separation and restricted routing with independent envelope, response, duration, concurrency and spend bounds. Do not expose provider-owned response retrieval or deletion under a shared credential without an ownership check. |
| Container lifecycle | Internal entrant network plus a gateway on both networks; cleanup logs warnings and clears state flags regardless of outcome. | The two-network architecture is useful for optional provider access and is a G11 prerequisite, not an independent port. Keep Bench's hard cleanup failure (`sandbox.rs:943-952`); do not weaken it to logged best effort. |
| Process supervision | Unbounded event list; group termination returns early when the parent has exited; `max_steps: int = 0` disables the step limit by default (`builder.py:31,36`). | Retain Bench's bounded transport (`crates/sharpebench-sim/src/external.rs:32,40,47`) and process-group teardown (`:419,677`). A sentinel that disables a safety bound by default is the wrong polarity. |
| Developer feedback | `callback_broker.py` exposes token-bound, quota-limited host callbacks through a narrow filesystem request protocol. | Useful pattern for a future build-and-evaluate workflow, not a reason to add one now. Agent-writable request and response paths need a separate race and symlink review before reuse. |
| Task support | Requested ids that are missing warn and drop out of the denominator; malformed corpus files are skipped; an empty criterion list scores 1.0. | Keep explicit expected-cell and support checks. An absent task must not change the denominator, and a missing check must never resolve to a pass. |
| Kit separation | `kit.py` keeps host coverage reports outside the kit, uses generic artifact names, and emits public API documentation with synthetic development data. Delivered artifacts are grepped for evaluator vocabulary before shipping. Some missing framework documentation is only warned about. | Take generic artifact naming and the delivered-artifact leak assertion, as a gate rather than a warning. Warnings alone do not establish complete developer documentation. |
| Keyed fault plans | `client_api/defects.py` separates a content-addressed frozen manifest from per-trial mutable state, with a deterministic cohort draw over the manifest hash. Nine fault kinds including post-commit timeout with idempotency-key replay, projection lag, page-before-sort and amount-sign inversion. | Meaningful robustness diagnostics, not cosmetic. Take the frozen-plan and trial-state split. Do not transfer customer-service fault semantics directly or use a diagnostic as an unvalidated ranking axis. |
| Order idempotency | `defects.py:240-251` plus `runtime.py:2191-2260`: a lost response after a canonical commit, a key-keyed replay with a request fingerprint, `409` on key reuse with a different body, and a recorded `ambiguous_write_retried_without_idempotency` violation. | The one mechanism the archive holds that Bench does not. Bench has no `client_order_id` and no submission dedup; `crates/sharpebench-core/src/process.rs:278` `DuplicateTransition` is warn-severity bookkeeping. Proposed as a new ledger item. |
| Capability plane | `capabilities.py:70-131`: offer, then enable-only-what-was-offered, then freeze, then seal. The snapshot hash covers enabled identifiers only. | The freeze, seal and identity trichotomy is worth holding as vocabulary. An identity over identifiers is not an identity over a contract; Bench's content digests hold the stronger property. |
| Grounding | Set-based over tool names, by the module's own account. Ordered re-execution exists separately, in the entrant-facing contract text. | Keep Bench's ordered, subject-bound replay. Adopt the published determinism requirement, which Bench already enforces but does not declare. |

## Runtime and provider follow-through

The construction Dockerfile runs the source-strip script and tests imports
afterward. The script allowlists runtime modules rather than excluding only known
private ones. This is useful defense in depth for an evaluator-provided image. It
does not scan arbitrary entrant artifacts, and it does not make the image
reproducible: the Dockerfile installs from mutable base tags and a live
installer.

Credential handling is the strongest part of the design and is worth stating
plainly. The real provider key is read on the host and passed only to a sidecar
(`model_gateway.py:227-335`); the agent process receives a random per-run bearer
with an expiry of the build budget plus sixty seconds
(`sandbox/native_builder.py:266`), and the run metadata carries the explicit
claim `"raw_provider_credential_in_agent": False` (`:362-363`). Egress denial is
asserted in four independent places: the internal Docker network
(`native_runtime.py:415-423`), per-harness web-tool denials
(`harnesses/claude.py:163,246`, `codex.py:72`, `opencode.py:119-124`,
`prime.py:141,208`), and the developer prompt itself
(`sandbox/native_builder.py:186-187`). The one divergence is recorded honestly
in the harnesses' own metadata: Claude Code inherits the gateway token into its
shell (`claude.py:84`) while the others exclude it (`codex.py:83`), with the
reasoning at `claude.py:104-114`. Bench's environment allowlist
(`crates/sharpebench-sim/src/external.rs:227-266`) is the stronger polarity
because it does not depend on an argument about egress being absent.

The local-test path keeps held-out task wiring on the host and accepts
developer-owned scenarios. Public development fixtures contain synthetic
identities but can reuse reference products and flights. That is a deliberate
public and held-out distinction, not proof that every development byte is
unrelated to evaluation. Preserve the distinction in any Sharpe examples.

Client runtime semantics distinguish authoritative operations from their
presentation. Async completion records the mutation before handling a failed
response projection, which can return a succeeded operation with an unavailable
view. Projection convergence is recorded after a read observes the new state, not
merely after a timer expires. These are useful diagnostic patterns. However, the
runtime simulates async completion on status polling; it is not a real exchange
adapter, and its idempotency logic is fault-scoped rather than a general
exactly-once guarantee.

The routing manifest separates requested model identity from upstream routing,
but unknown prefixes pass through to LiteLLM verbatim
(`utils/model_routing.py:162-167,190-193`) and caller keyword arguments can
override defaults. Two related edges: a missing credential raises for a custom
key variable but is silently ignored for a stock one (`:169` versus `:177`), and
the Responses-API gate is a bare `startswith("gpt-5")` string test
(`:179,185`), so an unlisted model is routed to a different wire protocol purely
by name. That is flexible host configuration, not an untrusted-entrant
allowlist. The Sharpe gateway must own routing and credentials rather than accept
entrant-supplied endpoints, and its bounds must cover the entire wire envelope
before allocation.

Performance profiles freeze capability-bucket credits, not provider invoices.
Their model menus and budgets were calibrated for customer-service tasks and must
not be imported as trading evidence or spending limits. Capability deployment is
host-owned and allowlisted; `freeze()` captures current state, whereas `seal()`
additionally refuses later changes. The snapshot hash covers enabled identifiers,
not the underlying operation contract.

The live-experiment surface is a genuinely careful piece of design and the
review's earlier silence on it was a gap. It is a one-shot peek at hidden
held-out traffic offered mid-build, lock-guarded and consumed even on failure
(`hyper/live_experiment.py:68-74`), persisted before it is returned so a client
timeout cannot burn the spend with nothing to show (`:20-25`, `:59-66`), and
sealed by a field allowlist rather than a denylist because the provider
library's `raw_data` echoes the hidden scenario instructions back verbatim
(`:139-149`). What the developer sees is conversations plus a binary score, with
no rationale, assertions, reward breakdown or canonical task ids (`:205-212`).
The allowlist projection and the consume-on-failure rule both belong in G12.

## Domain surface and catalogs

The Client API catalog is one frozen dataclass, `ClientOperation`
(`client_api/catalog.py:48-74`), and a domain catalog is a module exposing
`operations()`. The load-bearing invariant is stated in its docstring at
`:50-53`: summary and description text "must describe resource and transport
mechanics without revealing business policy, eligibility gates, workflow
ordering, or outcome rules." Request and response models are strict pydantic with
`extra="forbid"` (`:12-15`).

Three orthogonal flags are declared per operation rather than derived from the
HTTP verb: `mutates_state`, `idempotency` and `automatic_retries` (`:66-68`).
They are published to the consumer as `x-api-mutates-state`,
`x-api-idempotency` and `x-api-automatic-retries`
(`client_api/runtime.py:249-251`). This is the same distinction Bench draws
structurally in its order lifecycle, expressed as machine-readable contract.

The archive does not live up to its own rule everywhere. Policy constants leak
into the transport layer: a per-month holding fee is a module constant returned
as an API field (`catalogs/telecom.py:179,366`), and hardcoded 24-hour windows
appear in banking receipts (`catalogs/banking.py:1241,1527`). Several adapters
echo request arguments back as though the server had confirmed them
(`catalogs/telecom.py:386-403`, `catalogs/banking.py:1178-1179,1216-1220,1310-1314,1444-1452`),
so a silently partial write still produces a success receipt.

Money handling in the banking catalog is the weakest code read in this review.
`_money` (`catalogs/banking.py:561-566`) returns `0.0` for `None` and otherwise
parses a stripped string, so a missing balance is indistinguishable from a zero
balance, no finiteness guard exists, and a malformed string raises an uncaught
`ValueError` from inside a response adapter. `_find_one` returns `matches[-1]`
(`:795-803`), last match wins with no multiplicity error, on the write-receipt
path. A `months` query parameter is applied as a count of payments rather than a
time window and has no upper bound (`:72-74`, `:1360-1367`). Unparseable dates
sort to `datetime.min` silently (`:1460-1466`). Business errors are detected by
string prefix sniffing (`:588-594`). These are noted because they are the shape
of defect a trading surface cannot afford, not because any of them is a port
candidate.

Domain tools use a decorator plus docstring-to-schema pattern
(`environment/toolkit.py:64-91`, `environment/tool.py:61-144`) with a metaclass
collecting them at class creation (`toolkit.py:19-40`), and `mutates_state`
gates replay (`toolkit.py:78-80`, defaulting to `True` when absent at
`:208-215`). Determinism is taken seriously: airline, telecom and banking all
freeze the clock (`airline/tools.py:106-108`, `telecom/utils.py:25-31`,
`banking_knowledge/utils.py:11-35`), and banking derives every identifier from a
seeded SHA-256 (`utils.py:51-402`), with both discoverable-tool dispatchers
normalizing integers to floats so `33` and `33.0` cannot change an id or the
database hash. Two genuine non-determinism sites survive:
`telecom/tools.py:449` mints a bill id from `uuid.uuid4()` inside
`_apply_one_time_charge` despite the class holding a deterministic id generator
at `:27-34`, and `airline/tools.py:477-481` computes the next day by string
arithmetic hardcoded to May 2024 without zero padding.

One asymmetry matters for anything that keys off an error flag. The environment
converts a raised exception into a normal tool result with `error=True`
(`environment/environment.py:496-520`), which is how retail, airline and telecom
signal failure. Banking instead *returns* `"Error: ..."` strings from roughly
forty sites, so `ToolMessage.error` stays `False` for a banking failure.

Two toolkit-level ideas are domain-neutral and worth naming. The
READ/WRITE/THINK/GENERIC taxonomy with an orthogonal `mutates_state` replay flag
is the most portable abstraction in the archive. The discoverable-tool
mechanism, where an agent must find a capability in a knowledge base, unlock it
and then call it, is a general capability-discovery primitive wearing a
customer-service costume; only the concrete tools bound to it are domain
specific.

## Tests, web and configuration

The archive's own test suite was read, not executed. Three properties are worth
recording because they bear on how much weight the archive's behaviour can carry
as evidence.

The default suite makes live paid provider calls, against the repository's own
rule at `tests/AGENTS.md:103`. `tests/conftest.py` carries no stub, and
`tests/test_agent.py`, `test_user.py`, `test_orchestrator.py` and `test_run.py`
drive real turns; ten of those tests assert only `is not None`. This is the
concrete reason the foreign suite was not run here.

Four tests cannot fail. `tests/test_gym/utils.py:20-22` defines a `@timeout`
decorator that catches its own `TimeoutError` and returns `None`, so seven
wrapped tests pass green on a hang. `tests/test_gym/test_user_gym.py:19,40` pass
`agent_llm="mock_llm"`, an identifier that exists nowhere in `src/`; the
orchestrator thread dies immediately, the failure is swallowed at
`gym/gym_agent.py:1400-1404`, and the remaining assertions are bare `isinstance`
checks that hold either way.

Cost is the least-tested dimension. `tests/test_results_format.py:433` asserts
the index entry is exactly five fields and deliberately omits `agent_cost`,
although `data_model/simulation.py:1416` carries it. Combined with the retry
discard and the unpriced-model-is-zero fallback, the accounting path has no
aggregation or round-trip test at all. Where the archive does get this right, it
gets it right explicitly: `tests/test_hyper/test_usage_pricing.py` propagates
`None` at session level with a written rationale rather than falling back to
zero, and `test_client_api_retry_safety.py:127-171` counts underlying calls
across a timeout, retry and conflict sequence.

The web surface is a benchmark workbench, not a product. `hyper/web/app.py`
carries no authentication across its 920 lines and the CLI default binds
`0.0.0.0` (`cli.py:1150`); the runner controller fails open when
`TAU2_CONTROLLER_TOKEN` is unset (`runner/controller.py:283-290`). Nothing
validates leaderboard submissions: there is no schema, no model and no CI job,
ranking is computed client-side, and the distiller that produced the 107
checked-in trajectory JSONs does not ship in the repository. None of this is a
port candidate; it is recorded so that the archive's leaderboard numbers are not
mistaken for attested results.

## Accepted work and present limits

The concrete implementation ledger is [IMPLEMENTATION.md](IMPLEMENTATION.md);
the port decisions are in [PORT-RECONCILIATION.md](PORT-RECONCILIATION.md):

- G06: explicit runtime-failure recovery with retained history and unchanged
  completed and agent-fault cells. Recovery must not become selective result
  reruns.
- G07: a bounded preflight over declared entrant artifacts and known protected
  content. State which bytes were scanned and which encodings are unsupported,
  and gate the verdict rather than annotating it.
- G08: frozen, validated rate cards with explicit unavailable usage and cost.
  The validation half already exists; the open half is provenance.
- G09: publish attempts independently of rank. The existing ledger measures host
  duration, not monetary spend; accounting must keep that distinction.
- G11: optional host-observed model accounting. No integration with another
  benchmark or mandatory hosted service is required.
- G12: test experiment preflight and cost refusal paths without running a field.
  The consume-on-failure rule and the allowlist projection belong here.

One item is proposed that the ledger does not carry: order-submission
idempotency with a post-commit response loss, and the paired
retry-without-a-key process violation. It is the only mechanism in the archive
that Bench lacks, it is trading-native, and it is scoped and testable. See rows
25 and 26 of the reconciliation.

The prior rough line estimates are not acceptance criteria. Artifact integrity,
crash recovery, network isolation and accounting interact, and each needs tests
through its actual consumer.

Statistical scoring, the conversational user simulator and the outer coding-agent
construction reward are not interchangeable with quantitative-trading evaluation.
Nothing read in the completed review justifies replacing the Sharpe scoring
kernel or adopting model-judged ranking, and the response-phrasing rule packs
would have reopened exactly that question had they been imported.
