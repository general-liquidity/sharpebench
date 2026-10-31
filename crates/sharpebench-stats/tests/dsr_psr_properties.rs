//! Seeded property tests for the Probabilistic and Deflated Sharpe Ratios.
//!
//! The workspace keeps its dependency surface small (see `deny.toml`), so these
//! use a seeded SplitMix64 loop instead of a property-testing crate. Every seed
//! and case count is fixed, so a failure reproduces exactly and the suite stays
//! fast in CI. A failing case prints its seed and case index.
//!
//! Each property is stated over the domain where it holds as mathematics. The
//! PSR here uses the plug-in standard error at the observed Sharpe (Bailey and
//! Lopez de Prado 2012, 2014), so monotonicity in the observed Sharpe holds
//! only where `1 - g3 (s + b) / 2 + k s b > 0`, with `k = (g4 - 1) / 4`. Outside
//! that region a higher Sharpe can lower the PSR. That is pinned as its own test
//! below, so the restriction is visible rather than hidden in a generator.

use sharpebench_stats::deflated_sharpe::{
    checked_probabilistic_sharpe_ratio, deflated_sharpe_ratio_against_null,
    probabilistic_sharpe_ratio, sharpe_ratio,
};
use sharpebench_stats::stats::{kurtosis, skewness};

const SEED: u64 = 0x5EED_D5A0_2026_1101;
const CASES: usize = 300;

/// SplitMix64 (Steele, Lea and Flood 2014), the generator the crate itself
/// uses for its bootstraps. Reimplemented here because the crate's copy is
/// private.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// A non-constant return track of 8 to 199 observations with a drift, a
/// volatility, and a random share of one-sided jumps so skewness and kurtosis
/// vary across cases.
fn track(rng: &mut SplitMix64) -> Vec<f64> {
    let n = 8 + rng.below(192);
    let drift = rng.range(-0.01, 0.01);
    let vol = rng.range(0.001, 0.05);
    let jump_prob = rng.range(0.0, 0.2);
    let jump = rng.range(-0.2, 0.2);
    loop {
        let xs: Vec<f64> = (0..n)
            .map(|_| {
                let u = rng.range(-1.0, 1.0);
                let j = if rng.unit() < jump_prob { jump } else { 0.0 };
                drift + vol * u + j
            })
            .collect();
        if xs.iter().any(|x| *x != xs[0]) {
            return xs;
        }
    }
}

fn each_case(salt: u64, mut check: impl FnMut(usize, &mut SplitMix64, &[f64])) {
    let mut rng = SplitMix64(SEED ^ salt);
    for case in 0..CASES {
        let xs = track(&mut rng);
        check(case, &mut rng, &xs);
    }
}

fn dsr(xs: &[f64], n_trials: u32, null: f64, sr_std: f64) -> f64 {
    deflated_sharpe_ratio_against_null(xs, n_trials, null, sr_std).expect("a valid track")
}

/// Trial counts dense at the small end, where the expected maximum moves most,
/// then sparse up to a million.
fn trial_counts() -> Vec<u32> {
    let mut counts: Vec<u32> = (1..=200).collect();
    let mut n = 250u32;
    while n <= 1_000_000 {
        counts.push(n);
        n = n * 5 / 4;
    }
    counts
}

/// Admitting more trials can only raise the bar a track has to clear. At one
/// trial there is nothing to deflate for and the DSR is the PSR against the
/// null.
#[test]
fn dsr_is_non_increasing_in_the_trial_count() {
    let counts = trial_counts();
    each_case(1, |case, rng, xs| {
        let sr_std = rng.range(0.0, 0.5);
        let null = rng.range(-0.1, 0.1);
        let psr = checked_probabilistic_sharpe_ratio(xs, null).unwrap();
        assert_eq!(dsr(xs, 1, null, sr_std), psr, "case {case}: one trial");
        let mut previous = psr;
        for &n in &counts {
            let current = dsr(xs, n, null, sr_std);
            assert!(
                current <= previous,
                "case {case}: DSR rose from {previous} to {current} at {n} trials \
                 (sr_std {sr_std}, null {null})"
            );
            previous = current;
        }
    });
}

