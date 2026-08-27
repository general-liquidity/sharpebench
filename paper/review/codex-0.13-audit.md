# Independent audit of v0.12.0..HEAD (61c479a)

Audited 2026-08-27 against a clean worktree at `61c479ac8f0bf1c9e959a55e5ac6f85fdf1a157a`
(`git status --porcelain` empty before and after; nothing was modified by this audit).

Scope: the seven commits after `v0.12.0`, none of which was independently verified.

```
61c479a fix(release): resolve the MCP dependency after publication
b5dd3b8 chore(provenance): record the clean portable snapshot
694db66 chore(provenance): bind the portable source identity
955638e fix(provenance): canonicalize source line endings
ac85b3e chore(provenance): record a clean v0.13.0 generation head
7591f70 chore(provenance): bind the v0.13.0 source snapshot
d2559ce release: v0.13.0
```

**Zero Rust source files changed in the range.** `git diff v0.12.0..HEAD --name-only`
outside of `Cargo.toml` / `package.json` / lockfiles is exactly:

```
.github/workflows/ci.yml
.github/workflows/release.yml
.gitignore
CHANGELOG.md
crates/sharpebench-py/pyproject.toml
paper/evidence/provenance.json
paper/main.pdf
paper/sections/A-commands.tex
paper/src/check-provenance.py
paper/src/make-provenance.py
```

That matters for interpreting the gates below: an unchanged test count is the expected
result, not evidence that anything was verified.

## Gates, measured

| Gate | Result |
|---|---|
| `cargo test --release --workspace` | **604 passed, 0 failed, 3 ignored** (exit 0) |
| `cargo clippy --all-targets --workspace -- -D warnings` | **exit 0, zero warnings** |
| `latexmk -pdf` (clean tree, no cached aux) | **53 pages, 0 overfull, 0 undefined refs, 0 undefined citations** |
| `python paper/src/check-provenance.py` | exit 0, `OK: 152 sources and 22 artifacts match the tree` |
| `cargo run -p sharpebench -- audit` | `All 9 attacks demoted` |

All four match the stated v0.12.0 baseline. Test totals were summed from every
`test result:` line of a full release run, not read off one line.

The three ignored tests are `sandbox::tests::docker_spawn_smoke`,
`sandbox::tests::live_hostile_probe_passes_inside_the_hardened_boundary`, and
`tests::present_shim_passes_the_preflight`, which is exactly what
`08-reproducibility.tex` says they are.

The paper build was done in a scratch copy so the repo's `main.pdf` was not touched.
`pdftotext` on the committed `paper/main.pdf` and on the fresh build produce
**byte-identical text** (1717 lines each), so the committed PDF is genuinely the
current sources.

---

# Findings, ordered by severity

## 1. MODERATE. The paper says the sandbox job "has completed twice". It has completed seven times.

`paper/sections/07-limitations.tex` and `paper/sections/08-reproducibility.tex`:

> That job has completed twice, on the last two commits of the work this revision
> documents, and both tests passed each time. [...] Two green runs a minute apart, on
> the same hosted-runner image and against the same benign POSIX fixture tag, are the
> whole of the live evidence.

and `paper/sections/A-commands.tex:162`:

> Each of the two observed runs reports 2 passed and 0 failed.

Enumerating the `live container boundary (hostile probe)` job across CI history gives
**seven successful completions**, not two:

```
61c479ac 2026-08-26T14:02:45Z success   <- HEAD
b5dd3b83 2026-08-26T13:28:50Z success
ac85b3ee 2026-08-26T13:22:57Z success
5562333c 2026-08-26T09:01:16Z success   <- v0.12.0
56ee5e5a 2026-08-26T08:57:09Z success
d62f60b7 2026-08-26T07:58:41Z success
46e5a0d6 2026-08-26T07:57:38Z success
```

The claim was already stale at `v0.12.0` (the v0.12.0 run is itself a third
execution). It is now stale by five runs in a document that was rebuilt at HEAD.

