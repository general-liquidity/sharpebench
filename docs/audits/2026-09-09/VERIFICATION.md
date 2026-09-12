# Verification record

## Current status

This record is append-only and its sections are timestamped checkpoints, not a
queue. A "Not established" list states what was true when its round closed, and
a later round may have closed an item without editing the earlier text: the
entry below dated 2026-09-11 says nothing in SharpeBench compares its committed
module against a fresh build, and Bench PR #100 added exactly that gate hours
later. Read the latest round first, and treat an earlier limit as open only if
no later round names it.

## Verifying the verification, 2026-09-12

Five pull requests, each merged with every check green on its exact pushed head
and the merged tree identical to the tested tree.

| PR | Work | Main after merge |
|---|---|---|
| Bench #109 | A scored model must have accounting evidence, whatever files exist | `1045e82` |
| Bench #110 | The roster validated on every path; a comparator's number required finite | `bb8de43` |
| Arena #62 | The blocking-event contract derived from the engine, not restated | `89af7fa` |
| Bench #111 | The census and the controls wired into the producing path | `0faa698` |

The round is worth recording for where the defects were. All four were in code
this project wrote to prevent exactly that class of defect, and three of them
were added in the two rounds immediately before. The accounting reconciliation
that published nothing, the roster that validated one path and not the other,
the control that refused a bad residual while ignoring a bad mean, and the event
table kept by hand next to the enum it was supposed to mirror. Every one passed
its own suite: nineteen assembler tests, twelve census tests, thirteen control
tests, all green while the defect held.

Two claims in this record were also wrong and are corrected in place rather than
quietly edited. G26 said an outside review read the reference repositories in
full, when that review's own coverage ledger marks most of its inventory unread,
and said every finding was closed, when the sibling product's open items were
recorded as open in the row immediately below it. A record that overstates its
own completeness is the same failure as a test that passes for the wrong reason.

The reviewer's remaining observation is also recorded and is not closed: the
sections of this file are timestamped checkpoints rather than a queue, and one
of them still says SharpeBench has no gate comparing its committed module
against a fresh build, which a pull request added hours after that section was
written. A note at the top of this file now says how to read it.

### Not established

No gateway has served a real provider and no field has completed, so the
accounting repairs are established against fabricated inputs and a read-only
inventory rather than against a bill. The economic comparator control is
deliberately undeclared, because the only buy-and-hold in this field is a ranked
entrant and binding it as a control would be the bypass the refusal exists to
prevent. The published packages still predate every repair in this round and the
two before it. The momentum style remains sampled and ungraded. The mutation
gate's four-way split is delivered and its speedup remains projected rather than
measured against a population.

## Reading the sources directly, 2026-09-11

Five pull requests, each merged with every check green on its exact pushed head
and the merged tree identical to the tested tree. Four of the five had to be
brought up to date and tested again, because each merge invalidated the next.

| PR | Work | Main after merge |
|---|---|---|
| Bench #101 | Usage evidence required per record; the published cell held unique | `c4e3f9e` |
| Bench #102 | Library types for a declared comparison axis and for a regrade receipt | `a99c0b7` |
| Bench #103 | Three claims narrowed to what the sources license | `781880c` |
| Bench #104 | An operator rescore over a declared bundle | `79e8b80` |
| Bench #105 | The trial census and the typed suite controls | `d4165ec` |

What distinguishes this round from the one before it is that the reading was
done here rather than accepted. That changed the answers. Of the paper-derived
concerns carried into it, most were already correct in the code and the useful
output was an anchor saying so. One was a defect in the review's own framing:
the differential-return question does not arise, because under the zero-rate
cash convention the benchmark series is constant zero, so the differential is
the raw series and the implemented ratio is Sharpe's ex-post definition exactly
rather than an approximation of it. Every call site was traced to establish that
no risky benchmark reaches the scalar form.

Four printed formulas in one supplied paper and one uniqueness claim in another
were confirmed wrong by independent arithmetic, and none had ever been
transcribed into either product. A source error is only a defect here if
somebody copied it, and nobody did. One paper is recorded as consulted and
correctly not adopted, its objective being an expected-utility ranking of known
distributions rather than an inferential statement about an observed track.

Three failures this round were in the checking rather than in the code, which is
the reason the isolated-cause rule keeps earning its place. A mutation that
replaced a documented scope with an overclaim failed a presence check rather
than the guard meant to catch overclaims, so it was evidence for the wrong
thing and was replaced by one that keeps every required phrase and only adds the
overclaim. A fixture tripped two rules at once and was split so each rule is
asserted alone. And a test asserting that a control refuses was rewritten to
assert which refusal it returns, because four of those refusals have a second
available cause and mere refusal proves none of them.

### Not established

No gateway has served a real provider and the field has still never completed,
so the accounting repairs remain established against synthetic inputs. The
rescore command's re-execution path has no test, since it needs a live container
daemon, though its delegates are covered. A rescore bundle binds content and not
an author, so it is not signed, and pairing it with the attestation chain is not
done. The regrade receipt takes the source digest on trust without reading a
byte, which is the value a rescore verifies, so the two compose only by hand.
The momentum style remains sampled and ungraded until its producer is rerun.

## Acting on the independent assessment, 2026-09-11

Seven pull requests, four in SharpeBench and three in SharpeArena, each merged
with every check green on its exact pushed head and the merged tree identical to
the tested tree. Four of the seven had to be brought up to date and tested again
because another of them landed first.

| PR | Work | Main after merge |
|---|---|---|
| Bench #96 | The journal's identity read once; a failed settlement refused on reopen | `fbc00a0` |
| Bench #95 | Six fail-open paths in the field assembly and the agent shim | `76fbea2` |
| Bench #97 | A p-value stated as a p-value; the conversion's assumption written down | `ebf1c40` |
| Bench #98 | The last posterior phrasing, and the module rebuilt against its source | `3fe5e4e` |
| Arena #57 | Cross-runtime fixtures for the backtest path | `6c4b9be` |
| Arena #59 | Release-mode validation, a real metadata check, ties marked as ties | `c363cd2` |
| Arena #58 | Typed refusals in grading; the empty evaluation refused | `8e0b606` |

