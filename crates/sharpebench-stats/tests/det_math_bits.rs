//! Bit pins for `sharpebench_stats::det_math`, the deterministic `exp`,
//! `exp_m1`, `ln`, `ln_1p`, `powf` and `powi` the kernel scores with.
//!
//! The arguments are compile-time constants on purpose. An optimised build
//! folds a constant `f64::exp` or `f64::powi` inside the compiler, which need
//! not round like the runtime routine, and that is how
//! `erf(FRAC_1_SQRT_2)` once moved one ulp under `cargo test --release` only.
//! CI runs this file in both profiles, so a replacement of these bodies by a
//! libm call fails here in one of them.
//!
//! The expected bits of the transcendental functions are the correctly
//! rounded values, computed independently with 90-digit decimal arithmetic.
//! The `powi` bits are the IEEE binary-exponentiation loop of
//! `compiler-builtins`' `__powidf2`, evaluated in double precision.

use sharpebench_stats::det_math::{exp, exp_m1, ln, ln_1p, powf, powi};
const EXP_PINS: &[(f64, u64)] = &[
    (-0.5000000000000001, 0x3fe3_68b2_fc6f_9609), // 0.6065306597126333
    (-1e-300, 0x3ff0_0000_0000_0000),             // 1.0
    (1e-10, 0x3ff0_0000_0006_df38),               // 1.0000000001
    (0.5, 0x3ffa_6129_8e1e_069c),                 // 1.6487212707001282
    (-2.9278927520004583, 0x3fab_659e_d67e_62ac), // 0.05350967758494582
    (-4.060797665517086, 0x3f91_a61f_1948_01b8),  // 0.017235265653654713
    (7.0, 0x4091_2288_5aae_ddaa),                 // 1096.6331584284585
    (700.0, 0x7f0d_945d_f4f8_ec8e),               // 1.0142320547350045e+304
    (-745.0, 0x0000_0000_0000_0001),              // 5e-324
];

const EXP_M1_PINS: &[(f64, u64)] = &[
    (1e-10, 0x3ddb_7cdf_d9dd_a4e3),  // 1.00000000005e-10
    (-1e-10, 0xbddb_7cdf_d9d1_d693), // -9.999999999500001e-11
    (1e-05, 0x3ee4_f8bc_681c_dfb6),  // 1.0000050000166668e-05
    (-0.3, 0xbfd0_966f_2c79_07f6),   // -0.2591817793182821
    (0.5, 0x3fe4_c253_1c3c_0d38),    // 0.6487212707001282
    (2.0, 0x4019_8e64_b8d4_ddae),    // 6.38905609893065
    (-20.0, 0xbfef_ffff_fee4_b79b),  // -0.9999999979388464
    (30.0, 0x42a3_7047_0aec_26ed),   // 10686474581523.463
    (1e-17, 0x3c67_0ef5_4646_d497),  // 1e-17
];

const LN_PINS: &[(f64, u64)] = &[
    (1e-300, 0xc085_9634_47f8_7fb5),             // -690.7755278982137
    (0.02425, 0xc00d_c134_a0d2_8448),            // -3.719338661598645
    (0.5, 0xbfe6_2e42_fefa_39ef),                // -0.6931471805599453
    (2.0, 0x3fe6_2e42_fefa_39ef),                // 0.6931471805599453
    (1.0000000000000002, 0x3caf_ffff_ffff_ffff), // 2.2204460492503128e-16
    (0.9999999999999999, 0xbca0_0000_0000_0000), // -1.1102230246251565e-16
    (3.0, 0x3ff1_93ea_7aad_030b),                // 1.0986122886681098
    (1e+300, 0x4085_9634_47f8_7fb5),             // 690.7755278982137
];

