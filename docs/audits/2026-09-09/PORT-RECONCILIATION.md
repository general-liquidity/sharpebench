# Hyper-Tau port reconciliation

Companion to [HYPER-TAU-REVIEW.md](HYPER-TAU-REVIEW.md). That document records
what the archive's source actually does. This one decides, proposal by proposal,
what the Sharpe suite should do about it.

Every row carries six fields: the source path in the archive, the search run
against SharpeBench and SharpeArena for an existing consumer together with its
result, the concrete benefit, the threat and assumption changes a port would
introduce, the tests it would need, and a take / adapt / reject / defer decision
with justification.

Rules applied throughout:

- Where a mechanism already exists in Bench or Arena the row cites file and line
  rather than asserting the fact. A search that found nothing is reported as the
  command and its empty result, not as silence.
- Customer-service semantics are out of scope unless separately justified in the
  row. The outer agent-building product (construct a domain, judge the
  construction) is out of scope entirely: SharpeBench scores trading decisions,
  not agent authorship.
- No integration with the reference arena is proposed or required.
- Archive paths are relative to `src/tau2` unless stated otherwise. Sharpe paths
  are repository-relative.

## Decision summary

| # | Mechanism | Archive source | Decision |
|---|---|---|---|
| 1 | Known-content scan over entrant artifacts | sandbox/orchestrator.py:256 | Adapt (G07) |
| 2 | Runtime compatibility integer | runtime_contract.py:9 | Reject |
| 3 | Recorded image ID after launch | sandbox/native_runtime.py:574 | Reject (weaker than existing) |
| 4 | Provider retry with fixed backoff | hyper/_inner.py | Adapt |
| 5 | Message-fragment error classification | utils/llm_utils.py:118 | Reject |
| 6 | Empty-completion retry accounting | utils/llm_utils.py:680-735 | Reject |
| 7 | Zero-cost fallback for unpriced models | utils/llm_utils.py:232, :866 | Reject |
| 8 | Credit-rate validation | hyper/agent_context.py:83 | Reject as written; requirement already met |
| 9 | HTTP model gateway: credential separation | sandbox/model_gateway.py:227-305 | Take (G11) |
| 10 | HTTP model gateway: route and model allowlist | sandbox/model_gateway.py:95-134 | Adapt (G11) |
| 11 | Provider-owned response GET/DELETE passthrough | sandbox/model_gateway.py:112-113 | Reject |
| 12 | Sealed stdout RPC to a network-disabled container | sandbox/sealed_runner.py:187 | Reject (already present, stronger) |
| 13 | Line-count-bounded response queue | sandbox/sealed_runner.py:145 | Reject (already present, stronger) |
| 14 | Two-network gateway topology | sandbox/native_runtime.py:415-455 | Defer (G11 prerequisite) |
| 15 | Per-request model-call quota | sandbox/sealed_runner.py:79 | Adapt (G11) |
| 16 | One-shot live-experiment budget | hyper/live_experiment.py:68-74 | Adapt (G12) |
| 17 | Developer-visible field allowlist seal | hyper/live_experiment.py:139-212 | Take |
| 18 | Best-effort container cleanup | sandbox/native_runtime.py:713-731 | Reject (already present, stronger) |
| 19 | Unbounded process event list | sandbox/native_runtime.py:195-207 | Reject (already present, stronger) |
| 20 | Early-return process-group termination | sandbox/native_runtime.py:146-148 | Reject (already present, stronger) |
| 21 | Silent task-support shrinkage | sandbox/orchestrator.py:1026-1029 | Reject as behaviour; keep as regression motive |
| 22 | Per-file task parse-error tolerance | hyper/task_loader.py:158-160 | Reject |
| 23 | Crash-tolerant in-progress checkpoint | hyper/recording.py:178-286 | Reject (already present, stronger) |
| 24 | Published operation metadata triple | client_api/catalog.py:66-68 | Adapt |
| 25 | Post-commit timeout plus idempotency-key replay | client_api/defects.py:240-251 | Take (new ledger item) |
| 26 | Ambiguous-write-retried-without-key violation record | client_api/runtime.py:2254-2260 | Take (new ledger item) |
| 27 | Projection lag with convergence verification | client_api/defects.py:275-392 | Adapt |
| 28 | Async completion via seeded status polling | client_api/defects.py:198-218 | Reject |
| 29 | Pagination fault, mode `limit_before_sort` | client_api/defects.py:254-272 | Adapt |
| 30 | Amount-sign normalization fault | client_api/defects.py:164-173 | Adapt |
| 31 | Seeded rate-limit fault | client_api/defects.py:221-237 | Adapt |
| 32 | Frozen fault manifest, mutable trial state | client_api/defects.py:38-41, :674-692 | Take |
| 33 | Deterministic cohort draw over manifest hash | client_api/defects.py:569-622 | Adapt |
| 34 | Capability offer / enable / freeze / seal | client_api/capabilities.py:70-131 | Defer |
| 35 | Deployment snapshot hash over enabled IDs | client_api/capabilities.py:112-124 | Reject |
| 36 | Set-based discoverable-call grounding | hyper/grounding.py | Reject |
| 37 | Deterministic re-execution contract for tools | framework_reference/client_api_contract.md | Take |
| 38 | Frozen per-run action catalog | hyper/action_catalog.py:39-45 | Defer |
| 39 | Source-strip allowlist for the runtime image | docker/hyper-construction/strip_runtime_src.py | Adapt (G07 adjacent) |
| 40 | Public kit separation and generic artifact names | sandbox/kit.py | Adapt |
| 41 | Fact-coverage compiler and modality substitution | hyper/transformations/compile.py | Reject |
| 42 | Response-phrasing rule packs | hyper/response_phrasing.py | Reject |
| 43 | Held / contested / confirmable operator partition | hyper/client_sim/instructions.py | Reject |
| 44 | Zero default disables the step limit | sandbox/builder.py:31,36 | Reject |
| 45 | Hermetic environment for the agent process | harnesses/claude.py:104-116 | Reject (already present, stronger) |

Counts: take 6, adapt 13, reject 23, defer 3.

Eleven of the twenty-three rejections are rejections *because Bench or Arena
already implements the mechanism more strictly*; those rows cite the existing
Sharpe line. The remainder are rejections on the merits.

## Implementation status, 2026-09-10

Every accepted row is built or closed with a reason. Rows decided Reject are
not listed.

| # | Decision | Status |
|---|---|---|
| 1 | Adapt (G07) | Built, Bench PR #44 |
| 4 | Adapt | Built at the run-level retry, not the per-decision transport, Bench PR #59 |
| 9, 10, 15 | Take and Adapt (G11) | Built, Bench PRs #46 and #55; served to entrants in Bench PR #61 |
| 14 | Defer (G11 prerequisite) | Closed as unnecessary: model traffic rides stdio and the launch keeps `--network none`, Bench PR #61 |
| 16 | Adapt (G12) | Built, Bench PRs #46 and #56 |
| 17 | Take | Built, every entrant-visible surface sealed, Bench PR #59 |
| 24 | Adapt | Built with its own pinned digest rather than one folded into the capture contract, Bench PR #59 |
| 25, 26 | Take (G17) | Built, Bench PRs #47 and #54 |
| 27, 30, 31, 32, 33 | Take and Adapt | Built, Bench PR #58 |
| 29 | Adapt, conditional | Condition not met: the observation schema has no paging. A plan naming the mode is refused by name, and a test fails the day paging appears, Bench PR #58 |
| 34 | Defer | Still deferred: the run-identity requirement is met by the digests above, and nothing an offer and seal lifecycle would govern can change after construction |
| 37 | Take | Built, with a new re-execution check because replaying recorded decisions cannot detect a non-deterministic agent, Bench PR #59 |
| 38 | Defer | Still deferred: the only per-run surface added is frozen at construction |
| 39 | Adapt (G07 adjacent) | Built as a preflight runtime allowlist and a hardened probe run, Bench PR #61 |
| 40 | Adapt | Built as a gate on what the host hands an entrant, since no fixture ships beside held-out data, Bench PR #61 |

## 1. Known-content scan over entrant artifacts

**Source.** `sandbox/orchestrator.py:256` `_build_contamination_report`, called at
`:818`, patterns from `sandbox/starting_workspace.py:24-44`.