The round is worth recording for what it did not find as much as for what it
did. Three of the five paper-derived claims were already correct in the code,
and saying so with an anchor was the whole result for them. Four of the
adjudicated Arena findings were already closed by earlier rounds. One finding in
this project's own review was rejected outright and two more were not supported,
so the corrections went into our text rather than into the code. A review that
only ever confirms is not being read.

Three defects were found by doing the work rather than by being told about them.
The statistics files name a model that has no accounting row, in a proportion,
262 of 694, that makes the omission a third of the run rather than a stray file.
The committed module had drifted three minor versions from its source while every
gate stayed green, because the gates each build their own copy and none of them
compares against the committed one. And the module moves on changes that cannot
affect behaviour: a documentation-only edit three lines up shifted twenty-two
bytes of panic location records, which is the mechanism by which the first drift
went unnoticed.

Two of the failures this round were in the checking itself, which is the reason
the isolated-cause rule keeps earning its place. A mutation removing a guard from
a spec-hash input made every test in the installed package fail at import,
because moving the hash breaks the package's pin, so three unrelated groups
appeared to be defended by one guard until each mutated copy's pins were rebound.
And a bounds test used a weight that the sum-of-weights rule refuses as well, so
it asserted an outcome two causes could produce. Both were found and stated by
the people doing the work, not by a reviewer.

One number in this round was written down before it was measured, and was wrong.
A disposition named the fingerprint its own change would produce; the measured
value differed, and the document was corrected from the build rather than the
build from the document. It is recorded because the alternative is how a golden
becomes a wish.

### Not established

No gateway has served a real provider, no paid or concurrent run has happened,
and the field has still never completed, so the accounting repairs are established
against synthetic inputs and a read-only inventory of untracked remnants rather
than against a bill. The published package bytes for Arena 0.25.0 are a
reviewer's measurement repeated here, not reproduced under this round. The
momentum style remains sampled and ungraded until its producer is rerun.
Cross-runtime fixtures cover the backtest path only. Nothing in SharpeBench yet
compares its committed module against a fresh build, which is the drift found
here and not yet closed.

## The backtest path's cross-runtime evidence, 2026-09-11

`contract/attestation/backtest-goldens.json` pins `run_baseline` and `replay_run` output
bytes, read by the native suite, the wasm32 suite and the npm suite against the committed
bundle. Before it, the cross-runtime byte-identity evidence covered scenario generation
only: the wasm32 test named for replay never called `replay_run` and compared one module
against itself, and the npm smoke suite compared replay against its own output, so a
native-versus-wasm32 difference anywhere behind the backtest path was invisible to every
gate.

Nothing numerical moved. All four entries reproduce byte for byte on the host build, on a
freshly compiled wasm32 build and through the shipped `pkg/sharpearena_bg.wasm`, and no
existing golden, snapshot or artifact digest changed. Each new test was mutation-checked
by perturbing in place the exact behaviour it defends and restored from `HEAD`.

### Not established

No cross-runtime arithmetic mismatch was demonstrated, before or after. This round closed
an evidence gap and repaired no number. Coverage is the backtest path only: `walk_forward`,
`stress_suite` and `tag_regime` still have no committed cross-runtime fixture, and the
four entries are four inputs rather than the export's whole domain, so a divergence
reachable only by an input outside them stays invisible in the same way.

### The published 0.25.0 wasm bytes differ from the committed ones

Recorded here because it was measured and because nothing in the repository records it.
An independent reviewer fetched the published npm 0.25.0 artifact from the registry and
hashed its WASM: SHA-256 `f50a527b71c97e37f59b5f577baf35a6582eea0a687ed61d80ae90a30bfd4ca8`,
against `7e3d5faea27be55b6d566953d19c93b633d093ff01a79660c0979467a30482e4` for the bytes
at the `v0.25.0` tag. That is what the release job did at the time, since it deleted
`npm/sharpearena/pkg` and republished a rebuild; the job no longer does so (A4).

The two digests are not on the same footing here. The tag side was recomputed in this
repository, `git show v0.25.0:npm/sharpearena/pkg/sharpearena_bg.wasm | sha256sum`, and
agrees. The registry side is the reviewer's measurement, repeated rather than reproduced;
no fetch from the registry happened under this round. The committed bundle on `main` is a
third value again (`3bd54950af9360b2229a07eb6831ec9477feb49b3d507f74b1dcdb1dcc13d28d`),
because the unreleased `SPEC_HASH` move rebound it, which is expected and is not part of
this record.

What this does **not** establish, and must not be read as establishing: that the published
artifact is numerically wrong. In the same check it reported the expected spec hash and
reproduced both committed scenario goldens. Differing bytes are not evidence of divergence,
for the reason the A4 disposition gives at length. Nor does one measured release
generalize. Releases before 0.25.0 were not fetched or hashed, so nothing is claimed about
them.

Two further measurements narrow what the difference can be, and neither closes it. The
release recipe (wasm-pack 0.15.0, Rust 1.96.0, `wasm-pack build crates/sharpearena-wasm
--target nodejs --out-name sharpearena`) reproduced all five committed `pkg/` files byte
for byte on an unmodified parent commit locally, so the build is not irreproducible as
such. And CI, at the same pinned toolchain on `ubuntu-latest`, produced a bundle that
agreed on the spec hash, the crate version and both scenario goldens and still differed in
bytes, which is why the gate asserts behavioral equivalence rather than byte equality.
Together they say the byte difference is a property of the build environment rather than
of the source, which is narrower than "not reproducible" and still not an account of the
published artifact: no build environment has been shown to produce the published bytes,
and none was tried.

What remains open is the narrower thing: for 0.25.0 there is no evidence that the bytes on
the registry are a compilation of the tagged source, as opposed to a compilation that
answers like it on the inputs that were checked. Byte equality would have established it
and cannot hold across hosts. The forward repair is that the published artifact is now the
committed one and the suite runs against it in the same job immediately before publish, so
the question does not arise for the next release; it is not retroactive, and 0.25.0 is not
being republished to make it so.

## Making the stated facts checkable, 2026-09-11

Both pull requests merged with every check green on the exact pushed head, main
unmoved since that head was tested, and the merged tree identical to the tested
tree.

| PR | Work | Main after merge |
|---|---|---|
| Bench #91 | The gateway chapter's test evidence recounted and gated | `d367096` |
| Bench #92 | One rate card behind the shim and the assembler | `360964d` |

