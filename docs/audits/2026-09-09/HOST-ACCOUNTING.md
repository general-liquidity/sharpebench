# Host-observed model gateway accounting (G11) and field readiness (G12)

Evidence for the two open rows in
[IMPLEMENTATION.md](IMPLEMENTATION.md). G08 shipped frozen rate cards whose
usage is entrant-reported. G11 is the other half: usage the host itself
observed, under host-owned credentials, routing and budgets. G12 is the
readiness work that lets an operator see what a paid field would do without
running one.

Nothing here changes a published number. The scoring kernel, the ranking path,
the goldens, the examples and `paper/evidence/` are untouched; the only shared
file this work edits outside its own modules is a single import and a single
dispatch arm in the CLI entry point, plus three public accessors on `RateCard`.

## What host-observed usage is not

Host-observed usage is what the host saw a provider report on the wire, priced
once against a frozen rate card. It is not a verified invoice, not a billing
reconciliation and not an input to any score, rank or pass^k pool. It is
published beside the scored pool, exactly where the entrant-reported estimate
already sits, and the two are told apart by the `usage_source` label
(`host_observed` against `entrant_reported`). A total that contains any
unmeasured amount is not published as a total at all: it becomes
`unavailable` with a `known_subtotal_usd_nanos`.

## Protocol, and why it suits network-disabled entrants

The gateway is a host-side broker over newline-delimited JSON: one request line
in, one response line out, on a pipe the host already owns. It is the same
channel shape the external-agent protocol uses, so an entrant container with no
route to the internet, no listener, no port and no TLS stack can still be
evaluated against a hosted model.

The alternatives were worse for this repository:

- An HTTP sidecar would need the entrant to hold a URL and reach a socket. That
  means widening egress, or maintaining a per-entrant loopback listener, to make
  a convenience work. The goal explicitly rules that out.
- A provider SDK inside the entrant would need the credential inside the
  entrant. Then the host owns nothing.

What an entrant may write is content: an alias, messages, a bounded output
length and optional tool definitions. What it may not write is routing. The
request schema has **no field** for a URL, an endpoint, a header, a credential,
a provider name or any identifier a second entrant could also name, unknown
fields are refused, and a set of host-owned names is refused explicitly by name
so the refusal says what happened. Destination, credential, provider, model,
revision, allowed output length and the frozen rate card are all resolved from
the host's route table by exact alias match: there is no prefix pass-through and
no fallback to an unversioned alias.

The response carries no provider identifier. The only handle in it is an ordinal
the host assigns inside its own journal.

### No networked transport ships here

The bytes-on-the-wire step is the `ProviderTransport` seam. This crate ships no
implementation of it. Adding one would put egress into a library that is
otherwise offline, and it would make the tests non-hermetic by construction.
The operator supplies the transport, and the host hands it the bounds rather
than leaving them to its discretion. Every test in this work drives a scripted
fake that cannot open a socket. No test uses an API key, and no test makes a
network call.

### What the adapter owes, and what the broker actually checks

Handing a bound to an adapter is not enforcing it. Three of the bounds in the
table below travel with the call as obligations the adapter must satisfy, and
the broker's part in each is narrower than "enforced":

- `provider_read_timeout` bounds one socket read. The broker holds no socket and
  never sees a read. This one is entirely the adapter's.
- `provider_call_timeout` bounds one dispatch. The broker calls
  `ProviderTransport::call` synchronously and cannot cancel it, so an adapter
  that blocks forever blocks the broker with it. What the broker does is
  measure the wall clock across the call and refuse an answer that arrived after
  the deadline: the response is a `provider_timeout` refusal, and the call
  settles `Unknown(AdapterDeadlineExceeded)`, charged at its reservation. The
  request was on the wire, so a late answer must not become a free retry. This
  is enforcement of the outcome, not of the duration.
- `max_response_body_bytes` bounds the response buffer. The adapter allocates
  it, so only the adapter can bound the allocation. The broker checks the length
  of what it is handed, which is after the fact, and settles an oversized body
  as `Unknown(AmbiguousAfterCommit)`.

