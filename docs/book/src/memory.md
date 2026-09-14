# Memory and retrieval benchmark

`sharpebench-memory` applies the suite's skill-versus-luck discipline to a memory
or retrieval layer. It is a pure library over caller-supplied outcomes; it does
not run an agent, retrieve documents, or own a store.

## Three-arm ablation

The required arms are:

1. **baseline**: no memory, the performance floor;
2. **retrieval**: the system under test; and
3. **oracle**: gold records only, the attainable ceiling.

The report includes retrieval lift, stationary-bootstrap significance through
`sharpebench-stats`, fraction of the oracle ceiling, and cost-normalized lift per
extra token and unit of latency. A larger raw lift can therefore rank below a
smaller one if it costs much more to obtain.

### Ablation input contract

`ablation_report` validates its inputs at the boundary and returns an error
rather than a number derived from inputs it cannot score. It requires all three
arms to be non-empty, correctly tagged, and of equal length: the lift is paired
per task, and the oracle mean is a ceiling for the same task population, not for
a different task mix. Equal lengths cannot prove that the caller aligned the same
task identities in the same order; that alignment stays the caller's contract and
the crate states it rather than assuming it away. Every outcome score and every
reported token/latency cost must be finite, and `alpha` must be finite and inside
`(0, 1)`. `poisoning_report` applies the same finite-score, finite-cost and alpha
checks to its clean and poisoned arms.

`fraction_of_ceiling` is `retrieval_lift / (mean(oracle) - mean(baseline))` and
is floored at `0.0` whenever the oracle is no better than baseline. An oracle at
or below the baseline has not established a ceiling, so no fraction of one was
captured. Dividing by a negative gap instead returns a sign-flipped ratio, which
reports a retrieval arm that also lost ground as having captured a favorable
positive share of the ceiling.

#### Migration from the unmatched-oracle report

An oracle arm of a different length used to be accepted, and `fraction_of_ceiling`
then divided a lift measured on one task population by a gap measured on another.
That input is now an error. Supply the oracle scores for the same tasks, in the
same order, as the baseline and retrieval arms. Do not pad or truncate an oracle
series to clear the length check: that substitutes a fabricated ceiling for the
missing measurement. A previously reported `fraction_of_ceiling` computed from an
unmatched oracle, or from an oracle below baseline, is not corrected by re-running
the same inputs through the new code, because the new code refuses the first case
and floors the second.

## Integrity legs

- **Poisoning:** behavior-integrity delta, attack success, and significance after
  corrupted records enter the retrieval set.
- **Multi-session dependency:** conditioned lift and dependency satisfaction when
  later sessions declare dependencies on earlier sessions. See the scoring and
  inference contract below.
- **Scenario transitions:** a manifest that declares, per DAG edge, whether a
  later stage is a fresh episode with memory or a continuous portfolio, what may
  cross, each stage's effective date, and which earlier invariants must survive.
  See the manifest contract below.
- **Point-in-time correctness:** recall-audit counts and a hard leak flag for
  future information.
- **Confabulation:** regret from reinforced beliefs that were never retested and
  later resolved false.
- **Treatment activation and placebo control:** receipts showing that memory
  reached a decision boundary, and a length-matched placebo arm under the same
  model, tasks and budget. See the contract below.

The crate uses deterministic reductions and explicit resampling seeds, with
`#![forbid(unsafe_code)]`. Because it accepts outcome vectors rather
than executing an agent, any store or agent framework can feed it without becoming
a dependency of the benchmark.

## Treatment activation and placebo control

"Memory enabled" does not show that retrieved content reached a trading decision,
and a lift over baseline can come from the retrieved content, from extra prompt
bytes, or from extra compute. The `activation` module separates those readings.
It does not replace the oracle ceiling of `ablation_report`.

An `ActivationReceipt` covers one decision where memory was offered. It records the
SHA-256 and byte length of the exact offered content, when that content became
available, when the decision was taken, and the SHA-256 of every segment of the
decision-boundary input as the host framed it. The content counts as exposed only
when one whole segment has the same digest. That is structured evidence, not a
substring search over a log: bytes buried inside a larger segment are not exposure.
Exposure is derived from the digests and is never carried as a separate flag.

