# Publishing SharpeBench to crates.io

crates.io is Rust's package registry — the `cargo` equivalent of `npm publish`.
Publishing is **irreversible**: a version can be *yanked* (blocked from new
dependents) but never deleted, and a crate **name is claimed forever**. Decide
names *before* the first publish.

## 0. Decide the published names (do this first — it is permanent)

The crates are currently `sb-core`, `sb-sim`, … — short and generic. On crates.io
names are flat + global, so they may be taken and they don't group under the
project. Two options:

- **Keep `sb-*`** if the names are free (check `https://crates.io/crates/sb-core`, …).
- **Rename to `sharpebench-*`** (recommended) — groups on search and makes
  `cargo install sharpebench` read cleanly. The low-risk rename touches *no*
  `use sb_core::` imports — change only the package name, keep the lib name:

  ```toml
  # crates/sb-core/Cargo.toml
  [package]
  name = "sharpebench-core"     # the crates.io name
  [lib]
  name = "sb_core"              # internal import path stays `sb_core`
  ```

  and in each dependent, alias it back:

  ```toml
  sb_core = { package = "sharpebench-core", path = "../sb-core", version = "0.0.1" }
  ```

## 1. Account + token

1. Log into <https://crates.io> with GitHub.
2. Account Settings → API Tokens → new token (scopes: publish-new, publish-update).
3. `cargo login <token>` (or set `CARGO_REGISTRY_TOKEN` in the environment).

## 2. Pre-publish checklist

- [ ] Names decided (§0).
- [ ] `cargo deny check`, `cargo test --workspace`, `cargo clippy --workspace` all clean.
- [ ] Every crate has `description` + `license` (✓ inherited: `MIT OR Apache-2.0`).
- [ ] Path deps carry a `version` (✓ — crates.io requires it; path is ignored on publish).
- [ ] Mark any internal-only crate `publish = false` (e.g. if `sb-wasm` stays Gordon-internal).
- [ ] Clean working tree (`cargo publish` refuses a dirty tree).

## 3. Publish in dependency order

A crate must be live on crates.io before its dependents can resolve it:

```
sb-core   sb-protocol   sb-attest        # no internal deps — publish first
sb-sim    sb-leaderboard   sb-wasm       # depend on the row above
sb-harness                               # depends on core / protocol / sim
sb-cli                                   # depends on all of them (binary: `sharpebench`)
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