The reason the broker does not enforce the first and third itself is structural:
enforcing a deadline on a synchronous call needs a thread it can abandon or an
async runtime, and enforcing a pre-allocation bound needs the broker to own the
socket. Both would put the transport, and therefore egress, inside this crate.
The trait documentation states these obligations at the seam where an adapter
author reads them.

## Bound table

Every bound is checked before the allocation it governs. Defaults from
`GatewayLimits`.

| What is bounded | To what | Where |
|---|---|---|
| Request line, before parsing | 256 KiB | `max_request_line_bytes` |
| Messages in one request | 64 | `max_messages` |
| Content of one message | 32 KiB | `max_message_bytes` |
| Tool definitions in one request | 16 | `max_tools` |
| One tool payload (name plus schema) | 32 KiB | `max_tool_payload_bytes` |
| Output tokens asked of a provider | 8192, and the route's own ceiling, whichever is smaller | `max_output_tokens` |
| Input tokens reserved for framing the entrant never wrote | operator-set per route, required | `ModelRoute::input_token_overhead` |
| Provider response body | 1 MiB, an adapter obligation; the broker checks the length after the adapter allocated the buffer | `max_response_body_bytes` |
| Model text handed back | 128 KiB | `max_response_text_bytes` |
| Response line emitted | 256 KiB (an over-long line becomes a refusal) | `max_response_line_bytes` |
| Dispatches per entrant request | 1 plus 2 retries | `max_retries_per_request` |
| Concurrent provider calls, across every entrant | 4 | `max_concurrent_calls` via `CallPermits` |
| One provider socket read | 60 s, an adapter obligation the broker cannot check | `provider_read_timeout` |
| One dispatch, connect and all reads | 180 s, an adapter obligation; the broker refuses a late answer rather than cancelling the call | `provider_call_timeout` |
| Calls in the sweep, retries included | operator-set, required | `GatewayBudget::max_calls` |
| Money the sweep may commit | operator-set, required | `GatewayBudget::max_usd_nanos` |
| Journal read from disk | 16 MiB | `MAX_JOURNAL_BYTES` |
| Route manifest read from disk | 256 KiB | CLI `MAX_ROUTES_BYTES` |
| Rate card read from disk | 64 KiB | G08 `MAX_RATE_CARD_BYTES` |
| Gateway requests while answering one observation | 32; the one past it is a typed `decision_request_limit` refusal and never reaches the provider | `max_requests_per_decision` |
| One entrant stdout line, request or decision | 8 MiB, the external-agent decision-line cap; a request is then held to the 256 KiB bound before it is parsed | serving loop `MAX_ENTRANT_LINE` |
| Entrant stdout per process, requests included | 64 MiB | serving loop `MAX_ENTRANT_STDOUT` |
| Entrant wall clock per decision | 30 s, net of the time the host spends serving its requests | serving loop `DEFAULT_DECIDE_TIMEOUT` |

The concurrency ceiling is held by the host and shared across every entrant's
gateway, so it bounds the host's fan-out rather than any one entrant's
politeness. Its permit is released on drop, including on an unwind, so a
panicking transport cannot leak a slot.

## Budget reservation design

The money record is an append-only journal. Every derived number (spend,
outstanding reservations, calls started) is a fold over its records, so nothing
stored can drift from the append-only truth, and there is no in-place field a
recovery could rewrite.

One dispatch is one reservation and one settlement:

1. **Refuse first.** Shutdown, the call ceiling and the money ceiling are
   checked before anything is appended and before the permit is taken. A hard
   refusal appends nothing and dispatches nothing: no request starts after it.
2. **Reserve.** The reservation is the worst case for this call: content bytes
   as an upper bound on the input tokens the *content* becomes (no tokenizer
   emits more tokens than the text has bytes), plus the route's declared
   `input_token_overhead`, plus the requested output tokens, quoted against the
   frozen rate card in integer USD nanodollars. The overhead exists because
   content bytes bound only the part of the request the entrant wrote: the
   adapter's system framing and the wire encoding of tool definitions are billed
   input that never appears in the content, and empty content does not imply
   zero billed input. It is per route, because it is a property of one
   provider's documented framing, and it is required rather than defaulted,
   because a zero would silently restore a reservation that bounds only part of
   the billed request.