Every receipt constructor, including deserialization, refuses an empty decision id,
a malformed digest, and content available after the decision time. The last is
`PointInTimeViolation`, the same no-lookahead rule the PIT leg scores; availability
equal to the decision time is allowed.

`activation_status` classifies an arm as `Activated` (at least one receipt shows
exposure), `NotActivated` (receipts exist and none shows exposure, so memory was
written or offered but never reached a decision), or `Unavailable`. An opaque agent
that cannot produce receipts supplies `ActivationEvidence::Unavailable`. That is
diagnostic, not invalidating: the comparison is still computed, the arm is never
treated as activated, and the lift is labeled a proxy.

`placebo_controlled_report` compares a retrieval arm with a placebo arm. Both carry a
`TreatmentIdentity` of model id, task ids in scoring order, and declared budget, and
the report refuses a comparison whose identities differ with `ModelMismatch`,
`TaskMismatch` or `BudgetMismatch`. When both arms carry receipts, the placebo must
be exposed at exactly the retrieval arm's exposed decisions, at the same decision
time, with the same byte length and different bytes; each departure is its own
typed refusal. The report then keeps three claims apart:

1. **Activation established:** `activation_established` and `retrieval_activation`.
2. **Placebo-controlled lift:** `placebo_controlled_lift` (retrieval mean minus
   placebo mean), its paired stationary-bootstrap p-value, and `lift_evidence`,
   which is `PlaceboControlled` only when retrieval activated and the placebo
   matched, and a named proxy otherwise.
3. **Causal trading improvement:** never claimed. `not_established` always says so,
   together with what receipts cannot show: that the model used the exposed bytes,
   that the host framed and hashed the input honestly, and that the placebo carries
   no task information, which stays the caller's contract.

## Multi-session credit

`multi_session_report` validates a directed acyclic graph. It refuses repeated
session IDs, unknown/self dependencies, duplicate edges, cycles, empty or unpaired
arms, nonfinite scores or arithmetic, and alpha outside the finite interval `(0, 1)`.
Session IDs are opaque. Their numeric order does not establish execution chronology.
The caller must align paired task identities and supply scores with comparable units.

Each session's raw lift is the mean paired retrieval-minus-baseline difference.
`retained` means only that this observed lift is positive; it does not prove that
an agent stored or retrieved a fact. `qualified_retention` additionally requires
every prerequisite to qualify. A failed root therefore blocks its entire descendant
chain, even if an intermediate session has positive raw lift.

When prerequisites qualify, the session contributes its signed lift, including a
loss. Otherwise it contributes zero. Every session stays in the denominator.
`raw_mean_lift` and `conditioned_mean_lift` weight sessions equally, regardless of
their task counts. Output rows retain input order; reductions use session-ID order.

One graph is one chain, not an independent sample of its tasks or sessions. Its
report sets `inference_unavailable = IndependentReplicatesRequired`. It does not
concatenate tasks into a stationary-bootstrap series.

## Scenario-transition manifests

`sharpebench_memory::transition` binds a `ScenarioManifest` to the same session
DAG. `scenario_transition_report` runs `multi_session_report` unchanged and adds
one row per stage in effective-date order.

Each stage declares an effective date (`YYYY-MM-DD`), optional scripted fact
references and named invariants: a gross exposure cap or a point-in-time cutoff
on the fact versions the stage used. Each transition declares a carryover mode,
the memory artifacts allowed to cross, and the earlier stage's invariants it must
preserve. There is no default mode.

- `fresh_episode_with_memory`: the later stage opens on its own declared initial
  portfolio. Only allowed memory artifacts cross.
- `continuous_portfolio`: the later stage opens on the earlier stage's closing
  cash and positions, and allowed memory artifacts cross. It must not declare an
  initial portfolio, and a stage may have at most one continuous predecessor.

`ScenarioManifest::declare`, and JSON deserialization through it, refuse an
undeclared mode, effective dates that do not strictly increase along an edge, a
stage referencing a fact version dated after its own effective date, and a
preserved obligation the earlier stage does not declare. Scoring additionally
refuses a manifest whose stages or transitions are not exactly the DAG's sessions
and edges, a memory read that no incoming edge allows, and an allowed read no
predecessor wrote. Each cause has its own `TransitionError` variant.

