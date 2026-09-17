# Candidate lineage diagnostics

SharpeArena can generate a bounded pool of non-executable strategy candidates
inside one recorded search. Its v2 candidate ledger binds four facts that a
flat leaderboard loses:

- the exact generator identity;
- a host-derived strategy-family preimage and digest;
- references to earlier raw candidates, resolved to their content digests;
- citations to exact source digests registered by the operator before
  generation, with optional locator, revision, authors, and license metadata.

SharpeBench verifies that sidecar independently. It recomputes canonical JSON
hashes for the raw candidate, manifest, generator identity, family, base binding,
and lineage binding. It also derives the family preimage again from the raw
SharpeArena DSL, resolves parent IDs against earlier valid rows, checks every
summary count, and requires validation scores for exactly the selectable
candidates. A malformed or partial artifact produces no report.

```bash
sharpebench lineage strategy-evidence.json --json
```

The input is one completed SharpeArena strategy-search evidence record. A JSONL
journal containing exactly one nonblank record is accepted. A multi-record
journal is refused instead of silently choosing a run; extract the run you want
to inspect first, or count the whole journal with `--census` (below).
SharpeArena strategy evidence schemas 2 and 3 are accepted. Any other schema
is refused, because a later schema may add rules this verifier cannot check.
SharpeBench 0.27.0 and earlier read a schema 3 record without checking its
census or source dating.

The report contains the verified in-harness trial count, candidate ancestry,
cited sources, and one robustness row per host-derived strategy family. For
each family it reports the best and median validation-split median Deflated
Sharpe, plus their gap. A wide gap is evidence that one tuned variant carries
the family; a narrow gap is consistent with robustness across the variants that
were actually proposed and scored.

## Test-split consultation census

Each search selects on its validation split and evaluates only its winner on
the test split, deflated by that search's own observed trials. One journal can
hold many searches against the same test split, and an operator can revise the
prompt after reading earlier test scores. Every record then looks like a single
test look. The census counts the looks.

```bash
sharpebench lineage strategy-evidence.jsonl --census --json
```

Records are grouped by test split identity. A historical split is named by its
dataset content digest and its recorded window; execution seeds do not change
which bars were read, so a rerun with new seeds is the same split. A synthetic
split is generated from its seeds, so the sorted seeds join the identity. Costs
and dataset labels are excluded. Windows are compared as recorded, so an
omitted window end and an explicit end at the last bar count as different
splits, and so does a window moved by one bar.

For each split the report gives the number of test consultations, the
cumulative observed trials across them, the digests of the consulting records,
and the number of failed searches that named the split but stopped before
reading it. A completed record of either schema counts as a consultation. A
failed record counts only when it is schema 3, because only schema 3 failures
state their split and whether evaluation reached it; anything else is reported
as unidentified, never as clean. A record's digest is the SHA-256 of its stored
line.

From schema 3, SharpeArena stamps each record with its own census of the lines
already in the journal when the record is appended: prior consultations, their
record digests, prior and cumulative observed trials, unidentified prior
records, and `previous_record_sha256`, the digest of the last nonblank line
before the record. SharpeArena computes the census and appends the record
while it holds a lock on the journal, so concurrent searches on one journal
stay consistent. Census mode recomputes the history from the journal and
refuses a record whose declaration or chain disagrees. A record copied out of
the journal that wrote it fails, and so does a journal with an earlier line
removed, inserted or reordered, as long as a schema 3 record follows the
change. Lines cut from the end of the journal leave nothing to disagree with,
so the census is a lower bound unless the digest of the last line is kept
elsewhere. Every completed schema 2 or 3 record also has its lineage verified;
any failure produces no report. A single extracted record shows its declared
census, checked only for internal consistency, because the earlier records are
not present.

### Overlapping windows

