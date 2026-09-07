# Memory and retrieval benchmark

`sharpebench-memory` applies the suite's skill-versus-luck discipline to a memory
or retrieval layer. It is a pure library over caller-supplied outcomes; it does
not run an agent, retrieve documents, or own a store.

## Three-arm ablation

The required arms are:

1. **baseline** — no memory, the performance floor;
2. **retrieval** — the system under test; and
3. **oracle** — gold records only, the attainable ceiling.

The report includes retrieval lift, stationary-bootstrap significance through
`sharpebench-stats`, fraction of the oracle ceiling, and cost-normalized lift per
extra token and unit of latency. A larger raw lift can therefore rank below a
smaller one if it costs much more to obtain.

## Integrity legs

- **Poisoning:** behavior-integrity delta, attack success, and significance after
  corrupted records enter the retrieval set.
- **Multi-session dependency:** conditioned lift and dependency satisfaction when
  later sessions declare dependencies on earlier sessions. See the scoring and
  inference contract below.
- **Point-in-time correctness:** recall-audit counts and a hard leak flag for
  future information.
- **Confabulation:** regret from reinforced beliefs that were never retested and
  later resolved false.

The crate uses deterministic reductions and explicit resampling seeds, with
`#![forbid(unsafe_code)]`. Because it accepts outcome vectors rather
than executing an agent, any store or agent framework can feed it without becoming
a dependency of the benchmark.

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
