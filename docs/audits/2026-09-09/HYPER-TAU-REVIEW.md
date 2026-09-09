# Hyper-Tau porting review

Status: in progress. Inventory is complete; end-to-end source and corpus review
is not. This document replaces neither the prior assessment nor a finished
coverage claim.

## Inventory and coverage

The local archive contains 8,484 files: 7,818 under data, 323 under src,
172 under tests, 151 under web, three Docker files, and root/configuration
assets. Counting or searching a file is not reading its contents.

Complete reads in this review include:

- Root README, AGENTS, license and Python package configuration, plus tests/AGENTS.
- hyper/runtime_contract.py, performance.py, agent_context.py and _inner.py.
- hyper/sandbox/model_gateway.py, native_runtime.py, builder.py,
  callback_broker.py, callback_mcp.py, starting_workspace.py and
  result_serialization.py, orchestrator.py, sealed_runner.py and candidate_server.py.
- tests/plus_support/leakage.py.

Kit construction, provider adapters, client API, domain tools,
task corpus, test coverage,
web UI and remaining configuration are still open. No model calls or repository
test suites that require provider access have been run.

Paths below are relative to src/tau2 unless stated otherwise. The review concerns
the downloaded source, not a claim about a deployed service.

## Corrections to the earlier assessment

The code has two separate gateway arrangements. Native construction uses an
HTTP sidecar. The scored candidate in sealed_runner.py instead keeps Docker
networking disabled and multiplexes model requests over stdout to a host
provider caller. That host-mediated arrangement is a more direct candidate for
Bench's existing network-disabled boundary. Neither implementation should be
copied unchanged: the sealed runner's queue bounds line count rather than line
bytes, its outer request deadline does not interrupt a blocking provider call,
and model-call limits are per request rather than per sweep. These are source
observations, not demonstrated attacks on the deployed service.

| Mechanism | What the source actually does | Sharpe suite decision |
|---|---|---|
| Contamination scan | hyper/sandbox/orchestrator.py:256 searches known strings. Its call at line 818 follows final scoring; matches are recorded in the result, not used as a pre-launch refusal. Read errors are skipped. | Take bounded known-content detection, not this scanner unchanged. An unreadable or incompletely scanned artifact cannot receive a clean verdict. A clean scan cannot prove absence of arbitrary embedded or transformed data. |
| Runtime identity | native_runtime.py verifies an in-container integer contract export and records the resolved image ID. Configuration permits version/commit tags, not only content digests. | Keep Bench's stricter digest-pinned launch and Arena's semantic fingerprint. A compatibility version and immutable identity answer different questions. |
| Provider retries | _inner.py uses up to three attempts with 5/15-second backoff. Exception names and message fragments classify transient errors. Exhaustion and task errors both yield zero reward, with different failure labels. | Preserve Bench's typed runtime/agent distinction and incomplete-sweep withholding. Do not import error-message-driven attribution. Backoff and cancellation are operational behavior, not merely cosmetic. |
| Credit rates | agent_context.py:83 casts rates to float and checks negativity, which alone does not reject NaN or infinity. Usage parsing can default absent fields to zero. performance.py can likewise count absent usage as zero. | Require finite nonnegative rates, explicit units, frozen rate/model identity, validated usage counts and typed missing usage. A rate card is an operator declaration, not proof of a provider invoice. |
| Model gateway | model_gateway.py separates an ephemeral caller token from the provider credential and allowlists models/routes. Request bodies are capped at 64 MiB; the upstream client uses read=None and there is no corresponding bounded response/spend ledger in this module. | Take credential separation and restricted routing with independent size, duration, concurrency and spend bounds. Do not expose provider-owned response retrieval/deletion under a shared credential without ownership checks. |
| Container lifecycle | native_runtime.py uses an internal entrant network and a gateway attached to both networks. Cleanup calls use check=False; close does not inspect a nonzero Docker exit status. | The two-network architecture is useful for optional provider access. Keep Bench's explicit cleanup/resource-verdict failures; do not weaken them to logged best effort. |
| Process supervision | native_runtime.py streams lines into an unbounded event list. Group termination returns early if the parent has already exited. | Retain Bench's bounded transport and descendant-lifecycle protections. The reference implementation is not a replacement for them. |
| Developer feedback | callback_broker.py exposes token-bound, quota-limited host callbacks through a narrow filesystem request protocol. | Useful pattern for a future build/evaluate workflow, not a reason to add one now. Agent-writable request/response paths need a separate race/symlink review before reuse. |
| Task support | orchestrator.py's domain-task loader warns about missing requested IDs and returns the IDs it found; final reward uses the resulting list. | Keep explicit expected-cell/support checks. An absent requested task must not silently change the evaluation denominator. |

These are source-level observations and design decisions, not claims of a
reproduced exploit against Hyper-Tau.

## Accepted work and present limits

The concrete implementation ledger is [IMPLEMENTATION.md](IMPLEMENTATION.md):

- G06: explicit runtime-failure recovery with retained history and unchanged
  completed/agent-fault cells. Recovery must not become selective result reruns.
- G07: a bounded preflight over declared entrant artifacts and known protected
  content. State which bytes were scanned and which encodings are unsupported.
- G08: frozen, validated rate cards with explicit unavailable usage/cost.
- G09: publish attempts independently of rank. The existing ledger measures
  host duration, not monetary spend; accounting must keep that distinction.
- G11: optional host-observed model accounting. No integration with another
  benchmark or mandatory hosted service is required.
- G12: test experiment preflight and cost refusal paths without running a field.

The prior rough line estimates are not acceptance criteria. Artifact integrity,
crash recovery, network isolation and accounting interact, and each needs tests
through its actual consumer.

Statistical scoring, the conversational user simulator and the outer coding-agent
construction reward are not interchangeable with quantitative-trading evaluation.
Nothing read so far justifies replacing the Sharpe scoring kernel or adopting
model-judged ranking.
