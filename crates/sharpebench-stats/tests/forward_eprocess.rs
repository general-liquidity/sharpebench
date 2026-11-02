//! The forward e-process: its size under many nulls, its power against the
//! audit note's Brownian benchmarks, the cap and breach rules, and e-BH.
//!
//! The note is scratchpad/sota-audit/02-statistics.md, item 2, with its script
//! calc/eproc_power.py. Its table (unit annual volatility, annual Sharpe s,
//! horizon T years) is reproduced here column by column: the fixed-horizon
//! and oracle likelihood-ratio columns in closed form to the note's three
//! decimals, and the normal-mixture column by an independent Monte Carlo
//! within its error. Every simulation is offline, seeded, and uses no data.

use sharpebench_stats::forward_eprocess::{
    bet_cap, e_benjamini_hochberg, fixed_horizon_power, oracle_likelihood_ratio_power, BettorState,
    BET_CAP_FRACTION, DEFAULT_LOSS_BOUND, WARMUP_BARS, WEALTH_CEILING,
};
use sharpebench_stats::stats::norm_cdf;

/// SplitMix64, as the crate's bootstrap uses, kept local to the test.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in (0, 1).
    fn unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    /// Standard normal by Box-Muller.
    fn normal(&mut self) -> f64 {
        let (u, v) = (self.unit(), self.unit());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

/// Binomial standard error of a rate estimated from `n` paths.
fn standard_error(rate: f64, n: usize) -> f64 {
    (rate * (1.0 - rate) / n as f64).sqrt()
}

// ---------------------------------------------------------------- power

/// The note's table: (alpha, s, T, fixed, oracle LR, normal mixture).
const NOTE_TABLE: [(f64, f64, f64, f64, f64, f64); 4] = [
    (0.00427, 1.0, 10.0, 0.703, 0.553, 0.421),
    (0.00427, 1.0, 20.0, 0.967, 0.909, 0.868),
    (0.00427, 1.5, 10.0, 0.983, 0.939, 0.910),
    (0.05, 1.0, 10.0, 0.935, 0.851, 0.769),
];

#[test]
fn closed_form_power_reproduces_the_note_table_to_three_decimals() {
    for (alpha, s, years, fixed, oracle, _) in NOTE_TABLE {
        let f = fixed_horizon_power(s, years, alpha).unwrap();
        let o = oracle_likelihood_ratio_power(s, years, alpha).unwrap();
        assert!((f - fixed).abs() <= 6e-4, "fixed {alpha} {s} {years}: {f}");
        assert!(
            (o - oracle).abs() <= 6e-4,
            "oracle {alpha} {s} {years}: {o}"
        );
        // An anytime-valid test never beats the fixed test at its horizon.
        assert!(o < f);
    }
    // PROTOCOL.md's 0.70 ceiling at s = 1 over 10 years, 12 entrants at 0.05.
    let ceiling = fixed_horizon_power(1.0, 10.0, 0.05 / 12.0).unwrap();
    assert!((ceiling - 0.70).abs() < 0.01, "{ceiling}");
}

fn years_to(power: impl Fn(f64) -> f64, target: f64) -> f64 {
    // The note's grid: 1, 1.25, 1.5, ... years.
    (0..400)
        .map(|i| 1.0 + 0.25 * i as f64)
        .find(|&t| power(t) >= target)
        .expect("reached within the grid")
}

#[test]
fn years_to_eighty_percent_power_match_the_note() {
    for (alpha, fixed, oracle) in [(0.00427, 12.25, 15.25), (0.05, 6.25, 8.75)] {
        let f = years_to(|t| fixed_horizon_power(1.0, t, alpha).unwrap(), 0.8);
        let o = years_to(
            |t| oracle_likelihood_ratio_power(1.0, t, alpha).unwrap(),
            0.8,
        );
        assert_eq!((f, o), (fixed, oracle), "alpha {alpha}");
    }
}

#[test]
fn power_inputs_at_the_domain_boundary_are_refused_or_accepted() {
    for (s, t, a) in [
        (0.0, 10.0, 0.05),
        (f64::NAN, 10.0, 0.05),
        (1.0, 0.0, 0.05),
        (1.0, f64::INFINITY, 0.05),
        (1.0, 10.0, 0.0),
        (1.0, 10.0, 1.0),
    ] {
        assert!(fixed_horizon_power(s, t, a).is_err(), "{s} {t} {a}");
        assert!(
            oracle_likelihood_ratio_power(s, t, a).is_err(),
            "{s} {t} {a}"
        );
    }
    // Just inside each edge the powers are probabilities.
    for (s, t, a) in [
        (1e-9, 10.0, 0.05),
        (1.0, 1e-9, 0.05),
        (1.0, 10.0, 1e-12),
        (1.0, 10.0, 1.0 - 1e-12),
    ] {
        for power in [
            fixed_horizon_power(s, t, a).unwrap(),
            oracle_likelihood_ratio_power(s, t, a).unwrap(),
        ] {
            assert!((0.0..=1.0 + 1e-9).contains(&power), "{s} {t} {a}: {power}");
        }
    }
}

const STEPS_PER_YEAR: usize = 252;

/// Fraction of `paths` Brownian tracks (unit annual volatility, annual Sharpe
/// `s`, daily bars) on which `crossed` reports a crossing within `years`.
fn brownian_power(
    s: f64,
    years: f64,
    paths: usize,
    seed: u64,
    mut start: impl FnMut() -> Box<dyn FnMut(f64) -> bool>,
) -> f64 {
    let dt = 1.0 / STEPS_PER_YEAR as f64;
    let bars = (years * STEPS_PER_YEAR as f64) as usize;
    let mut rng = Rng(seed);
    let mut hits = 0;
    for _ in 0..paths {
        let mut crossed = start();
        let mut hit = false;
        for _ in 0..bars {
            let x = s * dt + dt.sqrt() * rng.normal();
            hit |= crossed(x);
        }
        hits += usize::from(hit);
    }
    hits as f64 / paths as f64
}

#[test]
fn normal_mixture_column_reproduces_the_note_within_monte_carlo_error() {
    // The note's one-sided normal-mixture e-process with rho = 1:
    // M_t = 2 sqrt(rho / (t + rho)) exp(S^2 / (2 (t + rho))) Phi(S / sqrt(t + rho)).
    // Its column came from 4,000 paths; this is an independent generator.
    const PATHS: usize = 2_000;
    for (alpha, s, years, _, _, mixture) in [NOTE_TABLE[0], NOTE_TABLE[3]] {
        let threshold = (1.0 / alpha).ln();
        let rate = brownian_power(s, years, PATHS, 0x5EED_0001, || {
            let mut sum = 0.0;
            let mut t = 0.0;
            Box::new(move |x| {
                sum += x;
                t += 1.0 / STEPS_PER_YEAR as f64;
                let v = t + 1.0;
                let log_m = 2f64.ln()
                    + 0.5 * (1.0 / v).ln()
                    + sum * sum / (2.0 * v)
                    + norm_cdf(sum / v.sqrt()).ln();
                log_m >= threshold
            })
        });
        let tolerance = 4.0
            * (standard_error(mixture, 4_000).powi(2) + standard_error(mixture, PATHS).powi(2))
                .sqrt();
        assert!(
            (rate - mixture).abs() <= tolerance,
            "alpha {alpha}: {rate} vs {mixture}"
        );
    }
}

/// The shipped bettor on a Brownian track, crossing at `1 / alpha`.
fn betting_power(s: f64, years: f64, alpha: f64, loss_bound: f64, paths: usize) -> f64 {
    brownian_power(s, years, paths, 0x5EED_0002, || {
        let mut state = BettorState::new();
        Box::new(move |x| {
            state.step(x, loss_bound).unwrap();
            state.wealth >= 1.0 / alpha
        })
    })
}

#[test]
fn betting_power_sits_below_the_oracle_and_matches_an_independent_implementation() {
    // Rates from an independent numpy implementation of the same rule
    // (4,000 paths each), recorded in docs/book/src/forward-evidence.md.
    const PATHS: usize = 1_500;
    for (alpha, s, years, bound, reference) in [
        (0.00427, 1.5, 10.0, 0.5, 0.922),
        (0.05, 1.0, 10.0, 0.5, 0.742),
        (0.05, 1.0, 10.0, DEFAULT_LOSS_BOUND, 0.687),
    ] {
        let rate = betting_power(s, years, alpha, bound, PATHS);
        let tolerance = 4.0
            * (standard_error(reference, 4_000).powi(2) + standard_error(reference, PATHS).powi(2))
                .sqrt();
        assert!(
            (rate - reference).abs() <= tolerance,
            "{alpha} {s} {bound}: {rate} vs {reference}"
        );
        let oracle = oracle_likelihood_ratio_power(s, years, alpha).unwrap();
        assert!(
            rate <= oracle + tolerance,
            "{rate} above the oracle {oracle}"
        );
    }
}

// ---------------------------------------------------------------- validity

/// One bar of a null track, given the bettor's state and a regime cell.
type NullDraw = dyn FnMut(&mut Rng, &BettorState, &mut f64) -> f64;

/// The rate at which `sup_t K_t >= 1 / alpha` within `bars`, over `paths`
/// tracks drawn by `draw(rng, state)`. The generator sees the bettor's state,
/// so it can be adversarial, but each draw must have conditional mean at most
/// zero and stay at or above `-loss_bound`.
fn null_rejection_rate(
    alpha: f64,
    loss_bound: f64,
    paths: usize,
    bars: usize,
    seed: u64,
    mut draw: impl FnMut(&mut Rng, &BettorState, &mut f64) -> f64,
) -> f64 {
    let mut rng = Rng(seed);
    let mut rejections = 0;
    for _ in 0..paths {
        let mut state = BettorState::new();
        let mut regime = 0.0;
        for _ in 0..bars {
            let x = draw(&mut rng, &state, &mut regime);
            assert!(x >= -loss_bound, "the generator left the null's support");
            state.step(x, loss_bound).unwrap();
            if state.max_wealth >= 1.0 / alpha {
                rejections += 1;
                break;
            }
        }
        assert!(state.check().is_ok());
    }
    rejections as f64 / paths as f64
}

#[test]
fn size_holds_under_nulls_that_stress_the_bettor() {
    const PATHS: usize = 3_000;
    const BARS: usize = 600;
    let bound = 0.2;
    // Two-point, mean zero: many small gains and a rare loss at the bound, the
    // shape that tempts a plug-in bettor into its largest stakes.
    let rare_crash = move |rng: &mut Rng, _: &BettorState, _: &mut f64| {
        let p = 0.95;
        if rng.unit() < p {
            bound * (1.0 - p) / p
        } else {
            -bound
        }
    };
    // Bounded, mean zero, with volatility switching between two hidden
    // regimes that persist.
    let regimes = |rng: &mut Rng, _: &BettorState, regime: &mut f64| {
        if rng.unit() < 0.02 {
            *regime = 1.0 - *regime;
        }
        let scale = if *regime == 0.0 { 0.01 } else { 0.1 };
        scale * (2.0 * rng.unit() - 1.0)
    };
    // Adversarial: when the bettor stakes a lot, answer with a mean-zero
    // crash-prone draw; when it stakes little, with a mean-zero gain-prone one.
    let adversary = move |rng: &mut Rng, state: &BettorState, _: &mut f64| {
        let lambda = state.bet(bound).unwrap();
        let p = if lambda > 0.5 * bet_cap(bound).unwrap() {
            0.95
        } else {
            0.5
        };
        if rng.unit() < p {
            bound * (1.0 - p) / p
        } else {
            -bound
        }
    };
    // Strictly negative drift.
    let losing =
        |rng: &mut Rng, _: &BettorState, _: &mut f64| -0.002 + 0.05 * (2.0 * rng.unit() - 1.0);

    let mut cases: Vec<(&str, Box<NullDraw>)> = vec![
        ("rare crash", Box::new(rare_crash)),
        ("regimes", Box::new(regimes)),
        ("adversary", Box::new(adversary)),
        ("losing", Box::new(losing)),
    ];
    for alpha in [0.1, 0.05] {
        for (index, (name, draw)) in cases.iter_mut().enumerate() {
            let rate = null_rejection_rate(alpha, bound, PATHS, BARS, 0xA11CE + index as u64, draw);
            let limit = alpha + 3.0 * standard_error(alpha, PATHS);
            assert!(rate <= limit, "{name} at {alpha}: {rate} > {limit}");
        }
    }
}

#[test]
fn a_skilled_track_is_found_where_the_null_is_not() {
    // The same harness under a positive mean rejects often: the size test
    // above is not passing because nothing ever bets.
    let bound = 0.2;
    let rate = null_rejection_rate(0.05, bound, 400, 600, 7, |rng, _, _| {
        0.01 + 0.05 * (2.0 * rng.unit() - 1.0)
    });
    assert!(rate > 0.9, "{rate}");
}

// ---------------------------------------------------------------- the rule

#[test]
fn nothing_is_staked_during_the_warm_up() {
    let mut state = BettorState::new();
    for _ in 0..WARMUP_BARS {
        assert_eq!(state.bet(1.0).unwrap(), 0.0);
        state.step(0.01, 1.0).unwrap();
    }
    assert_eq!(state.wealth, 1.0);
    assert_eq!(state.bars, WARMUP_BARS);
    // The first bar after the warm-up stakes S / Q = 0.2 / 0.002 = 100,
    // which the cap of one half cuts.
    assert_eq!(state.bet(1.0).unwrap(), 0.5);
    assert_eq!(state.bet(100.0).unwrap(), 0.005);
}

/// A state past the warm-up with the given sums.
fn warmed(sum: f64, sum_sq: f64) -> BettorState {
    BettorState {
        bars: WARMUP_BARS,
        sum,
        sum_sq,
        ..BettorState::new()
    }
}

#[test]
fn the_bet_is_the_plug_in_ratio_below_the_cap_and_zero_without_a_positive_mean() {
    assert_eq!(warmed(0.5, 2.0).bet(0.5).unwrap(), 0.25);
    assert_eq!(warmed(4.0, 4.0).bet(1.0).unwrap(), 0.5);
    assert_eq!(warmed(0.0, 1.0).bet(1.0).unwrap(), 0.0);
    assert_eq!(warmed(-0.1, 1.0).bet(1.0).unwrap(), 0.0);
    assert_eq!(warmed(f64::NAN, 1.0).bet(1.0).unwrap(), 0.0);
    // A subnormal second moment overflows the ratio; the cap absorbs it.
    assert_eq!(warmed(1e-300, 5e-324).bet(1.0).unwrap(), 0.5);
    // One bar short of the warm-up bets nothing however good the sums are.
    let mut early = warmed(1.0, 1.0);
    early.bars = WARMUP_BARS - 1;
    assert_eq!(early.bet(1.0).unwrap(), 0.0);
}

#[test]
fn the_cap_is_half_the_inverse_bound_and_holds_every_loss_inside_the_bound_to_half() {
    assert_eq!(BET_CAP_FRACTION, 0.5);
    for bound in [1.0 / 1_048_576.0, 0.25, 0.5, 1.0, 4.0] {
        let cap = bet_cap(bound).unwrap();
        assert_eq!(cap, 0.5 / bound);
        let mut state = warmed(1e6, 1.0);
        assert_eq!(state.bet(bound).unwrap(), cap);
        // The worst bar inside the bound halves the wealth, no more.
        state.step(-bound, bound).unwrap();
        assert_eq!(state.wealth, 0.5);
        assert!(!state.breached);
    }
    for bound in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(bet_cap(bound).is_err());
        assert!(BettorState::new().bet(bound).is_err());
        assert!(BettorState::new().step(0.0, bound).is_err());
    }
}