Neither closed a live defect. The pricing tables agreed when they were checked,
and the chapter's stale counts misled a reader rather than mispricing a run.
What both closed is the same weakness the round kept finding in a different
register: a fact the project asserts that nothing compares against the code, so
it is true only until someone edits one side.

Two details are worth keeping. A drift gate must name which side moved, because
"the two disagree" is an outcome either side produces, so both gates classify
against a baseline revision and were demonstrated by mutating each side alone.
And a number belongs in a document only where it is a well-defined measure: the
arena sandbox holds two gateway tests among thirty-eight, so counting that file
would have placed a silently different measure in the same column, and those two
tests are named instead.

### Not established

Nothing changed here about what the gateway has actually done. No provider has
been served, no concurrent or paid run has happened, and the limits recorded in
the preceding rows still stand: cross-directory aliases, two byte-identical
journals in one directory deriving one identity, and a version check that
remains read-then-write.

## Closing the fail-open paths, 2026-09-11

Every pull request below merged with all checks green on its exact pushed head,
main unmoved since that head was tested, and the merged tree identical to the
tested tree.

| PR | Work | Main after merge |
|---|---|---|
| Bench #87 | The retry check on every path to a provider request | `a3fb777` |
| Bench #88 | Pre-identity journals owned; the version check's role named | `6e19fa2` |
| Bench #89 | An unpriced model refused; a replay screened by the identity rule | `3d3232c` |

The shape these share is worth naming alongside the isolated-cause rule above.
Each was a property the project publishes that quietly did not hold on some
input: a retry setting checked on one route to the provider and not another, a
cost reported as zero for a model the table did not know, a rate card chosen by
a prefix that also matches a different model, and an identity rule applied when
a decision is written but not when one is replayed. None of them failed loudly.
Each produced a plausible number or an accepted answer instead.

Two decisions in this round were settled by measurement rather than by
argument, and both went against the obvious answer. Locking a journal file
directly, to key ownership on the file rather than its name, was probed against
the operations saving actually performs: it prevents the sole owner from reading
its own version, and protects nothing past the first save, because the rename
that makes a save durable replaces the entry with a different file. And a lock
in a shared namespace was rejected because such directories are swept by age on
many hosts, so a long run's held lock can vanish and silently readmit a second
writer for every journal at once.

### Not established

No gateway has served a real provider, no concurrent or paid run has happened,
and every crashed holder in these tests is simulated. Cross-directory aliases
remain open, as do two byte-identical journals in one directory deriving one
identity, and a version check that is still read-then-write. The shim and the
assembler now carry two pricing tables stating the same rule, which a future
edit could desynchronize; that is recorded rather than prevented.

## Verifying the repairs, 2026-09-11

Every pull request below merged with all checks green on its exact pushed head,
main unmoved since that head was tested, and the merged tree identical to the
tested tree. Two branches were brought up to date and tested again before
merging, one because main had moved and one because its base had been merged as
a merge commit, so main was not an ancestor of it.

| PR | Work | Main after merge |
|---|---|---|
| Bench #83 | Model identity, call ledger, malformed cost | `183a457` |
| Bench #84 | Adversarial review of the G22 repairs | `cf943c3` |
| Bench #85 | Journal owned by its document; misdiagnosis and hermeticity | `50da2cc` |

The review was commissioned because work called done had twice been shown
defective by an independent read, so the same was assumed here. It found two
medium defects in repairs that had merged hours earlier, and a fourth instance
of the pattern recorded above: `a_takeover_is_explicit_and_records_who_it_displaced`
asserted that a takeover records who it displaced by comparing the recorded
process id against this process's own, which the taker satisfies by writing its
own id. Mutating the recorded value left the library suite green. It is isolated
now by writing a displaced document whose process id is asserted to differ from
this one, and by asserting the recorded timestamp as well.

Two repairs in this round rejected the obvious approach on measurement rather
than on argument. Canonicalizing a journal path does not resolve hard links,
produces verbatim paths on Windows, and fails for a journal that does not exist
yet; ownership keys on an identity inside the document instead. And the first
ledger exclusivity test asserted an outcome three causes could produce: it
passed against a build with the ceiling removed and failed only four runs in six
against a build with the exclusivity removed, so it was replaced with a case
where nothing is short of budget and exclusivity is the only thing that can
refuse.

### Not established

No gateway has served a real provider, and no concurrent or paid run has
happened. Every crashed holder in these tests is simulated rather than a real
crash, no second host and no network file system was involved, and the
concurrency evidence is threads within one process on one file system. Three
aliasing routes remain open and are stated in the type documentation: two
directory entries for one document in different directories, journals written
before the document identity existed, and a version check that is still
read-then-write rather than an atomic swap.

## Money-accounting repairs, 2026-09-11

Both pull requests merged with every check green on the exact pushed head, main
unmoved since that head was tested, and the merged tree identical to the tested
tree.

| PR | Work | Main after merge |
|---|---|---|
| Bench #81 | One writer per money journal; settlement fails closed | `1018445` |
| Bench #80 | Call ceiling bounds provider requests | `d46d2e6` |

The journal lock is evidenced by eight real threads released together by a
barrier, all opening one path: exactly one succeeds, seven receive typed
refusals, and the admitted writer's call is afterwards whole on disk. The
assertion is winner-independent, so it rests on no scheduling assumption.

### Three tests that would have passed while the thing they name was not what refused

This round produced one finding worth more than the three repairs. In three
separate cases a test asserted an outcome that several independent causes could
produce, so the assertion said nothing about the cause it was named for until
that cause was isolated.

- **The settlement latch.** The regression sabotaged the write by leaving a
  directory at the journal path. A gateway with no latch was then refused anyway
  by its next reservation's write, so deleting the latch left the suite green.
  Isolated by restoring the journal byte for byte before the second request, so
  only the latch can refuse it.
- **The retry setting.** A test asserted the setting on a client it built by
  calling the helper directly, so reverting the run to a bare client survived:
  nothing pinned the client the run actually constructs. Isolated by driving the
  entry point with no client of its own and asserting on the constructor's
  arguments.