This is a number in the paper that no longer matches the artifact it names. Two
mitigating facts: the error is conservative (it understates the evidence), and the
qualitative claim built on it, "two observations of one configuration, not a
guarantee", still holds because all seven runs are the same configuration on the same
fixture. It is still the exact defect class the artifact exists to avoid.

The job itself is real and non-vacuous. Log of the HEAD run (32977860380, job
98206901870):

```
resolved fixture: alpine@sha256:14358309a308569c32bdc37e2e0e9694be33a9d99e68afb0f5ff33cc1f695dce
running 2 tests
test sandbox::tests::live_hostile_probe_passes_inside_the_hardened_boundary ... ok
test sandbox::tests::docker_spawn_smoke ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 5.04s
```

Last actual execution: **2026-08-26T14:03:18Z**, on HEAD.

## 2. MODERATE. The paper says CI has nine jobs. It has ten.

`08-reproducibility.tex`:

> the nine defined in the main workflow (`check`, `test`, `python`, `determinism`,
> `sandbox`, `audit`, `realism`, `supply-chain` and `docs`) [...] The nine jobs do not
> carry the same evidential weight.

`d2559ce` added a tenth job to `.github/workflows/ci.yml`:

```
258:  paper-provenance:
259:    name: paper evidence provenance
268:        run: python paper/src/check-provenance.py
```

The paper was rebuilt in the same commit and the sentence was not updated. The
CHANGELOG does record the new job; the paper does not. The tenth job is the one that
enforces the provenance story the appendix spends a paragraph on, so its omission from
the CI inventory is the least convenient one to have missed.

Confirmed green on HEAD: `paper evidence provenance : success (2026-08-26T14:02:55Z)`.

## 3. MODERATE. `sharpebench-memory` is a workspace member at 0.13.0 that is published nowhere, and nothing checks for that.

`CHANGELOG.md` header:

> One workspace version covers every crate, the npm packages and the PyPI package.

Registry reality:

```
sharpebench                  0.13.0
sharpebench-core             0.13.0
sharpebench-stats            0.13.0
sharpebench-edge             0.13.0
sharpebench-protocol         0.13.0
sharpebench-sim              0.13.0
sharpebench-wasm             0.13.0
sharpebench-harness          0.13.0
sharpebench-attest           0.13.0
sharpebench-leaderboard      0.13.0
sharpebench-arena            0.13.0
sharpebench-memory           crate `sharpebench-memory` does not exist
```

`crates/sharpebench-memory/Cargo.toml` carries no `publish = false`. The release
workflow's crate loop lists eleven names and the comment above it says
"publish=false members are omitted", which is false for this crate: it is omitted by
an unexplained hard-coded list, not by a manifest flag. Nothing in CI compares the
publish list against the workspace members, so a new crate is silently unpublished
forever.

`08-reproducibility.tex` counts "twelve library crates under `crates/`" and says the
benchmark is "published to crates.io, npm and PyPI". The sentence is loose enough that
it is not strictly false, but a reader counting twelve crates and going to crates.io
finds eleven. npm (`@general-liquidity/sharpebench` and `-mcp`, both 0.13.0) and PyPI
(`sharpebench` 0.13.0) are consistent with the repo.

## 4. LOW-MODERATE. `make-provenance.py` is not byte-reproducible across platforms, in a manifest whose purpose is byte reproducibility.

I regenerated the manifest and compared to the committed one.

```
committed bytes 27445  CRLF count 0    sha256 4b924c11255814b8b8b7e8e0ead4a13e181e7a48f87ed551b89b95bdfda27ff4
regen     bytes 28183  CRLF count 738  sha256 7327bb4fd6946e2efcd41b2421f0f574f7060b6646e49a36908e93a41be0d59c
```

The script's final line is

```python
OUT.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
```

with no `newline="\n"`, so on Windows Python translates every `\n` to `\r\n` and the
generated file differs from the committed one in 738 places. `.gitattributes`
(`* text=auto eol=lf`) hides this at commit time, so the working tree looks clean, but
"regenerate the manifest and diff it" does not produce an identical file on the
platform this project is developed on. Every other producer I exercised writes LF
correctly (see finding 8), so this is specific to the provenance generator.

