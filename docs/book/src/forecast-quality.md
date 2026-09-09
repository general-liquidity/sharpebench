# Prospective forecast quality

`sharpebench forecast-quality` analyzes raw prospective forecasts separately
from the trading leaderboard. Forecast performance cannot satisfy, weaken, or
replace any trading-rank gate.

## Input boundary

The command accepts one `sharpe.forecast-evidence.v1` or
`sharpe.forecast-evidence.v2` file per agent:

```bash
sharpebench forecast-quality agent-a.json agent-b.json
sharpebench forecast-quality agent-a.json agent-b.json --json
sharpebench forecast-quality agent-a.json agent-b.json --output report.json
```

SharpeArena writes the native format, but the boundary is a versioned JSON file,
not a package dependency. SharpeBench rejects unknown fields, unknown versions,
bad digests, duplicate identities, broken revision chains, inconsistent clocks,
missing resolutions, nonfinite values, invalid probability vectors, and outcomes
that do not match the frozen contract.

Each file identifies the model, scaffold, prompt, operator, and configuration by
name or SHA-256. Each revision records whether consensus was visible and, when it
was, the consensus snapshot digest. A late revision remains auditable but never
becomes the scored forecast.

## Contract digests: canonical JSON v1 and the legacy encoding

A revision names its contract by `contract_sha256`, and SharpeBench recomputes
that digest from the contract it was given. Two encodings are accepted.

- **`sharpebench/canonical-json/v1`** is the current one. The digest is
  SHA-256 over the versioned pre-image of the contract's canonical text
  (`sharpebench_protocol::canonical::versioned_preimage`): one specified
  numeric form (fixed point while the decimal exponent is inside `-6 < n <= 21`,
  unpadded exponent outside it, integer-valued floats as integers, signed zero
  as `0`), RFC 8785 string escaping, members in code-point order. The version
  tag is part of the hashed bytes, so a v1 digest can never equal a digest of
  the same document under another form.
- **`legacy`** is the pre-migration encoding: the unframed text with
  `serde_json` number rendering and a two-digit padded exponent. It is
  accepted only when the revision digest equals the legacy recomputation of
  the contract. It exists because published evidence pins it:
  `paper/evidence/prospective-forecast-field/` carries 24 legacy digests, and
  none of them recomputes under v1 (`neutral_threshold: 0.0` is `0.0` under
  legacy and `0` under v1), so replacing the encoding would have invalidated
  frozen evidence.

A digest that recomputes under neither is refused as an unknown contract
digest. The report records the encoding each scored digest verified under in
`contract_digest_versions`, keyed by digest with the value
`sharpebench/canonical-json/v1` or `legacy`, and the human table prints the
two counts. A legacy field is therefore visible as one; it is not silently
promoted.

### Declaring the encoding: `sharpe.forecast-evidence.v2`

A v1 document cannot say which encoding it hashed under; SharpeBench infers it
by recomputing both. `sharpe.forecast-evidence.v2` is the v1 envelope plus one
field on every revision, beside the digest it describes:

```json
"contract_sha256": "0eb3250f...",
"contract_digest_encoding": "legacy"
```

`contract_digest_encoding` is a string and must be exactly
`sharpebench/canonical-json/v1` or `legacy`. It lives on the revision because
that is where `contract_sha256` lives: a contract record carries no digest of
itself, and on the producer side one contract answers to both digests, so the
declaration belongs next to the one digest a revision actually names.

A v2 revision is verified under the declared encoding only. There is no
inference: a digest that recomputes under the other encoding is refused, and
the error names the digest, the declared encoding and the encoding the digest
does recompute under (`revision <id> declares contract digest <sha256> under
sharpebench/canonical-json/v1, but it recomputes under legacy`); a digest that
matches no contract says so in the same shape. Any other label is refused as an
unknown encoding, and a v2 revision without the field is refused. The field set
stays exact in both directions: a v1 document carrying
`contract_digest_encoding` is refused rather than read as v2.

`contract_digest_versions` in the report has the same shape for both envelopes.
For v2 it records the declared encoding, which is also the verified one. A v1
document and its v2 restatement produce the same report byte for byte.

Migration notes for producers:

- New evidence should hash contracts under v1. The contract rendered under
  `1e-5` (the reported R07 case) is `0.00001` under v1 and is accepted.
- Python's `json.dumps(sort_keys=True, separators=(",", ":"))` is **not** v1:
  it renders `1e-05` where v1 renders `0.00001`, `1e-07` where v1 renders
  `1e-7`, `1e+16` where v1 renders `10000000000000000`, and `0.0`, `1.0` and
  `-0.0` where v1 renders `0`, `1` and `0`. String escaping and key order do
  agree. A producer that keeps hashing with `json.dumps` produces digests that
  match the legacy encoding only where its number text happens to coincide
  with `serde_json`'s, and match nothing where it does not.
- Support is exact by digest. A contract presented under its legacy digest by
  one agent and under its v1 digest by another is two digests and is not
  common support. A field should be produced under one encoding.

## Scores and calibration

SharpeBench ignores any producer-side calculation and recomputes the declared
score from the raw prediction and outcome:

| Forecast | Recomputed loss or diagnostic |
|---|---|
| point | squared error |
| binary probability | Brier or log loss |
| categorical distribution | multiclass Brier or log loss |
| Normal distribution | closed-form CRPS and probability integral transform |
| direction | zero-one loss outside the frozen neutral band |
| interval | proper interval score at the frozen alpha |

