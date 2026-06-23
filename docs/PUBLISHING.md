# Publishing SharpeBench to crates.io

crates.io is Rust's package registry — the `cargo` equivalent of `npm publish`.
Publishing is **irreversible**: a version can be *yanked* (blocked from new
dependents) but never deleted, and a crate **name is claimed forever**. Decide
names *before* the first publish.

## 0. Published names — `sharpebench-*`

The crates are named consistently throughout — directory, package, and import all
match: `crates/sharpebench-core`, package `sharpebench-core`, import
`sharpebench_core`. The CLI crate's package is `sharpebench` (so `cargo install
sharpebench` works). Nothing left to decide.

Names on crates.io are permanent (a version can be yanked but never deleted, and a
name is claimed forever), so confirm availability before the first publish.

## 1. Account + token

1. Log into <https://crates.io> with GitHub.
2. Account Settings → API Tokens → new token (scopes: publish-new, publish-update).
3. `cargo login <token>` (or set `CARGO_REGISTRY_TOKEN` in the environment).

## 2. Pre-publish checklist

- [ ] Names decided (§0).
- [ ] `cargo deny check`, `cargo test --workspace`, `cargo clippy --workspace` all clean.
- [ ] Every crate has `description` + `license` (✓ inherited: `MIT OR Apache-2.0`).
- [ ] Path deps carry a `version` (✓ — crates.io requires it; path is ignored on publish).
- [ ] Mark any internal-only crate `publish = false` (e.g. if `sharpebench-wasm` stays Gordon-internal).
- [ ] Clean working tree (`cargo publish` refuses a dirty tree).

## 3. Publish in dependency order

A crate must be live on crates.io before its dependents can resolve it:

```
sharpebench-core   sharpebench-protocol   sharpebench-attest    # no internal deps — first
sharpebench-sim    sharpebench-leaderboard   sharpebench-wasm   # depend on the row above
sharpebench-harness                                             # core / protocol / sim
sharpebench                                                     # the binary; depends on all
```

Helper script:

```bash
scripts/publish.sh --check      # package-verify the leaf crates locally (no upload)
scripts/publish.sh --execute    # real, ordered publish to crates.io
```

> `cargo publish --dry-run` only fully works for the leaf crates (those with no
> internal deps). Dependents verify against crates.io once their deps are live, so
> the real publish proceeds one crate at a time, in the order above.

## 4. After publishing

- `cargo install sharpebench` installs the CLI. The signed static **musl** binary
  also ships via GitHub Releases (`.github/workflows/release.yml`, on `git tag v*`).
- To automate later: add `CARGO_REGISTRY_TOKEN` as a repo secret and extend
  `release.yml` to run the ordered publish on tag. Do the **first** publish by hand
  — names are permanent, so verify everything once before automating.

## 5. Cutting a release with cargo-release (preferred)

`cargo-release` automates §3 plus the version bump and the inter-crate `version`
pin rewrite — the one easy-to-miss step. Config lives in `release.toml`.

```bash
cargo install cargo-release            # once
# verify green first — cargo-release does not run tests:
cargo test --workspace && cargo clippy --workspace --all-targets && cargo deny check
cargo release patch                    # DRY RUN: prints the bump, dep rewrites, publish order, tag
cargo release patch --execute          # 0.0.1 -> 0.0.2: bump, rewrite pins, ordered publish, tag v0.0.2, push
```

`cargo login <token>` once (or set `CARGO_REGISTRY_TOKEN`) before `--execute`. The
pushed `v0.0.2` tag triggers the signed musl binary build in `release.yml`. The
manual `scripts/publish.sh` flow in §3 remains as a fallback.
