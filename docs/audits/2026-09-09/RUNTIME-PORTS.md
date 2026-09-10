# Runtime ports that depended on the gateway: rows 39, 40, 14, 34 and 38

[PORT-RECONCILIATION.md](PORT-RECONCILIATION.md) decided rows 39 and 40 as
"adapt" and rows 14, 34 and 38 as "defer". Row 14 was deferred as a
prerequisite of the model gateway, and the gateway had no runtime until the
serving loop recorded in
[HOST-ACCOUNTING.md](HOST-ACCOUNTING.md#the-entrant-serving-loop-2026-09-10)
landed. This file records, row by row, what was built against the code as it
now stands, or the current evidence for leaving a row unbuilt. The
reconciliation itself is not edited.

No row here changes a published number. The scoring kernel, the ranking path,
the goldens, the examples and `paper/evidence/` are untouched; see
[Byte identity](#byte-identity).

## Row 39: source-strip allowlist for the runtime image (adapt, built)

**What the row decided.** Take allowlist polarity and the post-strip functional
test into G07. Do not present stripping as evidence of absence, and do not treat
a version-pinned Dockerfile as a reproducible image.

**What Bench has to apply it to.** Bench builds no evaluator image to strip: the
entrant image is entrant-supplied and digest-pinned. So "what the runtime image
may contain" is applied to the image the preflight already captures, as a
second polarity beside the scan policy. The scan policy refuses known content;
the allowlist refuses anything it was not told to expect.

**Built.** `crates/sharpebench-cli/src/artifact_preflight.rs`, opt in with
`--runtime-allowlist <allowlist.json>` next to `--scan-policy` and `--image`.

- Schema `sharpebench.runtime-allowlist.v1`: `{"schema_version": ..., "paths": [...]}`,
  at most 64 KiB, 1 to 4096 paths of at most 1024 bytes, unknown fields
  refused. A path is relative printable ASCII with no empty, `.` or `..`
  segment and is declared once. A path ending in `/` admits that directory and
  everything below it; any other path admits exactly that entry. A directory
  that is an ancestor of an admitted path is admitted itself, because an
  archive lists the directories it descends through, and admits nothing else.
- The allowlist is applied to the container export only after the harness scan
  enumerated the whole archive and found it clean, so the listing walks an
  archive whose structure and metadata sizes the scan already validated. Every
  entry, directories and links included, must be admitted. An unreadable
  listing, an unreadable name or an expired policy deadline leaves the report
  incomplete, and an incomplete report never admits.
- Refused entries are reported by count and archive-order index (at most 16
  indices). Entry names are withheld: the report can be published, and a name
  can be exactly what a policy protects.
- **The post-strip functional test.** An image that passes every scan leg and
  the allowlist, with its snapshot container removal verified, is run once from
  its configuration ID under the hardened launch a gateway sweep uses
  (`sharpebench_arena::sandbox::plan_gateway_launch`, `--network none`
  included), against one fixed synthetic observation. Its first line must be a
  decision valid for that observation, and removing its container by name must
  be verified. Otherwise the launch is refused, with the reason
  (`probe_did_not_complete`, `no_decision`, `invalid_decision`,
  `probe_output_exceeded`, `probe_output_unreadable`, `launch_refused`) or an
  unverified cleanup on the report. The probe runs only after every scan leg
  authorized the image, so a refused image is still never started.
- Authorization: with an allowlist, `authorizes_launch` additionally requires
  the allowlist to admit every entry and the probe to pass with its cleanup
  verified. `policy_sha256` becomes a digest over the scan policy digest and
  the allowlist digest (`sharpebench.image-preflight-policy.v2`), so a changed
  allowlist is a changed experiment for the scanned checkpoint identity, and
  `scan_policy_sha256` reports the scan policy's own digest. Without an
  allowlist, `policy_sha256` is the scan policy digest exactly as before and the
  three new report fields are absent.

**What this does not prove.** An allowlist result says which paths the export
holds. It says nothing about the bytes under an admitted path, which remain the
scan policy's business, and it does not make the image reproducible. A passing
probe says the admitted image answered one synthetic observation validly; it is
not a behavioural test of the entrant. The export of a created container also
holds entries the daemon itself adds, and an allowlist must name those too;
which ones a given daemon adds was not measured against a live daemon in this
work, so the first live use should expect to read the refused indices once.

**Tests.** Eight new unit tests in `artifact_preflight::tests` against the
injected Docker transport:
`an_allowlisted_image_is_probed_before_it_is_authorized`,
`an_entry_outside_the_allowlist_refuses_before_anything_runs`,
`an_admitted_image_that_fails_its_functional_probe_refuses`,
`the_policy_digest_binds_the_allowlist_only_when_one_applies`,
`the_allowlist_admits_subtrees_exact_entries_and_their_ancestors_only`,
`a_malformed_allowlist_is_refused`,
`host_named_artifacts_carry_no_evaluation_identity` (row 40) and
`an_allowlist_without_a_scan_policy_issues_no_docker_command`. The live leg
`live_docker_image_preflight` is unchanged and does not exercise the allowlist
or the probe; neither has run against a real daemon.

**Mutations** (in place on a clean committed tree, restored with
`git show HEAD:<path>`, verified with `cmp`):

| Invariant | Mutation | Result |
|---|---|---|
| An entry outside the allowlist refuses | the outside-entry branch in `check_allowlist` is made unreachable | killed: `an_entry_outside_the_allowlist_refuses_before_anything_runs` failed |
| A failed functional probe refuses | `authorizes_launch` accepts any probe report | killed: `an_admitted_image_that_fails_its_functional_probe_refuses` failed |

## Row 40: public kit separation and generic artifact names (adapt, built)

**What the row decided.** Apply generic-artifact naming and a delivered-artifact
leak assertion to any Bench example or fixture that ships alongside held-out
data, as a gate and not a warning.

**Where that set is today.** Literally read, it is empty. No example or fixture
in this repository ships alongside held-out data: the live forward window
records `"dataset_hash": null` and `"sealed_eval_salt_sha256": null`
(`arena/windows/window-003/window.json:37,40`), because forward data does not
exist before its reveal, and
`grep -rn -i "held-out\|held out\|heldout" examples/ suites/` returns nothing.
A gate over `examples/` would have no protected vocabulary to check against,
which is a gate in name only.

**Where Bench now does deliver bytes next to host-only material.** Two places,
both created or extended by this work, and the gate is applied to both:

- **Gateway answers.** The serving loop writes model answers onto the entrant's
  pipe, and the host that writes them holds route credentials, destinations and
  the journal location. Before any answer is written, `GatewayEntrant` checks
  it for each of those, raw and JSON-escaped. A hit replaces the answer with a
  typed `response_withheld` refusal; the call is still charged, because it
  happened. The broker never writes this material itself; the gate catches a
  provider or proxy that echoes it into the model text. This is a gate: the
  bytes do not cross.
- **The image preflight.** Everything the preflight names for Docker or hands
  the image is generic. After the one inspect that resolves the operator's
  reference, the image is reached only by its configuration ID; the snapshot
  and probe containers are named `sharpebench-preflight-<pid>-<n>` and
  `sharpebench-agent-<pid>-<n>`; no argument carries the policy or allowlist
  digest; and the probe observation names a synthetic instrument `PROBE` on
  `1970-01-01`, so it tells the image nothing about the evaluation it is
  entering. Allowlist refusals are reported by index, never by entry name.

**Tests.** `gateway::serve::tests::host_material_in_an_answer_never_reaches_the_entrant`
(credential, destination and a Windows-style journal path, each echoed by the
provider) and
`artifact_preflight::tests::host_named_artifacts_carry_no_evaluation_identity`,
with name withholding asserted in
`an_entry_outside_the_allowlist_refuses_before_anything_runs`.

**Mutation.** The `carries_host_material` gate in `decide_once` made
unreachable: killed, `host_material_in_an_answer_never_reaches_the_entrant`
failed.

## Row 14: two-network gateway topology (defer; reassessed, not needed)

**What the row decided.** Defer as a G11 prerequisite: do not build it until the
gateway is specified with its bounds and ownership rules, and not at all if
entrants bring their own credentials. The row's stated benefit was the standard
way to offer host-mediated provider access *through an HTTP sidecar* without
giving the entrant a route out.

**Reassessment against the gateway as built.** The gateway that exists is not
an HTTP sidecar, so the topology the row describes has nothing to connect:

- Model traffic rides the entrant's stdio, the pipe the host already owns
  (`gateway_serve.rs` module documentation, "Wire"). The entrant holds no URL,
  opens no socket and reaches no listener; the host makes the call through the
  operator's `ProviderTransport`.
- The container a gateway serves is launched with the same argv as any other
  sandboxed entrant, `--network none` included:
  `sandbox::tests::a_gateway_launch_is_the_hardened_network_disabled_launch`
  asserts the argv equals the `run_external_sandboxed` launch and contains the
  `--network none` pair and no environment-forwarding flag.
- No network is created anywhere:
  `grep -rn "network create\|--internal" crates/ --include=*.rs` returns
  nothing.
- End to end, a real child process reaches the model with no network at all:
  `a_spawned_entrant_process_reaches_the_model_through_its_stdio`.

Building the topology now would replace "no egress exists" with "egress exists
behind a proxy", which the row itself names as materially weaker, and would add
two containers and a network to teardown, for a transport the design does not
use. **Disposition: not built. The pipe-based loop makes it unnecessary.** It
becomes relevant again only if a future gateway needs an HTTP sidecar, which
HOST-ACCOUNTING.md rejects on the merits.

## Row 34: capability offer, enable, freeze, seal (defer; reassessed, still unneeded)

**What the row decided.** Defer: Bench has no capability plane and no need for
one. Revisit only if venue-feature gating is actually required. The row's one
threat: a capability plane adds a second axis along which two runs can differ,
which must enter run identity.

**Reassessment.** Parts 1 and 2 did add an axis along which runs differ, which
is exactly what the row asked to recheck:

- Model access is now such an axis (which aliases exist, at what price and
  ceiling). It enters run identity without a capability plane: the route-table
  identity digest and the budget are folded into the checkpoint's
  `invocation_sha256`, and the money journal is bound to the checkpoint
  contract (`run_gateway_sweep`; mutation-checked in HOST-ACCOUNTING.md).
- The runtime allowlist is another, and it enters identity through
  `policy_sha256` (row 39 above).
- Neither needs an offer/enable/freeze/seal lifecycle. A `RouteTable` has no
  mutation path after `RouteTable::new` (`gateway.rs:282`), so it is frozen and
  sealed by construction; a `RuntimeAllowlist` likewise. There is no later
  enable step for a lifecycle to guard.
- Venue-feature gating is still not required:
  `grep -rn -i -E "capabilit(y|ies)|margin_enabled|options_level|feature_gate|enable_offered" crates/ --include=*.rs`
  finds only Linux-capability hardening in the sandbox and a model-metadata
  field in an example, no venue feature.

**Disposition: still deferred.** The row's requirement, that a differing axis
enter run identity, is met directly by the digests above; the lifecycle it
would add has no mutable state to govern.

## Row 38: frozen per-run action catalog (defer; reassessed, still unneeded)

**What the row decided.** Defer: relevant only if Bench admits per-run variable
action surfaces, in which case freeze-at-construction with duplicate rejection
is the right shape.

**Reassessment.** The decision surface is still fixed at compile time by the
protocol crate (`Action` is a closed enum, `crates/sharpebench-protocol/src/lib.rs:154`,
mirrored by `crates/sharpebench-protocol/schema/decision.schema.json`), and the
serving loop parses decisions against that closed contract unchanged. Part 1
introduced one per-run variable surface: the set of model aliases an entrant may
name. It already has the shape the row prescribes: `RouteTable::new` refuses a
duplicate alias, there is no registration path after construction, resolution
is by exact match, and `RouteTable::identity_digest` binds the set into the
sweep identity. The tool definitions a gateway request may carry are entrant
content passed to the provider, not host actions, and are bounded as content.
`grep -rn -i -E "register_action|action_catalog|ActionCatalog" crates/ --include=*.rs`
returns nothing.

**Disposition: still deferred.** The one per-run surface Parts 1 and 2 created
is already frozen at construction with duplicate rejection; a separate catalog
would duplicate it.

## Byte identity

With `--runtime-allowlist` absent and no gateway sweep run, every existing
output is unchanged:

- `crates/sharpebench-core`, `crates/sharpebench-sim`,
  `crates/sharpebench-stats`, `crates/sharpebench-protocol`, `examples/`,
  `suites/`, `data/`, `arena/`, `paper/` and `crates/sharpebench-cli/src/main.rs`
  are not modified by this work (`git diff --stat 7fe5a06 HEAD` over those
  paths is empty before the provenance rebind). The goldens pass unchanged:
  `golden_scores` (five tests), `golden_input`,
  `synthetic_is_byte_identical_golden` and the WASM parity golden.
- A preflight without an allowlist reports the same `policy_sha256` and none of
  the three new fields (`the_policy_digest_binds_the_allowlist_only_when_one_applies`).
- A gateway journal no sweep owns serializes without the sweep field
  (`a_sweep_bound_journal_resumes_only_under_its_sweep`).
- One output of the gateway feature itself changes: the `sharpebench gateway`
  report's `limits` object gains `max_requests_per_decision`, because that is a
  bound the gateway now enforces and the report exists to list the enforced
  bounds. Its `spend` object gains `sweep_sha256` only for a sweep-bound
  journal.
