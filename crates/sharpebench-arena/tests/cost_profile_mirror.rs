//! The signed board's `CostProfile` documents itself as a mirror of the
//! simulator's cost model. This crate sees both types, so it holds the guard: a
//! cost field the mirror lacks fails here instead of leaving a board unable to
//! say what it was scored under.

use sharpebench_leaderboard::CostProfile;
use sharpebench_sim::{CostModel, ExecutionNoise, FixedDecisionDelay};

#[test]
fn every_simulator_cost_field_survives_the_signed_mirror() {
    let model = CostModel {
        fee_bps: 1.0,
        slippage_bps: 2.0,
        impact_bps: 3.0,
        financing_bps: 4.0,
        max_participation: 0.5,
        trf_cost: Some(0.001),
        noise: Some(ExecutionNoise {
            delay_prob: 0.1,
            min_fill_frac: 0.5,
            carry_floor: 0.01,
            queue_participation_ref: 0.2,
        }),
        short_borrow_bps: 6.0,
        fixed_delay: Some(FixedDecisionDelay::Fifo { bars: 2 }),
    };
    let wire = serde_json::to_value(model).unwrap();
    let profile: CostProfile = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&profile).unwrap(), wire);
}

#[test]
fn a_default_simulator_model_writes_no_field_a_legacy_board_lacks() {
    // `max_participation` is +infinity here, which JSON writes as null.
    let wire = serde_json::to_value(CostModel::default()).unwrap();
    let profile: CostProfile = serde_json::from_value(wire.clone()).unwrap();
    let mirrored = serde_json::to_value(&profile).unwrap();
    assert_eq!(mirrored["fixed_delay"], serde_json::Value::Null);
    assert_eq!(
        mirrored.as_object().unwrap().keys().collect::<Vec<_>>(),
        [
            "fee_bps",
            "financing_bps",
            "impact_bps",
            "max_participation",
            "slippage_bps"
        ]
    );
}
