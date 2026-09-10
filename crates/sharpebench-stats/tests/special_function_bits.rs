//! Bit pins for the special functions in `sharpebench_stats::stats`.
//!
//! `erf`, `norm_cdf` and `norm_ppf` feed PSR, the deflation bar, the DSR
//! interval and the CRPS of a normal forecast, and the golden fixtures print
//! those numbers at full `f64` precision. A 1-ULP change in any of the three
//! moves published bytes. This table freezes the exact bit pattern each function
//! returns for a fixed set of inputs: the branch boundaries the pre-migration
//! approximations used, subnormal and exact-zero arguments, the tails where the
//! functions saturate, and arguments the kernel passes while scoring the
//! committed golden fields. It is the regression a future replacement of these
//! bodies (a library, a different polynomial, a compiler-driven fold) has to
//! fail loudly against before the golden fixtures are consulted.
//!
//! The table was regenerated on 2026-09-10 for the `statrs` 0.19.1 migration
//! and belongs to the release that carries it. The values v0.19.0 shipped are
//! the pre-migration ones, produced by Abramowitz and Stegun 7.1.26 and
//! Acklam's rational approximation; 70 of the 93 entries moved, including
//! `erf(0)`, which was `1e-9` and is now exactly `0`. The evidence, the
//! per-function accuracy comparison and the artifact impact are in
//! `docs/audits/2026-09-09/NUMERICS-MIGRATION.md`.
//!
//! These entries are what the current implementation returns. They are close to
//! the correctly rounded values but are not claimed to be them: `statrs`'s
//! `erf` carries about 5e-11 of absolute error, and a pin is a change detector,
//! not a correctness proof. They must not be "corrected" without regenerating
//! every artifact that prints a number derived from them.

use sharpebench_stats::stats::{erf, norm_cdf, norm_ppf};

const ERF_PINS: &[(f64, u64)] = &[
    (0e0, 0x0000000000000000),                             // 0.0
    (-0e0, 0x0000000000000000),                            // 0.0
    (5e-324, 0x0000000000000001),                          // 5e-324
    (-5e-324, 0x8000000000000001),                         // -5e-324
    (2.2250738585072014e-308, 0x00120dd750429b6d),         // 2.5107269871883543e-308
    (1e-300, 0x01a82e6d98711d3a),                          // 1.1283791670955126e-300
    (1e-10, 0x3ddf044332d68161),                           // 1.1283791670955126e-10
    (1e-3, 0x3f527cc3804d374c),                            // 0.0011283787909692365
    (-1e-3, 0xbf527cc3804d374c),                           // -0.0011283787909692365
    (4.5144e-2, 0x3faa1037356c6e79),                       // 0.05090496565955233
    (-4.5144e-2, 0xbfaa1037356c6e79),                      // -0.05090496565955233
    (3.275911e-1, 0x3fd6d6735bbaac9a),                     // 0.35683902700356784
    (5e-1, 0x3fe0a7ef5c1223f8),                            // 0.5204998777636538
    (-5e-1, 0xbfe0a7ef5c1223f8),                           // -0.5204998777636538
    (std::f64::consts::FRAC_1_SQRT_2, 0x3fe5d897a23de9f4), // 0.6826894921098856
    (1e0, 0x3feaf767a7401241),                             // 0.8427007929427149
    (-1e0, 0xbfeaf767a7401241),                            // -0.8427007929427149
    (1.96e0, 0x3fefd2570d6cfa09),                          // 0.9944262754650605
    (-1.96e0, 0xbfefd2570d6cfa09),                         // -0.9944262754650605
    (3e0, 0x3fefffd1ac4135ea),                             // 0.9999779095029997
    (-3e0, 0xbfefffd1ac4135ea),                            // -0.9999779095029997
    (5.503319120939878e0, 0x3fefffffffffffc0),             // 0.9999999999999929
    (6e0, 0x3ff0000000000000),                             // 1.0
    (-6e0, 0xbff0000000000000),                            // -1.0
    (2.7e1, 0x3ff0000000000000),                           // 1.0
    (-2.7e1, 0xbff0000000000000),                          // -1.0
    (1e300, 0x3ff0000000000000),                           // 1.0
    (1.7976931348623157e308, 0x3ff0000000000000),          // 1.0
    (-1.7976931348623157e308, 0xbff0000000000000),         // -1.0
    (f64::INFINITY, 0x3ff0000000000000),                   // 1.0
    (f64::NEG_INFINITY, 0xbff0000000000000),               // -1.0
];