`observe_stage` builds stage k's observation from the manifest, the records of
its declared predecessors, and the latest version of each fact available on or
before stage k's effective date. Changing a later stage's facts or record
therefore cannot change an earlier observation.

A stage row lists its own failures (caller-reported safety failures, violated own
invariants, and use of a fact version dated after the stage) separately from
preservation violations, which are charged to the later stage that broke an
earlier stage's invariant. The earlier row is never rewritten.
`failed_stages` keeps every stage that failed, so a later success or credited
lift cannot hide an earlier safety failure. Safety failures do not change the
chain's memory credit; both are reported side by side.

## Independent chain comparisons

`replicated_multi_session_report` takes at least two `MemoryChainReplicate` values
with unique IDs and the same complete graph and per-session task counts. It reports
raw and conditioned tests separately. Only a positive conditioned mean with its
own upper-tail p-value below alpha sets `conditioned_significant`.

The comparison requires independent complete-chain replicates and exchangeable
baseline/retrieval labels within each complete chain under the null. The graph,
score units, credit rule, tested comparison, alpha, seed and resampling budget must
be chosen before looking at outcomes. Unique IDs and matching lengths cannot
verify independence, task pairing or those design choices. Searching across graphs
or configurations requires additional selection/multiplicity control.

Every null assignment swaps all arms within a selected chain together. The scorer
reevaluates its prerequisites under the swapped outcomes. Swapping only tasks or
negating the already-credited score would change the test: qualification depends
on outcomes and need not be symmetric. Each chain has only two orientations, so
the implementation scores both once and samples their joint assignments. Both
tests weight replicates equally and reduce them in replicate-ID order.

`PairedSwapConfig.resamples` must be between 1 and 1,000,000. If it covers all
`2^replicates` assignments, the test enumerates them and counts the inclusive upper
tail. Otherwise it uses seeded draws with replacement and reports
`(extreme + 1) / (draws + 1)`. The report includes method, actual assignment count,
tail count, seed when sampled, and floating-point tie tolerance. The tolerance is
100 machine epsilons times the largest absolute orientation score, a scale unchanged
by arm swaps. This can conservatively count nearly tied values together.
The paired-swap and Monte Carlo counting conventions follow the
[SciPy permutation-test documentation](https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.permutation_test.html).

These are tests of the stated arm-exchangeability null using a declared credit
functional, not general tests of zero mean, causal memory-use certificates, or
evidence about unmeasured agents and tasks. A small p-value also does not establish
practical importance or remove the need to report the observed effect.

### Migration from the flat pooled test

`MultiSessionReport.pooled_lift_pvalue` and `.significant` are removed. They tested
raw task differences, not the dependency-conditioned effect, and imposed a
stationary-series model on arbitrarily ordered dependent sessions. Do not replace
an unavailable result with `p = 0`, `p = 1`, or a positive verdict. Use the descriptive
report for a single chain; use the replicated API only when its design assumptions
are justified. Historical reports produced by the old API are not upgraded by
renaming their fields.

```rust
use sharpebench_memory::{
    replicated_multi_session_report, MemoryChainReplicate, SessionScores,
};
use sharpebench_stats::paired_randomization::PairedSwapConfig;

// Synthetic API example, not measured agent performance.
let replicates: Vec<_> = (0..6).map(|replicate_id| MemoryChainReplicate {
    replicate_id,
    sessions: vec![
        SessionScores::new(10, vec![0.0], vec![1.0], vec![]),
        SessionScores::new(20, vec![0.0], vec![2.0], vec![10]),
    ],
}).collect();
let report = replicated_multi_session_report(
    &replicates, 0.05, PairedSwapConfig { resamples: 9999, seed: 42 },
).expect("valid synthetic fixture");
assert_eq!(report.conditioned_lift_test.observed_mean, 1.5);
assert_eq!(report.conditioned_lift_test.pvalue, 1.0 / 64.0);
assert!(report.conditioned_significant);
```