3. **Persist, then dispatch.** The reservation is fsynced and renamed into place
   before the call starts. A process that dies during the call leaves a
   reservation behind.
4. **Settle exactly once.** `Priced` when the provider reported usage, quoted
   once, here. `Unknown` when the cost is not knowable, charged at the **full
   reservation**. `Released` only when the request provably did no billable work.

Who pays, by outcome:

| Outcome | Settlement | Money | Counts as a call |
|---|---|---|---|
| 2xx with usage | `Priced` | actual quote | yes |
| 2xx, empty completion, with usage | `Priced` | actual quote | yes |
| 2xx with no usage reported | `Unknown(UsageAbsent)` | full reservation | yes |
| 2xx, body unparseable, truncated or oversized | `Unknown(AmbiguousAfterCommit)` | full reservation | yes |
| 429 | `Released(ProviderRefusedBeforeWork)` | nothing | yes |
| other 4xx | `Released(ProviderRefusedBeforeWork)` | nothing | yes |
| 5xx | `Unknown(AmbiguousAfterCommit)` | full reservation | yes |
| timeout or drop after the request was committed | `Unknown(AmbiguousAfterCommit)` | full reservation | yes |
| adapter answered after the call deadline had passed | `Unknown(AdapterDeadlineExceeded)` | full reservation | yes |
| connection refused before the request left | `Released(ProviderRefusedBeforeWork)` | nothing | yes |
| cancelled after reserving, before dispatch | `Released(NeverDispatched)` | nothing | yes |
| process died mid-call | no settlement; folds as outstanding | full reservation | yes |

Two consequences are deliberate. Unknown completion or cancellation cost is
neither a free retry nor an automatic refund: the money stays committed and the
retry pays for itself, so a provider that times out repeatedly exhausts the
budget instead of looping for free. And a released reservation still consumes
one of the sweep's calls, because the call ceiling bounds attempts, not
successes.

Resume folds the amounts that were written. It never re-quotes: a journal is
bound to the route table digest and the budget, and a journal whose binding
differs is refused rather than truncated, merged or continued. The overhead is
inside `RouteTable::identity_digest`, so changing what the host authorizes per
call cannot continue an existing money journal.

### What a reservation is a ceiling on

A reservation bounds what this host will **authorize** before it dispatches. It
is not a bound on what a provider may **bill**. The host reserves against a
frozen rate card, an operator-declared framing overhead and the entrant's own
content; a provider that counts input differently, or bills for something the
host did not model, is outside anything this code can hold down.

So the observed price of a call can land above its reservation, and that is
handled rather than hidden:

- The observed amount is recorded as it was observed. It is never clipped down
  to the reservation, which would make the journal understate real spend.
- The gap is folded out separately as `SpendState::overspent_usd_nanos` and
  `overspent_calls`, and reported by `sharpebench gateway` alongside the spend,
  so an operator sees that the ceiling did not bind and by how much.
- Once committed money is past `max_usd_nanos`, `GatewayJournal::ceiling_breached`
  is true and the gateway starts no further call: every subsequent request is a
  `budget_exhausted` refusal before anything is reserved or dispatched. An
  overage is a fact to record, not authority to keep spending.

The deliberate choice here is that the successful call is still reported as a
success. The work happened and the entrant gets its completion; suppressing the
answer would lose real work and would not unspend the money. What must not
happen, and no longer can, is the sweep continuing past its ceiling on the
strength of an overage.

### Ownership of the money record

Atomic replacement of a whole file is not a shared budget. Two gateways that
each load their own snapshot and each replace the file whole will both spend,
both assign the same ordinals and the later save will erase the earlier record,
with no write-time race needed for it: two sequential dispatches after two opens
are enough.

The journal therefore carries a `version`, and `GatewayJournal::save` is a
compare-and-swap on it: a save from a snapshot the file has moved past is
refused as a typed `JournalSaveError::Conflict` instead of replacing a record
this process never read. A refusal before dispatch releases its reservation and
answers `journal_ownership_lost`, starting no call; a refusal at settlement
latches the same flag. Either way the gateway stops spending against a file it
no longer owns, and the records it appended stay in memory, reachable through
`ModelGateway::into_journal`, rather than being dropped.

