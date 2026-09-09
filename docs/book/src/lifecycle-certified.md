# Lifecycle-certified rank mode

The [process gate](methodology-process.md) counts events. The
[lifecycle ordering check](methodology-process.md#lifecycle-ordering-linked-by-subject)
is additive: it is available to a caller and is not a conjunct of the shipped
eligibility predicate. That is a deliberate protocol property. Every published
board, every golden fixture and every arena window was scored under the
counting gate, and a silent change to the predicate would re-score them.

A host that wants the ordering leg to bind opts into a **rank mode** by its
versioned identifier. The first one is `lifecycle-certified/v1`.

```text
sharpebench score field.json --rank-mode lifecycle-certified/v1
```

In Rust the same board comes from `rank_certified(subs, declarations, cfg,
RankMode::LifecycleCertifiedV1)`; `RankMode::parse` resolves the identifier
and refuses any other string, including a later version this kernel does not
implement. Without the flag, `score` is `rank_declared` byte for byte.

## A second verdict, not a different rank

The mode follows the shape a [declared mandate](methodology-pass-k.md#declaring-a-mandate-at-submission)
already uses: certification is a labeled verdict beside the host rank, never a
change to it. `rank_eligible`, the sort order and `rank_ordinal` are the ones
the legacy protocol produces. Each row gains one `certification` object:

| Field | Meaning |
|---|---|
| `mode` | The identifier the verdict was produced under |
| `certified` | `true` only when `withheld` is empty |
| `lifecycle_warnings` | Warn-severity ordering violations across all runs; reported, never withholding |
| `withheld` | Every property the record could not establish, host first, then by run |

The plain-text board adds a `certified` column only under a rank mode, so a
board rendered without one prints exactly as before.

## What v1 certifies, and what it withholds

A row is certified when all three hold:

1. It is `rank_eligible` under the host verdict. Certification strengthens
   eligibility; it never substitutes for it.
2. Every submitted run carries at least one lifecycle transition. The ordering
   check on a trace with no lifecycle events is vacuously clean, and a vacuous
   pass is not evidence that a lifecycle ran.
3. No run has a block-severity ordering violation (a submission with no
   passing risk evaluation for its own subject, a fill never acknowledged, and
   the other four described under process discipline).

Each failing property is named in `withheld` with a typed `property` tag,
following the repository's rule that an unavailable value is reported as
unavailable rather than coerced to a favourable one:

| `property` | Named fields | Meaning |
|---|---|---|
| `host_ineligible` | none | Not eligible under the host verdict |
| `lifecycle_evidence_absent` | `run` | That run's trace has no lifecycle transition |
| `lifecycle_ordering_blocked` | `run`, `block_violations` | That run's lifecycle has block-severity ordering failures |

Runs are indexed as submitted, window-major, before any shared-cell
restriction. A peer cannot hide a run's violation by omitting the cell in
which it happened, for the same reason the process gate reads unrestricted
runs.

## What v1 does not certify

The mode reads only the submission and its host score. It does not establish
run identity (use `--require-run-keys`, which is a separate refusal-based
check), does not verify a signed board chain (use `sharpebench verify`), and
does not fold certification into the [disqualification taxonomy](cli.md#disqualify),
which still explains the host verdict. Any of those would be a later version
with its own identifier, and a kernel that does not implement that version
refuses it rather than certifying under the wrong rules.

No committed evidence was rescored under this mode. The paper's field
prevalence of ordering violations remains unknown, because no submitted run
carries lifecycle events.