- **The runtime guard.** The first stand-ins for a client that accepts the
  retry setting and ignores it had no `messages.create`, so deleting the guard
  was caught by an incidental attribute error from the un-refused run rather
  than by the guard failing to fire. Isolated by subclassing the working
  stand-in so both answer normally and the guard is the only thing that can
  raise.

Two of the three were self-reported at staging by the agents that wrote them;
the third was found on a re-run. None was caught by a gate.

The same lesson in another register: two mutated constants sat on disk inside a
worktree that two processes were driving, and neither reached a commit. What
stopped them was explicit-path staging, and checking the committed tree
afterwards rather than trusting the working copy. That is also why every commit
on a branch was checked for the mutated construction rather than only its head.

### Not established

No gateway has served a real provider, and no concurrent or paid run has
happened. The lock is exercised by threads in one process against one file
system, which does not cover two hosts sharing a journal over a network file
system, where `create_new` is only as exclusive as the remote server makes it.
The call ceiling's guarantee is conditional: either the retry setting binds on
the client the run built, or the run refuses to start. It is not a claim about
any SDK version, and the semantics of the next major version were not read and
must not be assumed from the evidenced one.

## Remaining gaps, 2026-09-11

Every pull request below merged with all checks green on its exact pushed head,
main unmoved since that head was tested, and the merged tree identical to the
tested tree. A branch that fell behind main was brought up to date and tested
again before it merged.

| PR | Work | Main after merge |
|---|---|---|
| Bench #73 | Header cross-check; commitments bind the fault plan | `0eae8e8` |
| Bench #74 | LITE `sr_benchmark` annualized | `91d5d0e` |
| Bench #75 | npm finiteness checks; MCP lockfile | `84047ae` |
| Bench #76 | Incomplete-sweep fault report; external capture; image re-execution | `b830142` |
| Bench #77 | `sharpebench commit --fault-plan` | `bff54c0` |
| Release | SharpeArena v0.25.0 | `f520d61` |
| Release | SharpeBench v0.22.0 | `a0f0a42` |

Unfaulted commitments were shown byte-identical by pinning three hashes printed
by the main binary, and the committed `arena/` round-trips unchanged. Default
LITE verdicts were compared against main across 150 CLI runs, 422 WASM outputs
and 542 Python calls. The CLI changes were compared against main across 35
commands with only host durations and the runner hash masked.

Two defects surfaced as side findings. The WASM module committed on main had
been built before the 0.20.0 version bump and stamped `sharpebench-stats/0.19.0`;
CI rebuilds the module before testing, so only a local run against the
committed file showed it, and Bench PR #74's rebuild corrected it. And
`npm ci` failed in `npm/mcp` because its lockfile still resolved the kernel at
`^0.15.0`; the release job installs without the lockfile, which is why no
release had failed on it.

The SharpeArena release passed its registry check on the first run. Its
GitHub deployment records show `success` through the API; SharpeBench's
latest npm and PyPI records were marked inactive at 20:40 and 20:44 UTC on
2026-09-10 by a status change this work did not make, while every registry
serves the released versions.

All three SharpeBench releases in this record needed a rerun of the registry
check, each for registry lag rather than a failed publish: crates.io on v0.20.0,
and npm on v0.21.0 and v0.22.0. The v0.22.0 MCP publish itself went through on
the first attempt, because Bench PR #71 made it wait for the kernel's tarball;
only the final check, which waited 100 seconds for npm and did not retry
crates.io at all, ran too early. It now polls each registry for up to ten
minutes, Bench PR #79.

The live image re-execution test proves the hardened launch and typed refusals
against a real daemon, not a passing re-execution: the pinned Alpine fixture's
`/bin/sh` entrypoint does not speak the decision protocol. A passing
re-execution is covered with a fake sandbox.

## Follow-ups and releases, 2026-09-10

Every pull request below merged with all checks green on its exact pushed head,
main unmoved since that head was tested, and the merged tree identical to the
tested tree. Post-main CI passed after each release.

| PR | Work | Main after merge |
|---|---|---|
| Arena #44 | Paired-test prose | `9fb589f` |
| Bench #65 | Gateway chapter, evidence inventory, verdict wording | `95aed66` |
| Bench #66 | CLI fault plan, backoff, re-execution; protocol relaxations | `ba89ea4` |
| Release | SharpeBench v0.20.0 | `fa9525c` |
| Bench #67 | OOM verdict from the exit code | `23276af` |
| Arena #45 | Pin SharpeBench 0.20.0 | `dfd807e` |
| Bench #68 | Fault plan digest in window identity | `ce2691b` |
| Bench #69 | Live gateway and allowlist tests; Docker init-layer entries | `875c6bf` |
| Bench #70 | Opt-in Sharpe diagnostics | `ae25dbb` |
| Release | SharpeBench v0.21.0 | `f040493` |
| Arena #46 | Pin SharpeBench 0.21.0 | `dea7068` |
| Bench #71 | npm publish waits for the tarball | `4f09f2c` |

The live out-of-memory probe had failed four times on branches that did not
touch the sandbox. Recorded as a flake on its first failures, it was then traced to source: moby
28.0.4 sets `State.OOMKilled` only from containerd's `TaskOOM` event, which
containerd 2.3.4 publishes asynchronously and can drop when `memory.events` is
already gone or requeue after the exit (containerd #8893, open). The exit code is
recorded with the exit itself, and nothing inside the hardened launch can
SIGKILL the entrant, so an exited container with code 137 is now a breach.
Which loss path hit CI is not established; no containerd log was captured.

The first live run of the image allowlist found that it could never pass: a
container export always holds `.dockerenv`, `dev/console`, `dev/pts/`, `dev/shm/`
and `etc/resolv.conf`, and replaces `etc/hostname`, `etc/hosts` and `etc/mtab`,
none of which an image's own allowlist names. Those entries are now admitted
only as the empty files, directories and `/proc/mounts` link Docker creates, and
the live test fails if a daemon adds anything else.

The v0.20.0 registry check failed once because crates.io had not yet indexed
`sharpebench-wasm` when the check ran; the crate was already published, and a
rerun of that job alone passed.

The v0.21.0 npm job failed twice: npm served the kernel package's metadata
minutes before its tarball, and the MCP step's 150-second wait on metadata alone
first hit ETARGET and then E404 on the tarball. Both packages published on a
rerun once the tarball was downloadable, and the registry check passed on its
own rerun. Bench PR #71 makes the step wait up to 20 minutes for the tarball as
well as the metadata.

The opt-in diagnostics reproduce the 2026 paper's worked standard errors (0.379
and 0.214) and PSRs (0.966 and 0.900), and at zero autocorrelation match the
kernel's PSR bit for bit. Their implementation corrected one statement in the
literature audit: the observed-Sharpe and null-evaluated standard errors
coincide only when the observed Sharpe equals the benchmark.