#[test]
fn a_step_multiplies_the_wealth_by_one_plus_the_stake_times_the_return() {
    let mut state = warmed(0.5, 2.0);
    state.wealth = 2.0;
    state.max_wealth = 2.0;
    state.step(0.1, 0.5).unwrap();
    // stake 0.25, factor 1.025
    assert_eq!(state.wealth, 2.0 * (1.0 + 0.25 * 0.1));
    assert_eq!(state.max_wealth, state.wealth);
    assert_eq!(state.bars, WARMUP_BARS + 1);
    assert_eq!(state.sum, 0.5 + 0.1);
    assert_eq!(state.sum_sq, 2.0 + 0.1 * 0.1);
    state.step(-0.1, 0.5).unwrap();
    assert!(state.wealth < state.max_wealth);
    assert_eq!(state.anytime_p_value(), 1.0 / state.max_wealth);
}

#[test]
fn a_bar_below_the_bound_zeroes_the_wealth_and_stops_the_process() {
    let mut state = warmed(1.0, 1.0);
    state.step(0.3, 0.2).unwrap();
    let peak = state.max_wealth;
    assert!(peak > 1.0);
    // Exactly at the bound is inside it.
    let mut at = state;
    at.step(-0.2, 0.2).unwrap();
    assert!(!at.breached && at.wealth > 0.0);
    state.step(-0.200_000_1, 0.2).unwrap();
    assert!(state.breached && state.is_stopped());
    assert_eq!(state.wealth, 0.0);
    assert_eq!(state.max_wealth, peak);
    assert_eq!(state.bet(0.2).unwrap(), 0.0);
    // Later gains never revive it, but the sums still count every bar.
    let bars = state.bars;
    state.step(0.5, 0.2).unwrap();
    assert_eq!((state.wealth, state.bars), (0.0, bars + 1));
    assert!(state.check().is_ok());
}