The check has a residual window: the version is read, then a temporary is
renamed into place, and a second writer landing between those two steps is not
caught. Closing that needs a lease with an expiry or a lock whose stale state an
operator can clear, and a plain exclusive lock file would turn a crash into a
sweep that cannot resume. What the compare-and-swap removes is the far larger
window this defect actually lived in: two long-lived gateways spending, for
their whole lifetime, from the snapshot each read at open. Concurrency permits
do not help here, because they are per process.

## Identity

`RouteTable::identity_digest` covers, for every alias, the provider, model,
revision, rate-card digest, destination digest and output ceiling. Credential
values are absent from it: rotating a token is not an experiment change and must
not invalidate a resumable sweep, while changing a model, a revision, a price or
a destination must. Fold this digest into the sweep's `invocation_sha256`
alongside the G08 rate-card binding; the journal binds it directly.

## Test evidence

Hermetic fakes only. No API key, no network call, no model installation.

`crates/sharpebench-harness/src/gateway.rs` (30 tests) covers: forbidden fields
by name for twelve host-owned names; exact alias resolution; the host supplying
destination, credential, revision and every bound; the request envelope bounds;
the output-length bound against host and route; oversized, truncated,
unparseable and unknown-field provider bodies; the shared permit pool; the
reservation being on disk before dispatch; hard money and call refusals starting
no request; spend accumulating until a refusal; retries reserved and reconciled
exactly once; empty completions; absent usage as unknown rather than zero;
ambiguous post-commit failure charged with the retry paying again; rejection
before work releasing while a server failure settles unknown; resume without
repricing or reissuing; frozen model, revision, rate-card and destination
identity with credential rotation deliberately excluded; a journal refusing to
resume under a different revision; credential and provider-body redaction;
errors carrying no provider material; responses carrying no shared identifier;
the response line bound; cancellation starting no new call and leaving nothing
outstanding; rank neutrality; and route-table validation.

Four of those thirty are the regressions for the defects repaired on 2026-09-10:
`a_reservation_covers_framing_the_entrant_never_wrote` (an empty request under a
one-unit budget refuses before the wire rather than committing what the provider
reports), `usage_above_the_reservation_is_recorded_and_stops_the_sweep` (the
overage is folded out and the next call is refused),
`a_second_gateway_cannot_spend_the_journal_the_first_owns` (the second gateway is
refused `journal_ownership_lost`, dispatches nothing, and the first gateway's
record is what remains on disk), and
`an_answer_returned_after_the_deadline_is_refused_and_charged` (a fake that
sleeps past a 1 ms call deadline is refused and charged, not accepted).

`crates/sharpebench-harness/src/gateway_journal.rs` (10 tests) covers the fold:
unsettled reservations charged rather than refunded, unknown cost keeping the
whole reservation, released reservations still counting as calls, resume folding
recorded amounts, refusal of a journal bound elsewhere, refusal of a journal
that erases or double-settles an attempt, partial totals labelled partial,
missing usage unavailable rather than zero, the `host_observed` label, and
(added 2026-09-10) a sweep-bound journal resuming only under its own sweep.

`crates/sharpebench-cli/src/gateway_cli.rs` (6 tests) covers the operator
surface: the effective configuration report, a missing credential, missing and
zero budgets, malformed route manifests (wrong schema, empty, inline key
material), a journal bound to another route table, and (added 2026-09-10) a
sweep-bound journal reported with its sweep.

### Mutation results

Three invariants, each mutated in an isolated copy outside the worktree
(`%TEMP%\sb-mutate-g11`), never in the production tree.