## Port build and literature corrections, 2026-09-10

Six pull requests merged into SharpeBench and one into SharpeArena. Each had
every check green on its exact pushed head, main had not moved since that head
was tested, and the merged tree was compared against the tested tree and found
identical. A branch that was green on an older main was brought up to date and
tested again before it merged, rather than merged on its earlier result.

| PR | Row | Main after merge |
|---|---|---|
| Bench #58 | G18, fault injection | `c256adc` |
| Bench #60 | G19, literature corrections | `afd0be4` |
| Bench #61 | G11, G18, serving loop and image hygiene | `aba678e` |
| Bench #59 | G18, contract ports | `23efd4c` |
| Bench #62 | G19, LITE verdict units | `5eb826c` |
| Bench #63 | G19, remaining unit defaults | `277f733` |
| Arena #42 | G19, deflation input refusal | `21f9a6d` |

What the gates caught that the authoring work missed.

The mutation gate found nine surviving mutants in the contract ports: the
visibility audit could return an empty report, the seal report's emptiness
could ignore two of its three lists, an array that grew an object inside a
visible field was never tested, and the operation preimage could be replaced by
any constant because its only pin lived in another crate, which the protocol
crate's own mutation run does not execute. Tests now kill all nine, confirmed by
a targeted local run in which all thirteen mutants of those functions were
caught.

The paired-boundary gate rejected the shared annualized-to-per-period
conversion, which documents a finite, positive frequency and had no boundary
test. A test now pins it at one period a year, an infinite frequency (a zero
dispersion, the most favourable bar, which is why callers must refuse it), zero
and a negative value.

The macOS build of the Arena correction failed a pin that had been recorded on
Windows: the old estimator's deflated Sharpe differed by two units in the last
place between the two platforms' math libraries. The recorded constants are now
compared within sixteen units, while the comparison against the pinned
SharpeBench kernel, which runs on the same platform as the code under test,
stays exact.

The fault injector and the contract ports both rewrote the same retry driver.
The textual merge interleaved the two loops and was discarded; the driver was
rebuilt from the backoff version with the fault record added, so each attempt
carries both its injected faults and its scheduled backoff, and the full suite
passed on the combined tree before it was pushed.

One live-container run failed on the cgroup out-of-memory probe on a branch
that does not touch the sandbox, as it did once in the completion round. The
next run, on the same branch after main was merged in, passed. It is recorded
again as an observed flake in a timing-sensitive live probe, not as an
explained one.

Not established by this round: the gateway sandbox launch and the image
allowlist probe have not run against a live Docker daemon, and the allowlist
will likely need the files Docker adds to a container once it does; the fault
plan, backoff schedule and re-execution check are library calls with no CLI
flag yet; and the paper evidence was not regenerated, so the corrected prior
and the proposed serial-correlation term have no new measured result.

## Independent verification, 2026-09-10

A separate verification of the completion round confirmed the repository state and
the green CI, and refuted the claim that every implementation and safety property
was finished. Each finding below was re-established against source before it was
accepted, and the lifecycle finding was reproduced against the current library.

| Finding | Disposition |
|---|---|
| A keyed retry marked an unresolved write answered, so a later blind retry did not block | Repaired, PR #54, main `fbfd9cf` |
| The reservation bounded message content, not billed input, and settlement accepted the overshoot | Repaired, PR #55, main `2062444` |
| Two gateways on one journal each spent the same allowance | Repaired, PR #55 |
| The broker enforced no deadline and its bounds read as enforced | Late answers now refused; read timeout and byte cap stated as adapter obligations, PR #55 |
| The gateway reports configuration and nothing serves the entrant pipe | Accepted. G11 is marked partial, not repaired |
| The hosted field's call ceiling counted cached successes, not dispatches | Repaired, PR #56, main `7fe5a06` |
| An unknown local dataset selector published an empty field as complete | Repaired, PR #56, main `7fe5a06` |
| Zero bootstrap draws returned a zero-width interval rather than unavailability | Repaired, PR #56, main `7fe5a06` |

The verification also withdrew two claims made in this record. The tree-equality
claim was false for PR #50, as recorded above and in G16. And the special-function
measurement's grid counts, maximum errors and zero-verdict-change figures are
narrative: the measurement scripts, raw grids and comparison logs were not
committed, so those numbers are not independently reproducible from this tree. The
rejection of the migration does not depend on them, since it rests on the
three-platform CI result, which is reproducible.

Two process facts from the repair work bear on how far local results can be
trusted. The mutation gate caught a third untested boundary in the ambiguity repair
itself: resolving one subject's intent would have closed every open chain. It
caught a fourth in the interval repair, where no test fixed the resampled bounds
for a known seed, and then a flaw in the first attempt to close it: with seed 42
the XOR and OR mutants of the stream constant produce the identical stream,
because 42 shares no set bits with the constant's low byte, so the pin could not
tell them apart. The committed pin uses a seed that overlaps the constant. And a
local run reported two false evidence-coverage failures because two worktrees were
compiling into one shared Cargo target directory at once, a setting introduced that
day to save disk. With an isolated target the same tree passed all 662 affected
tests. CI builds fresh and stays authoritative; a local run taken while another
build shares its cache is not.

## Completion round, 2026-09-10

Eight pull requests merged into SharpeBench, each with its relevant checks green
on the exact pushed head and post-main CI green afterwards. The merged tree was
compared against the tested tree for seven of them. The comparison was skipped
for PR #50, whose merge sits 707 additions away from its tested head; see the
correction below.

| PR | Row | Main after merge |
|---|---|---|
| #45 | G02, G03 | `863b5e2` |
| #46 | G11, G12 | `0575972` |
| #44 | G07 | `e293093` |
| #48 | G05 | `072fdb4` |
| #47 | G17 | `35f60f5` |
| #50 | G10 | `3edf6a0` |
| #51 | G13 | `fde2024` |
| #52 | G14, G15 | `1dabf2d` |

