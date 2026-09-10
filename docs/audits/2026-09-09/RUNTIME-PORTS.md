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
holds entries the daemon itself adds. This paragraph first said those were not
measured; they now are, and the measurement showed a real defect (no allowlist
of an image's own paths could admit a real export), fixed by admitting the
daemon's init-layer entries in their exact shape. See
[Live verification](#live-verification-2026-09-10).

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
or the probe; two later live legs do (see
[Live verification](#live-verification-2026-09-10)).

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

## Live verification (2026-09-10)

The gateway launch and the runtime allowlist with its functional probe had only
injected-transport or unit coverage. Three ignored live tests now run them
against a real daemon, by exact name, in the CI job "live container boundary
(hostile probe)". The gateway test is skipped by name in the job's wholesale
arena step so it runs once.

**Environment.** GitHub-hosted `ubuntu-24.04` runner (image 20260907.300.1);
Docker Engine 28.0.4 (API 1.48), containerd v2.3.4, runc 1.5.1, storage driver
overlay2 on extfs, cgroup v2 with the systemd driver. Fixture
`alpine@sha256:14358309a308569c32bdc37e2e0e9694be33a9d99e68afb0f5ff33cc1f695dce`
(configuration ID `sha256:b66e0ce6...`), digest-pinned by the job. Evidence
run: [job 102991855984](https://github.com/general-liquidity/sharpebench/actions/runs/34513112007/job/102991855984)
on head `6527049`; every later head re-runs the same three tests.

**Gateway launch.**
`sandbox::tests::live_gateway_launch_serves_model_calls_over_stdio_with_no_network`
runs `run_gateway_sweep` with a journal on disk over one cell of three
decisions. The cell spawns the argv `gateway_launch` returns through
`EntrantLaunch::isolating_launcher`, with an explicit `/bin/sh -c <entrant>`
appended after the image positional because the fixture's own entrypoint is a
bare shell, and awaits `wait_until_running`. The provider cannot open a socket.
Log lines:

```text
gateway container sharpebench-agent-5907-0: state=Ok(ContainerExitState { status: "exited", oom_killed: false, exit_code: 0 }) verdict=Ok(WithinBudget) removed=Ok(()) remnant=false
model requests seen by the provider: ["ifaces=lo, egress_exit=1", "ifaces=lo, egress_exit=1", "ifaces=lo, egress_exit=1"]
test sandbox::tests::live_gateway_launch_serves_model_calls_over_stdio_with_no_network ... ok
```

Each request was written by the entrant inside the container after it listed
`/sys/class/net` (loopback only) and tried `wget` to `1.1.1.1:80` (exit 1).
Each decision is a valid hold only when the gateway's answer arrived on the
entrant's stdin, and the run finished with no failure record. The journal on
disk holds three reservations and three settlements, all priced. The container
was classified before removal and `docker inspect` found no remnant. No product
defect: the launch reached the entrant and the serving loop answered over the
container's stdio unchanged.

**What the daemon adds to an export.**
`artifact_preflight::tests::live_runtime_allowlist_admits_the_fixture_and_its_probe_passes`
compares an export taken with the preflight's own create arguments against the
fixture's layers from `docker save` (one layer, 519 entries). The export held
524 entries:

```text
docker-added export entry: .dockerenv Regular size=0 (image holds: nothing)
docker-added export entry: dev/console Regular size=0 (image holds: nothing)
docker-added export entry: dev/pts Directory size=0 (image holds: nothing)
docker-added export entry: dev/shm Directory size=0 (image holds: nothing)
docker-added export entry: etc/hostname Regular size=0 (image holds: etc/hostname Regular size=10)
docker-added export entry: etc/hosts Regular size=0 (image holds: etc/hosts Regular size=79)
docker-added export entry: etc/mtab Symlink size=0 -> /proc/mounts (image holds: etc/mtab Symlink size=0 -> ../proc/mounts)
docker-added export entry: etc/resolv.conf Regular size=0 (image holds: nothing)
```

Five paths are new and three of the image's own are replaced: the daemon's init
layer unlinks and recreates them, so the image's bytes at `etc/hostname` and
`etc/hosts` are not in the export (and a started container sees the daemon's
bind mounts there). No mount point beyond these appears; the preflight creates
its snapshot container with no tmpfs or volume.

**Product defect found and fixed.** An allowlist naming exactly the image's
own paths refused every real export: the five new entries are always there, and
an operator cannot list what they did not know the daemon adds. The init-layer
table is the daemon's, not the image's, so `check_allowlist` now admits those
entries without an allowlist line, and only in the daemon's shape
(`DOCKER_INIT_ENTRIES` in `artifact_preflight.rs`: empty regular files at
`.dockerenv`, `dev/console`, `etc/hostname`, `etc/hosts` and `etc/resolv.conf`;
directories at `dev`, `dev/pts`, `dev/shm`, `etc`, `proc` and `sys`; `etc/mtab`
linking to `/proc/mounts`). Refusal for real content is not weakened: an entry
at one of those paths with bytes, another type or another link target refuses
by index, and nothing below a directory is admitted by the rule. The report's
`runtime_allowlist` gains `docker_init_entries`. The live test asserts every
measured daemon-added entry satisfies the rule, so a daemon that adds something
else fails the job instead of widening the rule silently.

Unit test: `docker_init_entries_are_admitted_only_in_the_shape_docker_gives_them`
(the twelve entries admitted with an allowlist of `app/` alone; eight
wrong-shape cases and a file below `proc/` refused by index, never probed).
Mutations, each in an isolated `git archive` copy of the committed tree:

| Invariant | Mutation | Result |
|---|---|---|
| A daemon file must be empty | `EmptyFile` accepts any regular file | killed: case 0 (`.dockerenv` with one byte) admitted |
| `etc/mtab` must link to `/proc/mounts` | the link target check always passes | killed: case 3 (link to another path) admitted |

**Allowlist and probe results.** With an allowlist of the fixture's 519 own
paths (11040 bytes), each admitted exactly:

| Image | `entries` | `docker_init_entries` | `outside_allowlist` | Probe | Authorizes |
|---|---|---|---|---|---|
| pinned fixture (entrypoint `/bin/sh`) | 524 | 5 | 0 | `no_decision`, cleanup verified | no |
| same filesystem, committed with an answering entrypoint | 524 | 5 | 0 | passed, cleanup verified | yes |

The pinned fixture's shell reads the probe observation as a script and writes
nothing to stdout, so the refusal shows the probe runs the image's own
entrypoint under the hardened launch rather than assuming it works. The
answering image is committed by the test from a created, never-started
container of the fixture and removed afterwards; it is reached by its
configuration ID, as a launch after a passing preflight is.

`artifact_preflight::tests::live_runtime_allowlist_refuses_an_omitted_path_by_index`
drops `etc/alpine-release` from the same allowlist: `outside_allowlist: 1`,
`outside_indices: [89]`, the index of that entry in an independent export of
the same image; no probe ran, cleanup was verified, and the published report
does not contain the entry name.

## Byte identity

With `--runtime-allowlist` absent and no gateway sweep run, every existing
output is unchanged:

- `crates/sharpebench-core`, `crates/sharpebench-sim`,
  `crates/sharpebench-stats`, `crates/sharpebench-protocol`, `examples/`,
  `suites/`, `data/`, `arena/`, `paper/` and `crates/sharpebench-cli/src/main.rs`
  are not modified by this work: over those paths, the only commits in
  `git log --no-merges 7fe5a06..HEAD` are provenance rebinds of
  `paper/evidence/provenance.json`, which bind digests and move no evidence
  value. The goldens pass unchanged:
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
- The live-verification fix (2026-09-10) changes only a preflight run with an
  allowlist: `runtime_allowlist` gains `docker_init_entries`, and the
  daemon's init-layer entries admit in their exact shape. `policy_sha256` and
  the allowlist digest are unchanged, and no entry that an allowlist admitted
  before now refuses. Without `--runtime-allowlist` the report is unchanged.
