//! Anytime-valid evidence of skill from forward observations: a betting
//! e-process for the null that a strategy's conditional mean return is at most
//! zero, and the e-BH procedure for a family of such processes.
//!
//! Every other test in this crate is fixed-horizon: PSR, the deflated Sharpe
//! and the bootstrap are valid only at a sample size set in advance, and a
//! reader who looks after every forward window and stops when the number looks
//! good inflates their size. The process here is valid at every bar at once,
//! so forward evidence can accumulate window after window and be read at any
//! point. It answers a narrower question than the deflated Sharpe: whether the
//! returns have a positive conditional mean at all, not whether the Sharpe
//! clears a selection-adjusted bar. It is reported beside the kernel's gates
//! and never feeds them.
//!
//! # The null and the observation
//!
//! The observations are one return per bar, `x_1, x_2, ...`, in a fixed order
//! (the arena uses publish order, then bar order). `F_t` is everything known
//! after bar `t`. The null hypothesis for one strategy is
//!
//! ```text
//! H0:  E[x_t | F_{t-1}] <= 0  and  x_t >= -2 B_t  for every bar t,
//! ```
//!
//! where `B_t > 0` is the per-bar loss bound in force at bar `t`, fixed before
//! bar `t` (an `F_{t-1}`-measurable quantity). A conditional-mean null is
//! weaker than "the returns are iid with mean zero": it allows any
//! heteroskedasticity, regime switching or serial dependence in the
//! variance, and only asks that no bar is predictably profitable. It is the
//! null "Sharpe at most zero"; a positive Sharpe bar is not a mean null and is
//! not tested here.
//!
//! # The exact rule
//!
//! The bettor keeps the bar count `n`, the sum `S` and the sum of squares `Q`
//! of the observations it has seen, its wealth `K` (starting at 1), the
//! largest wealth so far, and whether a bound was breached. Before bar `t` it
//! stakes the fraction
//!
//! ```text
//! lambda_t = 0                              if n < WARMUP_BARS, or S <= 0,
//!                                           or the process has stopped;
//! lambda_t = min(S / Q, 1 / (2 B_t))        otherwise.
//! ```
//!
//! `S / Q` is the approximate growth-rate-adaptive (aGRAPA) bet for a mean
//! tested at zero: the sample mean over the sample second moment, which is the
//! sample mean over the sample variance plus the squared sample mean. The
//! source clips the bet at a constant over the distance from the tested mean
//! to the support bound; the clip here is that constant at one half.
//! After Waudby-Smith and Ramdas (2024), appendix B.3 (aGRAPA).
//! Only nonnegative bets are allowed, because the null is one-sided. Once bar
//! `t` is observed:
//!
//! ```text
//! breach (x_t < -B_t):   K_t = 0, and the process stops for good;
//! otherwise:             K_t = min(K_{t-1} (1 + lambda_t x_t), WEALTH_CEILING),
//!                        and the process stops for good if K_t reached the ceiling.
//! ```
//!
//! A stopped process keeps its wealth. Every operation is an IEEE-754 add,
//! multiply, divide or comparison in a fixed order, so the wealth is the same
//! bits on every platform.
//!
//! # Validity
//!
//! Claim: under `H0`, `K` is a nonnegative supermartingale with `K_0 = 1`, so
//! `P(sup_t K_t >= 1/alpha) <= alpha` for every `alpha` in `(0, 1]`.
//!
//! Proof. Write `f_t` for the factor that takes `K_{t-1}` to `K_t` before the
//! ceiling: `f_t = 1` once the process has stopped, `f_t = 0` on a breach, and
//! `f_t = 1 + lambda_t x_t` otherwise. `lambda_t` is a function of `n`, `S`, `Q`,
//! the stop flags and `B_t`, all fixed before bar `t`, so it is predictable,
//! and `0 <= lambda_t <= 1 / (2 B_t)`.
//!
//! 1. `f_t >= 0`. On a non-breach `x_t >= -B_t`, so `lambda_t x_t >= -1/2` and
//!    `f_t >= 1/2`.
//! 2. `f_t <= 1 + lambda_t x_t` whenever the process has not stopped. This is
//!    equality off a breach. On a breach `f_t = 0`, and `1 + lambda_t x_t >= 0`
//!    because `x_t >= -2 B_t` under `H0` and `lambda_t <= 1 / (2 B_t)`.
//! 3. Hence, before the stop, `E[f_t | F_{t-1}] <= 1 + lambda_t E[x_t | F_{t-1}]
//!    <= 1`, using that `lambda_t >= 0` is known at `t - 1` and the mean null.
//!    After the stop `f_t = 1`.
//! 4. The ceiling only lowers wealth: `K_t = min(K_{t-1} f_t, C) <= K_{t-1} f_t`.
//!    So `E[K_t | F_{t-1}] <= K_{t-1} E[f_t | F_{t-1}] <= K_{t-1}`, and `K_t >= 0`.
//!
//! `K` is a nonnegative supermartingale started at one. Ville's inequality for
//! such processes gives the claim, and makes `K` an e-process.
//! After Ramdas, Grünwald, Vovk and Shafer (2023) (test martingales, Ville's inequality).
//!
//! Three consequences are used by the arena:
//!
//! - **Anytime p-value.** `p_t = min(1, 1 / max_{s <= t} K_s)` satisfies
//!   `P(exists t: p_t <= alpha) <= alpha`. It is [`BettorState::anytime_p_value`].
//! - **E-value at a stopping time.** For any stopping time `tau`, `E[K_tau] <= 1`
//!   (optional stopping for nonnegative supermartingales). The current wealth
//!   read at a time chosen from the data seen so far is therefore an e-value.
//!   The running maximum is not an e-value; only its reciprocal, as above, is
//!   a valid p-value.
//! - **Optional continuation.** The index is the observation count, not
//!   calendar time, so a gap between forward windows, or a decision to keep
//!   going made after looking at the evidence, changes nothing.
//!   After Grünwald, de Heide and Koolen (2024), Proposition 2 and Corollary 1.
//!
//! The support condition is a model assumption, not something the arithmetic
//! can enforce. A declared bound `B` gives a margin: the guarantee needs only
//! `x_t >= -2B`, and any bar below `-B` zeroes the process, so a strategy that
//! breaks its declared bound can never be called skilled by it. A bar below
//! `-2B` is outside the null's support and the guarantee does not cover it.
//!
//! # Multiple testing
//!
//! [`e_benjamini_hochberg`] is the base e-BH procedure. It controls the false
//! discovery rate at the level given, with no correction, under arbitrary
//! dependence between the e-values (strategies trading one market path are
//! dependent).
//! After Wang and Ramdas (2022), section 4 and Theorem 2 (e-BH).
//! The guarantee is for e-values read at one common stopping time. Applying it
//! at every board and reporting the union of everything ever rejected is not
//! covered.
//!
//! # Power, for orientation
//!
//! [`fixed_horizon_power`] and [`oracle_likelihood_ratio_power`] are closed
//! forms under a Brownian approximation (unit annual volatility, annual Sharpe
//! `s`, horizon `T` years): the one-sided z test at a horizon fixed in advance,
//! and the anytime-valid likelihood-ratio e-process that knows `s`. No
//! anytime-valid test beats the first at its own horizon, and the second is
//! the best an e-process can do with the alternative known. The betting
//! process above estimates the alternative and is capped by the loss bound, so
//! it sits below both; the book page tabulates all three.
use crate::det_math;
use crate::stats::{norm_cdf, norm_ppf};
use crate::validation::StatisticalError;