The manifest is not self-hashed, so `check-provenance.py` cannot detect it and CI stays
green.

## 5. LOW-MODERATE. `generated_at_head` and `generated_at_head_dirty` are recorded but never validated. That half of the manifest is a gate that cannot fail.

After normalizing line endings, the regenerated manifest differs from the committed one
in exactly one field:

```
--- committed
+++ regen
-  "generated_at_head": "694db6679b2cdc7ca143e1f4ed75a0a3287e8dd0",
+  "generated_at_head": "61c479ac8f0bf1c9e959a55e5ac6f85fdf1a157a",
```

**Every source digest, every artifact digest, and `source_snapshot_sha256`
(`618eb545df056df23d692b73500a01f29bfc5edfceb29c9271b7c62ee8880da4`) reproduce
exactly.** There is no digest drift. That is the good news and it is the main thing
this audit was asked to establish.

The recorded head is two commits behind HEAD, not one. This is broader than the known
self-reference issue: `b5dd3b8` only rewrote `provenance.json` itself and `61c479a`
touched `.github/workflows/release.yml` and `npm/mcp/package-lock.json`, none of which
is inside `source_snapshot_scope`. So the source identity is genuinely unchanged and
the staleness is benign in substance.

The structural issue is that nothing checks it. `check-provenance.py` never compares
`generated_at_head` to `git rev-parse HEAD` and never fails on
`generated_at_head_dirty: true`. A manifest that openly declares it was generated from
a dirty tree at an arbitrary commit passes the `paper-provenance` CI job. Meanwhile
`A-commands.tex` advertises exactly those two fields as part of the integrity story:

> The manifest records the commit at which it was generated, whether that tree was dirty

They are recorded honestly and then never enforced. Note that the appendix does not
claim they are enforced, so this is an unguarded field rather than a false statement.

Two smaller notes on the same validator:

- The `source_snapshot_sha256` recomputation folds over `manifest["source_files"]`,
  the manifest's own records, not over the filesystem. It can only catch a hand-edited
  aggregate field, never drift. The per-file loop is what actually catches drift, so
  the check is weak rather than vacuous.
- A source file deleted from disk *and* from `source_files` passes cleanly. Only git
  history guards that.

## 6. LOW-MODERATE. The 0.13.0 release-workflow fix is real and correct, but the workaround is not covered by any test and the CHANGELOG does not mention it.

`61c479a` changed two things in the npm publish step:

```diff
-          npm install
+          npm install --package-lock=false
```

plus a corrected integrity in `npm/mcp/package-lock.json`.

Diagnosis, verified: `npm/mcp` depends on `@general-liquidity/sharpebench` at the same
release version. The committed lock pinned an `integrity` value for
`sharpebench-0.13.0.tgz` that was computed before that tarball was published, so it
could not match the registry's final tarball. npm accepted the fresh metadata (the
retry loop already waits for propagation) and then rejected the tarball against the
stale integrity, which blocked the MCP publish.

The fix is sound, not a papering-over: bypassing the lock for that one install is the
correct response to a lock that cannot possibly be right for a version that did not
exist when the lock was written. The alternative, dropping the dependency pin, would
be worse.

The committed lock is now consistent with the registry. Verified:

```
committed npm/mcp/package-lock.json integrity:
  sha512-ebEc5x30iKps5OGxDgKFfY9aG6XjARY4ZTZRdTMlYmjQSdt0PHLvVFB75KrnXYZvWw9D5RBqhA0Pkuon+tgAfA==
npm view @general-liquidity/sharpebench@0.13.0 dist.integrity:
  sha512-ebEc5x30iKps5OGxDgKFfY9aG6XjARY4ZTZRdTMlYmjQSdt0PHLvVFB75KrnXYZvWw9D5RBqhA0Pkuon+tgAfA==
```