Verification notes that bear on how much these results establish.

The live Docker preflight could not be exercised locally: the daemon on this
host answered long enough to measure `Config.Volumes` in its three forms and
then wedged, so the leg was wired into the live-container CI job by exact test
name and ran there against the pinned Alpine fixture. Never run all ignored CLI
tests wholesale, because a subprocess fixture requires an environment mode.

The mutation gate caught two surviving mutants that the authoring work missed,
one per pull request, and both were killed by tests rather than by weakening the
gate. In the selection refusal path no test used an alpha exactly equal to the
recommended floor, so widening the comparison survived; the floor is a minimum,
so an alpha sitting on it must not warn, now asserted on both the refusal and
the accepted path. In the lifecycle check no trace recorded an unobserved
acknowledgment against an order whose acknowledgment had already been observed,
so dropping the stage guard survived; an acknowledged order has an outcome and
cannot become ambiguous. Each was verified by mutating in place, observing the
failure, restoring from the committed tree and comparing byte for byte.

The statrs question was settled by CI rather than by argument. The migration was
implemented, pushed and tested precisely so the reproducibility claim could be
falsified: its regenerated goldens pass on Windows, where they were generated,
and fail on Linux and macOS, while the same three jobs on main with the
hand-rolled bodies pass everywhere. The accuracy measurement that motivated the
migration stands and is preserved; only its conclusion changed.

One live-container failure on the parity pull request was a flake in the cgroup
out-of-memory probe, not a regression: main had passed the same job shortly
before, and a rerun of the failed job alone passed. A rerun is not a diagnosis,
so this is recorded as an observed flake in a timing-sensitive live probe rather
than as an explained one.

Not established by this round: no empirical field was run, because this
environment holds no provider credentials; mdBook is not installed here, so the
book was checked by link script and the CI leg remains the real gate; and the
frozen paper evidence was neither regenerated nor reproduced, since the current
tree already diverges from that snapshot for reasons that predate this work and
sit in the simulator rather than in the statistics.

## Producer paths for the transfer boundaries, 2026-09-12

A third reviewer accepted the census and suite-control wiring and named three
types with no caller outside their own tests. Two are now wired and the third is
described as what it is rather than left as a claim.

`sharpebench compare` is the producer for `ComparisonReceipt::declare`. Five
cases run through the real binary against checkpoints written by
`SweepCheckpoint::save`: two arms differing only in the entrant declare a
comparison and the receipt names the five identities held fixed to reach it, an
off-axis dataset difference refuses naming the field and both digests and emits
no receipt, an undeclarable axis is a usage error that emits nothing, an arm with
no contract refuses and names itself, and three declared axes over the same pair
leave both checkpoint files byte-identical.

`SuiteControlEvidence::binding` is the producer for the coverage
`SUITE_CONTROL_INVENTORY` declares. Six cases: the coverage statement published
is the inventory's own rather than a second list, a changed outcome moves the
suite digest while the control that did not change keeps its own, the three
non-finite residuals bind to three different digests, reordering or dropping a
control moves the suite digest, the excluded prose line moves none of them, and
the preimage itself is read rather than inferred from a digest moving. The CLI
leg drives `run --json --suite-evidence` through the binary and checks the digest
is published beside what it covers and what it does not, is stable across two
runs, and leaves the board under the envelope byte-identical to the plain
board's.

Eight isolated mutations are caught, each mutated in place on the line it
defends, run, restored with `git show HEAD:<path>` and compared with `cmp`:
collapsing negative infinity onto infinity in the number renderer; dropping the
shortfall values from the preimage; hashing the control identity instead of its
preimage into the suite digest; binding the excluded prose line in place of the
control identity; recording the binding as used by the gate; accepting an
undeclarable axis as the entrant axis; exiting zero on a refused comparison; and
declaring the two arms the other way round. Each run leaves a passing control in
the same file, so nothing passes by emitting nothing. The restored sources are
byte-identical to the committed tree.

Not established by this round. `regrade_submission` still has no production
caller and is not wired: a rescore's published figure comes from
`verify_trajectory_strict`, whose per-run decision-count check is exactly the
condition `RegradeRefusal::FabricatedDecisions` names, so composing the two would
either recompute the submission a second time and discard it or move the number
onto a second replay path, and the receipt's `original_evaluator` is historical,
so a bundle that does not record it could only have it supplied by the operator,
which replaces one trusted input with another. The source digest
`RegradeRequest` takes on trust is still the value a rescore verifies, and the
two still compose only by hand. The 2026-09-09 row that read "every regrade
linked to its source" is corrected above. No empirical field was run and the
frozen paper evidence was neither regenerated nor reproduced.

## Initial review and isolation

- Baselines: Bench `5c4cfb2` (v0.19.0), Arena `4fdf672`.
  Both exact main heads had successful CI. Bench PR #39 at `bedbfb8`
  had 23 successful checks and remained unmerged.
- The two independent reviewers' ten distinct findings are in [AUDIT.md](AUDIT.md).
  Their coverage limits remain part of that report.
- Work is on separate `fix/verified-followups-2026-09-09` branches.
  Existing main checkouts, unrelated branches and historical evidence are preserved.
- The old Linux Bench binary and Windows Arena extension were stale.
  Tests below use fresh Linux builds. The current Python extension was rebuilt
  with maturin in an isolated virtual environment, then separately built as a
  wheel and installed into a fresh consumer environment. The 91 affected Python
  tests passed there, with imports verified to come from site-packages.
- PR #39's history was merged into the Bench repair branch without cherry-picking.
  Its four special-function test functions pass; no statrs dependency or
  replacement has been added. Its universal claim about other libraries'
  inability to compute empirical moments was narrowed.

## Red-to-green regressions

