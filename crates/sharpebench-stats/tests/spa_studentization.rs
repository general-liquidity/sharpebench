//! Finite, nontrivial SPA regressions. Expected exceedance counts were checked
//! independently with the rational oracle in spa_reference.py. None of the
//! three fixtures has a threshold tie. These pin arithmetic, not nominal coverage.

use sharpebench_stats::significance::{spa_consistent_pvalue, spa_pvalue};

fn heterogeneous_field() -> Vec<Vec<f64>> {
    vec![
        vec![
            0.28125, -0.5, 0.75, -0.25, 0.5, -0.75, 0.25, 0.0, 0.5, -0.25, 0.75, -0.5, 0.25, 0.0,
            -0.25, 0.5,
        ],
        vec![
            4.0, -8.0, 2.0, -4.0, 8.0, -2.0, 0.0, 4.0, -8.0, 2.0, -4.0, 8.0, -2.0, 0.0, 4.0, -2.0,
        ],
        vec![
            -0.5, -1.5, 0.0, -2.0, -0.5, -1.0, -1.5, 0.0, -2.0, -0.5, -1.0, -1.5, 0.0, -2.0, -0.5,
            -1.0,
        ],
    ]
}

#[test]
fn spa_studentization_pins_nontrivial_exceedance_counts() {
    // Neither a p-value floor nor one: changing centered variance or dividing
    // by the wrong scale must change a visible result on this mixed field.
    let field = heterogeneous_field();
    assert_eq!(spa_pvalue(&field, 7, 127, 0.5).unwrap(), 35.0 / 128.0);
    assert_eq!(
        spa_consistent_pvalue(&field, 7, 127, 0.5).unwrap(),
        27.0 / 128.0
    );
}

#[test]
fn spa_studentization_pins_the_declared_scale_floor() {
    let mut field = heterogeneous_field();
    for observation in &mut field[0] {
        *observation *= 2.0_f64.powi(-27);
    }
    // Only the first scale hits the 1e-8 floor. Without it, multiplying every
    // variance by a common factor cancels between observed and bootstrap
    // statistics in the untrimmed test. This fixture makes that boundary
    // observable while the other two scales remain unconstrained.
    assert_eq!(spa_pvalue(&field, 7, 127, 0.5).unwrap(), 75.0 / 128.0);
    assert_eq!(
        spa_consistent_pvalue(&field, 7, 127, 0.5).unwrap(),
        58.0 / 128.0
    );
}

#[test]
fn spa_studentization_centers_its_bootstrap_variance() {
    // Seven draws intentionally leave a visible bootstrap column mean. Using
    // (row + mean)^2 instead of (row - mean)^2 changes both exceedance counts.
    // This small draw count is an arithmetic fixture, not an inference setting.
    let field = heterogeneous_field();
    assert_eq!(spa_pvalue(&field, 2, 7, 0.5).unwrap(), 7.0 / 8.0);
    assert_eq!(spa_consistent_pvalue(&field, 2, 7, 0.5).unwrap(), 6.0 / 8.0);
}