/// Sharpe, skewness and kurtosis are all invariant under a positive rescaling
/// of returns, so the PSR and DSR are too. A power-of-two scale is exact in
/// floating point and must leave the bits unchanged; any other positive scale
/// may move the last few bits of the moments and is held to 1e-9.
#[test]
fn psr_and_dsr_are_invariant_to_positive_rescaling() {
    each_case(2, |case, rng, xs| {
        let b = rng.range(-0.3, 0.3);
        let n = 1 + rng.below(1000) as u32;
        let sr_std = rng.range(0.0, 0.3);
        let psr = probabilistic_sharpe_ratio(xs, b);
        let deflated = dsr(xs, n, b, sr_std);

        let exponent = rng.below(21) as i32 - 10;
        let pow2 = 2f64.powi(exponent);
        let exact: Vec<f64> = xs.iter().map(|x| x * pow2).collect();
        assert_eq!(
            probabilistic_sharpe_ratio(&exact, b).to_bits(),
            psr.to_bits(),
            "case {case}: PSR moved under scale 2^{exponent}"
        );
        assert_eq!(
            dsr(&exact, n, b, sr_std).to_bits(),
            deflated.to_bits(),
            "case {case}: DSR moved under scale 2^{exponent}"
        );

        let c = 10f64.powf(rng.range(-3.0, 3.0));
        let scaled: Vec<f64> = xs.iter().map(|x| x * c).collect();
        let psr_c = probabilistic_sharpe_ratio(&scaled, b);
        let dsr_c = dsr(&scaled, n, b, sr_std);
        assert!(
            (psr_c - psr).abs() <= 1e-9,
            "case {case}: PSR {psr} became {psr_c} under scale {c}"
        );
        assert!(
            (dsr_c - deflated).abs() <= 1e-9,
            "case {case}: DSR {deflated} became {dsr_c} under scale {c}"
        );
    });
}

/// Both are probabilities, whatever the benchmark, trial count or dispersion.
#[test]
fn psr_and_dsr_are_probabilities() {
    each_case(3, |case, rng, xs| {
        let b = rng.range(-5.0, 5.0);
        let n = 1 + rng.below(1_000_000) as u32;
        let sr_std = rng.range(0.0, 2.0);
        for (what, value) in [
            ("PSR", probabilistic_sharpe_ratio(xs, b)),
            (
                "checked PSR",
                checked_probabilistic_sharpe_ratio(xs, b).unwrap(),
            ),
            ("DSR", dsr(xs, n, b, sr_std)),
        ] {
            assert!(
                value.is_finite() && (0.0..=1.0).contains(&value),
                "case {case}: {what} {value} outside [0, 1] (b {b}, n {n}, sr_std {sr_std})"
            );
        }
    });
}

/// Whether `d/ds [(s - b) / sqrt(1 - g3 s + k s^2)] > 0` at observed Sharpe `s`.
/// The derivative's sign is that of `1 - g3 (s + b) / 2 + k s b`, linear in
/// `s`, so it is positive on an interval iff it is positive at both ends.
fn psr_increasing_at(s: f64, b: f64, g3: f64, g4: f64) -> bool {
    let k = (g4 - 1.0) / 4.0;
    1.0 - g3 * (s + b) / 2.0 + k * s * b > 0.0
}

