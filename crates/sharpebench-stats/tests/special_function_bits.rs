//! Bit pins for the hand-rolled special functions in `sharpebench_stats::stats`.
//!
//! `erf`, `norm_cdf` and `norm_ppf` feed PSR, the deflation bar, the DSR
//! interval and the CRPS of a normal forecast, and the golden fixtures print
//! those numbers at full `f64` precision. A 1-ULP change in any of the three
//! moves published bytes. This table freezes the exact bit pattern each function
//! returns for a fixed set of inputs: the branch boundaries of the two
//! approximations, subnormal and exact-zero arguments, the tails where the
//! closed forms saturate, and arguments the kernel passes while scoring the
//! committed golden fields. It is the regression a future replacement of these
//! bodies (a library, a different polynomial, a compiler-driven fold) has to
//! fail loudly against before the golden fixtures are consulted.
//!
//! The 2026-09 measurement against `statrs 0.19.1` is the reason the table
//! exists: see `docs/book/src/methodology-deflated-sharpe.md`, "Numerical
//! implementation of the normal functions". The entries are the outputs of the
//! implementation at that measurement; they are not claimed to be the
//! correctly rounded values (`erf(0)` here is `1e-9`, not `0`), and they must
//! not be "corrected" without regenerating every published artifact.

use sharpebench_stats::stats::{erf, norm_cdf, norm_ppf};

const ERF_PINS: &[(f64, u64)] = &[
    (0e0, 0x3e112e0be0000000),                     // 9.999999717180685e-10
    (-0e0, 0x3e112e0be0000000),                    // 9.999999717180685e-10
    (5e-324, 0x3e112e0be0000000),                  // 9.999999717180685e-10
    (-5e-324, 0xbe112e0be0000000),                 // -9.999999717180685e-10
    (2.2250738585072014e-308, 0x3e112e0be0000000), // 9.999999717180685e-10
    (1e-300, 0x3e112e0be0000000),                  // 9.999999717180685e-10
    (1e-10, 0x3e131e50e0000000),                   // 1.112838599048871e-9
    (1e-3, 0x3f527ccc2b809800),                    // 1.1283868643743311e-3
    (-1e-3, 0xbf527ccc2b809800),                   // -1.1283868643743311e-3
    (4.5144e-2, 0x3faa103be2a645f0),               // 5.09051050349959e-2
    (-4.5144e-2, 0xbfaa103be2a645f0),              // -5.09051050349959e-2
    (3.275911e-1, 0x3fd6d6733445de9a),             // 3.5683899025705446e-1
    (5e-1, 0x3fe0a7efa6731559),                    // 5.205000163047472e-1
    (-5e-1, 0xbfe0a7efa6731559),                   // -5.205000163047472e-1
    (std::f64::consts::FRAC_1_SQRT_2, 0x3fe5d89797a01c9d), // 6.826894723352727e-1
    (1e0, 0x3feaf7676fd90a94),                     // 8.427006897475899e-1
    (-1e0, 0xbfeaf7676fd90a94),                    // -8.427006897475899e-1
    (1.96e0, 0x3fefd256cf62ba4a),                  // 9.944261599064899e-1
    (-1.96e0, 0xbfefd256cf62ba4a),                 // -9.944261599064899e-1
    (3e0, 0x3fefffd1a463781a),                     // 9.999778948511022e-1
    (-3e0, 0xbfefffd1a463781a),                    // -9.999778948511022e-1
    (5.503319120939878e0, 0x3fefffffffffffc0),     // 9.999999999999929e-1
    (6e0, 0x3ff0000000000000),                     // 1e0
    (-6e0, 0xbff0000000000000),                    // -1e0
    (2.7e1, 0x3ff0000000000000),                   // 1e0
    (-2.7e1, 0xbff0000000000000),                  // -1e0
    (1e300, 0x3ff0000000000000),                   // 1e0
    (1.7976931348623157e308, 0x3ff0000000000000),  // 1e0
    (-1.7976931348623157e308, 0xbff0000000000000), // -1e0
    (f64::INFINITY, 0x3ff0000000000000),           // 1e0
    (f64::NEG_INFINITY, 0xbff0000000000000),       // -1e0
];

