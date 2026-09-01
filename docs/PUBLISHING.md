# Publishing SharpeBench

SharpeBench publishes from the tagged commit through GitHub Actions. The normal
path uses OIDC trusted publishing; no long-lived crates.io, npm, or PyPI token is
stored in the repository.

The operational runbook is [`RELEASING.md`](../RELEASING.md). This page explains
the registry setup and the exceptional first-publish case.

## Published surfaces

| Registry | Packages |
|---|---|
| crates.io | 12 `sharpebench-*` crates, including the `sharpebench` CLI and `sharpebench-memory` |
| npm | `@general-liquidity/sharpebench` and `@general-liquidity/sharpebench-mcp` |
| PyPI | `sharpebench` |
| GitHub Releases | Static musl CLI, checksum, and build provenance |

`xtask` and `examples/reference-agent` are private workspace members with
`publish = false`.

## Trusted-publisher identity

Each registry trusts the GitHub repository `general-liquidity/sharpebench`, the
workflow file `release.yml`, and its registry-specific GitHub Environment:
`crates`, `npm`, or `pypi`. GitHub exchanges the workflow's OIDC identity for a
short-lived publishing credential during the release run.

All current package names are already claimed. A normal release therefore needs
no `cargo login`, npm token, PyPI token, or repository secret.

## Adding a new Rust crate

crates.io cannot configure a trusted publisher before a crate exists. If the
workspace gains another public crate:

1. Confirm its permanent package name and set `publish = false` until the public
   API and dependency order are reviewed.
2. Run the complete local package, test, lint, license, and release rehearsal
   gates.
3. Claim the name once with a narrowly scoped crates.io token using
   `cargo publish -p <new-crate>`.
4. Configure the same GitHub trusted-publisher identity as the existing crates.
5. Add it to the dependency-ordered publish loop and registry verification list
   in `.github/workflows/release.yml`.
6. Remove the local token and restore the OIDC-only path.

The first publish is irreversible: a version can be yanked but not deleted, and
the crate name remains claimed. This one-time step is therefore intentionally
manual. It is not the release procedure for existing packages.

## Recovery after a partial release

Publishing is per package. If a registry job stops after some packages are live,
do not retag or overwrite published versions. Fix the workflow or registry
configuration, then rerun the same tag: the release jobs skip versions already
present and continue the dependency-ordered loop.

The release workflow validates the annotated tag and provenance before any
publish job, checks out the exact validated commit in every job, and verifies all
registries after publishing. See [`RELEASING.md`](../RELEASING.md) for the cut,
rehearsal, and tag invariants.