/// Shifting every return by a constant moves the mean and leaves the standard
/// deviation, skewness and kurtosis where they were, so it moves the observed
/// Sharpe with the other moments held fixed. Inside the region where the
/// plug-in PSR is increasing in the Sharpe, a larger shift never lowers it.
/// The 1e-12 allowance covers the last-bit drift of the recomputed moments.
#[test]
fn psr_is_non_decreasing_in_the_observed_sharpe_with_moments_fixed() {
    let mut checked = 0usize;
    each_case(4, |case, rng, xs| {
        let b = rng.range(-0.2, 0.2);
        let g3 = skewness(xs);
        let g4 = kurtosis(xs);
        let mut shifts: Vec<f64> = (0..8).map(|_| rng.range(-0.05, 0.05)).collect();
        shifts.sort_by(f64::total_cmp);
        for pair in shifts.windows(2) {
            let lo: Vec<f64> = xs.iter().map(|x| x + pair[0]).collect();
            let hi: Vec<f64> = xs.iter().map(|x| x + pair[1]).collect();
            let (s_lo, s_hi) = (sharpe_ratio(&lo), sharpe_ratio(&hi));
            if !(psr_increasing_at(s_lo, b, g3, g4) && psr_increasing_at(s_hi, b, g3, g4)) {
                continue;
            }
            checked += 1;
            let (p_lo, p_hi) = (
                probabilistic_sharpe_ratio(&lo, b),
                probabilistic_sharpe_ratio(&hi, b),
            );
            assert!(
                s_hi >= s_lo && p_hi >= p_lo - 1e-12,
                "case {case}: Sharpe {s_lo} -> {s_hi} moved PSR {p_lo} -> {p_hi} (b {b})"
            );
        }
    });
    assert!(
        checked >= CASES * 5,
        "the generator left the property almost untested: {checked} pairs"
    );
}

/// Outside that region the property fails, and it fails in the formula, not in
/// the code: the plug-in variance `1 - g3 s + k s^2` grows with `s^2`, so on a
/// positively skewed track with a large per-period Sharpe the standard error
/// grows faster than the Sharpe. Here a five-bar track with one jump (skewness
/// 1.5, kurtosis 3.25) is shifted from a per-period Sharpe of 4.9 to 22.8 and
/// its PSR against 0 falls. Changing the convention is a methodology version
/// bump (see `probabilistic_sharpe_ratio`), so this pins the current behaviour.
#[test]
fn psr_can_fall_as_the_sharpe_rises_outside_the_monotone_region() {
    let base = [0.0, 0.0, 0.0, 0.0, 0.01];
    let lower: Vec<f64> = base.iter().map(|x| x + 0.02).collect();
    let higher: Vec<f64> = base.iter().map(|x| x + 0.1).collect();
    let (s_lo, s_hi) = (sharpe_ratio(&lower), sharpe_ratio(&higher));
    assert!(s_hi > s_lo);
    assert!(!psr_increasing_at(
        s_lo,
        0.0,
        skewness(&lower),
        kurtosis(&lower)
    ));
    let (p_lo, p_hi) = (
        probabilistic_sharpe_ratio(&lower, 0.0),
        probabilistic_sharpe_ratio(&higher, 0.0),
    );
    assert!(
        p_hi < p_lo - 1e-4,
        "expected the PSR to fall, got {p_lo} -> {p_hi}"
    );
}

/// Negating every return negates the Sharpe and the skewness and leaves the
/// kurtosis, so the plug-in variance is unchanged and the z statistic against
/// a negated benchmark is exactly negated: `PSR(-x, -b) = 1 - PSR(x, b)`. The
/// DSR inherits this only when there is no deflation (one trial, or zero
/// dispersion), because the expected maximum is added to the null on both
/// sides rather than negated with it.
#[test]
fn psr_is_sign_symmetric_and_dsr_is_where_nothing_is_deflated() {
    each_case(5, |case, rng, xs| {
        let b = rng.range(-0.3, 0.3);
        let neg: Vec<f64> = xs.iter().map(|x| -x).collect();
        let sum = probabilistic_sharpe_ratio(xs, b) + probabilistic_sharpe_ratio(&neg, -b);
        assert!(
            (sum - 1.0).abs() <= 4.0 * f64::EPSILON,
            "case {case}: PSR(x, b) + PSR(-x, -b) = {sum}"
        );
        let n = 2 + rng.below(1000) as u32;
        for (trials, sr_std) in [(1, rng.range(0.0, 0.5)), (n, 0.0)] {
            let sum = dsr(xs, trials, b, sr_std) + dsr(&neg, trials, -b, sr_std);
            assert!(
                (sum - 1.0).abs() <= 4.0 * f64::EPSILON,
                "case {case}: undeflated DSR sum {sum} at {trials} trials, sr_std {sr_std}"
            );
        }
    });
}