| Invariant | Mutation | Result |
|---|---|---|
| All retries are counted | `GatewayJournal::reserve` returns the previous ordinal instead of appending on a retry | killed: `every_retry_is_reserved_and_reconciled_exactly_once` and 7 others failed |
| No cost is repriced on resume | the `Priced` fold uses the reservation instead of the recorded amount | killed: `a_resumed_journal_folds_recorded_amounts_and_never_reprices`, `a_resumed_gateway_keeps_earlier_spend_and_reprices_nothing` and 6 others failed |
| No request starts after a hard budget refusal | the money-ceiling check is removed from `dispatch_once` | killed: `a_hard_budget_refusal_starts_no_request`, `spend_accumulates_across_calls_until_the_budget_refuses` failed |

Three more for the 2026-09-10 repairs, mutated in place on a clean tree and
restored from `git show HEAD:<path>`, verified byte identical with `cmp`.

| Invariant | Mutation | Result |
|---|---|---|
| The reservation covers framing the entrant never wrote | `dispatch_once` reserves `request.content_bytes()` alone, dropping the route's `input_token_overhead` | killed: `a_reservation_covers_framing_the_entrant_never_wrote` and 5 others failed |
| A breached ceiling starts no further call | the `ceiling_breached` guard in `dispatch_once` is made unreachable | killed: `usage_above_the_reservation_is_recorded_and_stops_the_sweep` failed |
| A stale snapshot cannot replace the journal | the version comparison in `GatewayJournal::save` is made unreachable, so every save writes | killed: `a_second_gateway_cannot_spend_the_journal_the_first_owns` failed |
| An over-deadline answer is not accepted | the elapsed-time check after `transport.call` is made unreachable | killed: `an_answer_returned_after_the_deadline_is_refused_and_charged` failed |

The ceiling guard survived its first mutation, because a route whose reservation
is positive is already refused by the available-money check once committed money
has saturated it. The guard is load bearing only for a call that reserves
nothing, so the regression now drives a free route and a priced route sharing one
budget. That is recorded here rather than quietly fixed: a surviving mutant said
the test was weaker than the invariant it claimed.

## The entrant-serving loop (2026-09-10)

An independent verification found that the broker above had no runtime: the
`sharpebench gateway` command printed a report and returned, `serve_line` had
only test callers, the only transport was the test fake, and nothing attached
the broker to an entrant or read its journal as scored output. A
network-disabled entrant was therefore not evaluable through this path. This
section records the runtime that closes that gap.

### Design

The loop lives in `crates/sharpebench-harness/src/gateway_serve.rs`
(`sharpebench_harness::gateway::serve`). It is a library entry point, not a CLI
command, because serving needs a `ProviderTransport` and none ships here: an
operator calls it from a binary that supplies the transport. The CLI report
still never calls a provider.

- **The pipe.** The entrant's own stdio carries both protocols. After the host
  writes one observation line, the entrant may write any number of gateway
  request lines, each answered by exactly one response line on its stdin,
  before its decision line. A stdout line is a gateway request when it is a
  JSON object whose top-level `protocol` member is a string in the
  `sharpebench.model-gateway.` family; every other line is a decision under the
  closed decision contract, which refuses unknown fields, so no line can be
  both. The probe that tells them apart skips every other member without
  building a tree. A request in the family with an unsupported version is still
  routed to the broker and refused as `unknown_protocol`, rather than being
  misread as a malformed decision.
- **The agent.** `GatewayEntrant` is an `Agent` plus `TransportDiagnostics`
  like any external agent, so `run_external_backtest_observed`, the failure
  taxonomy and the scoring path are the ones every other entrant goes through.
  Its reader, writer, per-line and per-process bounds, exit handling and
  process-group teardown follow the external-agent transport; the host's time
  at the provider is excluded from the entrant's per-decision clock.
- **The sweep.** `run_gateway_sweep` wraps `run_resumable_sweep_observed`. It
  folds the route-table identity digest and the budget into the checkpoint's
  `invocation_sha256` (`sharpebench.gateway-invocation.v1`), binds the money
  journal to the checkpoint contract through a new optional
  `JournalIdentity::sweep_sha256` (`sharpebench.gateway-sweep.v1` over the
  entrant id and the contract), and admits a checkpoint and a journal only as a
  pair: a checkpoint whose journal is missing, and a journal with calls whose
  checkpoint is missing, are both refused before any entrant runs. A fresh pair
  starts by writing an empty journal, so a checkpoint never exists before its
  journal. A journal no sweep owns serializes exactly as before.