/// The per-bar loss bound when nothing tighter is in force: one bar loses at
/// most the whole account. It caps the bet at one half.
pub const DEFAULT_LOSS_BOUND: f64 = 1.0;

/// Bars a strategy is observed, betting nothing, before its first stake. The
/// plug-in bet is the sample mean over the sample second moment, which is
/// unstable on a handful of bars. Any predictable warm-up is valid; this one
/// only trades early power for a steadier first bet.
pub const WARMUP_BARS: u64 = 20;

/// The fraction of the inverse loss bound the bet may reach: the bet is at
/// most `BET_CAP_FRACTION / B`, so one bar inside the bound loses at most half
/// the wealth.
pub const BET_CAP_FRACTION: f64 = 0.5;

/// The wealth at which the process stops. Far beyond any `1 / alpha` a reader
/// would use, and far below `f64::MAX`, so the wealth is always finite and
/// serializes as a JSON number.
pub const WEALTH_CEILING: f64 = 1e300;

/// The largest stake [`BettorState::bet`] can return under `loss_bound`:
/// `BET_CAP_FRACTION / loss_bound`.
pub fn bet_cap(loss_bound: f64) -> Result<f64, StatisticalError> {
    check_loss_bound(loss_bound)?;
    Ok(BET_CAP_FRACTION / loss_bound)
}

fn check_loss_bound(loss_bound: f64) -> Result<(), StatisticalError> {
    if loss_bound.is_finite() && loss_bound > 0.0 {
        Ok(())
    } else {
        Err(StatisticalError::InvalidParameter {
            name: "loss_bound",
            requirement: "must be finite and positive",
        })
    }
}