const NORM_CDF_PINS: &[(f64, u64)] = &[
    (0e0, 0x3fe000000044b830),                  // 5.000000005e-1
    (-0e0, 0x3fe000000044b830),                 // 5.000000005e-1
    (5e-324, 0x3fe000000044b830),               // 5.000000005e-1
    (-5e-324, 0x3fdfffffff768fa1),              // 4.999999995e-1
    (1e-3, 0x3fe00344a6167902),                 // 5.003989452268629e-1
    (-1e-3, 0x3fdff976b3d30dfd),                // 4.9960105477313715e-1
    (6.3835e-2, 0x3fe0d07afa2e7cf7),            // 5.254492651328339e-1
    (-6.3835e-2, 0x3fde5f0a0ba30612),           // 4.7455073486716615e-1
    (5e-1, 0x3fe62075e2f9f520),                 // 6.914624627239938e-1
    (-5e-1, 0x3fd3bf143a0c15c0),                // 3.085375372760062e-1
    (1e0, 0x3feaec4bcbd00e4e),                  // 8.413447361676363e-1
    (-1e0, 0x3fc44ed0d0bfc6c8),                 // 1.5865526383236372e-1
    (1.6448536269514722e0, 0x3fee66666ddb85fc), // 9.500000138907008e-1
    (1.96e0, 0x3fef3337c24c9c55),               // 9.750021738917761e-1
    (-1.96e0, 0x3f999907b66c7560),              // 2.4997826108223875e-2
    (2.5758293035489004e0, 0x3fefd70a2d89cf22), // 9.94999970380807e-1
    (3e0, 0x3feff4f0e9d5a279),                  // 9.986500327186852e-1
    (-3e0, 0x3f561e2c54bb0e00),                 // 1.3499672813147567e-3
    (6e0, 0x3fefffffff77eb2e),                  // 9.99999999009878e-1
    (-6e0, 0x3e11029a30000000),                 // 9.9012192888992e-10
    (8.5e0, 0x3ff0000000000000),                // 1e0
    (-8.5e0, 0x0000000000000000),               // 0e0
    (4e1, 0x3ff0000000000000),                  // 1e0
    (-4e1, 0x0000000000000000),                 // 0e0
    // Arguments the kernel passes while scoring the committed golden fields.
    (4.307722342308559e-1, 0x3fe5557785ed37c0), // 6.666829696421175e-1
    (4.051472909722701e1, 0x3ff0000000000000),  // 1e0
    (1.8197728856386846e1, 0x3ff0000000000000), // 1e0
    (-1.1441085945058784e1, 0x0000000000000000), // 0e0
    (-1.1084057576966553e0, 0x3fc121c7731c9960), // 1.3384335632981514e-1
    (f64::INFINITY, 0x3ff0000000000000),        // 1e0
    (f64::NEG_INFINITY, 0x0000000000000000),    // 0e0
];

const NORM_PPF_PINS: &[(f64, u64)] = &[
    (0e0, 0xfff0000000000000),                     // -inf
    (-0e0, 0xfff0000000000000),                    // -inf
    (-1e0, 0xfff0000000000000),                    // -inf
    (1e0, 0x7ff0000000000000),                     // inf
    (2e0, 0x7ff0000000000000),                     // inf
    (5e-324, 0xc0433bd3f31179e2),                  // -3.846740568497968e1
    (2.2250738585072014e-308, 0xc042c27b05ec351a), // -3.751937936814848e1
    (1e-300, 0xc0428607406c74fe),                  // -3.704709630294563e1
    (1e-10, 0xc019720359482785),                   // -6.36134089949508e0
    (1e-4, 0xc00dc08bb6a0192a),                    // -3.7190164821251033e0
    (1e-3, 0xc008b8cbb6ee2822),                    // -3.090232304709404e0
    // The lower/central branch boundary of the rational approximation.
    (2.425e-2, 0xbfff913f9ae19f39), // -1.9729610490848712e0
    (2.4250000000000004e-2, 0xbfff913f9ae19f39), // -1.9729610490848712e0
    (5e-2, 0xbffa515208ea7fd1),     // -1.6448536251336814e0
    (1e-1, 0xbff4813c3681e9f4),     // -1.2815515641401563e0
    (2.5e-1, 0xbfe5956b87564c41),   // -6.744897502234225e-1
    (5e-1, 0x0000000000000000),     // 0e0
    (7.5e-1, 0x3fe5956b87564c41),   // 6.744897502234225e-1
    (9e-1, 0x3ff4813c3681e9f4),     // 1.2815515641401563e0
    (9.5e-1, 0x3ffa515208ea8020),   // 1.644853625133699e0
    (9.75e-1, 0x3fff5c03325b95a6),  // 1.959963986120195e0
    // The two quantiles `expected_max_sharpe` requests at the default n_trials.
    (9.8e-1, 0x40006e13e872e8e1),               // 2.053748909003034e0
    (9.926424111765711e-1, 0x400383b70908dee3), // 2.4393139558632e0
    (9.9e-1, 0x40029c5c463cecf2),               // 2.326347874388028e0
    (9.95e-1, 0x40049b4c653a0a4b),              // 2.5758293064439264e0
    (9.9975e-1, 0x400bd896cfc9a34a),            // 3.480756400433539e0
    (9.999e-1, 0x400dc08bb6a01968),             // 3.719016482125131e0
    (9.99999e-1, 0x40130381a9cfa8cd),           // 4.753424313830874e0
    (9.999999999e-1, 0x40197203586ddb29),       // 6.361340886788448e0
    (9.999999999999998e-1, 0x40204074bd8488cb), // 8.125890657833585e0
    (9.999999999999999e-1, 0x40206b48525582da), // 8.209536145151493e0
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