#[test]
fn the_wealth_stops_at_the_ceiling_and_stays_finite() {
    let mut state = warmed(1.0, 1.0);
    state.wealth = WEALTH_CEILING / 1.1;
    state.max_wealth = state.wealth;
    state.step(1.0, 1.0).unwrap();
    assert_eq!(state.wealth, WEALTH_CEILING);
    assert!(state.is_stopped());
    assert_eq!(state.bet(1.0).unwrap(), 0.0);
    state.step(-0.5, 1.0).unwrap();
    assert_eq!(state.wealth, WEALTH_CEILING);
    assert!(state.check().is_ok());
    // A product that overflows to infinity is clamped as well.
    let mut huge = warmed(1.0, 1.0);
    huge.wealth = 1e299;
    huge.max_wealth = 1e299;
    huge.step(1e150, 1.0).unwrap();
    assert_eq!(huge.wealth, WEALTH_CEILING);
    // Zero wealth without a breach is stopped too.
    let mut broke = warmed(1.0, 1.0);
    broke.wealth = 0.0;
    assert!(broke.is_stopped());
    broke.step(0.5, 1.0).unwrap();
    assert_eq!(broke.wealth, 0.0);
}

#[test]
fn unusable_observations_leave_the_state_unchanged() {
    let mut state = warmed(1.0, 1.0);
    let before = state;
    assert!(state.step(f64::NAN, 1.0).is_err());
    assert!(state.step(f64::INFINITY, 1.0).is_err());
    assert!(state.step(1e200, 1.0).is_err(), "the square overflows");
    assert_eq!(state, before);
}

