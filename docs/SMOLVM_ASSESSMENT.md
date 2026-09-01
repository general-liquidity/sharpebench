# smolvm assessment for SharpeBench

This ledger records the complete local smolvm review and the engineering choices
made for SharpeBench. It separates transferable boundary discipline from
microVM-specific infrastructure that would not improve the benchmark.

## Review coverage

The reviewed smolvm 1.13.1 snapshot contains 492 files: 222 Rust files (about
188,000 lines), 72 shell files (about 17,600 lines), 33 Markdown files, and the
remaining TypeScript, Python, Nix, workflow, packaging, and deployment files.
The review covered the 109-file host runtime, all 131 files across 15 workspace
crates, 42 integration-test files, 23 benchmark files, 45 SDK files, and every
other top-level product slice.

The snapshot has no Git metadata. Source references below are coordinates into
that tree rather than commit-stable citations.

## Repository organization

smolvm keeps many roots because it ships a cross-platform virtualization stack:
the host runtime, 15 internal crates, real-VM tests, benchmarks, demos, examples,
OS packages, Nix, deployment assets, language SDKs, and release workflows.
`libkrun/`, `libkrunfw/`, and `smolvm-sdk/` are Git submodules; `sdks/` contains
in-tree language wrappers; `lib/` is the Git-LFS-backed set of prebuilt runtime
libraries. The local export contains empty submodule directories and LFS pointer
stubs, so it can be audited but cannot boot a VM.

SharpeBench should not copy this physical layout. Its workspace already reflects
its product boundaries. The README can copy smolvm's concise, task-first
information architecture without moving packaging, papers, crates, and docs into
more root directories.

## Adopted and verified

| smolvm evidence | SharpeBench extraction | Correctness assessment |
|---|---|---|
| `tests/test_network.sh:22-64` refuses egress by classification and elapsed time instead of accepting any failed connect. | `sharpebench-arena::EgressTarget` covers cloud metadata, the wider link-local range, all RFC1918 classes, public internet, and a controlled host-loopback listener. `EgressVerdict` distinguishes policy denial, timeout, connection, and missing probe. | Correct and stronger than the original single public-IP probe. The Docker CI runs every class; a broken runner network or absent client cannot false-pass as containment. |
| smolvm lifecycle tests distinguish a dead process, timeout, and cleanup rather than accepting a generic non-zero exit. | External stdio is byte-bounded, startup death fails fast, a response written immediately before exit remains a success, and Unix process-group teardown sends TERM, waits briefly, then KILLs descendants. | Appropriate for an agent protocol. It closes OOM-by-one-line and orphaned-grandchild hangs without importing a VM process manager. |
| Resource limits and liveness are treated as observable runtime facts throughout the agent/runtime code. | Named entrant containers remain inspectable after exit; `State.OOMKilled=true` becomes `ResourceLimitExceeded`, a non-retryable agent fault counted against pass^k, and cleanup removes the container on finish and drop. | Correct. A live Docker CI test crosses a real 32 MiB cgroup limit and verifies the production inspector. The documented SIGKILL-between-spawn-and-drop remnant is honest and discoverable by name prefix. |
| `src/secrets.rs` and `scripts/check-secrets-guards.sh` make credential exposure an explicit boundary. | Host `--cmd` agents start with a cleared environment and a small platform allowlist. Named values require `SHARPEBENCH_AGENT_ENV`; the trusted Docker client alone inherits Docker context, while the entrant receives the container's fresh environment. | Correctly separates the trusted launcher from the untrusted workload. It avoids leaking API keys while retaining an explicit path for legitimate agent dependencies. |
| smolvm rejects unavailable images/configuration before treating a workload as running. | `require_local_image` uses `docker image inspect`; a mutable tag, missing daemon, absent digest-pinned image, or failed readiness probe is a refusal with no host fallback. | Correct. The original absent-image test passed only because the local daemon was absent; the live-daemon CI test now exercises the intended branch and pins the missing digest in the error. |
| `src/cli/cleanup_ephemeral.rs` makes effect ordering testable and retains records on failed deletion. | The release driver uses injected cleanup effects, safe worktree path checks, and a fetched throwaway worktree. It refuses local-only/empty notes, rebinds provenance, validates an annotated provenance-only tag, and atomically pushes branch and tag. | Correctly adapted. It avoids both a dirty operator checkout and smolvm's non-atomic release-push pattern. |
| `src/artifact_cache.rs:159-181` publishes a durable file with exclusive create, sync, rename, and parent sync. | Provenance rules are code-owned, source text is line-ending canonicalized, artifacts are byte-exact, clean-generation blobs are read from Git, and release tags are validated before publishing. | Stronger for research evidence. A manifest cannot redefine its own scope or exclusions, and a claimed clean source is contradicted against committed bytes. |
| smolvm's real-VM shell suites assert their fixtures have subjects and distinguish false-pass controls. | Docker acceptance tests explicitly fail when requested without a daemon, verify the probe client and namespace, and test the production path rather than a parallel helper. | Correct. Unlike smolvm's default GitHub CI, SharpeBench's dedicated job actually executes its Docker boundary. |
| SDK packaging scripts emphasize testing installed artifacts rather than repository imports. | npm and MCP build/test; npm pack installs offline in a throwaway project; Python wheels are built and imported; release verification checks every registry surface. | Correct and directly consumer-facing. |

