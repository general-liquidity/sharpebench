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
| Provider response body | 1 MiB | `max_response_body_bytes` |
| Model text handed back | 128 KiB | `max_response_text_bytes` |
| Response line emitted | 256 KiB (an over-long line becomes a refusal) | `max_response_line_bytes` |
| Dispatches per entrant request | 1 plus 2 retries | `max_retries_per_request` |
| Concurrent provider calls, across every entrant | 4 | `max_concurrent_calls` via `CallPermits` |
| One provider socket read | 60 s | `provider_read_timeout` |
| One dispatch, connect and all reads | 180 s | `provider_call_timeout` |
| Calls in the sweep, retries included | operator-set, required | `GatewayBudget::max_calls` |
| Money the sweep may commit | operator-set, required | `GatewayBudget::max_usd_nanos` |
| Journal read from disk | 16 MiB | `MAX_JOURNAL_BYTES` |
| Route manifest read from disk | 256 KiB | CLI `MAX_ROUTES_BYTES` |
| Rate card read from disk | 64 KiB | G08 `MAX_RATE_CARD_BYTES` |

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
   as an upper bound on input tokens (no tokenizer emits more tokens than the
   text has bytes) plus the requested output tokens, quoted against the frozen
   rate card in integer USD nanodollars.
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
differs is refused rather than truncated, merged or continued.

## Identity

`RouteTable::identity_digest` covers, for every alias, the provider, model,
revision, rate-card digest, destination digest and output ceiling. Credential
values are absent from it: rotating a token is not an experiment change and must
not invalidate a resumable sweep, while changing a model, a revision, a price or
a destination must. Fold this digest into the sweep's `invocation_sha256`
alongside the G08 rate-card binding; the journal binds it directly.

## Test evidence

Hermetic fakes only. No API key, no network call, no model installation.

`crates/sharpebench-harness/src/gateway.rs` (26 tests) covers: forbidden fields
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

`crates/sharpebench-harness/src/gateway_journal.rs` (9 tests) covers the fold:
unsettled reservations charged rather than refunded, unknown cost keeping the
whole reservation, released reservations still counting as calls, resume folding
recorded amounts, refusal of a journal bound elsewhere, refusal of a journal
that erases or double-settles an attempt, partial totals labelled partial,
missing usage unavailable rather than zero, and the `host_observed` label.

`crates/sharpebench-cli/src/gateway_cli.rs` (5 tests) covers the operator
surface: the effective configuration report, a missing credential, missing and
zero budgets, malformed route manifests (wrong schema, empty, inline key
material), and a journal bound to another route table.

### Mutation results

Three invariants, each mutated in an isolated copy outside the worktree
(`%TEMP%\sb-mutate-g11`), never in the production tree.

| Invariant | Mutation | Result |
|---|---|---|
| All retries are counted | `GatewayJournal::reserve` returns the previous ordinal instead of appending on a retry | killed: `every_retry_is_reserved_and_reconciled_exactly_once` and 7 others failed |
| No cost is repriced on resume | the `Priced` fold uses the reservation instead of the recorded amount | killed: `a_resumed_journal_folds_recorded_amounts_and_never_reprices`, `a_resumed_gateway_keeps_earlier_spend_and_reprices_nothing` and 6 others failed |
| No request starts after a hard budget refusal | the money-ceiling check is removed from `dispatch_once` | killed: `a_hard_budget_refusal_starts_no_request`, `spend_accumulates_across_calls_until_the_budget_refuses` failed |

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
