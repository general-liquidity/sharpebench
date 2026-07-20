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
| **crates.io** | the **8** `sharpebench-*` crates (see order below) | `v*` tag **and** `PUBLISH_CRATES=true` |
| **npm** | `@general-liquidity/sharpebench` + `@general-liquidity/sharpebench-mcp` | `v*` tag **and** `PUBLISH_NPM=true` |
| **PyPI** | `sharpebench` (pyo3 wheels, CPython 3.10-3.13, + sdist) | `v*` tag **and** `PUBLISH_PYPI=true` |

The `xtask` and `examples/reference-agent` workspace members are `publish = false`
and never reach crates.io.

### crates.io dependency order

CI publishes the crates one-by-one in this order (`cargo publish -p <crate>`, each
waiting for the index before the next), skipping any already live at the release
version. The **first, name-claiming** publish must follow the same order by hand:

```
Tier0  sharpebench-core   sharpebench-protocol   sharpebench-attest   # no intra-deps
Tier1  sharpebench-sim                                                # → core, protocol
Tier2  sharpebench-leaderboard   sharpebench-wasm   sharpebench-harness
         leaderboard → core, attest
         wasm        → core, attest
         harness     → core, protocol, sim
Tier3  sharpebench                                                    # the CLI binary
         → core, protocol, sim, harness, attest, leaderboard
```

## One-time setup (per registry, on the registry's own website)

You configure a *trusted publisher* once. Nothing is stored in GitHub except the
opt-in variable(s) and the environments. The publisher is always: **GitHub** owner
`general-liquidity`, repo `sharpebench`, workflow file `release.yml`.

| Registry | Where | Notes |
|---|---|---|
| **crates.io** | each crate → *Settings → Trusted Publishing* | A crate must **exist** before a trusted publisher can be added. Do **one** initial `cargo publish` with a token to claim each of the 8 names **in the dependency order above**, then add the trusted publisher to each crate and never use a token again. |
| **npm** | each package page → *Settings → Trusted Publisher* | If `…/sharpebench` + `…-mcp` already exist, configure the trusted publisher directly. If not, claim each once (`npm publish --access public`), then add the trusted publisher. Needs npm ≥ 11.5 (the workflow upgrades it). Provenance is automatic. |

### Claiming the crate names once (token, first time only)

Trusted publishing can't be added to a crate that doesn't exist yet, so the first
publish of each name needs a real token. `cargo login <token>` (token scopes:
publish-new, publish-update), then publish in dependency order:

```bash
cargo publish -p sharpebench-core
cargo publish -p sharpebench-protocol
cargo publish -p sharpebench-attest
cargo publish -p sharpebench-sim
cargo publish -p sharpebench-leaderboard
cargo publish -p sharpebench-wasm
cargo publish -p sharpebench-harness
cargo publish -p sharpebench          # the CLI binary crate (package name `sharpebench`)
```

(Avoid `cargo publish --workspace` for this — its publish planner can deadlock
part-way through with "no packages ready to publish but N packages remain… awaiting
confirmation", leaving some crates unpublished; publish per-crate as above.) After
each crate is live, add its trusted publisher on crates.io; thereafter CI publishes
tokenlessly. See [`docs/PUBLISHING.md`](docs/PUBLISHING.md) for the manual token
flow / name-availability notes.

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

1. Bump the version everywhere it's pinned:
   - `[workspace.package] version` in the root [`Cargo.toml`](Cargo.toml) (all 8
     crates inherit via `version.workspace = true`; cargo-release rewrites the
     inter-crate `version = "x"` pins — see [`release.toml`](release.toml)).
   - `version` in [`npm/package.json`](npm/package.json) **and**
     [`npm/mcp/package.json`](npm/mcp/package.json) (and the MCP package's
     `@general-liquidity/sharpebench` dependency range if you want it pinned to the
     new kernel).
2. Update `CHANGELOG.md` if/when one is added (none today).
3. Run the green checks first (CI does too, but cargo-release won't):
   ```bash
   cargo test --workspace && cargo clippy --workspace --all-targets && cargo deny check
   ```
4. Commit, then tag and push:
   ```bash
   git tag v0.0.6 && git push origin v0.0.6
   ```
   The tag triggers `release.yml`: the binary is built + attached to the Release
   always; the `crates` / `npm` jobs run if their `PUBLISH_*` variable is `true`.
   (Or run the workflow manually from the Actions tab via *workflow_dispatch*.)

`cargo release patch --execute` automates steps 1, 4, and the ordered crates.io
publish in one command (config in [`release.toml`](release.toml)); the CI path above
is the tokenless alternative once trusted publishing is wired.

With OIDC trusted publishing, npm attaches **provenance** automatically, so each
release carries a signed attestation that it was built from this repo + commit.

## Binary / GitHub Release behavior

The `binary` job always runs on a `v*` tag (no opt-in). It builds a fully static
`x86_64-unknown-linux-musl` release binary, writes a `sha256` checksum next to it,
and uploads both via `softprops/action-gh-release@v3` to the Release for that tag.
`cargo install sharpebench` (once the crate is published) is the alternate install
path.
