# Releasing

Every package in this repo publishes from CI via [`.github/workflows/release.yml`](.github/workflows/release.yml)
using **OIDC Trusted Publishing** — there are **no tokens to store or rotate** (after
the one-time crate claim, below). Each registry trusts this workflow directly; GitHub
mints a short-lived identity per run.

A tag (`v*`) always builds and attaches the signed static **musl** binary to the
GitHub Release. The crates.io and npm jobs only run when you opt them in.

## What ships where

| Target | Packages | Trigger |
|---|---|---|
| **GitHub Release** | `sharpebench-x86_64-linux-musl` static binary + `.sha256` | every `v*` tag (always) |
| **crates.io** | the **12** `sharpebench-*` crates (see order below) | `v*` tag **and** `PUBLISH_CRATES=true` |
| **npm** | `@general-liquidity/sharpebench` + `@general-liquidity/sharpebench-mcp` | `v*` tag **and** `PUBLISH_NPM=true` |
| **PyPI** | `sharpebench` (pyo3 wheels, CPython 3.10-3.13, + sdist) | `v*` tag **and** `PUBLISH_PYPI=true` |

The `xtask` and `examples/reference-agent` workspace members are `publish = false`
and never reach crates.io.

### crates.io dependency order

CI publishes the crates one-by-one in this order (`cargo publish -p <crate>`, each
waiting for the index before the next), skipping any already live at the release
version. The **first, name-claiming** publish must follow the same order by hand:

```
Tier0  sharpebench-stats   sharpebench-protocol   sharpebench-core   sharpebench-attest
         core → protocol
Tier1  sharpebench-memory   sharpebench-edge   sharpebench-sim
         memory → stats     edge → stats     sim → core, protocol
Tier2  sharpebench-leaderboard   sharpebench-wasm   sharpebench-harness
         leaderboard → core, attest
         wasm        → core, attest, edge
         harness     → core, protocol, sim
Tier3  sharpebench-arena                                              # → core, attest, leaderboard, sim
Tier4  sharpebench                                                    # the CLI binary
         → core, protocol, sim, harness, attest, leaderboard, edge, arena
```

Nothing depends on `sharpebench-memory`: it is a second benchmark that shares only
`sharpebench-stats`. It is published because it is a caller-facing library with a
documented API, not because the workspace needs it.

## One-time setup (per registry, on the registry's own website)

You configure a *trusted publisher* once. Nothing is stored in GitHub except the
opt-in variable(s) and the environments. The publisher is always: **GitHub** owner
`general-liquidity`, repo `sharpebench`, workflow file `release.yml`.

| Registry | Where | Notes |
|---|---|---|
| **crates.io** | each crate → *Settings → Trusted Publishing* | All 12 names are already published and configured for this workflow. A future new crate must be claimed once with a token before its trusted publisher can be added. |
| **npm** | each package page → *Settings → Trusted Publisher* | If `…/sharpebench` + `…-mcp` already exist, configure the trusted publisher directly. If not, claim each once (`npm publish --access public`), then add the trusted publisher. Needs npm ≥ 11.5 (the workflow upgrades it). Provenance is automatic. |

### Claiming a future crate name once

Trusted publishing cannot be added to a crate that does not exist yet. All current
SharpeBench packages are already claimed; this procedure applies only if a later
release adds another public crate. Use a narrowly scoped token for that first
publish, place the crate in the dependency order below, then add the same trusted
publisher as the existing packages and remove the token locally.

```bash
cargo publish -p sharpebench-stats
cargo publish -p sharpebench-memory
cargo publish -p sharpebench-edge
cargo publish -p sharpebench-protocol
cargo publish -p sharpebench-core
cargo publish -p sharpebench-attest
cargo publish -p sharpebench-sim
cargo publish -p sharpebench-leaderboard
cargo publish -p sharpebench-wasm
cargo publish -p sharpebench-harness
cargo publish -p sharpebench-arena
cargo publish -p sharpebench          # the CLI binary crate (package name `sharpebench`)
```