const LN_1P_PINS: &[(f64, u64)] = &[
    (1e-10, 0x3ddb_7cdf_d9d1_d693),         // 9.999999999500001e-11
    (-1e-10, 0xbddb_7cdf_d9dd_a4e3),        // -1.00000000005e-10
    (0.001, 0x3f50_6035_4f8c_3ebf),         // 0.0009995003330835331
    (0.5, 0x3fd9_f323_ecbf_984c),           // 0.4054651081081644
    (-0.5, 0xbfe6_2e42_fefa_39ef),          // -0.6931471805599453
    (-0.999, 0xc01b_a18a_998f_ff9f),        // -6.907755278982136
    (10000000000.0, 0x4037_069e_2aa3_184e), // 23.025850930040455
    (1e-17, 0x3c67_0ef5_4646_d497),         // 1e-17
];

const POWF_PINS: &[(f64, f64, u64)] = &[
    (2.0, 0.5, 0x3ff6_a09e_667f_3bcd),          // 1.4142135623730951
    (0.05, 1.0 / 600.0, 0x3fef_d733_40d1_fa02), // 0.9950195566196387
    (0.3, 2.5, 0x3fa9_3d32_bcea_fc29),          // 0.049295030175464945
    (10.0, -3.0, 0x3f50_624d_d2f1_a9fc),        // 0.001
    (1.5, 0.001, 0x3ff0_01a9_3f47_a2b4),        // 1.000405547320196
    (2.0, 3.0, 0x4020_0000_0000_0000),          // 8.0
    (-2.0, 3.0, 0xc020_0000_0000_0000),         // -8.0
    (-2.0, -2.0, 0x3fd0_0000_0000_0000),        // 0.25
    (0.001, 0.37, 0x3fb3_df36_8df4_157d),       // 0.07762471166286918
    (7.0, -1.0 / 3.0, 0x3fe0_ba6e_e5f9_aad4),   // 0.5227579585747102
];

const POWI_PINS: &[(f64, i32, u64)] = &[
    (1.1000000000052752, 3, 0x3ff5_4bc6_a7f0_ee91), // 1.3310000000191489
    (1.1000000000035168, 4, 0x3ff7_6cf4_1f22_76dc), // 1.4641000000187239
    (1.1000000000052752, -3, 0x3fe8_0ac5_565a_7389), // 0.7513148008907687
    (0.7, 7, 0x3fb5_152b_e12f_5a5d),                // 0.08235429999999995
    (1.3, 0, 0x3ff0_0000_0000_0000),                // 1.0
    (-1.25, 5, 0xc008_6a00_0000_0000),              // -3.0517578125
];

fn check(name: &str, pins: &[(f64, u64)], f: fn(f64) -> f64) {
    for &(x, expected) in pins {
        let got = f(x).to_bits();
        assert_eq!(
            got,
            expected,
            "{name}({x:e}) moved: expected bits 0x{expected:016x} ({:e}), got 0x{got:016x} ({:e})",
            f64::from_bits(expected),
            f64::from_bits(got)
        );
    }
}

#[test]
fn exp_bits_are_pinned() {
    check("exp", EXP_PINS, exp);
}

#[test]
fn exp_m1_bits_are_pinned() {
    check("exp_m1", EXP_M1_PINS, exp_m1);
}

#[test]
fn ln_bits_are_pinned() {
    check("ln", LN_PINS, ln);
}

#[test]
fn ln_1p_bits_are_pinned() {
    check("ln_1p", LN_1P_PINS, ln_1p);
}

#[test]
fn powf_bits_are_pinned() {
    for &(x, y, expected) in POWF_PINS {
        let got = powf(x, y).to_bits();
        assert_eq!(got, expected, "powf({x:e}, {y:e}) moved: got 0x{got:016x}");
    }
}

#[test]
fn powi_bits_are_pinned() {
    for &(x, n, expected) in POWI_PINS {
        let got = powi(x, n).to_bits();
        assert_eq!(got, expected, "powi({x:e}, {n}) moved: got 0x{got:016x}");
    }
}
