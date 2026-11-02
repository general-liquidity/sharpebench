# Evaluation tiers

Every board and every row carries an evaluation tier, which says what kind of
evidence the row is. There are two tiers, and only one of them can certify
skill.

| Tier | Where it comes from | What it may claim |
|---|---|---|
| `historical-advisory` | a submissions file, `sharpebench run` (historical windows only), an arena board that does not certify its rows, and any row whose evidence does not prove the forward-certifying conditions | advisory only: it never certifies skill, whatever its verdict |
| `forward-certifying` | a forward window, whose bars were revealed after the entrant was committed, under evidence that certifies the row | its verdict may be read as evidence of skill |

The tier is a label. It never changes a score, a rank, an ordinal, a gate or
the [headline verdict](reading-a-row.md#the-headline-verdict): a
`skill_supported` row on a historical-advisory board still reads
`skill_supported`, and the tier beside it says that the verdict is advisory.

## Why two tiers

A historical window's bars existed before the entrant was fixed. A pretrained
model may already hold those prices in its weights, an entrant with network
access may fetch them, and a submissions file may hold returns produced with
any amount of hindsight. None of that is visible in the returns, so a verdict
on such a window can be informative but cannot certify skill. A forward window
removes that possibility: its bars did not exist when the entrant was
committed, so no entrant could have seen them.

The [evidence mode](reading-a-row.md#three-meanings-of-certifying) already
records these facts row by row. The tier condenses them into the one question
a reader asks first, which is whether the verdict on the row can certify
skill.

## How the tier is decided

The rule fails closed: anything that does not prove the forward-certifying
conditions is `historical-advisory`.

1. A headline record takes its tier from its evidence
   (`EvaluationTier::of_evidence`). Only a certifying evidence mode of
   `forward` gives `forward-certifying`. A certifying `backtest` (a historical
   window with no network reach) is still `historical-advisory`, because the
   window is historical.
2. An arena board takes its tier from its signed header
   (`EvaluationTier::of_signed_forward_board`). Arena windows are forward
   windows, and the board is `forward-certifying` only when its header states
   `certifying: true`. A header without the field certifies nothing.
3. `sharpebench score` and `sharpebench run` always print
   `historical-advisory`: a submissions file is not a signed forward window,
   and `run` scores historical windows only.

## Where the tier appears

- The human board of `score` and `run` prints it on its first line, before
  the table:

  ```text
  evaluation tier: historical-advisory (advisory only: no signed forward window certifies these rows, so no verdict here certifies skill)
  ```

- Each headline record lists it first among its qualifiers, as
  `qualifiers.tier`, and the human `headline` line reports it as
  `, tier historical-advisory` after the hard reasons. A record written before
  the field existed reads back as `historical-advisory`.
- The entrant row of `sharpebench run --json` carries an `evaluation_tier`
  object (`sharpebench.evaluation-tier.v1`) beside `evidence_mode`.
- An arena `board.md` states the tier on the line after its title.
- The [leaderboard export](leaderboard-export.md) writes it on every row as
  `tier`, beside the deflated-Sharpe tie band `tie_band`, and its Markdown
  table lists each board's tier.

The JSON board array of `score --json` is unchanged: the tier is never written
into a scored row, so the sealed board and every golden keep their bytes.

## Separating agents before a forward window closes

A forward window lasts weeks, and a skill verdict needs many of them. The
[fast signals](fast-signals.md) report behaviour that separates agents much
sooner, in either tier. They are never evidence of skill and never a rank
input, so they do not stand in for a forward-certifying verdict.