| Finding | Failure before repair | Local verification after repair |
|---|---|---|
| F01, computed statistical overflow | Both new stats overflow tests failed; the full verdict emitted `[true,true]` instead of withholding rejection. | All stats and edge tests pass. A failure in one family member now withholds the entire snooping family. |
| F02, resolved forecast rewrite | All four Arena cases accepted changed prediction, identity, rationale or exposure despite unchanged sealed files. The Bench importer also accepted three rehashed resolved-field rewrites. | Arena prospective suite: 20 pass. Importer suite: 6 pass including subcases. The committed historical field still verifies without changing its files. |
| F03, replicate width | Keyed score reported 160 pooled observations instead of 80 effective observations. | Automatic width equals explicit width 2; widths 1 and 3 refuse. |
| F04, missing date axis | Retained periods were `[]` rather than `[d1,d3]`. | Each column retains its observed dates; equal-length mismatched dates refuse, aligned dates accept. |
| F05, unsupported confidence | Eight support/resampling tests and nine parameter/baseline tests failed before repair. | Rust and rebuilt Python refuse unsupported requests. A single-seed baseline preserves its point but withholds the interval; duplicate IDs refuse. |
| F06, unavailable baseline rank | Six error-key/confidence-mode cases published a float instead of the reason. | All three typed errors survive with confidence on or off; unavailable rows have no numeric rank and do not enter paired comparisons. |
| F07, invalid candidate selection | The empty candidate won against negative observed returns with no error. | Empty, single-observation, nonfinite and overflowing candidates withhold the whole selection; valid-input and append-stability tests pass. |
| F08, v2 import | Both supported v2 labels were rejected; v1-with-label was accepted. | Both supported labels accept under v2; missing, unknown and v1 labels refuse. |
| F09, numeric settlement identity | Comparing `1` with `1.0` failed as unequal settlement. | Integer/float and signed-zero pairs compare; opposite realized outcomes still refuse. |
| F10, unexplained rejection | A real invalid-CI configuration rejected a strong field without the expected statistical reason. | Deflation, bootstrap and selection errors have stable serialized labels and appear in rollups. |
| F11/F12, installed npm behavior | The new regression first failed on the old WASM's missing statistical disqualification, then on the rebuilt WASM's error discarded by the old wrapper. | Repaired wrapper and rebuilt WASM pass all 20 npm tests and the offline installed-tarball probe. Final rebuilding remains required after further numerical edits. |
| G09, discarded attempt summary | The real-CLI loopback regression failed on an isolated copy of the pre-change tree because incomplete-sweep JSON omitted accounting. | The repaired CLI reports 48 failed attempts for 16 exhausted cells, monetary cost unavailable, and no board. Two unit tests also pin unknown duration and unchanged score/order/reference rows. |
| F13, hidden deflation overflow | Three new regression functions failed against an isolated pre-fix tree: finite returns or parameters produced Ok(NaN), Ok(infinity), or a saturated numeric fallback. | Four regression functions pass, including finite valid-input bit comparisons against the previous scalar PSR and existing short/constant-series behavior. |

These regressions use synthetic inputs and existing artifacts. No model was
downloaded or called, no API credits were spent, and no market data was acquired.

## Local commands and results

Bench:

- `cargo test -p sharpebench --locked`: 63 tests passed for the initial
  seed/date repairs. A subsequent full workspace run (excluding xtask) passed,
  with 14 ignored tests recorded rather than counted as executed.
- `cargo test -p sharpebench-stats -p sharpebench-edge --locked`:
  102 stats unit tests, 10 statistical-boundary tests, 4 compatibility test
  functions, 31 edge unit tests and both doc tests passed.
- `cargo test -p sharpebench-core --locked --quiet`: 330 unit tests,
  37 integration tests and one doc test passed.
- Affected-package clippy with warnings denied passed.
- npm build, 20 tests, offline tarball installation, and the MCP build plus
  nine tests passed against the rebuilt sibling package. All were repeated
  successfully after F13 and the final WASM rebuild with wasm-bindgen 0.2.126,
  matching Cargo.lock.
- `python3 -m unittest paper/src/test_import_prospective_field.py`: 6 tests passed.
- `python3 scripts/check-paired-boundaries.py`: passes with its existing
  24-entry allowlist. A green result is not complete boundary coverage.

Arena:

- `cargo test -p sharpearena --locked --quiet`: 161 unit tests and 25
  integration tests passed.
- Workspace clippy with warnings denied passed.
- Rebuilt the native Python extension after the Rust confidence API change.
- Baseline, confidence and prospective-field Python suites: 91 tests passed.
- The existing 3-agent, 24-contract historical field still verifies.
  Bench's importer also verifies its 15-file committed source inventory at
  Arena `4fdf672`.

## Delivery and remaining limits

### Explicit runtime recovery

The bound checkpoint driver now saves each observed attempt before a later
attempt can start. Four integration tests cover recovery eligibility, retained
history, lifetime and per-round retry budgets, interrupted claims, changed
contracts and malformed runtime states. A unit test distinguishes identical
fresh executions from a replayed ledger batch. The real CLI loopback test
reports 48 attempts initially, 48 on default resume, and 96 after explicit
recovery; all cells remain runtime-failed, with no board. Invalid recovery
flag combinations refuse before launch.

All four integration tests and the eight existing ledger tests pass, as do
45 harness unit tests and affected-package clippy. Six isolated mutations
fail: disabled recovery, bypassed lifetime ceiling, deduplicated fresh
executions, reset per-round budget, omitted attempt persistence and erased
prior history. The restored integration target passes. A process killed in
an unobserved attempt can still leave incomplete accounting; no monetary
measurement or immutable-checkpoint claim is made.

### Delivery history

The repairs are granular commits. Arena PR #35 merged as `f7614dc`; its tree
matches tested head `f5939a9` and post-merge CI passed. Its first CI run caught
formatting in the separately excluded PyO3 crate, which was fixed and rebound.

Bench PR #40 is now merged. At its earlier pushed head `5618a46`, the ordinary workflow,
package checks, live Docker probe and three-platform matrix passed. Mutation
testing reported 73 mutants: 60 caught, three unviable, ten missed. The missed
SPA arithmetic mutations require stronger tests; the check is not bypassed or
explained away as a runner flake. Subsequent local changes need a new pushed
head and fresh CI, which are recorded below. CodeRabbit skipped review while the PRs were drafts.

The three new tests in stats/tests/spa_studentization.rs are independently
checked by spa_reference.py. That reference uses rational means, variances and
squared positive-statistic comparisons; only consistent-SPA exclusion uses
floating log/sqrt. All three fixtures have no ties at the observed statistic.
The reference is a numerical cross-check, not evidence of nominal test coverage.
Replaying the ten exact CI-missed arithmetic mutations in an isolated source
copy now produces ten failing test runs. Restoring the source passes all three
tests. This local replay does not replace the next complete CI mutation run.

