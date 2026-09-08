//! R02 regressions: the remaining statistical family must not turn an invalid
//! input into a number. Each invalid case asserts a typed unavailability
//! (`None`), and each valid case pins the exact value the same function returned
//! before the boundary checks were added, so a check cannot move a published
//! number.

use sharpebench_stats::dissent::{dissent, kendall_tau_b};
use sharpebench_stats::stats::sortino_ratio;
use sharpebench_stats::{gate_vs_human, spearman_rho};

/// A well-formed, finite pair used by every "valid input is unchanged" case.
const X: [f64; 6] = [3.0, 1.0, 4.0, 1.5, 5.0, 9.0];
const Y: [f64; 6] = [2.0, 1.0, 3.0, 2.5, 4.0, 8.0];
const RETURNS: [f64; 5] = [0.01, -0.02, 0.03, -0.005, 0.012];

fn with(index: usize, value: f64) -> Vec<f64> {
    let mut v = X.to_vec();
    v[index] = value;
    v
}

#[test]
fn spearman_rho_refuses_non_finite_severities() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            spearman_rho(&with(0, bad), &Y),
            None,
            "x carrying {bad} must not produce a rank correlation"
        );
        assert_eq!(
            spearman_rho(&X, &with(3, bad)),
            None,
            "y carrying {bad} must not produce a rank correlation"
        );
    }
}

#[test]
fn spearman_rho_valid_input_unchanged() {
    assert_eq!(spearman_rho(&X, &Y), Some(0.9428571428571428));
}

#[test]
fn kendall_tau_b_refuses_non_finite_scores() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(kendall_tau_b(&with(0, bad), &Y), None, "x carrying {bad}");
        assert_eq!(kendall_tau_b(&X, &with(3, bad)), None, "y carrying {bad}");
    }
}

#[test]
fn kendall_tau_b_valid_input_unchanged() {
    assert_eq!(kendall_tau_b(&X, &Y), Some(0.8666666666666667));
}

#[test]
fn dissent_rank_leg_unavailable_for_non_finite_arms() {
    let report = dissent(&with(0, f64::INFINITY), &Y).expect("shape is valid");
    assert_eq!(report.tau_b, None);
    assert_eq!(report.rank_dissent, None);
    assert_eq!(report.level_dissent, None);
    assert_eq!(
        report.default_verdict(),
        sharpebench_stats::DissentVerdict::Undetermined
    );
}

#[test]
fn dissent_valid_input_unchanged() {
    let report = dissent(&X, &Y).expect("shape is valid");
    assert_eq!(report.n, 6);
    assert_eq!(report.tau_b, Some(0.8666666666666667));
    assert_eq!(report.rank_dissent, Some(0.06666666666666665));
    assert_eq!(report.level_dissent, Some(0.10416666666666667));
}

#[test]
fn gate_vs_human_refuses_non_finite_inputs() {
    let verdict = [true, false, true, false, true, true];
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            gate_vs_human(&with(0, bad), 2.0, &verdict, Some(&Y)),
            None,
            "gate severity carrying {bad}"
        );
        assert_eq!(
            gate_vs_human(&X, 2.0, &verdict, Some(&with(2, bad))),
            None,
            "human severity carrying {bad}"
        );
        assert_eq!(
            gate_vs_human(&X, bad, &verdict, Some(&Y)),
            None,
            "threshold {bad}"
        );
    }
}

#[test]
fn gate_vs_human_valid_input_unchanged() {
    let verdict = [true, false, true, false, true, true];
    let a = gate_vs_human(&X, 2.0, &verdict, Some(&Y)).expect("shape is valid");
    assert_eq!(a.n, 6);
    assert_eq!(a.observed_agreement, 1.0);
    assert_eq!(a.chance_agreement, 0.5555555555555556);
    assert_eq!(a.kappa, Some(1.0));
    assert_eq!(a.rho, Some(0.9428571428571428));
    assert_eq!(a.gate_lenient, 0);
    assert_eq!(a.gate_strict, 0);
}

#[test]
fn sortino_ratio_refuses_non_finite_inputs() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut r = RETURNS.to_vec();
        r[1] = bad;
        assert_eq!(sortino_ratio(&r, 0.0), None, "return carrying {bad}");
        assert_eq!(sortino_ratio(&RETURNS, bad), None, "target {bad}");
    }
}

#[test]
fn sortino_ratio_valid_input_unchanged() {
    assert_eq!(sortino_ratio(&RETURNS, 0.0), Some(0.5857122361103716));
}