- **The output.** The sweep returns the `ResilientSubmission` unchanged and,
  beside it, a `HostObservedUsage` (`sharpebench.host-observed-usage.v1`): the
  host-observed `MonetarySummary`, the call counts, the overspend, whether the
  ceiling was breached and whether journal ownership was lost, with
  `rank_neutral: true`. `attach_host_observed_usage` puts it on the entrant's
  row of a JSON board beside its scores; the board stays an array and no score
  field moves.
- **The sandbox.** `sharpebench_arena::sandbox::gateway_launch` hands back the
  same hardened, `--network none`, inspectable `docker run` argv that
  `run_external_sandboxed` spawns, so the host can own the container's stdio.
  The launch plan was extracted from `run_external_sandboxed_with_command`, not
  changed. `wait_until_running` exposes the existing readiness wait.

### Properties kept

- No networked transport ships. The loop hands requests to `serve_line`; the
  bytes on the wire are still the operator's `ProviderTransport`.
- Credentials never reach the entrant. `EntrantLaunch::spawn` refuses, before
  anything starts, a launch whose program, arguments or declared environment
  carries a route credential; a host entrant gets a cleared environment; a
  launcher such as the Docker client inherits the host environment minus every
  variable whose value carries a credential. An answer that would carry a
  credential, a destination or the journal location, raw or JSON-escaped, is
  withheld as a typed `response_withheld` refusal (the call is still charged).
- Every bound in the table above is still enforced by the broker the loop
  calls; the loop adds the per-decision request ceiling and the entrant stream
  bounds listed there.

### End-to-end evidence

`crates/sharpebench-harness/src/gateway_serve_tests.rs` (12 tests). The
entrant is scripted on real OS pipes (`std::io::pipe`) or, in one test, is a
real child process (`sh` on Unix, PowerShell on Windows) on real stdio; the
provider is a scripted `ProviderTransport` that cannot open a socket. No
provider, key or network.

| Claim | Test |
|---|---|
| An entrant makes a call through its own pipe; the provider sees the host credential, the entrant sees an ordinal and text only | `an_entrant_reaches_the_model_through_its_own_pipe` |
| A real child process on real stdio reaches the model and is scored | `a_spawned_entrant_process_reaches_the_model_through_its_stdio` |
| A real sweep: every decision makes a call, the journal on disk records all 12, the output carries `host_observed` usage on the entrant's row, and removing that key gives back the unattached board exactly | `a_gateway_sweep_publishes_host_observed_usage_beside_the_rank` |
| A resume of a finished sweep makes no call, leaves the journal bytes identical and reports identical usage and returns | `a_resumed_gateway_sweep_makes_no_new_calls` |
| A crash mid-sweep reruns only the unfinished cell, keeps the interrupted cell's first calls charged, and ends with the returns of an uninterrupted sweep | `an_interrupted_gateway_sweep_reruns_only_unfinished_cells_and_keeps_their_spend` |
| A money refusal and a call refusal reach the entrant as typed errors (`budget_exhausted`, `call_limit_exhausted`) and nothing past the ceiling reaches the provider | `a_budget_refusal_reaches_the_entrant_as_a_typed_error` |
| The checkpoint carries the gateway binding; a missing journal, a missing checkpoint, a foreign sweep's journal and a changed budget are refused | `the_journal_and_the_checkpoint_are_bound_together` |
| An unsupported protocol version and an oversized request are typed refusals, not decision faults | `a_gateway_line_is_never_read_as_a_decision` |
| The per-decision ceiling refuses by type | `requests_past_the_per_decision_ceiling_are_refused_by_type` |
| A 400 ms provider under a 200 ms entrant clock is not an entrant timeout | `host_serving_time_does_not_count_against_the_entrant` |
| No launch carries a credential; a launcher inherits none | `credentials_never_reach_an_entrant_launch` |
| Host material echoed into an answer never crosses the pipe | `host_material_in_an_answer_never_reaches_the_entrant` |

