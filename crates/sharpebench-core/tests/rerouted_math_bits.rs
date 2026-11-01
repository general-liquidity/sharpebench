//! Bit pins for kernel values that pass through `sharpebench_stats::det_math`.
//!
//! These call sites used `f64::exp`, `ln`, `ln_1p`, `exp_m1`, `powf` and
//! `powi`, whose bits depend on the build profile (an optimised build folds a
//! constant call inside the compiler) and on the platform library. The inputs
//! below are compile-time constants so that the release build gets every
//! chance to fold. CI runs the file in debug and release; the expected bits
//! were recorded from the deterministic implementation and must not move.

use sharpebench_core::decay::{crowding_half_life, edge_half_life, CrowdingParams};
use sharpebench_core::det_math;
use sharpebench_core::greeks::{bs_greeks, bs_price};
use sharpebench_core::stats::{kurtosis, skewness};
use sharpebench_stats::opt_in_diagnostics::manipulation_proof_performance;

const RETURNS: [f64; 12] = [
    0.012, -0.007, 0.003, 0.021, -0.015, 0.004, 0.009, -0.002, 0.017, -0.011, 0.006, 0.001,
];

fn values() -> Vec<(&'static str, f64)> {
    let g = bs_greeks(100.0, 105.0, 0.5, 0.03, 0.2, true).expect("greeks");
    let crowd = crowding_half_life(
        0.4,
        CrowdingParams {
            theta: 0.01,
            delta_max: 0.05,
            curvature: 1.7,
        },
    );
    // An input where the platform `exp` inside `norm_pdf` rounds the other way
    // on x86_64-pc-windows-gnu (found by search: 80 of 20,000 such inputs).
    let sensitive = bs_greeks(51.78125, 45.0, 1.25, 0.02, 0.37, false).expect("greeks");
    let ic: Vec<f64> = (0..30).map(|t| 0.2 * det_math::powi(0.93, t)).collect();
    vec![
        (
            "bs_price call",
            bs_price(100.0, 105.0, 0.5, 0.03, 0.2, true).unwrap(),
        ),
        (
            "bs_price put",
            bs_price(100.0, 95.0, 1.25, 0.01, 0.35, false).unwrap(),
        ),
        ("delta", g.delta),
        ("gamma", g.gamma),
        ("theta", g.theta),
        ("vega", g.vega),
        ("rho", g.rho),
        ("gamma, libm-sensitive input", sensitive.gamma),
        ("crowding_decay", crowd.crowding_decay),
        ("crowding half-life", crowd.expected_half_life.unwrap()),
        ("edge_half_life", edge_half_life(&ic).unwrap()),
        ("skewness", skewness(&RETURNS)),
        ("kurtosis", kurtosis(&RETURNS)),
        (
            "mppm rho=3",
            manipulation_proof_performance(&RETURNS, 3.0, 252.0).unwrap(),
        ),
        (
            "mppm rho=1",
            manipulation_proof_performance(&RETURNS, 1.0, 252.0).unwrap(),
        ),
    ]
}

const PINS: &[(&str, u64)] = &[
    ("bs_price call", 0x4010_b692_4d59_6f60), // 4.178292473395999
    ("bs_price put", 0x4028_475b_eee3_7518),  // 12.139373269334598
    ("delta", 0x3fdb_b99e_d1aa_0ee0),         // 0.4332043693068339
    ("gamma", 0x3f9c_7b0c_9769_44cf),         // 0.02781314539089835
    ("theta", 0xc01a_f294_2fc2_f6cf),         // -6.736893411898293
    ("vega", 0x403b_d02a_4bdc_cd34),          // 27.813145390898356
    ("rho", 0x4033_9231_ca21_adb3),           // 19.571072228643697
    ("gamma, libm-sensitive input", 0x3f8f_bbaa_79e7_86d7),
    ("crowding_decay", 0x3f85_914e_a110_d34c), // 0.0105310576346739
    ("crowding half-life", 0x4040_e165_7c42_d9ed), // 33.76090958847258
    ("edge_half_life", 0x4023_1a48_e8f8_44da), // 9.551337509447354
    ("skewness", 0xbfae_b93e_b8b8_5233),       // -0.06000705723650803
    ("kurtosis", 0x4001_7b15_37d2_fdf0),       // 2.1850990639938956
    ("mppm rho=3", 0x3fe8_324c_7a82_3fb7),     // 0.756139983439417
    ("mppm rho=1", 0x3fe9_1015_2fa6_fc34),     // 0.7832132273396213
];

#[test]
fn rerouted_kernel_values_are_pinned() {
    let got = values();
    assert_eq!(got.len(), PINS.len());
    for ((name, v), &(pin_name, bits)) in got.iter().zip(PINS) {
        assert_eq!(*name, pin_name);
        assert_eq!(
            v.to_bits(),
            bits,
            "{name} moved: expected 0x{bits:016x} ({:e}), got 0x{:016x} ({v:e})",
            f64::from_bits(bits),
            v.to_bits()
        );
    }
}