Residual risk: the same failure recurs at 0.14.0 unless the lock is regenerated after
each publish, and the fix depends on a human remembering to do so. `--package-lock=false`
makes the MCP publish install unpinned transitively, which is a supply-chain loosening
in the publish path that is not called out anywhere. UNVERIFIED whether the next
release actually works; that needs a real tag push.

CHANGELOG: the `[Unreleased]` section is empty and this commit appears in no section.
It is a post-tag fix to shipped release machinery, so it belongs under `[Unreleased]`.

## 7. LOW. Cross-reference friction between the appendix and the recorded artifact.

`A-commands.tex` describes the external-rules field as running under
`CostProfile::None`, `CostProfile::Typical` and `CostProfile::WorstCase`. Those are the
correct Rust variant names (`crates/sharpebench-sim/src/costs.rs:183`), but the records
serialize them as `frictionless`, `typical`, `stressed`. A reader grepping the JSONL for
the names the appendix gives finds nothing. Not an error, just a lookup that fails.

Also minor: `paper/main.pdf` is not in `artifact_scope`, and the `data/*.csv.sha256`
sidecars are not in `source_snapshot_scope` while the CSVs they attest to are. Neither
is a correctness problem; both are surfaces the manifest does not cover.

---

# What is correct, and verified by execution

**The line-ending canonicalization (`955638e`) is a correct fix for a real bug, and the
committed artifacts still reproduce byte-identically.**

I checked whether the stated root cause was real. Four tracked text files have CRLF in
the Windows working tree while their committed blobs are LF:

```
CRLF in worktree:  crates/sharpebench-core/src/composite.rs
                   crates/sharpebench-core/src/lib.rs
                   crates/sharpebench-stats/src/significance.rs
                   paper/main.tex
                   paper/figures/sharpebench-luck-demotion.pdf   (binary, CRLF in blob too)
CRLF in committed blob:  paper/figures/sharpebench-luck-demotion.pdf   only
```

So the raw-byte source hashes really did differ between the dev worktree and a Linux
checkout for exactly four text files, and canonicalizing CRLF to LF for source (and
only source) is the right correction. It is validated by execution, not just by the
commit message: the `paper-provenance` job runs `check-provenance.py` on a fresh Linux
checkout and is green on HEAD.

The canonicalization is *not* masking anything. Artifacts are still hashed byte-exact
(`canonical_text` is passed only for the source group), and no committed artifact
contains CRLF except the PDF figure, whose worktree bytes and blob bytes agree.

**`external-rules.jsonl` reproduces byte-identically.** I re-ran the producer to a
different output path (about 100 seconds):

```
cargo run --release -p sharpebench-harness --example external_rules_eval -- <scratch>/external-rules-repro.jsonl
wrote 351 records

repro   bytes 329910  CRLF 0  sha256 14fd1306755457d3e16162ec983eca53e7b69f6ed000fc6e9a49875589c2a9af
commit  bytes 329910  CRLF 0  sha256 14fd1306755457d3e16162ec983eca53e7b69f6ed000fc6e9a49875589c2a9af
byte identical: True
```

This matches the digest recorded in `provenance.json` exactly. Note that the producer
writes LF even on Windows and that the output path does not leak into the records, both
of which are what determinism requires.

That hash is **not** printed anywhere in the paper. `955638e`'s predecessor removed the
quoted digest, and `A-commands.tex` now states plainly:

> No digest is copied into this appendix: the manifest is the machine-readable
> authority, which removes the circularity that arose when this source file quoted a
> hash over a snapshot containing itself.

`grep` for `14fd1306` in the rendered PDF text returns 0 hits, and no 40+ hex string
appears in any `.tex` source. The self-reference problem was solved by deletion rather
than by a stale-by-one-commit convention, which is the stronger fix. **There is no
paper-versus-manifest digest drift to report, because there are no digests in the
paper.**

**Every result number I could check against its artifact matches.**