/// The bettor's whole state after some number of bars. Everything the next
/// bet depends on is here, so a ledger that publishes this record lets anyone
/// continue the process, or recompute it, bit for bit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BettorState {
    /// Bars observed, the warm-up included.
    pub bars: u64,
    /// Sum of the observations, in observation order.
    pub sum: f64,
    /// Sum of their squares, in observation order.
    pub sum_sq: f64,
    /// The e-process: current wealth, starting at 1.
    pub wealth: f64,
    /// The largest wealth so far, at least 1.
    pub max_wealth: f64,
    /// A bar fell below the loss bound in force. The wealth is then 0 for good.
    pub breached: bool,
}

impl Default for BettorState {
    fn default() -> Self {
        Self::new()
    }
}

impl BettorState {
    /// Unit wealth, nothing observed.
    pub const fn new() -> Self {
        Self {
            bars: 0,
            sum: 0.0,
            sum_sq: 0.0,
            wealth: 1.0,
            max_wealth: 1.0,
            breached: false,
        }
    }

    /// Whether this state is one [`BettorState::step`] can produce from
    /// [`BettorState::new`]: finite sums, a nonnegative sum of squares, wealth
    /// in `[0, WEALTH_CEILING]` and at most the running maximum, a running
    /// maximum of at least 1, and zero wealth after a breach. A ledger read
    /// back from a document is checked with this before it is continued.
    pub fn check(&self) -> Result<(), StatisticalError> {
        let invalid = |requirement| {
            Err(StatisticalError::InvalidParameter {
                name: "bettor state",
                requirement,
            })
        };
        if !self.sum.is_finite() || !self.sum_sq.is_finite() || self.sum_sq < 0.0 {
            return invalid("must have finite sums and a nonnegative sum of squares");
        }
        if !(0.0..=WEALTH_CEILING).contains(&self.wealth) {
            return invalid("must have wealth in [0, WEALTH_CEILING]");
        }
        if !(1.0..=WEALTH_CEILING).contains(&self.max_wealth) || self.wealth > self.max_wealth {
            return invalid(
                "must have a running maximum in [1, WEALTH_CEILING] at least the wealth",
            );
        }
        if self.breached && self.wealth != 0.0 {
            return invalid("must have zero wealth after a breach");
        }
        Ok(())
    }

    /// No further bar changes the wealth: a bound was breached, the wealth is
    /// zero, or it reached [`WEALTH_CEILING`].
    pub fn is_stopped(&self) -> bool {
        self.breached || self.wealth == 0.0 || self.wealth >= WEALTH_CEILING
    }

    /// The stake for the next bar under `loss_bound`, from this state alone.
    /// See the module docs for the rule; the result is in
    /// `[0, bet_cap(loss_bound)]`.
    pub fn bet(&self, loss_bound: f64) -> Result<f64, StatisticalError> {
        let cap = bet_cap(loss_bound)?;
        // A NaN sum, which `check` refuses anyway, also bets nothing.
        if self.is_stopped() || self.bars < WARMUP_BARS || self.sum.is_nan() || self.sum <= 0.0 {
            return Ok(0.0);
        }
        // sum > 0 means some observation is nonzero, so sum_sq > 0 and the
        // ratio is positive; a subnormal sum_sq can make it overflow to
        // infinity, which the cap absorbs.
        let ratio = self.sum / self.sum_sq;
        Ok(if ratio > cap { cap } else { ratio })
    }

    /// Observe one bar: stake [`BettorState::bet`] on `x` under `loss_bound`,
    /// then add `x` to the sums. A non-finite `x`, an invalid bound, or a sum
    /// that would overflow is refused and leaves the state unchanged.
    pub fn step(&mut self, x: f64, loss_bound: f64) -> Result<(), StatisticalError> {
        if !x.is_finite() {
            return Err(StatisticalError::NonFiniteObservation { index: 0 });
        }
        let lambda = self.bet(loss_bound)?;
        let sum = self.sum + x;
        let sum_sq = self.sum_sq + x * x;
        if !sum.is_finite() || !sum_sq.is_finite() {
            return Err(StatisticalError::NonFiniteComputation {
                quantity: "forward e-process sums",
            });
        }
        if !self.is_stopped() {
            if x < -loss_bound {
                self.breached = true;
                self.wealth = 0.0;
            } else {
                // lambda <= 1/(2B) and x >= -B, so the factor is at least 1/2
                // and the product is positive; it may round up to infinity
                // only past the ceiling, which it is clamped to.
                let next = self.wealth * (1.0 + lambda * x);
                self.wealth = if next >= WEALTH_CEILING {
                    WEALTH_CEILING
                } else {
                    next
                };
                if self.wealth > self.max_wealth {
                    self.max_wealth = self.wealth;
                }
            }
        }
        self.bars += 1;
        self.sum = sum;
        self.sum_sq = sum_sq;
        Ok(())
    }