Supporting tests: `gateway_journal::tests::a_sweep_bound_journal_resumes_only_under_its_sweep`,
`gateway_cli::tests::a_sweep_bound_journal_is_reported_with_its_sweep` and
`sandbox::tests::a_gateway_launch_is_the_hardened_network_disabled_launch`.

### Mutation results for the serving loop

Mutated in place on a clean committed tree, restored with
`git show HEAD:<path> > <path>`, and verified byte identical with `cmp` against
a copy of the committed file.

| Invariant | Mutation | Result |
|---|---|---|
| Host serving time is not the entrant's | the deadline extension adds `Duration::ZERO` instead of the serving time | killed: `host_serving_time_does_not_count_against_the_entrant` failed |
| The checkpoint is bound to the gateway | `run_gateway_sweep` keeps the caller's invocation digest instead of folding in the route table and budget | killed: `the_journal_and_the_checkpoint_are_bound_together` failed |
| A checkpoint without its journal is refused | the missing-journal refusal in `admit_pair` is made unreachable | killed: `the_journal_and_the_checkpoint_are_bound_together` failed |
| The per-decision ceiling holds | the ceiling comparison uses `u32::MAX` | killed: `requests_past_the_per_decision_ceiling_are_refused_by_type` failed |
| No launch carries a credential | `spawn` ignores the result of `refuse_credentials` | killed: `credentials_never_reach_an_entrant_launch` failed |
| Host material never crosses the pipe | the `carries_host_material` gate is made unreachable | killed: `host_material_in_an_answer_never_reaches_the_entrant` failed |

### What is still not here

No provider adapter ships, so no field run can be made from this repository;
that is the design, not a gap. The CLI does not run gateway sweeps, for the same
reason. A live Docker leg for `gateway_launch` would need a daemon and an image
that speaks the gateway protocol; the argv is pinned against the one
`run_external_sandboxed` uses instead. This work edits, outside its own
modules, `crates/sharpebench-arena/src/sandbox.rs` where the gateway attaches
and nothing in `main.rs`.

## G12: field readiness

Neither field producer can run here: this environment has no provider
credentials, and the goal forbids paid calls. Readiness therefore means the
runner refuses correctly and reports what it would do.

`llm_field_eval` gains `plan_field`, pure in a credential lookup so every
refusal path is testable with no process environment and no key. It refuses a
missing or blank credential, a missing call ceiling, a ceiling that is not a
positive count, and a model outside the declared field. `LLM_MAX_CALLS` is now
required rather than optional: this producer makes paid calls, and a field
started without a stated ceiling has no bound on what it spends before anyone
notices. `LLM_MODEL` selects one declared model. `SHARPEBENCH_DRY_RUN` prints
the plan (models, datasets, ceiling, effective controls, output path) and stops
before the first spawn and before any output file is opened.

`local_open_weight_field_eval` gains `plan_local`, likewise pure. It refuses an
absent or empty model list, a repeated tag, two tags that would collide
downstream, an unparseable or non-positive control, and an unparseable thinking
flag; the ad-hoc `env_parse` and `model_tags` panics they replace are deleted.
Its dry run runs before `probe_shim`, so a readiness report starts no
interpreter and loads no model.

## Verification

Run from the worktree root.

| Command | Exit |
|---|---|
| `cargo deny check` | 0 |
| `cargo fmt --all --check` | 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | 0 |
| `cargo nextest run -p sharpebench-harness -p sharpebench` | 0 (213 passed, 2 skipped) |
| `python paper/src/check-provenance.py` | 0 (OK) |

Re-run after the 2026-09-10 repairs, from the repair worktree root.

| Command | Exit |
|---|---|
| `cargo fmt --all --check` | 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | 0 |
| `cargo nextest run -p sharpebench-harness -p sharpebench` | 0 (278 passed, 3 skipped) |
| `python paper/src/check-provenance.py` | 0 (OK) |

No golden, example or `paper/evidence/` value moves: the repairs change what the
host authorizes before a call and what it does with a journal file, and nothing
in the scoring kernel, the ranking path or the frozen evidence reads either.