const NORM_CDF_PINS: &[(f64, u64)] = &[
    (0e0, 0x3fe0000000000000),                  // 0.5
    (-0e0, 0x3fe0000000000000),                 // 0.5
    (5e-324, 0x3fe0000000000000),               // 0.5
    (-5e-324, 0x3fe0000000000000),              // 0.5
    (1e-3, 0x3fe00344a4786030),                 // 0.500398942213911
    (-1e-3, 0x3fdff976b70f3f9f),                // 0.49960105778608893
    (6.3835e-2, 0x3fe0d07ad4c4ae37),            // 0.5254491954451116
    (-6.3835e-2, 0x3fde5f0a5676a392),           // 0.4745508045548884
    (5e-1, 0x3fe62075e232ac77),                 // 0.6914624612740131
    (-5e-1, 0x3fd3bf143b9aa712),                // 0.3085375387259869
    (1e0, 0x3feaec4bd11ef4fa),                  // 0.8413447460549428
    (-1e0, 0x3fc44ed0bb842c1a),                 // 0.15865525394505725
    (1.6448536269514722e0, 0x3fee666666661bfe), // 0.9499999999978852
    (1.96e0, 0x3fef33379d3bfae9),               // 0.9750021048529024
    (-1.96e0, 0x3f99990c5880a2dc),              // 0.02499789514709759
    (2.5758293035489004e0, 0x3fefd70a3d70ab47), // 0.9950000000002114
    (3e0, 0x3feff4f10f033f1a),                  // 0.9986501019684255
    (-3e0, 0x3f561de1f981cbb1),                 // 0.0013498980315744642
    (6e0, 0x3fefffffff786788),                  // 0.9999999990134123
    (-6e0, 0x3e10f30ef00af678),                 // 9.865876450619014e-10
    (8.5e0, 0x3ff0000000000000),                // 1.0
    (-8.5e0, 0x3c65dbbaccf6944a),               // 9.479534822701647e-18
    (4e1, 0x3ff0000000000000),                  // 1.0
    (-4e1, 0x0000000000000000),                 // 0.0
    // Arguments the kernel passes while scoring the committed golden fields.
    (4.307722342308559e-1, 0x3fe5557798d2e816), // 0.6666830048409362
    (4.051472909722701e1, 0x3ff0000000000000),  // 1.0
    (1.8197728856386846e1, 0x3ff0000000000000), // 1.0
    (-1.1441085945058784e1, 0x39ba6da3df2a7727), // 1.3030148907742227e-30
    (-1.1084057576966553e0, 0x3fc121c70f061cd6), // 0.13384330972278374
    (f64::INFINITY, 0x3ff0000000000000),        // 1.0
    (f64::NEG_INFINITY, 0x0000000000000000),    // 0.0
];

const NORM_PPF_PINS: &[(f64, u64)] = &[
    (0e0, 0xfff0000000000000),                     // -inf
    (-0e0, 0xfff0000000000000),                    // -inf
    (-1e0, 0xfff0000000000000),                    // -inf
    (1e0, 0x7ff0000000000000),                     // inf
    (2e0, 0x7ff0000000000000),                     // inf
    (5e-324, 0xc0433bd3f27fcd03),                  // -38.467405617144344
    (2.2250738585072014e-308, 0xc042c27b05bf1a0b), // -37.5193793471445
    (1e-300, 0xc04286074064c26f),                  // -37.04709629936121
    (1e-10, 0xc0197203597a2155),                   // -6.361340902404057
    (1e-4, 0xc00dc08bb712893c),                    // -3.719016485455681
    (1e-3, 0xc008b8cbb7204470),                    // -3.090232306167813
    // The lower/central branch boundary the pre-migration Acklam body used.
    (2.425e-2, 0xbfff913f9b7aa943),              // -1.972961051311885
    (2.4250000000000004e-2, 0xbfff913f9b7aa943), // -1.972961051311885
    (5e-2, 0xbffa515209676abc),                  // -1.6448536269514724
    (1e-1, 0xbff4813c36e26d33),                  // -1.2815515655446006
    (2.5e-1, 0xbfe5956b87528a4a),                // -0.6744897501960818
    (5e-1, 0x0000000000000000),                  // 0.0
    (7.5e-1, 0x3fe5956b87528a4a),                // 0.6744897501960818
    (9e-1, 0x3ff4813c36e26d33),                  // 1.2815515655446006
    (9.5e-1, 0x3ffa515209676abc),                // 1.6448536269514724
    (9.75e-1, 0x3fff5c0331eeff83),               // 1.9599639845400538
    // The two quantiles `expected_max_sharpe` requests at the default n_trials.
    (9.8e-1, 0x40006e13e8aadfdb),               // 2.0537489106318225
    (9.926424111765711e-1, 0x400383b708c3f809), // 2.4393139538578947
    (9.9e-1, 0x40029c5c4630ff0e),               // 2.3263478740408408
    (9.95e-1, 0x40049b4c64d69161),              // 2.575829303548901
    (9.9975e-1, 0x400bd896d05013ca),            // 3.4807564043462422
    (9.999e-1, 0x400dc08bb712897b),             // 3.719016485455709
    (9.99999e-1, 0x40130381a97985f0),           // 4.753424308817088
    (9.999999999e-1, 0x40197203589fd4f7),       // 6.361340889697423
    (9.999999999999998e-1, 0x40204074bdbf8865), // 8.125890664701908
    (9.999999999999999e-1, 0x40206b48528cea52), // 8.209536151601387
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
fn erf_bits_are_pinned() {
    check("erf", ERF_PINS, erf);
}

#[test]
fn norm_cdf_bits_are_pinned() {
    check("norm_cdf", NORM_CDF_PINS, norm_cdf);
}

#[test]
fn norm_ppf_bits_are_pinned() {
    check("norm_ppf", NORM_PPF_PINS, norm_ppf);
}

#[test]
fn nan_propagates_through_every_special_function() {
    // Not a bit pin (NaN payloads are not stable), but the contract a
    // replacement must keep: a NaN argument yields NaN, never a panic.
    assert!(erf(f64::NAN).is_nan());
    assert!(norm_cdf(f64::NAN).is_nan());
    assert!(norm_ppf(f64::NAN).is_nan());
}