| Paper claim | Measured |
|---|---|
| 604 passing, 3 ignored (`08-reproducibility`, `A-commands:154`) | 604 / 0 / 3 |
| 53 pages, 0 overfull, 0 undefined | 53 / 0 / 0 |
| nine frozen dataset-timeframe combinations | 9 CSVs in `data/`, 9 distinct `dataset` values in every field |
| 512 records per dataset (`A-commands:55`) | 512 x 9 = 4608 |
| thirteen-agent external field, 351 records | 351 records, 13 distinct `agent_id` |
| "refuse in all 351 cells" | `rank_eligible` true in 0 of 351 |
| three cost profiles | 3 distinct values |
| relative mandate: 81 records, one per (dataset, agent) | 81 records, 9 datasets x 9 agents |
| "refuses every entrant as well" | `rank_eligible_default` 0/81, `rank_eligible_relative` 0/81 |
| luck floor: one row per agent plus one summary per daily dataset | 2000 agent rows + 2 summary rows = 2002, datasets `crypto-majors-1d` and `us-indices-1d` |
| "leaves every zero-skill entrant ineligible" | 0/2000 eligible under configured, field and shipped-floor paths |
| "the bar sits near an annualized Sharpe of 3" | min eligible injected Sharpe 3.17 (daily-shaped), 2.52 (weekly-shaped) |
| nine claimed defenses, none regressing | `All 9 attacks demoted` |
| twelve library crates, fourteen workspace members | 12 lib crates + xtask + reference-agent = 14 members, `sharpebench-py` excluded |

**No remaining "skip that reads as a pass".** The two live sandbox tests now assert
`docker_available()` as a *failure* condition, so requesting them with `--ignored`
without a daemon fails loudly rather than passing:

```rust
assert!(
    docker_available(),
    "this test was requested explicitly with --ignored, so an absent Docker daemon is a \
     failure, not a reason to pass"
);
```

`docker_spawn_smoke`'s assertion is now on transport health
(`health.last_error == Some(DecideError::Timeout)`), which distinguishes "container ran
and stayed silent" from "container never started"; the old `orders.is_empty()` held for
both. `live_hostile_probe_passes_inside_the_hardened_boundary` asserts
`passed_checks.len() == 7` plus each named check, so a dropped check fails the test.
Both assertions are non-vacuous and both were observed to pass on real Docker at HEAD.

A repository-wide scan found no other instance of the pattern: zero `#[test]`
functions containing an early `return;` alongside assertions, and no
`eprintln!`-then-`return` skips in test code.

---

# CHANGELOG omissions (item 6)

Substantial things in `git log v0.12.0..HEAD` that the CHANGELOG does not mention:

1. **`61c479a` in full.** The npm publish workaround (`--package-lock=false`) and the
   corrected MCP lock integrity appear in no section. `[Unreleased]` is empty. This is
   a change to shipped release machinery made after the tag.
2. **The five-commit provenance churn is compressed into three bullets that read as a
   single clean change.** The history shows two manifests committed with
   `generated_at_head_dirty: true` (`7591f70`, `694db66`) and two follow-up commits
   whose only content is flipping that flag back to `false`. Nothing wrong shipped, but
   the CHANGELOG presents a settled result where the history shows four attempts.
3. **`.gitignore` gained `/crates/sharpebench-py/.venv`** in `d2559ce`. Trivial, but it
   is the mechanism by which the "excludes virtual environments" fix stays true.
4. **`sharpebench-memory` remains unpublished at 0.13.0** while the CHANGELOG header
   asserts one version covers every crate. See finding 3.

---

# What I could not verify

- Whether the **next** release actually publishes cleanly with `--package-lock=false`.
  That requires pushing a `v*` tag. UNVERIFIED.
- Whether the `paper-provenance` job would fail on a genuine mismatch. It is green on
  HEAD, and the script's per-file loop is straightforwardly correct by inspection, but
  I did not inject a mutation into CI to confirm the job is non-vacuous end to end. A
  one-byte edit to a tracked source file plus a re-run would settle it; locally, the
  same script does report `source: DIGEST` on drift.
- The `determinism`, `realism`, `supply-chain` and `python` jobs were read but not
  independently re-executed; I relied on the GitHub run records for those.
