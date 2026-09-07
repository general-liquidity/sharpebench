use sharpebench_memory::{
    multi_session_report, replicated_multi_session_report, ChainInferenceUnavailable,
    MemoryChainReplicate, SessionScores,
};
use sharpebench_stats::paired_randomization::{PairedSwapConfig, PairedSwapMethod};

const CONFIG: PairedSwapConfig = PairedSwapConfig {
    resamples: 9999,
    seed: 42,
};

fn session(id: u64, lift: f64, deps: &[u64]) -> SessionScores {
    SessionScores::new(id, vec![0.0; 4], vec![lift; 4], deps.to_vec())
}

fn replicate(id: u64, root: f64, child: f64) -> MemoryChainReplicate {
    MemoryChainReplicate {
        replicate_id: id,
        sessions: vec![session(10, root, &[]), session(20, child, &[10])],
    }
}

#[test]
fn many_tasks_in_one_chain_do_not_create_an_inferential_sample() {
    let sessions = vec![SessionScores::new(
        7,
        vec![0.0; 10_000],
        vec![1.0; 10_000],
        vec![],
    )];
    let descriptive = multi_session_report(&sessions, 0.05).unwrap();
    assert_eq!(descriptive.conditioned_mean_lift, 1.0);
    assert_eq!(
        descriptive.inference_unavailable,
        ChainInferenceUnavailable::IndependentReplicatesRequired
    );
    assert!(replicated_multi_session_report(
        &[MemoryChainReplicate {
            replicate_id: 1,
            sessions
        }],
        0.05,
        CONFIG
    )
    .is_err());
}

#[test]
fn raw_significance_does_not_qualify_a_failed_memory_chain() {
    let replicates: Vec<_> = (0..6).map(|id| replicate(id, -1.0, 5.0)).collect();
    let report = replicated_multi_session_report(&replicates, 0.05, CONFIG).unwrap();
    assert_eq!(report.raw_lift_test.observed_mean, 2.0);
    assert_eq!(report.raw_lift_test.pvalue, 1.0 / 64.0);
    assert_eq!(report.conditioned_lift_test.observed_mean, -0.5);
    assert_eq!(report.conditioned_lift_test.pvalue, 1.0 / 64.0);
    // Even a small p-value for a large upper-tail observation cannot turn a
    // negative credited effect into positive evidence. Gate outcomes need not
    // be antisymmetric: swapped root +1 unlocks child -5, scoring -2, not +0.5.
    assert!(!report.conditioned_significant);
}

#[test]
fn swapping_recomputes_qualification_instead_of_negating_observed_credit() {
    // Replicate A: root +1/child -5 -> observed -2, swapped root -1/blocked child -> -.5.
    // Replicate B: root +2/child +2 -> observed 2, swapped root -2/blocked child -> -1.
    // All joint means: observed 0, swap A .75, swap B -1.5, swap both -.75.
    // Upper tail is 2/4. Naive sign flips of [-2, 2] yield 3/4 instead.
    let report = replicated_multi_session_report(
        &[replicate(1, 1.0, -5.0), replicate(2, 2.0, 2.0)],
        0.05,
        CONFIG,
    )
    .unwrap();
    assert_eq!(report.conditioned_lift_test.observed_mean, 0.0);
    assert_eq!(report.conditioned_lift_test.pvalue, 0.5);
    assert_eq!(report.conditioned_lift_test.extreme_assignments, 2);
    assert_eq!(report.conditioned_lift_test.assignments, 4);
}

#[test]
fn positive_credited_effect_still_requires_its_own_pvalue() {
    let replicates: Vec<_> = (0..6)
        .map(|id| {
            if id < 4 {
                replicate(id, 1.0, 1.0)
            } else {
                replicate(id, -1.0, 1.5)
            }
        })
        .collect();
    let report = replicated_multi_session_report(&replicates, 0.05, CONFIG).unwrap();
    assert_eq!(report.raw_lift_test.pvalue, 1.0 / 64.0);
    assert_eq!(report.conditioned_lift_test.observed_mean, 0.5);
    // Four successful chains must keep their observed orientation, but either
    // labeling of the two broken chains reaches the conditioned upper tail.
    assert_eq!(report.conditioned_lift_test.pvalue, 4.0 / 64.0);
    assert!(!report.conditioned_significant);
}

#[test]
fn successful_independent_chains_can_support_a_positive_credited_effect() {
    let replicates: Vec<_> = (0..6).map(|id| replicate(id, 1.0, 2.0)).collect();
    let report = replicated_multi_session_report(&replicates, 0.05, CONFIG).unwrap();
    assert_eq!(report.conditioned_lift_test.observed_mean, 1.5);
    assert_eq!(report.conditioned_lift_test.pvalue, 1.0 / 64.0);
    assert!(report.conditioned_significant);
    assert_eq!(report.conditioned_lift_test.independent_units, 6);
    assert_eq!(
        report.conditioned_lift_test.method,
        PairedSwapMethod::ExactEnumeration
    );
    assert!(
        !replicated_multi_session_report(&replicates, 1.0 / 64.0, CONFIG)
            .unwrap()
            .conditioned_significant
    );
}