    /// The anytime-valid p-value `min(1, 1 / max wealth)`.
    pub fn anytime_p_value(&self) -> f64 {
        // max_wealth >= 1 in every state `check` accepts.
        1.0 / self.max_wealth
    }
}

/// The base e-BH procedure at false-discovery level `level`: sort the
/// e-values from largest to smallest as `e_[1] >= ... >= e_[K]`, find the
/// largest `k` with `e_[k] >= K / (level * k)`, and reject the hypotheses with
/// the `k` largest e-values. Returns a rejection mask in the input order. Ties
/// break by input index, and an empty family rejects nothing.
///
/// Every e-value must be finite and nonnegative, and `level` in `(0, 1]`.
pub fn e_benjamini_hochberg(e_values: &[f64], level: f64) -> Result<Vec<bool>, StatisticalError> {
    if !(level.is_finite() && level > 0.0 && level <= 1.0) {
        return Err(StatisticalError::InvalidParameter {
            name: "level",
            requirement: "must be finite and in (0, 1]",
        });
    }
    if let Some(index) = e_values.iter().position(|e| !(e.is_finite() && *e >= 0.0)) {
        return Err(StatisticalError::NonFiniteObservation { index });
    }
    let family = e_values.len();
    let mut order: Vec<usize> = (0..family).collect();
    // Largest first; equal e-values keep input order (the sort is stable).
    order.sort_by(|&a, &b| e_values[b].total_cmp(&e_values[a]));
    let threshold = family as f64 / level;
    // inv: `rejected_count` is the largest k in 1..=rank with
    // e_[k] * k >= K / level, or 0 when there is none yet.
    let mut rejected_count = 0;
    for (rank, &index) in order.iter().enumerate() {
        let k = rank + 1;
        if e_values[index] * k as f64 >= threshold {
            rejected_count = k;
        }
    }
    let mut mask = vec![false; family];
    for &index in &order[..rejected_count] {
        mask[index] = true;
    }
    Ok(mask)
}

fn power_inputs(annual_sharpe: f64, years: f64, alpha: f64) -> Result<(), StatisticalError> {
    let invalid = |name, requirement| Err(StatisticalError::InvalidParameter { name, requirement });
    if !(annual_sharpe.is_finite() && annual_sharpe > 0.0) {
        return invalid("annual_sharpe", "must be finite and positive");
    }
    if !(years.is_finite() && years > 0.0) {
        return invalid("years", "must be finite and positive");
    }
    if !(alpha.is_finite() && alpha > 0.0 && alpha < 1.0) {
        return invalid("alpha", "must be finite and in (0, 1)");
    }
    Ok(())
}

/// Power of the one-sided z test of a zero mean at a horizon of `years` fixed
/// in advance, under the Brownian approximation with unit annual volatility:
/// `Phi(s sqrt(T) - z_{1 - alpha})`. The ceiling no anytime-valid test beats
/// at that horizon.
pub fn fixed_horizon_power(
    annual_sharpe: f64,
    years: f64,
    alpha: f64,
) -> Result<f64, StatisticalError> {
    power_inputs(annual_sharpe, years, alpha)?;
    Ok(norm_cdf(
        annual_sharpe * years.sqrt() - norm_ppf(1.0 - alpha),
    ))
}

/// Probability that the likelihood-ratio e-process tuned to the true annual
/// Sharpe `s` crosses `1 / alpha` within `years`, under the same Brownian
/// approximation. Its log is `s W_t - s^2 t / 2`, a Brownian motion with drift
/// `s / 2` and unit variance under the alternative, which first reaches
/// `c = ln(1 / alpha) / s` by time `T` with probability
/// `Phi((s T / 2 - c) / sqrt(T)) + exp(s c) Phi((-s T / 2 - c) / sqrt(T))`,
/// and `exp(s c) = 1 / alpha`.
pub fn oracle_likelihood_ratio_power(
    annual_sharpe: f64,
    years: f64,
    alpha: f64,
) -> Result<f64, StatisticalError> {
    power_inputs(annual_sharpe, years, alpha)?;
    let drift = annual_sharpe / 2.0;
    let barrier = det_math::ln(1.0 / alpha) / annual_sharpe;
    let root_t = years.sqrt();
    Ok(norm_cdf((drift * years - barrier) / root_t)
        + norm_cdf((-drift * years - barrier) / root_t) / alpha)
}