## Existing equivalents

- Closed protocol schemas, deterministic score goldens, trajectory replay,
  content commitments, sealed data, and HMAC/Ed25519 chains already exceeded the
  comparable smolvm integrity mechanisms for benchmark evidence.
- SharpeBench already carries typed failure and disqualification taxonomies; it
  did not need smolvm's message convention copied into the scoring kernel.
- The simulator's point-in-time agent boundary is an information-flow property,
  not process containment. Docker containment supplements it for `--image`; it
  does not replace or prove the PIT property.

## Deliberately not ported

| smolvm subsystem | Decision |
|---|---|
| libkrun/libkrunfw and per-entrant microVMs | Rejected for the present local benchmark. A digest-pinned, no-network Docker boundary is implemented and CI-exercised. Moving to a VM would add a large platform/runtime/release surface and much higher startup cost without solving hosted intake, identity, or scheduling. Reconsider only for a hostile multi-tenant service with a documented kernel-isolation requirement. |
| `.smolmachine`, snapshots, forkpoints, and portable checkpoints | Rejected as submission/evidence formats. The compatible pack footer uses CRC32; a later SHA-256 sidecar protects a shared extraction cache, not the provenance of source, configuration, decisions, and results. OCI image digests plus SharpeBench attestations fit the current boundary. |
| CUDA/NVML API remoting, Vulkan/VNC, S3/FUSE, OCI registry/cache, Kubernetes/containerd shim, fleet admission, and cloud APIs | Not relevant to local scoring or the file-backed arena. They belong to a hosting control plane the product does not claim. |
| Bundled smolvm binaries | Rejected. They are unnecessary, the local LFS/submodule payloads are absent, and redistribution would bring libkrunfw/LGPL and bundled GPL-kernel obligations. |
| smolvm Node SDK | Prior art only. Its build references a missing `crates/smolvm-napi`, and the main CI does not exercise it. SharpeBench's npm/MCP packages are tested as packed consumer artifacts. |
| Shared artifact-cache eviction and P2P distribution | Conditional. Relevant only if a hosted arena later owns a shared image/model cache. Docker's local image store and digest refusal are the current boundary. |

## Is this a real agent sandbox?

For the `sharpebench run --image <repository@sha256:...>` path, yes in the
ordinary container sense: an entrant is actually launched through a fail-closed,
CI-exercised Docker boundary with network/IPC isolation, a read-only root,
non-root uid, dropped capabilities, no-new-privileges, noexec scratch mounts,
resource limits, bounded transport, descendant teardown, and post-exit OOM
classification. `--cmd` and `--http` are explicitly not that sandbox.

The qualification matters: this is not a microVM, not a proof against Docker or
kernel escape, and not a multi-tenant hosting product. The acceptance evidence is
one pinned benign fixture on a Docker-enabled CI runner; no hostile third-party
entrant has been operated as a tenant. Those limits are part of the product
contract, not omissions to hide in a footnote.

## Conclusion

All smolvm ideas relevant to SharpeBench's current scoring and local-containment
scope have been implemented or already had a stronger product-native equivalent.
The rejected systems are deployment/runtime products, not missing benchmark
controls. Adopting them now would enlarge the trusted computing base and root
layout without strengthening a claim SharpeBench currently makes.
