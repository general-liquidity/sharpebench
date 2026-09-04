# SharpeBench status and roadmap

This file began as the pre-build plan. The planned kernel, simulator, harness,
attestation, leaderboard, arena, language bindings, and release system now ship;
this page records the remaining product work instead of preserving a stale
architecture sketch.

## Shipped

- Pure Rust statistics and scoring kernels: PSR, deflated Sharpe, pass^k,
  stationary-bootstrap significance, field-level multiple-testing reports,
  process and mandate gates, and typed disqualification reasons.
- Point-in-time simulator, reference entrants, seeded execution noise,
  walk-forward windows, frozen datasets, trajectory capture, and independent
  replay verification.
- Language-neutral observation/decision contract with closed JSON Schemas.
- CLI, Rust crates, WASM/npm, MCP, and Python ranker over the same scoring code.
- Forward commitments, sealed data, HMAC and Ed25519 result chains, and a
  file-backed forward arena.
- A fail-closed Docker path for untrusted entrants, with no-network/read-only/
  non-root/capability/resource controls, bounded transport, process-group
  teardown, egress-class probes, and OOM classification.
- Provenance-bound releases using an isolated worktree, annotated tag
  validation, atomic push, OIDC publishing, package smoke installs, and registry
  verification.
- The independent `sharpebench-memory` ablation benchmark.
- Prospective forecast analysis over a closed cross-product artifact, including
  exact common support, proper scores, fixed-bin calibration, resolution-clock
  block resampling, Holm adjustment, rank isolation, and an independent report
  reconstruction. One superseded three-model engineering pilot is committed for
  lifecycle auditability, not as current model evidence.

## Remaining product work

These need external infrastructure or a product decision rather than another
kernel primitive:

- Hosted intake, identity, scheduling, and public board operations for the
  forward arena. The file-backed lifecycle and cryptographic documents already
  exist; there is no multi-tenant service.
- A public dataset-curation and deprecation protocol.
- Keyed single-name equity data if that universe is admitted.
- The first completed forward window. The operating record is under
  [`arena/`](../arena/README.md); an open window is not a result.
- A current-model trading-agent field. The archived prospective pilot evaluates
  an obsolete convenience-sample forecast head with no tools, memory, portfolio,
  or order loop. A current declared model panel, repeated multi-window fields,
  and a complete agent scaffold remain external experiments, not missing scorer
  primitives.

## Reproducibility scope

The Rust toolchain and Cargo dependency graph are pinned. `flake.nix` reads the
toolchain version from `rust-toolchain.toml` and builds from `Cargo.lock`, but
this repository does not currently commit a `flake.lock`. The Rust version has
one source of truth; the nixpkgs and rust-overlay revisions are not immutable
until a lock file is generated and committed. Documentation should not call the
current Nix input graph permanently hermetic.

For the methodology and current interfaces, use the [mdBook](book/). For release
operations, use [`RELEASING.md`](../RELEASING.md).
