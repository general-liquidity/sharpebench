use sharpebench_core::budget_curve::{budget_curve, BudgetCurveOpts};

fn window(mean: f64) -> Vec<f64> {
    (0..40)
        .map(|i| mean + 0.02 * (i as f64 * 0.7).sin())
        .collect()
}

#[test]
fn overflowing_budget_difference_is_refused() {
    let a = window(0.0007);
    let b = window(0.002);
    let opts = BudgetCurveOpts {
        n_boot: 8,
        ..BudgetCurveOpts::default()
    };
    let result = budget_curve(&[(-f64::MAX, &a), (f64::MAX, &b)], &opts);
    let error = result.expect_err("overflowing budget delta cannot masquerade as a plateau");
    assert!(
        error.contains("point 1 budget difference must be finite"),
        "{error}"
    );
}

#[test]
fn overflowing_marginal_is_refused() {
    let a = window(0.0007);
    let b = window(0.002);
    let opts = BudgetCurveOpts {
        n_boot: 8,
        ..BudgetCurveOpts::default()
    };
    for (left, right) in [(&a, &b), (&b, &a)] {
        let result = budget_curve(&[(0.0, left), (f64::from_bits(1), right)], &opts);
        let error = result.expect_err("an infinite marginal cannot enter the report");
        assert!(
            error.contains("point 1 marginal DSR per budget must be finite"),
            "{error}"
        );
    }
}

#[test]
fn extreme_but_representable_budget_arithmetic_remains_available() {
    let a = window(0.0007);
    let b = window(0.002);
    let opts = BudgetCurveOpts {
        n_boot: 8,
        ..BudgetCurveOpts::default()
    };
    let large = budget_curve(&[(0.0, &a), (f64::MAX, &b)], &opts).unwrap();
    let marginal = large.points[1].marginal_dsr_per_budget.unwrap();
    assert!(marginal.is_finite() && marginal > 0.0);
    assert!(large.is_monotone_improving);
    // A subnormal increment is valid when its actual marginal is finite.
    let flat = budget_curve(&[(0.0, &a), (f64::from_bits(1), &a)], &opts).unwrap();
    assert_eq!(flat.points[1].marginal_dsr_per_budget, Some(0.0));
    assert_eq!(flat.non_improvement_onset, Some(f64::from_bits(1)));
}
