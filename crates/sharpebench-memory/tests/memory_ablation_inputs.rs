//! Boundary contract for the three-arm ablation and the poisoning leg.
//!
//! These are the regressions for the audit row "matching oracle/task
//! populations, finite parameters, oracle floor". Every case fails closed with a
//! boundary error rather than reporting a number derived from an unmatched task
//! population, a nonfinite input, or an oracle that is not a ceiling.

use sharpebench_memory::{ablation_report, poisoning_report, Arm, ArmCost, ArmScores};

const EPS: f64 = 1e-9;

fn baseline(scores: Vec<f64>) -> ArmScores {
    ArmScores::new(Arm::Baseline, scores)
}
fn retrieval(scores: Vec<f64>) -> ArmScores {
    ArmScores::new(Arm::Retrieval, scores)
}
fn oracle(scores: Vec<f64>) -> ArmScores {
    ArmScores::new(Arm::Oracle, scores)
}
fn poisoned(scores: Vec<f64>) -> ArmScores {
    ArmScores::new(Arm::Poisoned, scores)
}

#[test]
fn oracle_task_population_must_match_the_paired_arms() {
    let b = baseline(vec![0.1, 0.2, 0.3, 0.4]);
    let r = retrieval(vec![0.5, 0.6, 0.7, 0.8]);
    // A ceiling measured on a different task mix is not this ablation's ceiling.
    let short_oracle = oracle(vec![0.9, 0.9]);

    let error = ablation_report(&b, &r, &short_oracle, 0.05)
        .expect_err("an oracle over a different task population must be refused");
    assert!(
        error.contains("oracle"),
        "error must name the oracle arm, got {error}"
    );

    // The matched population is still accepted.
    let matched = oracle(vec![0.9, 0.9, 0.9, 0.9]);
    assert!(ablation_report(&b, &r, &matched, 0.05).is_ok());
}

#[test]
fn oracle_below_baseline_reports_the_documented_zero_floor() {
    // The supplied oracle is worse than baseline, so the ceiling gap is negative
    // and the captured fraction is not defined. Reporting `lift / gap` would turn
    // a retrieval arm that also lost ground into a favorable positive fraction.
    let b = baseline(vec![0.50, 0.50]);
    let r = retrieval(vec![0.30, 0.30]);
    let o = oracle(vec![0.10, 0.10]);

    let report = ablation_report(&b, &r, &o, 0.05).expect("finite matched arms");
    assert!(report.retrieval_lift < 0.0);
    assert!(
        report.fraction_of_ceiling.abs() < EPS,
        "a degenerate ceiling must floor at zero, got {}",
        report.fraction_of_ceiling
    );

    // Same floor when the retrieval arm gained but the oracle is still below
    // baseline: no ceiling was established, so no fraction of one was captured.
    let gained = retrieval(vec![0.70, 0.70]);
    let report = ablation_report(&b, &gained, &o, 0.05).expect("finite matched arms");
    assert!(report.retrieval_lift > 0.0);
    assert!(
        report.fraction_of_ceiling.abs() < EPS,
        "a degenerate ceiling must floor at zero, got {}",
        report.fraction_of_ceiling
    );
}

#[test]
fn nonfinite_ablation_scores_are_refused() {
    let b = baseline(vec![0.1, f64::NAN]);
    let r = retrieval(vec![0.5, 0.6]);
    let o = oracle(vec![0.9, 0.9]);
    assert!(ablation_report(&b, &r, &o, 0.05).is_err());

    let b = baseline(vec![0.1, 0.2]);
    let r = retrieval(vec![0.5, f64::INFINITY]);
    assert!(ablation_report(&b, &r, &o, 0.05).is_err());

    let r = retrieval(vec![0.5, 0.6]);
    let o = oracle(vec![0.9, f64::NEG_INFINITY]);
    assert!(ablation_report(&b, &r, &o, 0.05).is_err());
}

#[test]
fn nonfinite_arm_costs_are_refused() {
    let b = baseline(vec![0.1, 0.2]);
    let o = oracle(vec![0.9, 0.9]);
    let r = retrieval(vec![0.5, 0.6]).with_cost(ArmCost::new(f64::NAN, 5.0));
    assert!(ablation_report(&b, &r, &o, 0.05).is_err());

    let r = retrieval(vec![0.5, 0.6]).with_cost(ArmCost::new(10.0, f64::INFINITY));
    assert!(ablation_report(&b, &r, &o, 0.05).is_err());
}

#[test]
fn ablation_alpha_must_be_finite_and_inside_the_unit_interval() {
    let b = baseline(vec![0.1, 0.2]);
    let r = retrieval(vec![0.5, 0.6]);
    let o = oracle(vec![0.9, 0.9]);
    for alpha in [f64::NAN, 0.0, 1.0, 2.0, -0.1] {
        assert!(
            ablation_report(&b, &r, &o, alpha).is_err(),
            "alpha {alpha} must be refused"
        );
    }
    assert!(ablation_report(&b, &r, &o, 0.05).is_ok());
}

#[test]
fn poisoning_refuses_nonfinite_scores_and_invalid_alpha() {
    let clean = retrieval(vec![0.8, 0.9]);
    let dirty = poisoned(vec![0.4, f64::NAN]);
    assert!(poisoning_report(&clean, &dirty, 0.05).is_err());

    let dirty = poisoned(vec![0.4, 0.5]);
    for alpha in [f64::NAN, 0.0, 1.0, 3.0] {
        assert!(
            poisoning_report(&clean, &dirty, alpha).is_err(),
            "alpha {alpha} must be refused"
        );
    }
    assert!(poisoning_report(&clean, &dirty, 0.05).is_ok());
}