At head 1f286db the complete mutation run caught the prior SPA gaps, but ten
mutations in the newly added checked-PSR helper survived (115 caught, three
unviable, ten missed). The valid-input compatibility fixtures were centered or
constant, so several moment terms vanished. Adding a two-observation case and
an asymmetric nonzero-mean case catches all ten exact mutations locally; the
unmodified four-test target passes. Only tests changed in this follow-up, not
the numerical implementation or WASM artifact. The subsequent complete CI run passed.

Final package rebuilding and installed consumers passed. Bench PR #40 merged
as `4b3cc0d`, tree-identical to tested head `ecdcea2`. Its complete mutation
run reported 125 caught, three unviable, zero missed and zero timed out.
Post-main CI and npm runs 34403159826 and 34403159785 succeeded. PR #39 was
closed as merged by the retained ancestry, not by discarding its changes.

Bench PR #41 merged as `4a453d0`, tree-identical to tested head `eb8d1a4`.
All 23 checks passed; post-main CI 34404055056 and npm 34404054949 succeeded.
Arena documentation PR #37 merged as `1ec75cb`, tree-identical to `110188a`;
post-main CI 34403517298 succeeded. None of this is a release.

### Frozen token-rate accounting

Seven harness regressions cover the strict card schema, exact integer quotes,
identity changes, overflow, missing and mixed usage, observer transparency,
failed-attempt retention and checkpoint recovery. Two new CLI tests use a
loopback fixture, not a model provider. A 240-decision run produces exactly
690000 USD nanodollars under the synthetic card; resume makes no further
calls and reproduces its accounting. Changing the card refuses before another
request and leaves the checkpoint unchanged. After removing accounting, the
priced and unpriced boards are identical. Missing usage emits no total.

Seven isolated mutations are caught: substituting the input rate for output,
accepting mixed cards, treating absent attempt usage as free, treating failed
usage as complete, accepting invalid reasoning counts, dropping failed usage,
and removing the card from the CLI checkpoint identity. Restored controls pass.
The 45 harness unit tests, eight existing ledger tests, four recovery tests,
48 CLI unit tests and both existing CLI recovery tests also pass. Affected
clippy passes with warnings denied. CI and merging of this new card work
remain pending.

The first PR #42 macOS run failed in the loopback fixture, not a score
assertion: the accepted socket returned `WouldBlock` during header parsing.
The fixture now explicitly resets accepted sockets to blocking mode while
retaining its read timeout. A third CLI test forces the initial nonblocking
state; removing the reset in an isolated copy reproduces `WouldBlock`, and
restoring it passes. All three pricing CLI tests and targeted clippy pass
locally. The corrected head `d957fe5` passed all PR checks, including macOS
and Windows. PR #42 merged as `630183a` with an identical tree; post-main
CI/npm runs 34407524104 and 34407524144 succeeded.

### Raw artifact scan engine: partial G07 implementation

The feature branch adds a streaming byte engine with a validated policy and
explicit raw-file scope. Nine integration tests and one deadline unit test
pass. Thirteen isolated mutations are caught: dropping sequence matching,
dropping whole-file digest matches, bypassing the file-byte limit, accepting an
empty scope, swallowing read errors, accepting truncated files, ignoring prior
incompleteness, omitting names from inventory identity, overflowing the match
list, accepting duplicate entries, resetting match state at chunk boundaries,
accepting an empty policy, and omitting the final deadline check.
Restored controls pass; targeted clippy passes with warnings denied.
The full harness package suite passes 98 tests with two explicitly ignored
tests (the slow CI leg and the installed sibling shim). Rustdoc with warnings
denied and workspace formatting also pass.

This does not yet establish pre-launch protection. No artifact enumerator,
Docker export capture or CLI refusal path is wired to this engine at this
checkpoint. Its caller must impose blocking-I/O deadlines and report
enumeration failures. Negative raw-byte matching cannot exclude compressed,
encoded, transformed or previously memorized content.

### Non-extracting TAR reader: G07 integration in progress

Nine synthetic archive tests and one deadline unit test pass. They cover
repeated paths, concatenated archives, complete archive hashing, link/header
content, GNU long names, PAX metadata, unsupported sparse/size forms, malformed
records, bounded metadata allocation, padding/count limits, truncation, read
errors, dangling extensions and duplicate pending extensions. No archive is
extracted and no Docker container is started by these tests.

Thirteen isolated mutations are caught: stopping at zero blocks, removing
header matching, removing body matching, accepting PAX size overrides,
ignoring blank PAX records, removing the metadata cap, excluding padding from
the byte bound, swallowing enumeration errors, replacing the archive digest,
hiding extension entries, accepting dangling extensions, accepting duplicate
extensions and ignoring the reader deadline. Restored controls pass and the
restored source is byte-identical to the feature worktree after formatting.
The final harness suite passes 108 tests with two explicit ignores. Targeted
clippy, rustdoc with warnings denied and formatting pass.

The existing byte-engine head `c4d1c29` passed all PR #44 checks. That result
does not cover this subsequent reader addition; its new dependency and package
checks must run on the updated head. The dependency is `tar` 0.4.46 with
default features disabled, adding `filetime` transitively. Docker capture,
image configuration/volume handling and real launch refusal remain open.

Rates use the legacy entrant-reported token fields. Individual omitted counts
default to zero in that protocol; neither count completeness nor the declared
model is independently verified. Partial usage, failed requests and absent
records remain explicit limitations. Quotes are not provider invoices and do
not replace the legacy cost-normalized score columns.

Arena still consumes published Bench `=0.19.0`; the new Bench arithmetic and
diagnostic repairs do not reach that dependency until a subsequent authorized
release and pin update. Arena's own sealed-evidence and confidence repairs are
local to Arena. Historical numerical evidence has not been regenerated.

Hyper-Tau coverage and its proposed ports remain open; the detailed
[assessment](HYPER-TAU-REVIEW.md) records source reads and corrections.
In particular, the
artifact scan must not be described as proof of no contamination, rate cards
must not accept nonfinite rates or silently treat missing usage as zero, and
gateway accounting must not imply provider billing was independently verified.
