# Host-observed model gateway

[Frozen token rate cards](cli.md#frozen-token-rates) price what an entrant said
it used. The gateway is the other half: an entrant that has no network at all
reaches a hosted model through the host, and the host records what it observed
each call cost, under host-owned credentials, routing and budgets.

The usage is published beside the scored pool and is never a rank input. It is
told apart from the entrant's own estimate by a `usage_source` label,
`host_observed` against `entrant_reported`.

> Host-observed usage is what the host saw a provider report on the wire, priced
> once against a frozen rate card. It is **not a verified invoice**, not a
> billing reconciliation, and not an input to any score, rank or pass^k pool.

There are two pieces. The **broker** (`sharpebench_harness::gateway`) turns one
request line into one response line and keeps the money journal. The **serving
loop** (`sharpebench_harness::gateway::serve`) attaches the broker to a real
entrant and a real sweep. Both are library code: this repository ships no
provider adapter, so no model call can be made from it as built.

## What an entrant sends and receives

The model calls ride the entrant's own stdin and stdout, the pipe the
[external-agent protocol](submitting.md) already uses. There is no listener,
port, URL or TLS stack on the entrant side, so a container launched with
`--network none` stays that way.

After the host writes one observation line, the entrant may write gateway
request lines before its decision line. Each request is answered by exactly one
response line on its stdin. The exchange for one observation looks like this:

```text
host    -> entrant   observation
entrant -> host      gateway request
host    -> entrant   gateway response
          (repeat, at most 32 requests per decision)
entrant -> host      decision
```

A stdout line is a gateway request only when it is a JSON object whose
top-level `protocol` member is a string starting with
`sharpebench.model-gateway.`. Every other line is read as the decision, under
the closed decision contract. That contract refuses unknown fields, so no line
can be both. A request that names a version this host does not speak, such as
`sharpebench.model-gateway.v9`, is still routed to the broker and refused as
`unknown_protocol`, rather than misread as a malformed decision.

A request, as the scripted entrant in
`a_spawned_entrant_process_reaches_the_model_through_its_stdio` writes it:

```json
{"protocol":"sharpebench.model-gateway.v1","model_alias":"fake.v1","messages":[{"role":"user","content":"size it"}],"max_output_tokens":16}
```

| Field | Rule |
|---|---|
| `protocol` | Exactly `sharpebench.model-gateway.v1` |
| `model_alias` | An alias the host published, matched exactly: no prefix match, no fallback to an unversioned alias |
| `messages` | 1 to 64 messages, each `{"role": "system" \| "user" \| "assistant", "content": ...}` with at most 32 KiB of content |
| `max_output_tokens` | 1 up to the smaller of 8192 and the route's own ceiling |
| `tools` | Optional, at most 16 `{"name": ..., "schema_json": ...}` definitions of at most 32 KiB each |

That is all an entrant may write: content, never routing. The schema has no
field for a URL, an endpoint, a header, a credential, a provider name or any
identifier a second entrant could also name, and unknown fields are refused.
Seventeen host-owned names (`url`, `endpoint`, `base_url`, `destination`,
`headers`, `header`, `authorization`, `api_key`, `apikey`, `credential`,
`token`, `provider`, `response_id`, `request_id`, `session_id`,
`conversation_id`, `user`) are refused by name as `forbidden_field`, so the
refusal says what happened.

The answer the entrant in `an_entrant_reaches_the_model_through_its_own_pipe`
receives when the scripted provider replies `0.25` and reports its usage:

```json
{"protocol":"sharpebench.model-gateway.v1","ok":true,"ordinal":0,"text":"0.25","finish_reason":"stop","usage_observed":true}
```

`ordinal` is the call's position in this host's own journal, the only handle
the response carries. There is no provider identifier, so two entrants cannot
correlate themselves through the gateway. `usage_observed: false` means the
provider reported no usage and the cost is unknown, never that it was zero. The
test asserts the line carries neither the credential nor the destination host.

A refusal has `ok: false` and a typed error. This is what the third request of
one decision receives in
`requests_past_the_per_decision_ceiling_are_refused_by_type`, where the ceiling
is set to two:

```json
{"protocol":"sharpebench.model-gateway.v1","ok":false,"usage_observed":false,"error":{"kind":"decision_request_limit","detail":"the decision made too many gateway requests"}}
```

The `detail` is a fixed sentence per kind and never carries run-specific
material. The vocabulary is closed:

| Kind | Meaning |
|---|---|
| `request_too_large`, `malformed_request`, `forbidden_field`, `unknown_protocol` | The line was refused before any route was resolved |
| `unknown_model_alias` | The alias is not published by this host |
| `too_many_messages`, `message_too_large`, `too_many_tools`, `tool_payload_too_large`, `output_tokens_out_of_range` | A request bound was exceeded |
| `budget_exhausted`, `call_limit_exhausted` | The sweep's money or call ceiling refuses the call; nothing is dispatched |
| `concurrency_limit`, `shutting_down` | The host is at its shared call ceiling, or stopping |
| `journal_unwritable`, `journal_ownership_lost` | The money record could not be made durable, or another writer owns it; the gateway stops spending |
| `provider_rate_limited`, `provider_unavailable`, `provider_timeout`, `provider_response_invalid` | The provider call failed after up to two retries |
| `decision_request_limit` | The 33rd request while answering one observation; it never reaches the provider |
| `response_withheld` | The answer carried host material and was withheld; the call is still charged |

**Whose clock.** An entrant has 30 seconds per decision. The time the host
spends serving a gateway request is not the entrant's: the deadline moves out by
exactly that time, so a slow provider does not become an entrant timeout. What
bounds the host's share instead is the per-decision request ceiling, the per-call
deadline handed to the adapter, and the sweep budget.

**Faults.** A timeout, an oversized line, an invalid decision or an early exit is
handled by the external-agent failure taxonomy unchanged: the step holds, a
timed-out entrant is terminated and holds for the rest of the run, and three
consecutive faults open the circuit breaker, after which every step holds. A
request written by an entrant that
has already exited is not served, because nobody is left to read the answer.

## What an operator must supply

**A provider adapter.** The bytes-on-the-wire step is the `ProviderTransport`
trait, and this repository ships no implementation of it. Adding one would put
egress into a library that is otherwise offline and would make its tests
non-hermetic by construction. The operator implements

```rust
pub trait ProviderTransport {
    fn call(&mut self, call: ProviderCall<'_>) -> ProviderOutcome;
}
```

`ProviderCall` hands over the destination, the credential, the provider, model
and revision from the rate card, the messages and tools, the output ceiling, and
three bounds. The adapter reports `Answered { status, body }`,
`RefusedBeforeWork(fault)` when the request provably did no billable work, or
`AmbiguousAfterCommit(fault)` when it was sent and the outcome is unknown. A 2xx
body must be the normalized
`{"text": ..., "finish_reason": ..., "usage": {"input_tokens": ..., "output_tokens": ...}}`
shape, unknown fields refused; putting a provider's own JSON into that shape is
the adapter's job, and anything else settles as unknown cost.

Handing a bound to an adapter is not enforcing it. Three are obligations the
adapter must meet, and the broker's part in each is narrower:

| Bound | What the broker can do |
|---|---|
| `read_timeout`, 60 s per socket read | Nothing: it holds no socket |
| `call_timeout`, 180 s per dispatch | It calls the adapter synchronously and cannot cancel it. It refuses an answer that arrived after the deadline as `provider_timeout` and charges the full reservation. An adapter that blocks forever blocks the broker with it |
| `max_response_body_bytes`, 1 MiB | The adapter allocates the buffer. The broker checks the length of what it is handed, after the fact, and settles an oversized body as unknown cost |

**A route table.** Each alias maps to a destination, a credential, a frozen
[rate card](cli.md#frozen-token-rates) (which carries the provider, model and
revision), an output ceiling and an input-token overhead. The overhead is the
input the provider bills that never appears in the entrant's content: the
adapter's system framing and the wire encoding of tool schemas. It is required
and must be positive, because a zero would silently restore a reservation that
bounds only part of the billed request. The file form is
`sharpebench.gateway-routes.v1`, capped at 256 KiB, with unknown fields refused:

```json
{
  "schema_version": "sharpebench.gateway-routes.v1",
  "routes": [
    {
      "alias": "analyst.v1",
      "destination": "https://provider.example/v1/messages",
      "credential_env": "EXAMPLE_PROVIDER_KEY",
      "max_output_tokens": 4096,
      "input_token_overhead": 8,
      "rate_card": { "schema_version": "sharpebench.token-rate-card.v1", "...": "..." }
    }
  ]
}
```

The credential is named, never written: a manifest that could carry key material
would put it in every diff and every backup. An unset variable is a refusal, not
an empty key that would fail at the wire.

**A budget.** A call ceiling and a money ceiling in integer USD nanodollars,
both required and both nonzero. A paid run with no stated ceiling has no bound
on what it spends before anyone notices, and a ceiling of zero would refuse
every call.

**A binary that runs the sweep.** The CLI does not run gateway sweeps, because
serving needs an adapter and none ships. The operator's binary calls
`run_gateway_sweep(sweep, host, attempt)`, where `attempt` starts one entrant per
(window, seed) cell and calls `gateway_backtest` with its pipes.

**A launch.** For a container, `sharpebench_arena::sandbox::gateway_launch`
returns the same hardened `docker run` argv that `run_external_sandboxed` spawns,
`--network none` included and no environment-forwarding flag, handed back instead
of spawned so the host owns the container's stdio. The gateway adds no network:
model traffic leaves the container the way decisions do. After spawning it, call
`wait_until_running`; after the run, read the resource verdict and remove the
container as the sandboxed agent does. The launch shares the sandboxed path's
refusals: an unpinned image is refused, and so is a host with no daemon, unless
the operator opted into an unsandboxed local run, in which case the plan names
no container and its program is the entrant itself, to be started with a
cleared environment.

`EntrantLaunch::spawn` starts the process and refuses, before anything is
spawned, a launch whose program, arguments or declared environment would carry a
route credential. A host-executed entrant (`EntrantLaunch::host`) gets a cleared
environment with only the variables named. A launcher that isolates the entrant
itself, such as the Docker client (`EntrantLaunch::isolating_launcher`), needs
the host's configuration (`DOCKER_HOST` and friends), so it inherits the host
environment minus every variable whose value carries a credential.

## The operator report

```bash
sharpebench gateway --routes routes.json \
  --budget-usd-nanos <n> --max-calls <n> \
  [--journal journal.json] [--json]
```

This command never calls a provider. It resolves the route manifest, binds
credentials from the environment by name, and prints the route-table identity,
the aliases, which variable backs each alias (its value is always
`<redacted>`), the budget, the `limits` the gateway enforces (the
`GatewayLimits` defaults: the request, message, tool, output, response, retry,
concurrency and timeout bounds, and `max_requests_per_decision`), and, given a
journal, what it has already committed: calls started, priced, unknown,
outstanding and available money, any overspend and whether the ceiling was
breached. A journal bound to a sweep also reports its `sweep_sha256`. It exits 2
when the configuration would not support a run.

## Bounds

Every bound is checked before the allocation it governs. Defaults come from
`GatewayLimits`.

| What is bounded | To what |
|---|---|
| Request line, before parsing | 256 KiB |
| Messages in one request | 1 to 64 |
| Content of one message | 32 KiB |
| Tool definitions in one request | 16 |
| One tool payload (name plus schema) | 32 KiB |
| Output tokens asked of a provider | 8192, and the route's own ceiling, whichever is smaller |
| Input tokens reserved for framing the entrant never wrote | operator-set per route, required |
| Provider response body | 1 MiB, an adapter obligation; the broker checks the length afterwards |
| Model text handed back | 128 KiB |
| Response line emitted | 256 KiB; an over-long line becomes a refusal |
| Dispatches per entrant request | 1 plus 2 retries, for rate-limit, unavailable, timeout and invalid-response failures only |
| Concurrent provider calls, across every entrant | 4 |
| One provider socket read | 60 s, an adapter obligation the broker cannot check |
| One dispatch, connect and all reads | 180 s, an adapter obligation; the broker refuses a late answer rather than cancelling the call |
| Calls in the sweep, retries included | operator-set, required |
| Money the sweep may commit | operator-set, required |
| Gateway requests while answering one observation | 32; the next is a `decision_request_limit` refusal and never reaches the provider |
| One entrant stdout line, request or decision | 8 MiB, the external-agent decision-line cap; a request is then held to the 256 KiB bound before it is parsed |
| Entrant stdout per process, requests included | 64 MiB |
| Entrant wall clock per decision | 30 s, net of the time the host spends serving its requests |
| Journal read from disk | 16 MiB |
| Route manifest read from disk | 256 KiB |
| Rate card read from disk | 64 KiB |

The concurrency ceiling is held by the host and shared across every entrant's
gateway, so it bounds the host's fan-out rather than any one entrant's
politeness. Its permit is released on drop, including on an unwind, so a
panicking adapter cannot leak a slot. The per-decision request ceiling is a
circuit breaker against a runaway loop, not a budget: it resets with every
observation, and the sweep-scoped budget is the journal's.

## Reservations and settlement

The money record is an append-only journal. Every derived number (spend,
outstanding reservations, calls started) is a fold over its records, so nothing
stored can drift from the append-only truth and there is no in-place field a
recovery could rewrite.

One dispatch is one reservation and one settlement:

1. **Refuse first.** Shutdown, a breached ceiling, the call ceiling and the
   money ceiling are checked before anything is appended and before the permit
   is taken. A hard refusal appends nothing and dispatches nothing.
2. **Reserve the worst case.** Content bytes bound the input tokens the content
   becomes (no tokenizer emits more tokens than the text has bytes), plus the
   route's input-token overhead, plus the requested output tokens, quoted
   against the frozen rate card in integer USD nanodollars.
3. **Persist, then dispatch.** The reservation is fsynced and renamed into place
   before the call starts, so a process that dies during the call leaves a
   reservation behind.
4. **Settle exactly once.**

| Outcome | Settlement | Money | Counts as a call |
|---|---|---|---|
| 2xx with usage | `Priced` | actual quote | yes |
| 2xx, empty completion, with usage | `Priced` | actual quote | yes |
| 2xx with no usage reported | `Unknown` | full reservation | yes |
| 2xx, body unparseable, truncated or oversized | `Unknown` | full reservation | yes |
| 429 or other 4xx | `Released` | nothing | yes |
| 5xx | `Unknown` | full reservation | yes |
| timeout or drop after the request was committed | `Unknown` | full reservation | yes |
| adapter answered after the call deadline | `Unknown` | full reservation | yes |
| connection refused before the request left | `Released` | nothing | yes |
| cancelled after reserving, before dispatch | `Released` | nothing | yes |
| process died mid-call | none; folds as outstanding | full reservation | yes |

Two consequences are deliberate. **Unknown cost is charged at the full
reservation**, so an unknown completion is neither a free retry nor an automatic
refund: the retry pays for itself, and a provider that times out repeatedly
exhausts the budget instead of looping for free. And a released reservation
still consumes one of the sweep's calls, because the call ceiling bounds
attempts, not successes.

A reservation bounds what the host **authorizes**, not what a provider may
**bill**. An observed price above the reservation is recorded as observed, never
clipped, folded out as `overspent_usd_nanos` and `overspent_calls`, and once
committed money passes the ceiling every later request is `budget_exhausted`
before anything is reserved. The call that overspent still returns its answer:
the work happened, and suppressing it would not unspend the money.

A total that contains any unmeasured amount is not published as a total at all.
It becomes `unavailable`, with a separately named `known_subtotal_usd_nanos`.

**One writer per journal.** A gateway that spends a journal owns it exclusively.
Opening takes a lock file, `<journal>.lock`, created with `create_new` so the
file system picks the winner, and holds it until the gateway drops. It also
takes a second lock named for the journal document itself,
`sb-gateway-journal-<id>.lock`, where the id is carried inside the document and
survives every save, so one journal reached under two names in one directory is
one lock and not two. A journal that does not exist yet names no document, so
opening writes one and binds to it there and then. A document written before it
carried an identity is owned on one derived from its own bytes, which every name
for it derives alike, so two gateways opening one of those concurrently contend
for a single lock instead of assigning themselves an identity each. A second gateway on the same
host is refused when it opens, by type, naming the lock it could not take. A
lock left behind by a crashed process is refused too, not broken: nothing on
disk tells a dead holder from a live one, and breaking it on a guess is how two
writers end up on one budget again. Clearing it is `JournalLock::take_over`,
which an operator performs deliberately and which records the displaced holder
and the stated reason inside the new lock. Each lock names the holder that wrote
it, and a holder releases only a file that still names it, so a takeover of a
process that turns out to be alive does not end with that process unlocking the
journal under its successor. Reading takes no lock, so `sharpebench gateway` can
inspect a sweep that is running; the report says whether the path is owned, in
`journal_lock_held`.

Underneath that, the journal carries a version and a save is a compare-and-swap
on it: a gateway whose snapshot the file has moved past is refused, answers
`journal_ownership_lost` and starts no further call, instead of erasing a record
it never read. That check is now the second line of defence, and what it defends
against is named: a journal that moved under a single writer, such as one
restored from a backup mid-sweep, and the second writer a takeover deliberately
creates. A takeover displaces a holder that may still be alive, and a save
consults no lock, so the version is what refuses that holder's next write.

**A settlement that cannot be written stops the gateway.** The file then holds a
reservation whose outcome is missing, and a reservation is not what the call
cost: this gateway records an observed price above its reservation, so a restart
that folded the reservation instead would under-report real spend. Both a
refused write and an I/O failure therefore latch, the answer is refused rather
than handed back over a record that no longer says what it cost, every later
request is refused as `journal_unwritable` or `journal_ownership_lost`, and the
sweep's `HostObservedUsage` carries the flag beside the figures.

The two flags mean what they say. A save whose rename landed and whose
durability could not be confirmed is the owner's own I/O fault: it keeps the
version it wrote, so its next save is not mistaken for another writer's, and it
publishes `journal_unwritable`. Only a version that moved under this gateway
publishes `journal_ownership_lost`.

## Identity and resume

`RouteTable::identity_digest` covers, for every alias, the provider, model,
revision, rate-card digest, destination digest, output ceiling and input-token
overhead. Credential values are absent from it: rotating a token is not an
experiment change and must not invalidate a resumable sweep, while changing a
model, a revision, a price, a destination or what the host authorizes per call
must.

`run_gateway_sweep` folds that digest and the budget into the checkpoint's
`invocation_sha256` (`sharpebench.gateway-invocation.v1`), so a changed route
table or ceiling cannot resume the checkpoint. The money journal is bound to the
checkpoint contract through `sweep_sha256` (`sharpebench.gateway-sweep.v1`, over
the entrant id and the contract), and the two are accepted only as a pair:

- a checkpoint whose journal is missing is refused, because resuming would
  report the calls its finished cells made as never made;
- a journal with calls whose checkpoint is missing is refused, because a new
  checkpoint would rerun cells the journal already charged;
- a journal bound to another sweep, route table or budget is refused rather
  than truncated, merged or continued.

A fresh pair starts by writing an empty journal, so a checkpoint never exists
before its journal. Completed cells are skipped on resume and make no new
calls; their spend is folded from the journal, never re-quoted.

## What leaves the host

The sweep returns the `ResilientSubmission` (the scored pool, failures and
attempt accounting) exactly as any external sweep does, and beside it a
`HostObservedUsage` record, `sharpebench.host-observed-usage.v1`: the route-table
and sweep digests, the host-observed monetary summary, the priced, unknown and
released call counts, any overspend, whether the ceiling was breached, whether
journal ownership was lost, whether a settlement could not be written
(`journal_unwritable`), and `rank_neutral: true`.
`attach_host_observed_usage` puts it on the entrant's row of a JSON board under
`host_observed_usage`. The board stays an array, no score field moves, and
removing that key gives back the unattached board exactly.

Before any answer is written to the entrant, the serving loop checks it for each
route credential, each route destination and the journal's location, raw and
JSON-escaped (needles of four bytes or more). A hit replaces the answer with a
`response_withheld` refusal. The broker never writes this material itself; the
gate catches a provider or proxy that echoes it into the model text. It is a
gate, not a warning: the bytes do not cross, and the call is still charged,
because it happened.

## Field readiness

Neither field producer can be run from this repository: there are no provider
credentials here and the audit goal forbids paid calls. Readiness therefore
means the runner refuses correctly and reports what it would do.

`plan_field` is pure in a credential lookup, so every refusal path is testable
with no process environment and no key. It refuses a missing or blank
credential, a missing call ceiling, a ceiling that is not a positive count, and
a model outside the declared field. `LLM_MAX_CALLS` is required rather than
optional. `LLM_MODEL` selects one declared model, and `SHARPEBENCH_DRY_RUN`
prints the plan (models, datasets, ceiling, effective controls, output path)
then stops before the first spawn and before any output file is opened.

`LLM_MAX_CALLS` bounds provider requests per model, not cached results and not
dispatches from one process. The Python adapter reserves one unit in a per-model
ledger beside the response cache, fsynced before the request goes out, and
builds its client with the SDK's automatic retries disabled, so one reserved
unit is exactly one HTTP request. The setting is verified on the constructed
client before the run starts, so the bound is conditional on that check rather
than on an SDK version: a client that reports a non-zero retry setting, or none
that can be read, refuses the run. A transient provider failure spends its unit
and fails the subprocess rather than being retried under the same unit; the
harness respawn draws the next unit from the same ledger, which is what keeps
the ceiling whole across the subprocesses the run spawns.

`plan_local` is likewise pure. It refuses an absent or empty model list, a
repeated tag, two tags that would collide downstream, an unparseable or
non-positive control, and an unparseable thinking flag. Its dry run happens
before the shim probe, so a readiness report starts no interpreter and loads no
model.

## Live Docker run

`sandbox::tests::live_gateway_launch_serves_model_calls_over_stdio_with_no_network`
runs by exact name in the live-container CI job (Docker 28.0.4). A real
`run_gateway_sweep` with its journal on disk runs one cell of three decisions;
the cell starts the digest-pinned Alpine fixture from the argv `gateway_launch`
returns, spawned through `EntrantLaunch::isolating_launcher` and awaited with
`wait_until_running`. The fixture's own entrypoint is `/bin/sh`, which would read
the observations as a script, so the test appends an explicit container command
after the image positional; everything before it is `gateway_launch`'s argv
unchanged. Inside the container the entrant listed its interfaces and tried one
outbound connect to `1.1.1.1:80` before its first observation, and put both in
every model request. The provider behind the transport seam cannot open a socket
and answers each call with a fixed text. Observed:

- all three model requests arrived at the provider carrying
  `ifaces=lo, egress_exit=1`: only loopback existed and the connect failed;
- every decision was a valid hold, which the entrant writes only after the
  gateway's answer arrived on its stdin, so the run completed with no failure;
- the journal on disk reserved and settled all three calls, all priced;
- the container exited 0, classified `WithinBudget`, was removed, and
  `docker inspect` afterwards found nothing.

The image preflight's functional probe also runs an image through
`plan_gateway_launch` with no appended command, from the image's own
entrypoint, against the same daemon ([image preflight](image-preflight.md)).

## What is not yet verified

- **One daemon, one fixture.** The live run above is one Docker version on one
  CI runner with a shell-script entrant, not an entrant image that ships a
  gateway client of its own.
- **No real provider.** Every test drives a scripted adapter that cannot open a
  socket. Nothing here shows that a particular provider's usage report, framing
  overhead or billing matches what the host reserved and recorded; that is what
  "host-observed, not verified billing" means.
- **Adapter obligations.** The read timeout, the call timeout and the
  response-body bound are the adapter's to honour. The broker can refuse a late
  or oversized answer after the fact; it cannot stop an adapter that blocks or
  over-allocates.
- **Journal ownership across hosts.** The lock is one host's file system. Two
  hosts reaching the same journal path over a network file system are not
  separated by it: `create_new` is only as exclusive as the remote server makes
  it, and NFS does not guarantee that. One host per journal path is a deployment
  rule, not something this code enforces.
- **Aliases in different directories.** Both locks sit beside the journal, so a
  document reached through two directory entries in two different directories
  derives two locks and both gateways open. Aliases that share a directory are
  refused, a document written before it carried an identity included. This limit
  is deliberate: closing it needs a lock either on the journal file, which
  breaks the rename the journal is persisted through, or in a shared namespace
  that is swept by age or scoped to one user, which would weaken ownership for
  every journal to close the case of a hard link into a second directory.
- **The version check is not a concurrency control.** A save reads the version
  on disk and renames after a create, a write and an `fsync`. Two writers that
  both read the same version inside that window both proceed. It is a second
  line of defence behind the lock, and it is not a substitute for one. It is not
  redundant behind the lock either: the writer a takeover adds is refused by it
  alone.
- **A crashed holder needs an operator.** The stale lock is refused rather than
  broken, so a host that died mid-sweep does not resume unattended. That is the
  deliberate trade: an unattended resume is exactly the automatic break that
  would put two writers back on one budget.
- **No CLI sweep.** No `sharpebench` subcommand runs a gateway sweep; an
  operator's own binary has to.

## Test evidence

Hermetic fakes only: no API key, no network call, no model installation. The one
live leg is the Docker run above, whose provider is also a fake.

| Where | Tests | Covers |
|---|---|---|
| `crates/sharpebench-harness/src/gateway.rs` | 30 | forbidden fields by name, exact alias resolution, the host supplying destination, credential, revision and every bound, the envelope bounds, oversized, truncated, unparseable and unknown-field provider bodies, the shared permit pool, the reservation reaching disk before dispatch, hard money and call refusals starting no request, retries reconciled exactly once, absent usage as unknown rather than zero, framing overhead in the reservation, overspend stopping the sweep, a second gateway refused the journal the first owns, a late answer refused and charged, resume without repricing, credential rotation excluded from identity, redaction and rank neutrality |
| `crates/sharpebench-harness/src/gateway_journal.rs` | 10 | the fold, partial and unavailable totals, the `host_observed` label, and a sweep-bound journal resuming only under its sweep |
| `crates/sharpebench-harness/src/gateway_serve_tests.rs` | 12 | the serving loop on real OS pipes and one real child process: a call through the entrant's own pipe, a scored sweep with usage on the entrant's row, resume making no new calls, an interrupted sweep rerunning only unfinished cells, typed budget refusals, the checkpoint and journal pair, gateway lines never read as decisions, the per-decision ceiling, host serving time excluded from the entrant clock, no credential in any launch, and host material withheld |
| `crates/sharpebench-cli/src/gateway_cli.rs` | 6 | the operator report, including `limits.max_requests_per_decision` |
| `crates/sharpebench-arena/src/sandbox.rs` | 2 | the gateway launch is the hardened `--network none` launch; live, a gateway sweep served over a real container's stdio with no network, journaled, and the container removed |

Each invariant of the serving loop (host serving time, the checkpoint binding,
the missing-journal refusal, the per-decision ceiling, the credential refusal at
launch and the host-material gate) and of the broker's money path was
mutation-checked: each mutant was killed. The mutations and their results are
recorded in the
[host accounting audit](https://github.com/general-liquidity/sharpebench/blob/main/docs/audits/2026-09-09/HOST-ACCOUNTING.md).