**Existing Sharpe consumer.** `rg -n -i "preflight" crates/` returns
`crates/sharpebench-harness/Cargo.toml:23` and an unrelated Python-import probe
in `crates/sharpebench-harness/examples/local_open_weight_field_eval.rs:259`.
No entrant-artifact scan exists. The dataset side does:
`crates/sharpebench-attest/src/canary.rs:38,60,67,73` (`make_canary`,
`embed_canary`, `detect_leak`, `verify_canary`),
`crates/sharpebench-attest/src/sealed.rs:32` (sealed held-out bytes), and
`crates/sharpebench-sim/src/data.rs:341-346` (contamination-masked dataset).
The gap is already on the ledger as G07 in `IMPLEMENTATION.md:36`.

**Benefit.** A declared-artifact scan that states which bytes it read gives an
entrant submission a bounded, reportable integrity check before it can earn a
clean verdict.

**Threat and assumption changes.** The archive's version changes the threat model
in three ways that must not be inherited. It runs after final scoring
(`:790-813` computes `final_test_reward`, `:818` builds the report), so a match
cannot refuse a launch. It skips unreadable files silently
(`:285-287` `except OSError: continue`). And in delta mode it skips every file
classified `baseline` (`:279-283`), which in this archive includes the shipped
starting workspaces under `data/tau2/hyper/workspaces/gen_luna_*` and
`gen_sonnet5_*` whose directory names identify them as model-generated against
the reference domain. The pattern list is four to seven literal import-path
substrings; any renamed, encoded or paraphrased copy passes. A clean scan is
therefore evidence about one encoding, not absence.

**Tests.** A planted known string in a declared artifact must block, not
annotate. An unreadable or unsupported-encoding artifact must produce an
explicit `unscanned` disposition, never a clean verdict. The scanned-byte count
must appear in the result and be asserted. A scan must run before any score is
computed, asserted by ordering in the recorded trace, not by inspection.

**Decision: adapt.** Take the idea of a canonical shared pattern list
(`starting_workspace.py:27-29` correctly notes the authoring gate and the runtime
scan must not drift) and the per-match path/line/excerpt record. Reject the
placement, the read-error skip and the baseline skip. G07 must state which bytes
were scanned and which encodings are unsupported, and must gate the verdict.

## 2. Runtime compatibility integer

**Source.** `runtime_contract.py:9` `CONSTRUCTION_RUNTIME_CONTRACT_VERSION = 7`,
verified in-container at `sandbox/native_runtime.py:589-620`.

**Existing Sharpe consumer.** Contract identity exists and is content-addressed:
`crates/sharpearena/src/spec_hash.rs:14` (`SPEC_HASH_HEX` from a build-time
environment binding), materialized at
`crates/sharpearena/contract/attestation/spec-hash.json:15`
(`"spec_hash": "da33606fa6a74aac"`). Bench has per-artifact digests:
`crates/sharpebench-harness/src/accounting.rs:81` (`RateCard::digest`),
`arena/windows/window-003/window.json:35-37` (`score_config_sha256`,
`scorer_artifact_sha256`, `sealed_eval_salt_sha256`).

**Benefit.** A pre-launch handshake that fails before any model spend when host
and image disagree about the API is genuinely cheaper than discovering the
mismatch mid-run.

**Threat and assumption changes.** The integer answers "do these two speak the
same protocol". It does not answer "is this the artifact I pinned". Treating it
as identity would let any rebuild of the same tag pass an identity check.

**Decision: reject.** The pre-launch handshake idea is already covered by
Arena's spec hash, which is strictly stronger because it is derived from content.
Recording a compatibility integer alongside a digest adds a second, weaker
identity that invites confusion about which one is authoritative.

## 3. Recorded image ID after launch

**Source.** `sandbox/native_runtime.py:305-312` rejects `:latest` and implicit
latest; `:574-584` runs `docker inspect --format {{.Image}}` and stores the
resulting `sha256:` value in run metadata at `:699`.

**Existing Sharpe consumer.** `crates/sharpebench-arena/src/sandbox.rs:790`
refuses any image not written as `<repository>@sha256:<64 hex>`, with the
refusal text at `:792`; the escape hatch is an explicit named flag,
`sandbox.rs:45` `pub allow_unpinned_image: bool`. Identity is also recorded:
`sandbox.rs:94-98` (`image`, `image_id`, `docker_server_version`) with
`:645` asserting the `sha256:` prefix.

**Benefit.** None over what exists.

**Threat and assumption changes.** The archive's rule admits any versioned or
commit tag. A tag is mutable: it can be repushed to different content between
the pin and the run. Recording the resolved digest *after* launch documents what
ran; it does not constrain what may run.

**Decision: reject.** Bench already pins at launch and records identity. Adopting
the archive's rule would be a regression. This is the third distinct thing the
review must keep separate: a mutable-tag refusal, a compatibility handshake, and
a digest-pinned launch are not interchangeable.

## 4. Provider retry with fixed backoff

**Source.** `hyper/_inner.py`: three attempts, 5 and 15 second waits.
`utils/retry.py:52` is a second, separate policy (three attempts, exponential
1 to 10 seconds, `reraise=True`).

**Existing Sharpe consumer.**
`crates/sharpebench-sim/src/transport.rs:66` `decide_with_retry`, with a
per-endpoint breaker at `transport.rs:86`. The typed fault distinction is
first-class: `crates/sharpebench-harness/src/failure.rs:28` `FailureKind`
separates `SpawnError | TransportError | Timeout` from
`AgentProtocolViolation | ResourceLimitExceeded`, with the predicate at
`failure.rs:55` and the reason at `failure.rs:18`. No delay is applied between
Rust retries: `rg -n "backoff|sleep" crates/sharpebench-sim/src/transport.rs`
returns nothing.

**Benefit.** A bounded delay between transport retries is real operational
behaviour, not decoration: it is what distinguishes a retry storm against a
degraded venue from a paced recovery, and the delay pattern is diagnostic
evidence when a sweep is later audited.

**Threat and assumption changes.** Adding a delay lengthens wall-clock time per
cell and therefore interacts with any deadline. It must be accounted in the
recorded attempt duration rather than hidden inside it, or `AttemptDuration`
becomes misleading.

**Decision: adapt.** Add an explicit, recorded backoff schedule to
`decide_with_retry`, with the waited duration appearing in the attempt record.
Keep Bench's typed runtime-versus-agent classification unchanged.

## 5. Message-fragment error classification

**Source.** `utils/llm_utils.py:118` `_is_retryable_error` classifies by
exception name and message substring.

**Existing Sharpe consumer.** `crates/sharpebench-harness/src/failure.rs:28` is
a closed typed enum; `failure.rs:75` `apply_oom_verdict` lets a container
verdict override transport classification.

**Benefit.** None.

**Threat and assumption changes.** Attribution by provider message text is not
stable across provider releases, and it is influenceable by anything that can
shape an error string. In a benchmark, misattribution moves a cell between the
runtime-fault bucket (excluded from pass^k) and the agent-fault bucket (counted),
which changes published scores.

**Decision: reject.** Typed classification is the invariant at `failure.rs:18`
and must not be diluted.

## 6. Empty-completion retry accounting

**Source.** `utils/llm_utils.py:680-735`. The loop reassigns `response` on every
attempt; `cost = get_response_cost(response)` at `:735` and
`usage = get_response_usage(response)` at `:736` run once, after the loop, over
the final response only. `kwargs["num_retries"] = DEFAULT_MAX_RETRIES` at
`:621-622` additionally delegates a second, invisible retry layer to LiteLLM.
The Responses path repeats the shape at `:844-863`.

**Existing Sharpe consumer.**
`crates/sharpebench-harness/src/failure.rs:189` states the opposite rule
directly: two records that are identical "are two attempts, not one, and
deduplicating them" would be wrong.
`crates/sharpebench-harness/src/accounting.rs:100-106` keeps
`observed_decisions`, `unpriced_decisions` and a `complete` flag rather than a
single total.

**Benefit.** None.

**Threat and assumption changes.** Every discarded attempt was billed. Reporting
only the surviving attempt understates spend by an amount that grows exactly
when the model is behaving worst. Importing this would put a systematic
downward bias into any cost-efficiency axis.

**Decision: reject.** G11 must retain every observed attempt. An empty completion
is spent work.

## 7. Zero-cost fallback for unpriced models

**Source.** `utils/llm_utils.py:229-232` logs and returns `0.0` when LiteLLM has
no price entry; `:866` sets `cost = 0.0` unconditionally on the Responses path;
`:588` reads `getattr(usage, "output_tokens", 0)`, so an absent field becomes
zero rather than missing. The archive's own tests pin the unpriced-zero
behaviour.