Binary reports include fixed-bin reliability, resolution, uncertainty, and a
Brier skill score against the observed base-rate forecast. Categorical reports
calibrate the selected category's confidence. Normal reports include the PIT
mean, variance, and histogram against the uniform reference. Resolution rates
and blind versus consensus-exposed counts remain visible beside score means.

## Exact support and dependence

Agents are compared only on the exact contract-digest intersection resolved by
the whole field. An unmatched question or horizon is excluded for every pair,
and the excluded count is reported per agent. This prevents a favorable
pair-specific subset from becoming the comparison set.

The resampler treats all assets and questions with the same resolution clock as
one block. It draws whole blocks, preserving contemporaneous dependence rather
than pretending every forecast is independent. The report gives the observed
mean loss difference, a percentile interval, and a two-sided block-bootstrap
p-value. Holm adjustment controls the familywise error rate across all reported
pairs.

Relevant options are:

```text
--bootstrap-samples N   deterministic resample count (default 2000)
--seed N                explicit resampling seed
--confidence C          interval coverage inside (0, 1)
--alpha A               familywise significance level inside (0, 1)
--bins N                calibration and PIT bin count
--output PATH           write the complete JSON report to PATH
```

`--output` is independent of display mode: the CLI writes the same complete,
pretty-printed report whether stdout is the human table or `--json`. This gives
paper and CI pipelines a named artifact without shell-dependent redirection.

## Executable cross-product example

[`examples/forecast-quality`](../../../examples/forecast-quality/) contains two
fields of evidence ledgers generated by SharpeArena, each with a report
independently produced by this command. The supported field has twelve
contracts in six resolution-time blocks and reports an interval, a p-value and
a Holm verdict; the withheld field is the first eight of those contracts in two
blocks, where the block-resampling law cannot resolve the default familywise
level, so the report records the reason and no interval or p-value. The core
test suite verifies the producer-file digests and recomputes both reports
byte-for-byte with frozen resampling settings. The example exercises exact
common support, resolution-time blocks, the block-count requirement for
inference, revision eligibility, and blind versus consensus-visible exposure.
It is a compatibility fixture, not an empirical agent result.

## Superseded prospective engineering pilot

[`paper/evidence/prospective-forecast-field`](../../../paper/evidence/prospective-forecast-field/)
is a checked import of one closed SharpeArena field. The importer requires every
source byte to match the committed Arena HEAD, validates the forecast and
resolution digest chains, requires identical complete support across agents,
and records the source repository, commit, path, plan digest, and copied-file
digests.

The field contains 24 binary contracts for four Binance Spot pairs at six
resolution clocks. Three older, already-cached local model snapshots used one
fixed forecast scaffold. The raw report remains committed to preserve the
protocol audit trail, but this convenience-sample panel is superseded and is
excluded from current model evaluation, the academic paper's empirical
conclusions, and every trading-rank decision.

Even in its original scope, the preregistered minimum for a comparative claim
was 30 settlement blocks, so
the six-block report does not support model superiority even if a nominal test
had crossed its threshold. It also does not score a trading loop: the models had
no tools, memory, portfolio, or order interface. The committed
`report-check.json` comes from a standalone Python implementation that
reconstructs support, calibration, Brier loss, block resampling, and Holm
adjustment from the imported ledgers.

The exact pipeline is:

```bash
python paper/src/import-prospective-field.py \
  --source ../sharpearena/paper/evidence/prospective-forecast-field \
  --output paper/evidence/prospective-forecast-field

sharpebench forecast-quality \
  paper/evidence/prospective-forecast-field/resolved/phi-4.json \
  paper/evidence/prospective-forecast-field/resolved/qwen-0.5b.json \
  paper/evidence/prospective-forecast-field/resolved/qwen-7b.json \
  --bootstrap-samples 2000 --seed 260904 --confidence 0.95 \
  --alpha 0.05 --bins 5 \
  --output paper/evidence/prospective-forecast-field/report.json

python paper/src/check-prospective-forecast-report.py \
  --field-dir paper/evidence/prospective-forecast-field \
  --report paper/evidence/prospective-forecast-field/report.json
```

## Interpretation limits

- The ledger clock establishes logical order, not independently verified wall
  time.
- Exact common support removes question mismatch. It does not make agents,
  prompts, or information sets identical.
- Resolution-time blocks preserve a declared dependence unit. They do not prove
  that no longer-range dependence exists.
- Calibration and proper scores describe forecast quality. Trading eligibility
  still requires the Deflated Sharpe, pass^k, significance, process, and mandate
  gates.
- The pairwise `mean_loss_difference` pools every common contract regardless
  of scoring rule or target unit, so a field that mixes a dimensionless Brier
  loss with a point squared error in a currency averages incompatible units and
  its sign can change under a unit rescale (audit finding R08, deferred).
  Every committed field is one stratum: binary Brier, one target, one unit.
  The core test
  `mixed_scoring_rules_are_silently_pooled_into_one_mean_loss_difference_r08`
  pins the pooling; the deferral reopens when a committed comparison mixes
  strata and a stratified analysis would change its reported verdict.
- A normalized next-token logit over labels `0` and `1` is an operational
  probability under that scaffold, not an unconstrained subjective probability
  from the model.