#[test]
fn reported_statistic_uses_equal_session_and_equal_replicate_weights() {
    let mut replicates = vec![replicate(1, 1.0, 3.0), replicate(2, 2.0, 6.0)];
    for r in &mut replicates {
        r.sessions[1].baseline = vec![0.0; 100];
        r.sessions[1].retrieval = vec![if r.replicate_id == 1 { 3.0 } else { 6.0 }; 100];
    }
    let report = replicated_multi_session_report(&replicates, 0.05, CONFIG).unwrap();
    assert_eq!(report.raw_lift_test.observed_mean, 3.0); // ((1+3)/2 + (2+6)/2)/2
    assert_eq!(report.conditioned_lift_test.observed_mean, 3.0);
    assert_eq!(report.conditioned_lift_test.independent_units, 2);
}

#[test]
fn input_reordering_cannot_select_a_better_seeded_pvalue() {
    let mut replicates: Vec<_> = (0..12)
        .map(|id| {
            let child = if id % 3 == 0 { -3.0 } else { 1.0 };
            replicate(id, 1.0, child)
        })
        .collect();
    let config = PairedSwapConfig {
        resamples: 127,
        seed: 881,
    };
    let original = replicated_multi_session_report(&replicates, 0.05, config).unwrap();
    replicates.reverse();
    for r in &mut replicates {
        r.sessions.reverse();
    }
    let reversed = replicated_multi_session_report(&replicates, 0.05, config).unwrap();
    assert_eq!(original.raw_lift_test, reversed.raw_lift_test);
    assert_eq!(
        original.conditioned_lift_test,
        reversed.conditioned_lift_test
    );
    assert_eq!(
        original.conditioned_significant,
        reversed.conditioned_significant
    );
    assert_eq!(
        original.conditioned_lift_test.method,
        PairedSwapMethod::MonteCarlo
    );
    assert!(original.conditioned_lift_test.extreme_assignments > 0);
    assert!(original.conditioned_lift_test.extreme_assignments < config.resamples);
}

#[test]
fn complete_geometry_and_unique_replicates_are_enforced() {
    let reference = replicate(1, 1.0, 1.0);
    let other = replicate(2, 1.0, 1.0);
    for mutation in 0..6 {
        let mut broken = other.clone();
        match mutation {
            0 => broken.replicate_id = reference.replicate_id,
            1 => {
                broken.sessions.pop();
            }
            2 => broken.sessions[1].depends_on.clear(),
            3 => {
                broken.sessions[1].baseline.push(0.0);
                broken.sessions[1].retrieval.push(1.0);
            }
            4 => broken.sessions[1].session_id = 21,
            5 => broken.sessions[1].retrieval[0] = f64::NAN,
            _ => unreachable!(),
        }
        assert!(
            replicated_multi_session_report(&[reference.clone(), broken], 0.05, CONFIG).is_err(),
            "mutation {mutation}"
        );
    }
    assert!(replicated_multi_session_report(&[reference, other], 0.05, CONFIG).is_ok());
    assert!(replicated_multi_session_report(&[], 0.05, CONFIG).is_err());
}

#[test]
fn exhaustive_synthetic_null_orbit_has_no_excess_rejections() {
    // This finite 16-assignment check is not an empirical agent experiment or a
    // proof for all populations. Every possible joint arm labeling is inspected.
    let base: Vec<_> = (0..4).map(|id| replicate(id, 1.0, 2.0)).collect();
    let mut pvalues = Vec::new();
    for mask in 0..16 {
        let mut labeled = base.clone();
        for (i, r) in labeled.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                for session in &mut r.sessions {
                    std::mem::swap(&mut session.baseline, &mut session.retrieval);
                }
            }
        }
        pvalues.push(
            replicated_multi_session_report(&labeled, 0.05, CONFIG)
                .unwrap()
                .conditioned_lift_test
                .pvalue,
        );
    }
    assert_eq!(pvalues.iter().copied().fold(1.0, f64::min), 1.0 / 16.0);
    assert!(pvalues.contains(&1.0));
    for cutoff in [1.0 / 16.0, 0.125, 0.25, 0.5, 1.0] {
        let count = pvalues.iter().filter(|&&p| p <= cutoff).count();
        assert!(
            count as f64 / 16.0 <= cutoff,
            "cutoff {cutoff}: {count} rejections"
        );
    }
}