(Avoid `cargo publish --workspace` for this — its publish planner can deadlock
part-way through with "no packages ready to publish but N packages remain… awaiting
confirmation", leaving some crates unpublished; publish per-crate as above.) After
the new crate is live, add its trusted publisher on crates.io; thereafter CI publishes
it tokenlessly. See [`docs/PUBLISHING.md`](docs/PUBLISHING.md) for registry setup
and recovery notes.

### Opt-in repository variables + environments

In **Settings → Variables → Actions**, set the flag(s):

```
PUBLISH_CRATES=true
PUBLISH_NPM=true
PUBLISH_PYPI=true
```

With a variable unset, that job is skipped — so the workflow is safe to land before
anything is configured (the binary job still runs).

Create three **GitHub Environments** (Settings → Environments) named `crates`,
`npm` and `pypi`. They scope the OIDC identity and let you add protection rules (required
reviewers, branch restrictions) to the publishing steps. Their names must match the
`environment:` fields in `release.yml` and the trusted-publisher configs.

## Cutting a release

1. Confirm the version surfaces the driver will bump together:
   - `[workspace.package] version` in the root [`Cargo.toml`](Cargo.toml) (all 14
     Rust workspace members inherit via `version.workspace = true`; two are
     private, and cargo-release rewrites the
     inter-crate `version = "x"` pins — see [`release.toml`](release.toml)).
   - `version` in [`npm/package.json`](npm/package.json) **and**
     [`npm/mcp/package.json`](npm/mcp/package.json) (and the MCP package's
     `@general-liquidity/sharpebench` dependency range if you want it pinned to the
     new kernel).
2. Finish and push the `[Unreleased]` section of `CHANGELOG.md`. The driver
   promotes it and adds both compare links inside the isolated release tree; it
   refuses notes that exist only in the operator's checkout.
3. Run the green checks first (CI does too, but cargo-release won't):
   ```bash
   cargo test --workspace && cargo clippy --workspace --all-targets && cargo deny check
   ```
4. Rehearse, then execute the isolated release driver:
   ```bash
   python scripts/release.py rehearse patch
   python scripts/release.py execute patch
   ```
   Both commands fetch `origin/main` and cut from a throwaway worktree. The
   operator's checkout is never the release tree, so local dirt and a stale local
   branch cannot enter the release. The driver refuses changelog entries that are
   only local, promotes the pushed notes, runs `cargo release` without publishing
   or pushing, regenerates provenance on the clean version commit, tags that
   rebind, and atomically pushes the commits plus tag. `rehearse` executes the same
   sequence without the tag or push and deletes its temporary branch.

   The tag must point at the rebind, not at the version bump. The bump rewrites
   files inside the provenance source scope, so the manifest written during it
   records a dirty generation and the tagged tree fails its own provenance gate.
   That is what happened to v0.14.1, whose tag `d44c15a` is red on the
   `paper evidence provenance` job for exactly this reason.
   The tag triggers `release.yml`: the binary is built + attached to the Release
   always; the `crates` / `npm` jobs run if their `PUBLISH_*` variable is `true`.
   (Or run the workflow manually from the Actions tab via *workflow_dispatch*.)

The driver delegates the workspace bump and replacement rules to `cargo-release`
(config in [`release.toml`](release.toml)); publishing remains tokenless in CI after
the tag is pushed.

With OIDC trusted publishing, npm attaches **provenance** automatically, so each
release carries a signed attestation that it was built from this repo + commit.

## Binary / GitHub Release behavior

The `binary` job always runs on a `v*` tag (no opt-in). It builds a fully static
`x86_64-unknown-linux-musl` release binary, writes a `sha256` checksum next to it,
and uploads both via `softprops/action-gh-release@v3` to the Release for that tag.
`cargo install sharpebench` (once the crate is published) is the alternate install
path.