**Existing Sharpe consumer.** `crates/sharpebench-harness/src/accounting.rs:106`
`pub complete: bool` with the comment at `:104-105`: known counts stay a partial
subtotal, "never silently promoted to complete monetary cost".
`crates/sharpebench-harness/src/failure.rs:98` `AttemptDuration::Unavailable`.
`crates/sharpebench-cli/src/main.rs:2249` surfaces
`monetary_cost.status == "unavailable"`. Statuses asserted at
`crates/sharpebench-harness/tests/frozen_rate_card.rs:109,118,122`.

**Benefit.** None.

**Threat and assumption changes.** Zero is a value; missing is not. Conflating
them makes the cheapest-looking entrant the one whose provider Bench cannot
price, which is an incentive to route through unpriced models.

**Decision: reject.** Bench's typed-unavailable representation is correct and
already shipped.

## 8. Credit-rate validation

**Source.** `hyper/agent_context.py:83` casts to `float` and rejects negatives.
NaN and infinity pass that check, and `float("nan") < 0` is false.
`performance.py` can count absent usage as zero.

**Existing Sharpe consumer.**
`crates/sharpebench-harness/src/accounting.rs:18-28`: `RateCard` uses integer
nanos per token (`input_usd_nanos_per_token`, `output_usd_nanos_per_token`,
`:23-24`), so non-finite values are unrepresentable by construction rather than
rejected by a predicate. Schema is pinned at `:11`
(`"sharpebench.token-rate-card.v1"`), size bounded at `:12`
(`MAX_RATE_CARD_BYTES = 64 * 1024`), unknown fields denied at `:28`, identity at
`:81` `digest`, arithmetic overflow typed at `:87` `quote_nanos -> Option<u128>`.

**Benefit.** None.

**Threat and assumption changes.** Adopting float rates would reintroduce the
non-finite class that the integer representation removes.

**Decision: reject as written; the underlying requirement is already met.**
G08's frozen validated rate card is `accounting.rs`. The open part of G08 is
provenance, not validation: `IMPLEMENTATION.md:37` records estimates as
entrant-reported, and a rate card remains an operator declaration rather than a
provider invoice.

## 9. Model gateway: credential separation

**Source.** `sandbox/model_gateway.py:227-305`
`ModelGatewaySpec.from_host_environment` reads the real provider key on the host,
mints `token=secrets.token_urlsafe(32)` with `expires_at`, and passes the real
key only to a sidecar via `sidecar_environment()` at `:312-335`. Run metadata
asserts the property at `:362-363`:
`"credential": "random-per-run-bearer"`, `"raw_provider_credential_in_agent": False`.
Expiry is enforced at `:425`, scope at `:459`. Lifetime is
`budget.max_time_seconds + 60` (`sandbox/native_builder.py:266`).