Exact identity misses a shifted window. A search over bars 12 to 21 that
follows two searches over bars 10 to 19 counts as a new split, although 8 of its
10 bars were read twice before. To catch that, a schema 3 census also records
`test_window_bars`, the half-open bar interval SharpeArena's kernel resolved for
the test split, and `test_dataset_bars`, the panel's bar count. An earlier
consultation overlaps when it read the same panel (the same content digest, and
for a synthetic split at least one common seed) through a window that
intersects `test_window_bars`. Its omitted window bounds resolve against this
panel's bar count; the same content digest names the same panel, so the count
applies to both. The census gives the number of overlapping earlier
consultations, their record digests and observed trials, and
`prior_consulted_test_bars`, the number of bars in this window that at least
one of them read. An exact match always overlaps.

Census mode recomputes all of it and shows the overlap count and the
prior-read bars on each record. The verifier also checks that
`test_window_bars` resolves the identity's window, that a completed synthetic
record's bar count equals its recorded `n_days`, and, when `--dataset` supplies
the historical panel, that the panel has the declared number of bars. Schema 2
records carry no census, so only schema 3 records report overlap; their counts
include earlier schema 2 consultations.

### Which multiplicity applies

The census reports two counts, and they bound different uses.
`prior_test_consultations + 1` counts test looks: the multiplicity when only the
searches' final test results were compared. `cumulative_observed_n_trials`
counts every candidate that the consulting searches generated: the multiplicity
when later generations were steered by earlier test results. The overlap
counts give the same two bounds for reads of shared bars. The census feeds
neither into deflation.

The census covers one journal file. Searches written to a separate journal
file, or run without recording, are not counted, and nothing in a record can
reveal them. The census is diagnostic: it never changes a record's DSR trial
count, eligibility, or rank.

## Dated idea sources

A source record may carry `available_on`, the operator-stated first calendar
day (`YYYY-MM-DD`) its content existed. The field has no time zone: write the
day in the date convention of the dataset's bar labels, which is the trading
venue's local date. It is optional and omitted when absent, so undated records
keep their bytes. Schema 3 reports, any report with a dated source or a
supplied dataset, and every record verified in census mode add a source-dating
section. It gives the counts of cited, dated, and undated sources, and for the
selection and test splits the calendar day of the first bar and the number of
cited sources dated on or after that day.

A source dated on or after a split's first bar day may carry information from
that split. A source dated on that day itself is counted, because its time
within the day is unknown and it may postdate the first bar. A source dated the
day before cannot contain a return the split scored, since the first scored
return runs from the first bar to the next one. An undated source is counted as
undated, never as early.

SharpeArena stores split windows as bar indices, so the verifier resolves a
split's first day from the dataset itself:

```bash
sharpebench lineage strategy-evidence.jsonl --census --dataset prices.csv --json
```

`--dataset` may be repeated. A dataset is matched to a split by content digest
(of the file bytes, or of its text with line endings normalized as SharpeArena
reads it) and parsed with the simulator's own CSV reader, so the window index
names the bar the kernel stepped. When the day cannot be established the split
is reported unavailable with a typed reason rather than passed:
`split_not_recorded`, `synthetic_split_has_no_calendar`, `dataset_not_supplied`,
`window_outside_dataset`, or `date_not_iso8601` (the first bar's label does not
begin with a calendar day). A schema 3 record also declares its own source
dating; the verifier refuses a declaration that disagrees with a recomputed
split, and neither trusts nor refuses the declaration for a split it could not
locate or was not given the dataset for.

`available_on` is a stated date, not a proven one. The check shows that the
chain of custody names a source from inside the evaluated period; it cannot
show that an undated or misdated source was published earlier.

## Guarantee boundary

Lineage is a diagnostic contract, not a scoring input. Family grouping, the
test-split census, and source dating never merge proposals, change the DSR
trial count, change eligibility, or move the rank key. Invalid and duplicate
proposals remain in the observed-trial denominator. The scoring kernel
continues to consume ordinary submissions and does not import SharpeArena.

The verifier establishes internal consistency and the declared chain of
custody. It does not establish that a cited source caused an idea, that a source
is scientifically sound, or that searches outside the recorded Arena journal
did not happen. A source digest proves which bytes were named only when the
source preimage is available through the recorded locator or another evidence
channel.
The family rule is specific to the current closed SharpeArena strategy DSL; a
future DSL change must update both derivations or verification fails.
