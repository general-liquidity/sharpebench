# Validation status and frozen evidence

Reviewed 2026-10-10. Implementation, software verification, exploratory
calibration and confirmatory validation are different evidence classes.

The benchmark implements its scoring gates and has regression, golden,
packaged-consumer and provenance checks. Those checks do not establish that the
complete eligibility rule has a calibrated real-market error rate or that an
eligible system is deployable profitably.

## Joint-gate calibration

The [development evidence README](../../../paper/evidence/development/README.md)
documents P12-A's exploratory calculation. It models pooled DSR against fixed
committed bars, per-window reliability, and a bootstrap leg on supported
geometries. It does not execute a live agent or simulate the whole field's
calibration path. The hourly crypto geometry omits the bootstrap leg and does
not have joint-rule rows.

Important qualifications:

- The recorded artifact's named two-sided 95 percent interval fields used
  five percent per tail, making them two-sided 90 percent intervals. The current
  producer corrects the tails; the old artifact has not been regenerated.
- The process, mandate and risk gates are omitted conjuncts. Adding them can
  only reduce acceptance on unchanged inputs with unchanged modeled-leg outputs.
- Fixed deflation bars, independent entries, Gaussian return assumptions,
  omitted execution effects and averaged bootstrap randomness are assumptions,
  not additional conjuncts. Changing them can change modeled outputs in either
  direction. The exploratory rates are not unconditional upper bounds on the
  shipped rule under arbitrary fields or market conditions.
- Influential-vote and dispersion disclosures are diagnostics, not gates.
  The current dispersion-vote membership fence is a separate scoring mechanism;
  using an old fixed bar does not exercise it end to end.

The current producer's interval and scope corrections change what a new run
would record. Existing numerical evidence remains historical; a repaired
producer does not retrospectively validate its previous artifact.

## Work that still requires new evidence

| Work | Why software checks do not close it |
| --- | --- |
| Freeze a confirmatory protocol and analysis plan | Requires explicit sampling, dependence, precision, missingness, stopping and claim definitions |
| Validate the joint decision under that protocol | Requires a separately authorized campaign and evidence identities; development draws are not untouched confirmation |
| Calibrate field-dependent bars and entrants jointly | Fixed historical bars do not measure changing vote membership, outliers or cross-agent dependence |
| Establish realism, agent skill or deployable profitability | Requires suitable task/market/agent evidence, not synthetic regression controls alone |
| Propagate a repair to SharpeArena | Requires a Bench release and Arena's exact dependency-pin update; sibling source changes do not propagate automatically |

These are remaining measurement and delivery decisions, not permission to run
experiments, rewrite frozen results, publish packages or change gate thresholds.
The [study protocol validator](../../../crates/sharpebench-study/src/validate.rs)
can check a declared protocol contract; it does not supply missing empirical
validation or authorize the campaign.

The study report constructor also validates the supplied protocol before
reporting or deciding claims. It preserves the original typed validator refusal
and rechecks mutable fields even after an earlier successful validation. This
prevents invalid precision requirements or forbidden tier/claim combinations
from producing an ordinary report. Valid report JSON is unchanged; the Rust
refusal enum has an additional protocol-error variant. These checks do not
authenticate an approver or prove the supplied counts were actually executed.

Error-rate decision limits and power decision bounds must be finite values in
`[0, 1]`. The validator and JSON schema now enforce that same domain. Invalid
thresholds receive an existing typed parameter refusal, not an ordinary
unsupported or supported claim. Both endpoints remain accepted. This tightens
previous permissive input acceptance, not the wire shape; it does not choose
owner targets, equate decision thresholds with estimand targets, or establish
statistical calibration.

The shipped placeholder protocol is explicitly unsealed and retains owner
decisions for targets and sample counts. The development producer above is not
an execution of that study-protocol contract: its own CLI accepts replication
counts and exploratory settings, not an approved frozen protocol. A passing
protocol validator therefore does not establish that those historical draws
met a frozen study's acceptance criteria.

The study library's minimum-count planner uses the anticipated rate and rounded
expected events. Event-count jumps can increase a Wilson width, so the search
must not assume monotonicity. Current code searches integer ranges with
conservative pruning and exact leaf checks. This repairs the planning count;
it does not guarantee that future realized counts meet the precision target.

## Reading a claim correctly

Use the source/artifact identity and stated evidence tier with every number.
An unchanged frozen file does not prove a changed engine reproduces its result.
A green test suite establishes the checked software behavior, not independence
of market histories, uncontaminated training or verified external custody.

For current scoring mechanics, see [deflation and its vote fence](methodology-deflated-sharpe.md).
For operational scope and pending decisions, see [the product plan](../../PLAN.md).