**Existing Sharpe consumer.** Environment-level separation exists:
`crates/sharpebench-sim/src/external.rs:227` ("Everything else, API keys first
among them, stays in the harness"), the allowlists at `:229` and `:243`, explicit
per-variable opt-in at `:249` (`SHARPEBENCH_AGENT_ENV`) and `:256`
(`AGENT_ENV_SECRET`), the fail-toward-secrecy predicate at `:266`
`is_credential_name`, and credential exclusion from resume identity at
`crates/sharpebench-harness/src/checkpoint.rs:42-43`. A gateway does not exist:
`rg -n -i -e "model_gateway|model_routing|route_model" crates/` returns zero
matches. G11 is open at `IMPLEMENTATION.md:41`.

**Benefit.** Bench's current answer to "the entrant needs a model" is that the
entrant brings its own credential. An ephemeral per-run bearer in front of a
host-held key lets the host observe and bound model usage without ever handing
over the key, which is the precondition for host-observed accounting.

**Threat and assumption changes.** It moves Bench from "no provider credential
is anywhere near the entrant" to "a host-held credential exists in the run, one
hop away". That is a real increase in blast radius and must be paid for with
expiry, scope, bounds and an ownership rule. It also makes the host liable for
spend the entrant induces, which today it is not.

**Decision: take.** The ephemeral-token-plus-recorded-assertion pattern is the
right shape for G11, including the explicit
`raw_provider_credential_in_agent` claim in run metadata. Bench should record
that assertion and test it, not merely intend it.

## 10. Model gateway: route and model allowlist

**Source.** `sandbox/model_gateway.py:95-124` `_allowed_upstream_path`
enumerates permitted method and path shapes per wire format;
`:126-134` `_model_in_scope` requires inference requests to use the run's
selected model. Request bodies are capped at `:28`
(`MAX_REQUEST_BYTES = 64 * 1024 * 1024`), checked at `:449`.

**Existing Sharpe consumer.** None; same zero-result search as row 9. Routing
ownership in the archive is separately weak: `utils/model_routing.py` passes
unknown prefixes through to LiteLLM and lets caller keyword arguments override
defaults, which is host configuration rather than an untrusted-entrant
allowlist.

**Benefit.** A closed enumeration of permitted routes and a pinned model make the
gateway's reachable surface finite and auditable.

**Threat and assumption changes.** The archive's bounds are incomplete in three
places that Bench must close. `MAX_REQUEST_BYTES` is checked against
`Content-Length` only, so it bounds a declared body and not the whole wire
envelope: header and path bytes are unbounded. The upstream client is built with
`timeout=httpx.Timeout(connect=30, read=None, write=60, pool=30)` at `:552`, so
there is no read deadline and no response-size cap at all. There is no spend
ledger in the module. Bench's own bounds discipline
(`crates/sharpebench-sim/src/external.rs:32,40,47`: an 8 MiB response cap, a
matching per-line cap, and a 64 MiB cumulative stdout quota) is the standard to
meet, and the rationale at `external.rs:40-46` is the exact argument against a
line-count bound.

**Decision: adapt.** Take the closed route enumeration and the pinned-model
check. Add independent request-envelope, response-size, duration, concurrency
and spend bounds before allocation. Do not ship a gateway whose upstream read
timeout is unset.

## 11. Provider-owned response GET and DELETE passthrough

**Source.** `sandbox/model_gateway.py:112-113`: for a two-segment
`responses/{id}` path the gateway permits `GET` and `DELETE`, and
`:114-116` permits `POST .../cancel` and `GET .../input_items`. Authorization is
the shared ephemeral bearer at `:402-411`. `_model_in_scope` does not apply,
because `_MODEL_SCOPED_PATHS` at `:111-114` lists only the inference paths.

**Existing Sharpe consumer.** Not applicable; Bench has no gateway.

**Benefit.** None that outweighs the exposure.

**Threat and assumption changes.** The gateway's upstream credential is a
provider account key. Nothing binds a requested response id to the run that
created it. A candidate that can guess or observe an id can read another run's
model output, or delete it, through the host's own credential. In a benchmark
with concurrent entrants that is a cross-entrant read and a cross-entrant
destructive write.

**Decision: reject.** If a future Bench gateway must proxy response retrieval,
it retrieves only ids the gateway itself minted for that run, from a
run-scoped table. Do not expose provider-owned object lifecycle under a shared
credential without an ownership check.

## 12. Sealed stdout RPC to a network-disabled container

**Source.** `sandbox/sealed_runner.py:187-188` launches with
`--network none` and `:193` `--pids-limit`; model and Client API traffic is
multiplexed over stdout to a host caller (`:376-403`).

**Existing Sharpe consumer.**
`crates/sharpebench-arena/src/sandbox.rs:223` passes `--network`, asserted as
`["--network", "none"]` at `sandbox.rs:1551`, alongside `--read-only`,
`--cap-drop` and `--security-opt no-new-privileges=true` at `:227-231`,
`--memory`/`--memory-swap` at `:234-236`, `--pids-limit` at `:240` and `--tmpfs`
at `:244,:246`. Docker absence is a hard error, never a silent unsandboxed
fallback (`sandbox.rs:15`). Egress denial is probed live, including the cloud
metadata endpoint at `sandbox.rs:1442`. The transport itself exists:
`crates/sharpebench-sim/src/external.rs:1-6` (line-delimited JSON over stdio) and
`crates/sharpebench-sim/src/lib.rs:27`.

**Benefit.** None over what exists.

**Threat and assumption changes.** None; Bench's boundary is strictly harder.

**Decision: reject.** The architecture is confirmation that Bench's existing
network-disabled boundary is the right one, not a source of new code. The one
transferable observation is negative and is recorded in row 13.

## 13. Line-count-bounded response queue

**Source.** `sandbox/sealed_runner.py:145`
`self._stdout_lines: queue.Queue[Optional[str]] = queue.Queue(maxsize=1024)`.
The bound is on the number of queued lines. Nothing bounds the bytes in one
line, and the reader thread's `readline` will grow a single string for as long
as the candidate withholds a newline.

**Existing Sharpe consumer.**
`crates/sharpebench-sim/src/external.rs:40` `MAX_AGENT_LINE`, whose comment at
`:42-46` states the failure mode exactly: "an unbounded `read_line` grows a
`String` for as long as the agent keeps writing without a newline: one hostile
(or merely wedged) entrant takes the whole harness down with it, losing every
other agent's results in the same sweep." Backpressure is separately bounded at
`external.rs:67` (`mpsc::sync_channel(1)`), cumulative output at `:47`, and
oversize is a typed non-retryable fault at
`crates/sharpebench-sim/src/transport.rs:36` `Oversized`.

**Benefit.** None.

**Decision: reject.** Bench already holds the stronger invariant and documents
why. This row exists to record that the archive is a counter-example, not a
model.

## 14. Two-network gateway topology

**Source.** `sandbox/native_runtime.py:415-423` creates an
`--internal` Docker network; the entrant container joins only that network
(`:448-450`), and the gateway sidecar is attached to both it and an egress
network (`:481-490`), running `--read-only` with `--cap-drop` (`:452-455`).
`NATIVE_NETWORK_PROFILE = "provider-only"` at `:34`, recorded at `:696-698`.

**Existing Sharpe consumer.** Bench runs a single network-disabled container
(`crates/sharpebench-arena/src/sandbox.rs:223,1551`). There is no sidecar and no
second network.

**Benefit.** If Bench ever needs to offer optional host-mediated provider access
to an otherwise isolated entrant, the internal-network-plus-sidecar topology is
the standard way to do it without giving the entrant a route to the internet.

**Threat and assumption changes.** It replaces "no egress exists" with "egress
exists behind a proxy we wrote", which is a materially weaker property and moves
the correctness burden onto the proxy. It also makes the sandbox's teardown
responsible for two containers and a network, each of which can fail
independently.

**Decision: defer.** This is a G11 prerequisite, not an independent port. It
should not be built until the gateway of rows 9 and 10 is specified with its
bounds and ownership rules, and it must not be built at all if entrants continue
to bring their own credentials, since in that case the isolation Bench has today
is both simpler and stronger.

## 15. Per-request model-call quota

**Source.** `sandbox/sealed_runner.py:79`
`max_model_calls_per_request: int = 32`, with the counter reset per request at
`:352` and enforced at `:378-383`. The same shape bounds Client API calls.
`hyper/live_experiment.py:106-134` adds a per-run quota (`max_runs: int = 10`,
"A failed run still consumes a quota slot").

**Existing Sharpe consumer.**
`rg -n -i -e "spending cap|spend_cap|cost_cap|max_spend" crates/ scripts/`
returns one hit, and it is a doc comment about an entrant's own exported
variable: `crates/sharpebench-harness/examples/llm_field_eval.rs:17`. No budget
type exists. What does exist is a *resource* budget:
`crates/sharpebench-arena/src/sandbox.rs:234-240` (memory, PIDs), whose breach is
a typed agent fault at `crates/sharpebench-harness/src/failure.rs:49`
`ResourceLimitExceeded`.

**Benefit.** A hard per-request ceiling is a cheap circuit breaker against a
runaway loop, and it is enforceable without any accounting infrastructure.

**Threat and assumption changes.** A per-request counter is not a budget. It
resets every request and lives only in the calling process's memory, so it
survives neither a restart nor a sweep. An entrant that stays under the ceiling
on every request has no ceiling at all across a sweep. Bench's existing
`checkpoint.rs` persistence is the natural place for a sweep-scoped counterpart,
and the two are complementary rather than alternatives.

**Decision: adapt.** Take the per-request ceiling as a circuit breaker. Add a
sweep-scoped, persisted budget keyed to the checkpoint identity
(`crates/sharpebench-harness/src/checkpoint.rs:33`) as the actual budget. Both
belong to G11.

**Tests.** A request exceeding the per-request ceiling fails typed, not silently
truncated. A sweep that resumes after a crash resumes its spend counter rather
than restarting it. A budget breach is a typed refusal that withholds the score
rather than emitting a partial one.

## 16. One-shot live-experiment budget

**Source.** `hyper/live_experiment.py:68-74`: a lock-guarded `_used` flag,
consumed even when the run fails, with the docstring "Run the experiment once,
consuming the attempt even on failure". The refusal at `:59-66` names the saved
report so a client timeout does not destroy the result, with the reason at
`:20-25`. Registration is task-gated at `sandbox/orchestrator.py:667-673`.

**Existing Sharpe consumer.** Nothing equivalent.
`rg -n -i "one.?shot|single_use|consume" crates/` finds no budget primitive.
G12 (experiment preflight and cost refusal) is open at `IMPLEMENTATION.md:41`.

**Benefit.** The pattern that matters is not the one-shot itself but the rule
that a *failed* attempt still consumes the quota, and that the result is
persisted before it is returned so a transport failure cannot burn the spend
with nothing to show. Both are directly applicable to G12's cost-refusal paths.

**Threat and assumption changes.** `_used` is per-process, in memory. It does not
survive a restart, so it is a guard rather than a budget. Bench should not
reproduce that limitation.

**Decision: adapt.** Take "a failed attempt consumes the quota" and
"persist before returning" into G12. Persist the counter itself, unlike the
source.

**Tests.** A crashed experiment consumes its slot. A second attempt is refused
with a pointer to the persisted first result. The counter survives a restart.

## 17. Developer-visible field allowlist seal

**Source.** `hyper/live_experiment.py:139-143` states the reason plainly: the
provider library's `raw_data` echoes the request back, including the hidden
scenario instructions, "so dumping messages wholesale hands the sampled task
definitions to the sandbox". `developer_visible_message_json` at `:149` projects
onto a fixed tuple of fields, and `:205-212` limits the returned report to
conversations plus a binary score, with no rationale, assertions, reward
breakdown or canonical task ids.

**Existing Sharpe consumer.** Bench holds the governance property
(`crates/sharpebench-attest/src/sealed.rs:1`, salt custody at
`crates/sharpebench-arena/src/lib.rs:128,463`) but the projection discipline is
not implemented for anything an entrant reads back.
`rg -n -i "allowlist" crates/` matches only the environment allowlists in
`crates/sharpebench-sim/src/external.rs:229,243`.

**Benefit.** Held-out leakage through a diagnostic response is a real and easy
failure. An allowlist projection is failure-closed: a field added later is
invisible until someone adds it deliberately. A denylist is failure-open.

**Threat and assumption changes.** None adverse. It narrows what leaves the
host.

**Decision: take.** Any Bench surface that returns held-out evaluation detail to
an entrant must project through a field allowlist, and the projection function
must be a single named seal with a test, as it is at `:149`.

**Tests.** Adding a field to the underlying record must not change the projected
output. A record carrying a planted secret in a non-allowlisted field must
project without it.

## 18. Best-effort container cleanup

**Source.** `sandbox/native_runtime.py:713-731` `close`. Each of the three
removals (gateway, container, network) catches `RuntimeError`, logs a warning,
and then clears the corresponding `_started` flag regardless. The failure is
neither raised nor reflected in the run verdict, and internal state is marked
clean even when the resource may still exist.

**Existing Sharpe consumer.**
`crates/sharpebench-arena/src/sandbox.rs:943-952` matches on both the inspect and
the remove result and returns `SandboxError::Inspection` if either failed,
including a combined message at `:950`. The rationale is at `sandbox.rs:925`:
"because silently scoring either state would weaken the sandbox contract." The
drop path is separately covered at `:977-986`.

**Benefit.** None.

**Decision: reject.** Bench's hard-failure cleanup is the correct behaviour and
must not be relaxed toward logged best effort.

## 19. Unbounded process event list

**Source.** `sandbox/native_runtime.py:195` `events: list[NativeProcessEvent] = []`
appended per line at `:207` under a lock, for the life of a build whose default
budget is `DEFAULT_BUILD_TIME_SECONDS = 8 * 60 * 60`
(`sandbox/builder.py:18`). Truncation exists only at display and persist time
(`hyper/recording.py:324` caps a tool result at 2000 characters;
`hyper/visualizer.py:97,201,219,262`), not at ingest.

**Existing Sharpe consumer.** `crates/sharpebench-sim/src/external.rs:47`
`MAX_AGENT_STDOUT` bounds cumulative accepted stdout per attempt; `:67`
`mpsc::sync_channel(1)` bounds resident queued output.

**Decision: reject.** Bench bounds at ingest, which is the only place the bound
matters. Note also the asymmetry in the archive's own recorder: tool arguments
are stored whole while tool results are capped at
`hyper/recording.py:324`, so the cap does not bound the larger of the two
attacker-controlled fields.

## 20. Early-return process-group termination

**Source.** `sandbox/native_runtime.py:146-148`: `_terminate_process_group`
returns immediately when `process.poll() is not None`. The docstring claims it
terminates "a process and every descendant in its process group", but a direct
child that has already exited leaves its group unsignalled and its descendants
running.

**Existing Sharpe consumer.**
`crates/sharpebench-sim/src/external.rs:410-419` gives the agent its own process
group precisely so teardown can reach all of it (`command.process_group(0)`),
with `external.rs:677` `fn signal_group(leader: u32, signal: &str)` signalling
the group rather than the leader.

**Decision: reject.** Bench's handling is correct and the archive's is a live
descendant-leak. Recorded here as a negative finding.

## 21. Silent task-support shrinkage

**Source.** `sandbox/orchestrator.py:1026-1029`: requested task ids not present
in the domain produce `logger.warning`, and the returned list is filtered to
those found. The reward at `:794-796` is
`mean(r.reward for r in final_results) if final_results else 0.0`, so the
denominator is whatever survived. A task that fails to load raises the mean.

**Existing Sharpe consumer.**
`crates/sharpebench-cli/src/main.rs:1125` emits
`"error": "incomplete_external_sweep"` and `:1132` refuses with the expected and
completed cell counts, emitting no score and no board. Tested at
`crates/sharpebench-cli/tests/attempt_accounting_cli.rs:11`. On the forecast
side, `crates/sharpebench-core/src/forecast.rs:1096` `CommonSupport` carries
`n_contracts` and `contract_sha256`; coverage is refused at
`paper/src/check-prospective-forecast-report.py:150` and
`paper/src/import-prospective-field.py:160`.

**Benefit.** None as behaviour.

**Threat and assumption changes.** A shrinking denominator is the quietest way a
benchmark can produce a wrong number, because nothing errors.

**Decision: reject the behaviour; keep it as a regression motive.** Bench's
explicit expected-versus-found refusal is correct. The archive is useful as a
worked example of the failure for the G07 and G12 test narratives.

## 22. Per-file task parse-error tolerance

**Source.** `hyper/task_loader.py:158-160` catches every exception per task file,
logs an error and continues; `:151` warns and returns an empty list when the
tasks directory is absent. A third instance is in the grounding predicate:
`hyper/grounding.py:76-78` returns an empty required set when
`task.evaluation_criteria is None`, so a criteria-less task passes trivially.

**Existing Sharpe consumer.** Same citations as row 21.

**Decision: reject.** A malformed corpus member must be a loud refusal, not a
smaller corpus. The `evaluation_criteria is None` case is the sharper lesson: a
missing check should never resolve to a pass.

## 23. Crash-tolerant in-progress checkpoint

**Source.** `hyper/recording.py:286` rewrites an `.in_progress.json` after every
completed eval task; `:46-65` writes atomically via a temporary file, `fsync` and
`os.replace`; `:178` records that "A persistence failure must not abort an
otherwise valid benchmark run".

**Existing Sharpe consumer.**
`crates/sharpebench-harness/src/checkpoint.rs:1` is a resumable, crash-tolerant
checkpoint for the external-agent sweep; `:9-11` describes per-task
`pending | claimed | done | failed` status so a resumed sweep runs only what did
not finish; `:12-15` covers multi-worker claim and stale reset on a monotonic
epoch rather than wall clock; `:17-19` states the byte-identical guarantee.
G06 is merged (`IMPLEMENTATION.md:35`).

**Decision: reject.** Bench's version is strictly more capable. The one detail
worth noting is the archive's microsecond-resolution filenames with a collision
loop (`recording.py:127,379-388`), which is a reasonable answer to a problem
Bench solves differently through checkpoint identity.

## 24. Published operation metadata triple

**Source.** `client_api/catalog.py:66-68`: every operation declares
`mutates_state`, `idempotency` (`safe | not_guaranteed`) and `automatic_retries`
(`allowed | forbidden`), independent of the HTTP verb. They are published to the
consumer as `x-api-mutates-state`, `x-api-idempotency` and
`x-api-automatic-retries` at `client_api/runtime.py:249-251`. The centralized
derivation is at `client_api/catalogs/banking.py:907-908`, with one deliberate
exception at `:1967-1969` where an idempotent PUT declares
`idempotency="safe"` despite mutating.

**Existing Sharpe consumer.** Bench encodes the equivalent distinction
structurally rather than declaratively: the order lifecycle in
`crates/sharpebench-core/src/process.rs` distinguishes submission,
acknowledgment, fill and reconciliation with typed violations at `:250-278`.
There is no per-operation published metadata.
`rg -n "mutates_state|idempotency" crates/` matches only
`crates/sharpebench-core/src/forecast.rs:240`, on forecast revisions.

**Benefit.** Making "this call changes the world and must not be retried
blind" a machine-readable property of the operation, rather than a fact the
scorer knows implicitly, lets both the entrant and the process checker read the
same declaration. It is the same distinction the deny-list draws by tool name,
expressed as contract.

**Threat and assumption changes.** The declaration becomes part of the contract
surface, so changing it changes what entrants were told. It must therefore be
covered by contract identity, which is exactly what the archive fails to do
(row 35).

**Decision: adapt.** Add the triple as declared metadata on the Bench decision
and order protocol, and fold it into the existing contract digest. Do not adopt
the archive's convention of leaving it derivable per catalog: a single
derivation with one audited exception is the maintainable form.

## 25. Post-commit timeout with idempotency-key replay

**Source.** `client_api/defects.py:240-251` `PostCommitTimeoutDefect`: lose a
successful mutation's response after its canonical commit, with
`timeout_status: Literal[504]` and `idempotency_header` defaulting to
`"Idempotency-Key"`. Enforced at `client_api/runtime.py:2191-2238`: the record
key is `(defect.id, operation_id, path, idempotency_key)`, the request is
fingerprinted as `sha256({"query": ..., "body": ...})`, reuse of a key with a
different body returns `409 idempotency_key_reused`, and a matching replay
returns the retained status and body.

**Existing Sharpe consumer.**
`rg -n -i -e "idempot" -e "exactly.?once" -e "client_order_id" crates/ scripts/ paper/src/`
finds `idempotency_key` only on forecast contract revisions
(`crates/sharpebench-core/src/forecast.rs:240`, uniqueness at `:547`). There is
no `client_order_id`, no submission dedup, and no async-mutation-versus-
projection distinction. The nearest control is warn-severity:
`crates/sharpebench-core/src/process.rs:278`
`DuplicateTransition { order, phase }`, whose own comment calls it "duplicated
bookkeeping, not a bypassed control."

**Benefit.** This is the most directly transferable mechanism in the archive and
the one genuine gap the repository's ledger does not already track. The scenario
is native to trading: the venue commits a fill, the response is lost in
transport, the agent retries. Whether the agent carried a client order id
decides whether it just doubled a position. Bench can currently observe the
duplicate as a warning but cannot distinguish a safe retry from an unsafe one,
because nothing in the protocol lets an agent express "this is the same order".

**Threat and assumption changes.** It adds an entrant-supplied key to the order
protocol, which is new attacker-controlled input and needs the archive's
same-key-different-body refusal to avoid becoming a way to overwrite a prior
order's identity. It also requires the simulator to be able to lose a response
after committing, which today it cannot: `crates/sharpebench-sim/src/engine.rs`
has no post-commit failure path. That is a simulator change, not only a
protocol change.

**Tests.** A retry with the same key after an injected post-commit timeout must
produce one position change, not two. A retry with the same key and a different
body must be refused. A retry without a key after an ambiguous write must be
recorded as a violation (row 26). Replay of the captured trajectory
(`crates/sharpebench-sim/src/trajectory.rs:1`) must reproduce the run exactly
with the fault active, since the fault is seeded.

**Decision: take.** Propose as a new ledger item, distinct from G07 and G11. It
is scoped, testable and trading-native, and it is the one place where the
archive holds something Bench does not.

## 26. Ambiguous-write-retried-without-key violation record

**Source.** `client_api/runtime.py:2254-2260` appends to
`retry_safety_violations` with `"reason": "ambiguous_write_retried_without_idempotency"`.

**Existing Sharpe consumer.**
`crates/sharpebench-core/src/process.rs:250-278` is a typed violation vocabulary
over an ordered trace, with block-versus-warn severity at `:291`
`pub fn is_block(&self)`, and the design note at `:26-27` that matching is not by
name. This is exactly the right host for a new variant.

**Benefit.** The graded artifact is not "did the call error" but "did the agent
retry an ambiguous write blind". That is a process property, deterministic over
a recorded trace, needing no judge.

**Threat and assumption changes.** Adding a block-severity variant changes what
fails, so it must ship behind the same severity discipline as the existing
variants and be introduced as warn-severity until calibrated.

**Decision: take.** Add as a `process.rs` violation variant alongside row 25.
Bounded change with an obvious test.

## 27. Projection lag with convergence verification

**Source.** `client_api/defects.py:275-392`: the read model goes stale without
delaying the authoritative write, with capture timing, trigger operations,
alternate read surfaces copying only `projected_fields`, and optional
`verification_operation_ids` ordering assertions. The client runtime records
convergence after a read observes the new state rather than after a timer
expires.

**Existing Sharpe consumer.** No stale-read fault exists. The simulator's
fault surface is seeded execution noise only:
`crates/sharpebench-sim/src/costs.rs:77` `ExecutionNoise`, declared at `:33`
(fill delay, partial fills), with engine paths at
`crates/sharpebench-sim/src/engine.rs:186` and `:60`. Arena has its own at
`crates/sharpearena/src/exec_noise.rs:68`.

**Benefit.** "The order is filled but `get_portfolio` does not show it yet" is a
real venue behaviour and a real agent failure: the agent resubmits. The
`verification_operation_ids` idea, that the agent must be observed confirming
convergence before asserting the new state, is a process check rather than an
outcome check and composes with `process.rs`.

**Threat and assumption changes.** It weakens an invariant entrants may rely on
today, that a read after a write reflects the write. That must be declared in
the contract before it is injected, or the fault is a trap rather than a test.

**Decision: adapt.** Take the write-authoritative-read-stale separation and the
convergence-observed-not-timed rule. Reject the domain-specific alternate-read-
surface machinery. Declare the relaxed consistency in the contract first.

## 28. Async completion via seeded status polling

**Source.** `client_api/defects.py:198-218` converts a synchronous mutation into
a workflow with a `status_path` containing exactly one `{workflow_id}` plus
`retry_after_seconds`; the status resource is served unadvertised
(`client_api/runtime.py:1966`).

**Existing Sharpe consumer.** None, and none wanted.

**Benefit.** Limited. Real venues do have asynchronous acknowledgment, but they
express it through order state transitions, which Bench already models
(`crates/sharpebench-core/src/process.rs:264-269`:
`AcknowledgmentWithoutSubmission`, `FillWithoutAcknowledgment`,
`ReconciliationWithoutFill`).

**Threat and assumption changes.** A seeded timer that flips a workflow to
terminal is not exchange behaviour, and building agent evaluation around
polling a synthetic workflow resource would teach a pattern that does not
transfer. The useful half, "the mutation is authoritative before its
presentation is available", is row 27.

**Decision: reject.** Keep order-state transitions as the asynchrony model.

## 29. Pagination fault, mode `limit_before_sort`

**Source.** `client_api/defects.py:254-272` defines modes
`cursor | truncate | lost_cursor | limit_before_sort` with
`ordering: canonical | reverse_canonical`; `client_api/runtime.py:805-813`
implements page-then-order. Cursors are opaque `cur_<24 hex>` bound to
`(defect_id, sha256(query), offset)` at `:753-789`, and a cursor replayed against
a different query yields `400 invalid_cursor` at `:818-835`.

**Existing Sharpe consumer.** None. Bench's data surface returns complete
results; `rg -n -i "paginat|cursor" crates/` finds no paging in the sim or
protocol crates.

**Benefit.** `limit_before_sort` is a classic correctness bug with an exact
trading analogue: "the ten largest positions" or "the most recent fills"
returning the wrong ten because the window was taken before the ordering. An
agent that does not detect it will reason confidently over a wrong window.

**Threat and assumption changes.** It requires Bench to have a paged read at all,
which today it does not. Introducing pagination purely to have a fault to inject
would be the wrong order of operations.

**Decision: adapt, conditionally.** Record `limit_before_sort` as the fault to
implement *if and when* a paged read is added to the Bench data surface. Do not
add pagination for the fault's sake. The cursor-binding discipline
(a cursor is valid only against the query that minted it) should be adopted at
the same time if it is.

## 30. Amount-sign normalization fault

**Source.** `client_api/defects.py:164-173` `response_amount_sign` normalizes
amounts by a discriminator to a fixed sign.

**Existing Sharpe consumer.** No sign-convention fault exists.
`rg -n -i "sign_convention|amount_sign" crates/` returns nothing. The nearest
thing is `crates/sharpebench-core/src/process.rs:257` `OrderSubjectDrift`, which
catches a different confusion.

**Benefit.** Sign convention is a genuine and expensive source of trading error:
a short expressed as a negative quantity versus a positive quantity with a side
field, a realized loss expressed as a negative or as a positive debit. An
injected sign inversion tests whether an agent reads the convention or assumes
it.

**Threat and assumption changes.** Modest. It is a response-shape perturbation
applied to a deep copy without touching canonical state
(`client_api/defects.py:721-812`), so the ledger stays authoritative. Bench must
apply the same discipline: perturb the projection, never the book.

**Decision: adapt.** Implement as a seeded response perturbation on the
observation, with the canonical position unchanged, and grade whether the agent's
subsequent decisions are consistent with its own stated reading.

## 31. Seeded rate-limit fault

**Source.** `client_api/defects.py:221-237` rejects one call ordinal until a
seeded monotonic deadline; enforced at `client_api/runtime.py:853-875`.

**Existing Sharpe consumer.** `crates/sharpebench-sim/src/transport.rs:86`
`CircuitBreaker` handles the harness side of a failing endpoint, and
`transport.rs:66` `decide_with_retry` retries, but nothing injects a rate limit
into the venue surface the agent sees.

**Benefit.** Every venue rate-limits. Whether an agent backs off or hammers is a
behaviour worth grading, and the seeded-monotonic-deadline construction keeps it
reproducible.

**Threat and assumption changes.** It changes per-cell wall time and therefore
interacts with deadlines and with row 4's backoff accounting. Recorded durations
must separate waiting-on-a-rate-limit from thinking.

**Decision: adapt.** Take the seeded-deadline construction. Grade the agent's
response as a process property rather than folding it into the return.

## 32. Frozen fault manifest with mutable trial state

**Source.** `client_api/defects.py:38-41` `_FrozenModel`
(`extra="forbid", frozen=True`) covers the manifest, activations and compiled
profile. Per-trial mutable state is confined to
`TrialDefectState` at `:674-692` (`call_counts`, `storage`,
`next_call_ordinal`, `reset()`). The manifest is content-addressed:
`manifest_sha256` over canonical JSON at `:815-824` and `:847-860`.

**Existing Sharpe consumer.** `crates/sharpebench-sim/src/costs.rs:77`
`ExecutionNoise` is seed-driven and configuration-frozen, but there is no
separation of a hashed fault *plan* from per-run mutable fault state, and no
manifest digest for the fault configuration. Window identity covers the scorer
and the salt (`arena/windows/window-003/window.json:35-37`) but not a fault plan.

**Benefit.** This is the structural discipline that makes fault injection
admissible in a published benchmark rather than a source of flakiness: the plan
is an immutable, hashed input to the experiment, and everything that varies
during a run is confined to one resettable object. It is a prerequisite for rows
25, 27, 29, 30 and 31 being reportable at all.

**Threat and assumption changes.** None adverse; it strictly increases what is
pinned. It does add an artifact whose digest must enter the window identity or
the pinning is decorative.

**Decision: take.** Adopt the frozen-plan-plus-trial-state split and the
manifest digest, and fold that digest into window identity alongside
`score_config_sha256`.

**Tests.** Two runs with the same manifest digest and seed produce byte-identical
traces. Mutating any manifest field changes the digest. Trial state after
`reset()` is indistinguishable from fresh.

## 33. Deterministic cohort draw over the manifest hash

**Source.** `client_api/defects.py:569-622`: a SHA-256 draw over
`(manifest_sha256, group or independent, id, seed, scenario_id)`, with mutually
exclusive groups occupying disjoint sub-ranges of one shared draw, validated to
sum to at most one and to use a single seed at `:491-511`.

**Existing Sharpe consumer.** Bench seeds noise per run
(`crates/sharpebench-sim/src/costs.rs:77`) but does not assign cells to fault
cohorts, and has no disjoint-subrange construction.
`rg -n -i "cohort" crates/` returns nothing.

**Benefit.** Deriving cohort assignment from the plan digest rather than a
separate random stream means the assignment is reproducible from the published
identity alone, with no extra state to record. The disjoint-subrange trick is a
neat way to keep mutually exclusive faults exclusive without a second draw.

**Threat and assumption changes.** Cohorting means not every cell sees every
fault, which changes the denominator for any per-fault statistic. That must be
reported explicitly, which is exactly the discipline row 21 demands.

**Decision: adapt.** Take the digest-derived draw if and when Bench cohorts
faults. Require explicit per-fault denominators in any reported result.

## 34. Capability offer, enable, freeze, seal

**Source.** `client_api/capabilities.py:70-86` `offer()`, `:88-95`
`enable_offered()` refusing anything not previously offered, `:97-110` `apply()`
rejecting non-allowlisted ids and any action after sealing, `:112-124` `freeze()`,
`:126-131` `seal()` as freeze plus refusal of later change.

**Existing Sharpe consumer.** No freeze-versus-seal state machine exists.
`crates/sharpebench-attest/src/lib.rs:42` `seal_dataset` is dataset-byte
encryption, a different meaning of the word. Window identity
(`arena/windows/window-003/window.json:35-37`) pins artifacts but does not model
a capability lifecycle.

**Benefit.** The distinction is worth naming, and the review should hold it:
*freeze* captures current state, *seal* additionally refuses later change, and
*contract identity* is a hash of what the surface actually is. They are three
things and the archive conflates the third with the first (row 35). A
venue-feature gate (margin enabled, options level, sandbox versus live) is a
plausible future Bench need with this exact shape.

**Threat and assumption changes.** Introducing a capability plane adds a second
axis along which two runs can differ, which must enter run identity or runs
become incomparable.

**Decision: defer.** Bench has no capability plane and no current need for one.
Record the freeze / seal / identity trichotomy in the review as vocabulary, and
revisit only if venue-feature gating is actually required.

## 35. Deployment snapshot hash over enabled IDs

**Source.** `client_api/capabilities.py:112-124`: `freeze()` hashes exactly
`json.dumps({"enabled": sorted_ids}, sort_keys=True, separators=(",", ":"))`.
Nothing about method, path, argument types, response schema or `mutates_state`
enters the digest. The materialized contract is written as a plain file with no
digest at `sandbox/kit.py:1189-1198`, and
`capabilities.py:133-155` `render_enabled_contract` re-derives the operation
document live by scanning for a matching `operationId`. A catalog edit that
changes an enabled operation's arguments therefore produces an identical
snapshot hash.

**Existing Sharpe consumer.** Bench and Arena hash content, not name lists:
`crates/sharpearena/src/spec_hash.rs:14`,
`crates/sharpebench-harness/src/accounting.rs:81`,
`crates/sharpebench-core/src/forecast.rs:230`
(`contract_digest_encoding`, so the encoding is itself declared).

**Benefit.** None.

**Threat and assumption changes.** If this were read as evidence that
hyper-tau freezes its API contract, the inference would be wrong. Only the fault
manifest is content-addressed (`client_api/defects.py:815-824`), and that hashes
the fault plan, not the surface.

**Decision: reject.** An identity over identifiers is not an identity over a
contract. Bench's content digests already hold the stronger property and must
not be weakened toward an id-list hash.

## 36. Set-based discoverable-call grounding

**Source.** `hyper/grounding.py`. The predicate is
`{observed mutating} | {observed & required} == {required}` over
`frozenset[str]` of tool names. The module documents the property itself at
`:21-23`: "Rows are keyed by tool name only, and upserted, call multiplicity and
arguments never influenced the row set." A criteria-less task returns an empty
required set and passes trivially (`:76-78`).

**Existing Sharpe consumer.** Ordered verification exists at two layers.
`crates/sharpebench-sim/src/trajectory.rs:1` is the replay-recompute
verification boundary, with `:10-12` stating that replaying a captured
trajectory reproduces the run exactly.
`crates/sharpebench-core/src/process.rs:12` requires that "the risk evaluation
happened *before the order it authorizes*", with subject-bound violations at
`:250` `UnauthorizedSubmission { subject, order, authorized_subjects }`,
`:257` `OrderSubjectDrift`, `:264-271` the lifecycle-ordering variants, and
`:271` `OutOfOrderTransition`.

**Benefit.** None for trading.

**Threat and assumption changes.** A name-set predicate cannot distinguish
selling one share from selling ten thousand, nor one call from fifty, because
neither arguments nor multiplicity enter the set. For a customer-service
reference database whose rows were upserted by name, the reduction is faithful.
For an order book it is not a proof of anything.

**Decision: reject.** Bench's ordered, subject-bound replay is the correct
instrument and the archive's own docstring explains why the set version cannot
substitute for it.

## 37. Deterministic re-execution contract for tools

**Source.** `data/tau2/hyper/framework_reference/client_api_contract.md`:
"During grading, a conversation's recorded tool calls are re-executed in order
against a fresh toolkit instance and a fresh backend, so each tool must behave as
a deterministic function of the backend state, its arguments, and the calls that
preceded it in the same conversation. Behavior that depends on anything else,
wall-clock time, randomness, or state carried over from outside the
conversation, may diverge on re-execution and fail the conversation."

**Existing Sharpe consumer.** The mechanism exists:
`crates/sharpebench-sim/src/trajectory.rs:10-12` guarantees that replaying a
captured trajectory reproduces the original run exactly, and
`crates/sharpebench-core` is required to be pure (`AGENTS.md`: no I/O, no system
clock, no ambient randomness). What does not exist is the *published contract
text* telling an entrant that non-determinism will fail its run.
`rg -n -i "re-execut|replay" crates/sharpebench-protocol/` returns nothing.

**Benefit.** This is the cheapest row in the document. The property is already
enforced; stating it in the entrant-facing protocol documentation converts a
silent failure into a declared rule, and makes divergence-on-replay a fair
verdict rather than a surprise.

**Threat and assumption changes.** None. It documents an existing invariant.

**Decision: take.** Add the determinism requirement to the entrant-facing
protocol contract, phrased as the archive phrases it: a deterministic function of
backend state, arguments, and prior calls in the same run.

**Tests.** A fixture agent that reads the wall clock must fail replay
verification with a typed divergence, not a silent difference.

## 38. Frozen per-run action catalog

**Source.** `hyper/action_catalog.py:39-45` rejects duplicate names and freezes
everything: `MappingProxyType` over the name map, a tuple of definitions, and
`@dataclass(frozen=True)` definitions at `:13` with `MappingProxyType` schemas at
`:54-55`. There is no registration path. It is built once per run inside a
frozen `ActionInterface` (`hyper/agent_context.py:20-38`, constructed at `:616`).
It is not coupled to `grounding.py`; neither module imports the other.

**Existing Sharpe consumer.** The decision surface is fixed by the protocol
crate rather than by a runtime catalog:
`crates/sharpebench-protocol/src/lib.rs:3`, with JSON Schemas mirrored at
`crates/sharpearena/contract/{observation,decision}.schema.json`.

**Benefit.** Marginal. Bench's surface is fixed at compile time, which is
stronger than a frozen runtime snapshot.

**Threat and assumption changes.** None.

**Decision: defer.** Relevant only if Bench ever admits per-run variable action
surfaces, in which case freeze-at-construction with duplicate rejection is the
right shape. Contrast with `hyper/transformations/base.py:250-265`, which does
keep a mutable module-level registry: the archive is not uniform about this.

## 39. Source-strip allowlist for the runtime image

**Source.** `docker/hyper-construction/strip_runtime_src.py`, run by the
construction Dockerfile, which then tests imports. The script allowlists runtime
modules rather than excluding known private ones, and its static tests check
sensitive modules and imports.

**Existing Sharpe consumer.** Bench's entrant image is entrant-supplied and
digest-pinned (`crates/sharpebench-arena/src/sandbox.rs:790`), so there is no
evaluator-provided image to strip. The nearest analogue is the sealed dataset
(`crates/sharpebench-attest/src/sealed.rs:32`).

**Benefit.** Allowlist-over-denylist is the correct polarity for any Bench
artifact that ships toward an entrant, and the pattern of proving the stripped
artifact still works (import test) rather than assuming it does is worth copying
into G07's artifact handling.

**Threat and assumption changes.** Stripping an image proves what that image
does not contain. It proves nothing about entrant artifacts, and it does not
make the image reproducible: the same Dockerfile installs from mutable base tags
and a live installer, so a pinned application version is not a pinned image. The
same limitation is broader than the Dockerfile. The transformations package
cannot regenerate `data/`: its rendering parsers are held out of tree
(`hyper/transformations/transcript_artifacts.py:17-19`, "maintained privately
with the authoring tooling"), so 6,736 corpus files under
`data/tau2/hyper/sops` are pre-committed bytes with no in-repo producer.

**Decision: adapt.** Take allowlist polarity and the post-strip functional test
into G07. Do not present source stripping as evidence of absence, and do not
treat a version-pinned Dockerfile as a reproducible image.

## 40. Public kit separation and generic artifact names

**Source.** `sandbox/kit.py` keeps host coverage reports outside the developer
kit, uses generic artifact names, and emits public API documentation with
synthetic development data. Missing framework documentation is only warned
about. The neutralization discipline is systematic in
`hyper/transformations/`: `base.py:4-17` defines `neutralize` as stripping
section ids and generation filenames, and delivered artifacts are grepped for
evaluator vocabulary before shipping
(`knowledge_base_html_export.py:225-230`, `helpdesk_automation_export.py:39-45`,
`contact_center_qa_export.py:161-168`).

**Existing Sharpe consumer.** The governance boundary exists
(`crates/sharpebench-attest/src/sealed.rs:1`, custody at
`crates/sharpebench-arena/src/lib.rs:128,463`, `docs/GOVERNANCE.md:31`), but no
public-versus-held-out split exists in `examples/`. The review already flagged
this as forward-looking in HYPER-TAU-REVIEW.md, "Runtime and provider
follow-through".

**Benefit.** The reusable pieces are the generic-artifact-name rule (a filename
must not carry evaluation identity) and the delivered-artifact leak grep (assert
that shipped bytes contain none of a named vocabulary). Both are cheap and
testable.

**Threat and assumption changes.** A warning is not a gate. The archive's own
"missing framework documentation is only warned about" is the pattern to avoid.

**Decision: adapt.** Apply generic-artifact naming and a delivered-artifact leak
assertion to any Bench example or fixture that ships alongside held-out data.
Make the assertion a gate, not a warning.

## 41. Fact-coverage compiler and modality substitution

**Source.** `hyper/transformations/compile.py` (1890 lines) proves, per atomic
fact, where a developer can learn it, hard-failing or routing to an explicit
appendix; `modality.py:11-14` enforces substitution rather than addition
("shipping a screenshot's visible text next to its PNG would let vision models
grep the text and skip `view_image`, collapsing the image-fact-discovery signal
the benchmark measures"); ordering is content-seeded
(`compile.py:379-380`, `:1249`, `:1859`) and ZIP outputs are byte-reproducible
(`api_contract_pack.py:71-104`, `:232`).

**Existing Sharpe consumer.** Not applicable. Bench's inputs are market data and
a scoring contract, not an evidence corpus.

**Benefit.** The determinism engineering is exemplary and worth citing as prior
art for reproducible artifact construction. The substitution invariant is a real
insight about multimodal benchmark design.

**Threat and assumption changes.** Adopting any of it would mean building an
evidence-distribution experiment, which is the outer agent-building product this
reconciliation excludes.

**Decision: reject.** Out of scope by the stated boundary. Cited for its
determinism discipline, not ported.

## 42. Response-phrasing rule packs

**Source.** `hyper/response_phrasing.py` loads YAML rule packs
(`data/tau2/hyper/response_phrasing/*.yaml`, six files, 261 to 333 lines) and
grafts `ResponseAssertion` and `NLAssertion` objects onto a task's evaluation
criteria, with per-domain safety declarations, composition and cycle detection
(`:166-169`).

**Existing Sharpe consumer.** None, and none wanted.
`rg -n -i "nl_assertion|phrasing" crates/` returns nothing.

**Benefit.** None for trading.

**Threat and assumption changes.** The rules are conversational style
constraints ("never use the word unfortunately", "do not apologize more than
once per conversation"). Half are graded by natural-language assertion, which
means an LLM judge. Importing this would introduce model-judged ranking into a
benchmark whose scoring kernel is deterministic.

**Decision: reject.** Customer-service semantics, and it would reopen the
judged-ranking question the review already closed.

## 43. Held / contested / confirmable operator partition

**Source.** `hyper/client_sim/instructions.py`: atomic policy facts are
partitioned into facts only the operator knows, facts whose artifacts disagree,
and facts the operator can only confirm, with everything else answered by an
explicit "my memory is unreliable outside these points". Invariants are enforced
at render time (`:324-345`, `:471-477`) and rendering is deterministic with no
model call (`:18-20`). The machinery is domain-neutral; exactly one hard-coded
domain table exists at `:35-42`.

**Existing Sharpe consumer.** None. Bench has no simulated human counterparty.

**Benefit.** As a design pattern it is genuinely general and the deterministic,
diffable, unit-testable prompt construction is good practice.

**Threat and assumption changes.** It presupposes a conversational counterparty
that Bench does not have and should not acquire. The relevant Gordon-side
analogue (`ask_user`) is a different mechanism with different semantics.

**Decision: reject.** No consumer, and adding one would import the
conversational evaluation surface the suite deliberately excludes.

## 44. Zero default disables the step limit

**Source.** `sandbox/builder.py:31,36`: `max_steps: int = 0`, where zero disables
the limit. Steps are therefore telemetry by default
(`hyper/visualizer.py:288` prints "Steps (telemetry only)"). The only wall-clock
bound is `DEFAULT_BUILD_TIME_SECONDS = 8 * 60 * 60` (`builder.py:18`).

**Existing Sharpe consumer.**
`crates/sharpebench-sim/src/external.rs:126` `STDIO_DECIDE_TIMEOUT` is a
30-second per-decision deadline with an override at `:457` and the sandbox
plumbing at `crates/sharpebench-arena/src/sandbox.rs:909`; readiness is bounded
at `sandbox.rs:286`.

**Benefit.** None.

**Threat and assumption changes.** A sentinel that disables a safety bound by
default is the wrong polarity. Bench's bounds are on by default with explicit
named overrides (`allow_unpinned_image` at `sandbox.rs:45` is the pattern).

**Decision: reject.** Recorded as a negative finding.

## 45. Hermetic environment for the agent process

**Source.** `harnesses/claude.py:104-116` declines to scrub the agent's shell
environment, reasoning that scrubbing needs bubblewrap, which the container
forbids, and that the per-run gateway token is model-scoped and cannot be
exfiltrated because there is no egress. Two harnesses differ and record the
difference in their own metadata: `claude.py:84`
`"gateway_token_inherited_by_shell": True` versus `False` for the others, with
`codex.py:83` excluding the token explicitly.

**Existing Sharpe consumer.**
`crates/sharpebench-sim/src/external.rs:227-266`: an allowlist rather than a
denylist (`HERMETIC_ENV_ALLOWLIST` at `:229` and `:243`), per-variable opt-in at
`:249`, secret handling at `:256`, and a fail-toward-secrecy predicate at `:266`
whose rationale is at `:261`.

**Benefit.** None. The archive's honesty about the divergence is admirable but
the property itself is weaker.

**Decision: reject.** Bench's allowlist is the correct polarity, and it does not
depend on an argument about egress being absent.

## What this reconciliation does not cover

- The outer construction loop (build a domain, score the build) and its reward,
  by the stated boundary.
- The voice, gym, user-simulator and metrics subtrees, which serve the
  conversational product.
- Any integration with the reference arena, which was not required.
- Whether the deployed Hyper-Tau service behaves as this source does. Every
  finding is a source-level observation about a downloaded archive.
