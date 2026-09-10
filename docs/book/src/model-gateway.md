# Host-observed model gateway

[Frozen token rate cards](cli.md#frozen-token-rates) price what an entrant said
it used. The gateway is the other half: usage the host itself observed, under
host-owned credentials, routing and budgets.

Both are published beside the scored pool and neither is a rank input. They are
told apart by a `usage_source` label, `host_observed` against
`entrant_reported`.

> Host-observed usage is what the host saw a provider report on the wire, priced
> once against a frozen rate card. It is **not a verified invoice**, not a
> billing reconciliation, and not an input to any score, rank or pass^k pool.

## The operator surface

```bash
sharpebench gateway --routes routes.json \
  --budget-usd-nanos <n> --max-calls <n> \
  [--journal journal.json] [--json]
```

This surface never calls a provider. It resolves the host's route manifest,
binds credentials from the environment by name, and prints the frozen identity a
sweep would run under, the bounds it would enforce, and what the money journal
has already committed. It exits nonzero when the configuration would not support
a run.

Both ceilings are required. A paid run started without a stated call ceiling and
a stated money ceiling has no bound on what it spends before anyone notices, and
a ceiling of zero would refuse every call, so both are refused as
configurations.

The route manifest is `sharpebench.gateway-routes.v1`, capped at 256 KiB, with
unknown fields refused:

```json
{
  "schema_version": "sharpebench.gateway-routes.v1",
  "routes": [
    {
      "alias": "analyst.v1",
      "destination": "https://provider.example/v1/messages",
      "credential_env": "EXAMPLE_PROVIDER_KEY",
      "max_output_tokens": 4096,
      "rate_card": { "schema_version": "sharpebench.token-rate-card.v1", "...": "..." }
    }
  ]
}
```

The credential is named, never written here: a manifest that could carry key
material would put it in every diff and every backup. An unset variable is a
refusal, not an empty key that would fail at the wire. The report says which
variable backs each alias and that its value is redacted; it never prints the
value.

## Protocol

The gateway is a host-side broker over newline-delimited JSON: one request line
in, one response line out, on a pipe the host already owns. It is the same
channel shape the external-agent protocol uses, so an entrant container with no
route to the internet, no listener, no port and no TLS stack can still be
evaluated against a hosted model.

An entrant may write **content**: an alias, messages, a bounded output length
and optional tool definitions. It may not write **routing**. The request schema
has no field for a URL, an endpoint, a header, a credential, a provider name or
any identifier a second entrant could also name. Unknown fields are refused, and
a set of host-owned names is refused explicitly by name so the refusal says what
happened.

Destination, credential, provider, model, revision, allowed output length and
the frozen rate card are all resolved from the host's route table by exact alias
match. There is no prefix pass-through and no fallback to an unversioned alias.
The response carries no provider identifier; its only handle is an ordinal the
host assigns inside its own journal.

### No networked transport ships here

The bytes-on-the-wire step is a `ProviderTransport` seam, and this crate ships
no implementation of it. Adding one would put egress into a library that is
otherwise offline and would make its tests non-hermetic by construction. The
operator supplies the transport, and the host hands it the bounds rather than
leaving them to its discretion. Every test drives a scripted fake that cannot
open a socket: no API key, no network call, no model installation.

## Bounds

Every bound is checked before the allocation it governs.

| What is bounded | To what |
|---|---|
| Request line, before parsing | 256 KiB |
| Messages in one request | 64 |
| Content of one message | 32 KiB |
| Tool definitions in one request | 16 |
| One tool payload (name plus schema) | 32 KiB |
| Output tokens asked of a provider | 8192, and the route's own ceiling, whichever is smaller |
| Provider response body | 1 MiB |
| Model text handed back | 128 KiB |
| Response line emitted | 256 KiB; an over-long line becomes a refusal |
| Dispatches per entrant request | 1 plus 2 retries |
| Concurrent provider calls, across every entrant | 4 |
| One provider socket read | 60 s |
| One dispatch, connect and all reads | 180 s |
| Calls in the sweep, retries included | operator-set, required |
| Money the sweep may commit | operator-set, required |
| Journal read from disk | 16 MiB |
| Route manifest read from disk | 256 KiB |
| Rate card read from disk | 64 KiB |

The concurrency ceiling is held by the host and shared across every entrant's
gateway, so it bounds the host's fan-out rather than any one entrant's
politeness. Its permit is released on drop, including on an unwind, so a
panicking transport cannot leak a slot.

## Reservations and settlement

The money record is an append-only journal. Every derived number (spend,
outstanding reservations, calls started) is a fold over its records, so nothing
stored can drift from the append-only truth and there is no in-place field a
recovery could rewrite.

One dispatch is one reservation and one settlement:

1. **Refuse first.** Shutdown, the call ceiling and the money ceiling are
   checked before anything is appended and before the permit is taken. A hard
   refusal appends nothing and dispatches nothing.
2. **Reserve the worst case.** Content bytes bound the input tokens (no
   tokenizer emits more tokens than the text has bytes) plus the requested
   output tokens, quoted against the frozen rate card in integer USD
   nanodollars.
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
| connection refused before the request left | `Released` | nothing | yes |
| cancelled after reserving, before dispatch | `Released` | nothing | yes |
| process died mid-call | none; folds as outstanding | full reservation | yes |

Two consequences are deliberate. **Unknown cost is charged at the full
reservation**, so an unknown completion is neither a free retry nor an automatic
refund: the retry pays for itself, and a provider that times out repeatedly
exhausts the budget instead of looping for free. And a released reservation
still consumes one of the sweep's calls, because the call ceiling bounds
attempts, not successes.

A total that contains any unmeasured amount is not published as a total at all.
It becomes `unavailable`, with a separately named `known_subtotal_usd_nanos`.

Resume folds the amounts that were written and never re-quotes. A journal is
bound to the route table digest and the budget, and a journal whose binding
differs is refused rather than truncated, merged or continued.

## Identity

`RouteTable::identity_digest` covers, for every alias, the provider, model,
revision, rate-card digest, destination digest and output ceiling. Credential
values are absent from it: rotating a token is not an experiment change and must
not invalidate a resumable sweep, while changing a model, a revision, a price or
a destination must. The digest folds into the sweep's `invocation_sha256`
alongside the rate-card binding, and the journal binds it directly.

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

`plan_local` is likewise pure. It refuses an absent or empty model list, a
repeated tag, two tags that would collide downstream, an unparseable or
non-positive control, and an unparseable thinking flag. Its dry run happens
before the shim probe, so a readiness report starts no interpreter and loads no
model.

## Test evidence

Hermetic fakes only. Twenty-six gateway tests cover forbidden fields by name,
exact alias resolution, the host supplying destination, credential, revision and
every bound, the envelope bounds, oversized and truncated and unparseable
provider bodies, the shared permit pool, the reservation reaching disk before
dispatch, hard money and call refusals starting no request, retries reconciled
exactly once, absent usage as unknown rather than zero, resume without repricing
or reissuing, credential rotation deliberately excluded from identity, redaction
of credential and provider material, rank neutrality, and route-table
validation. Nine journal tests cover the fold, and five CLI tests cover the
operator surface.

Three invariants were mutation-checked in an isolated copy outside the
worktree: that all retries are counted, that no cost is repriced on resume, and
that no request starts after a hard budget refusal. Each mutation was killed.