#[test]
fn check_refuses_every_state_a_step_cannot_produce() {
    assert!(BettorState::new().check().is_ok());
    assert_eq!(BettorState::default(), BettorState::new());
    let base = BettorState::new();
    for bad in [
        BettorState {
            sum: f64::NAN,
            ..base
        },
        BettorState {
            sum_sq: f64::INFINITY,
            ..base
        },
        BettorState {
            sum_sq: -1e-9,
            ..base
        },
        BettorState {
            wealth: -1.0,
            ..base
        },
        BettorState {
            wealth: f64::NAN,
            ..base
        },
        BettorState {
            wealth: 2.0,
            ..base
        },
        BettorState {
            max_wealth: 0.5,
            wealth: 0.5,
            ..base
        },
        BettorState {
            max_wealth: f64::INFINITY,
            ..base
        },
        BettorState {
            breached: true,
            ..base
        },
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
    // The boundaries themselves are states a step can produce.
    assert!(BettorState {
        wealth: 0.0,
        breached: true,
        ..base
    }
    .check()
    .is_ok());
    assert!(BettorState {
        wealth: WEALTH_CEILING,
        max_wealth: WEALTH_CEILING,
        ..base
    }
    .check()
    .is_ok());
}

// ---------------------------------------------------------------- e-BH

#[test]
fn e_bh_rejects_the_largest_k_with_k_e_over_k_at_least_one_over_the_level() {
    // K = 4, level 0.5: thresholds K / (level k) = 8, 4, 8/3, 2.
    let e = [3.0, 9.0, 0.5, 2.0];
    // e_[1] = 9 >= 8; e_[2] = 3 < 4; e_[3] = 2 < 8/3; e_[4] = 0.5 < 2.
    assert_eq!(
        e_benjamini_hochberg(&e, 0.5).unwrap(),
        [false, true, false, false]
    );
    // Step-up: the largest qualifying k wins even if a smaller k fails.
    let e = [1.0, 1.0, 3.0, 3.0];
    // e_[1] = 3 < 8, e_[2] = 3 < 4, e_[3] = 1 < 8/3, e_[4] = 1 < 2: none.
    assert_eq!(e_benjamini_hochberg(&e, 0.5).unwrap(), [false; 4]);
    let e = [2.0, 2.0, 2.0, 2.0];
    // e_[4] = 2 >= 2 exactly: all four.
    assert_eq!(e_benjamini_hochberg(&e, 0.5).unwrap(), [true; 4]);
    let e = [5.0, 2.1, 2.1, 0.0];
    // e_[1] = 5 < 8, e_[2] = 2.1 < 4, e_[3] = 2.1 < 8/3: none; e_[4] = 0.
    assert_eq!(e_benjamini_hochberg(&e, 0.5).unwrap(), [false; 4]);
    let e = [4.0, 4.0, 0.0, 0.0];
    // e_[2] = 4 >= 4 exactly.
    assert_eq!(
        e_benjamini_hochberg(&e, 0.5).unwrap(),
        [true, true, false, false]
    );
}

#[test]
fn e_benjamini_hochberg_boundary_cases_step_up_and_refuse_invalid_inputs() {
    // K = 3, level 1: thresholds 3, 1.5, 1. With 3.1, 1.5, 1.5 every rank
    // qualifies; with 0.9 last, rank 3 fails (0.9 < 1) and rank 2 is the cut.
    assert_eq!(
        e_benjamini_hochberg(&[1.5, 3.1, 1.5], 1.0).unwrap(),
        [true, true, true]
    );
    assert_eq!(
        e_benjamini_hochberg(&[0.9, 3.1, 1.5], 1.0).unwrap(),
        [false, true, true]
    );
    assert!(e_benjamini_hochberg(&[], 0.05).unwrap().is_empty());
    assert_eq!(e_benjamini_hochberg(&[20.0], 0.05).unwrap(), [true]);
    assert_eq!(e_benjamini_hochberg(&[19.9], 0.05).unwrap(), [false]);
    // A family of untested members at unit wealth raises the bar for the rest.
    assert_eq!(
        e_benjamini_hochberg(&[20.0, 1.0], 0.05).unwrap(),
        [false, false]
    );
    // Level 1 is the inclusive edge of (0, 1]; zero e-values are allowed.
    assert_eq!(
        e_benjamini_hochberg(&[1.0, 0.0], 1.0).unwrap(),
        [false, false]
    );
    assert_eq!(
        e_benjamini_hochberg(&[2.0, 0.0], 1.0).unwrap(),
        [true, false]
    );
    for level in [0.0, -0.1, 1.000_000_1, f64::NAN] {
        assert!(e_benjamini_hochberg(&[1.0], level).is_err(), "{level}");
    }
    for e in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(e_benjamini_hochberg(&[1.0, e], 0.05).is_err(), "{e}");
    }
}
