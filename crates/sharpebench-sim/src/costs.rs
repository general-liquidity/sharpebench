//! Transaction costs + a tiny seeded PRNG for execution noise.
//!
//! Realistic costs (fees, slippage, and seed-varying execution noise) are what
//! make pass^k meaningful: the same agent run under different execution seeds
//! produces slightly different returns, so a one-seed fluke can't top the board.

use serde::{Deserialize, Serialize};

/// Basis-point transaction cost model.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CostModel {
    pub fee_bps: f64,
    pub slippage_bps: f64,
    /// Own-order market-impact coefficient (bps at 100% participation). Slippage
    /// grows with the square root of the trade's share of portfolio NAV, so an
    /// agent that wins by betting huge pays for the size it moves.
    pub impact_bps: f64,
    /// Per-step financing cost (bps) charged on leveraged exposure above 1× NAV —
    /// the cost of carrying borrowed money. Long-only, fully-invested books
    /// (gross ≤ 1) pay nothing; leverage pays for the size it borrows.
    pub financing_bps: f64,
    /// Liquidity cap: the most an agent may trade in one step, as a fraction of
    /// NAV. An order larger than this only **partially fills**; the remainder is
    /// left for later steps. `f64::INFINITY` (the default) = unlimited liquidity.
    pub max_participation: f64,
    /// Optional proportional turnover cost (per-unit, e.g. `0.001` = 10 bps) used
    /// by [`trf_factor`] to compute the cost-aware reallocation factor (Jiang et
    /// al.). `None` (the default) leaves cost behaviour byte-identical to the
    /// fee/slippage/impact model — the turnover factor is opt-in, consumed by
    /// callers that want the closed-form remainder rather than per-order fills.
    #[serde(default)]
    pub trf_cost: Option<f64>,
    /// Opt-in seed-driven execution noise (fill delay, partial fills,
    /// queue-position slippage). `None` (the default) leaves every fill
    /// byte-identical to the fee/slippage/impact model; see [`ExecutionNoise`].
    #[serde(default)]
    pub noise: Option<ExecutionNoise>,
}

impl Default for CostModel {
    fn default() -> Self {
        Self {
            fee_bps: 2.0,
            slippage_bps: 3.0,
            impact_bps: 50.0,
            financing_bps: 5.0,
            max_participation: f64::INFINITY,
            trf_cost: None,
            noise: None,
        }
    }
}

/// Seed-driven execution noise beyond the base slippage draw. Every quantity is
/// a pure function of `(seed, step, symbol)`, drawn from a stream derived for
/// that triple rather than from the book's sequential RNG, so an order's noise
/// does not depend on how many orders preceded it and the base slippage draw is
/// untouched.
///
/// Per order on symbol `i` at step `t`, three uniforms `u_d, u_f, u_q` are drawn:
///
/// * **Fill delay.** With probability `delay_prob` (`u_d < delay_prob`) a fresh
///   order does not fill this bar; it is carried and fills at the next bar's
///   price, unless the agent re-issues an order on that symbol, which supersedes
///   it (cancel/replace). A carried order is never delayed a second time.
/// * **Partial fill.** The filled fraction of the target change is
///   `phi = min_fill_frac + (1 - min_fill_frac) * u_f`; the remainder is carried
///   to the next bar as an order for the same target weight. A remainder worth
///   less than `carry_floor` of NAV fills in full instead, so a carry drains in
///   finitely many bars rather than shrinking geometrically forever.
/// * **Queue-position slippage.** An additional adverse price move of
///   `u_q * range_t * min(1, participation / queue_participation_ref)`, where
///   `range_t = |c_t / c_{t-1} - 1|` is the bar's close-to-close absolute move
///   (the dataset carries closes only, so this is the range proxy) and
///   `participation` is the filled trade value over NAV.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExecutionNoise {
    /// Probability a fresh order is deferred to the next bar.
    pub delay_prob: f64,
    /// Floor on the fraction of the target change filled in one bar.
    pub min_fill_frac: f64,
    /// Unfilled remainder, as a fraction of NAV, below which an order fills in
    /// full rather than carrying the residual.
    pub carry_floor: f64,
    /// Participation (trade value over NAV) at which queue slippage reaches the
    /// full bar range; below it the range is scaled down linearly.
    pub queue_participation_ref: f64,
}

impl Default for ExecutionNoise {
    fn default() -> Self {
        Self {
            delay_prob: 0.25,
            min_fill_frac: 0.5,
            carry_floor: 0.001,
            queue_participation_ref: 0.10,
        }
    }
}

impl ExecutionNoise {
    /// Validate every public execution-noise parameter before it reaches the
    /// simulator. Invalid noise is a configuration error, never a silently
    /// clamped market model: negative fill fractions can reverse an order and a
    /// negative carry floor can leave an order pending indefinitely.
    pub fn validate(&self) -> Result<(), String> {
        if !self.delay_prob.is_finite() || !(0.0..=1.0).contains(&self.delay_prob) {
            return Err("execution noise delay_prob must be finite and in [0, 1]".to_string());
        }
        if !self.min_fill_frac.is_finite() || !(0.0..=1.0).contains(&self.min_fill_frac) {
            return Err("execution noise min_fill_frac must be finite and in [0, 1]".to_string());
        }
        if !self.carry_floor.is_finite() || self.carry_floor < 0.0 {
            return Err("execution noise carry_floor must be finite and >= 0".to_string());
        }
        if !self.queue_participation_ref.is_finite() || self.queue_participation_ref <= 0.0 {
            return Err(
                "execution noise queue_participation_ref must be finite and > 0".to_string(),
            );
        }
        Ok(())
    }

    /// The derived noise stream for one `(seed, step, symbol index)` triple.
    pub fn stream(seed: u64, step: usize, symbol_index: usize) -> Rng {
        Rng::new(
            seed ^ (step as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93)
                ^ (symbol_index as u64 + 1).wrapping_mul(0x2545_F491_4F6C_DD1D),
        )
    }
}

impl CostModel {
    /// Reject malformed optional execution noise at the public simulation
    /// boundary. A config without noise remains backwards compatible.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(noise) = self.noise {
            noise.validate()?;
        }
        Ok(())
    }
}

/// The transaction-remainder factor `μ` (Jiang et al., 2017): the fraction of
/// portfolio value that survives reallocating from `weights_prev` (the drifted
/// pre-trade weights) to `weights_new` (the targets) at proportional turnover
/// cost `c`. Solves the fixed point
///
/// ```text
/// μ = (1 − c·w0 − (2c − c²)·Σ max(w_prev_i − μ·w_new_i, 0)) / (1 − c·w0)
/// ```
///
/// where `w0 = 1 − Σ w_new` is the residual cash weight of the target book. The
/// iteration is deterministic — only mul/add/div/max — and is capped at a pinned
/// 20 sweeps (it contracts to the 1e-10 tolerance well inside that). `c = 0`
/// returns exactly `μ = 1` (no cost, nothing lost to turnover).
pub fn trf_factor(weights_prev: &[f64], weights_new: &[f64], c: f64) -> f64 {
    let sum_new: f64 = weights_new.iter().sum();
    let w0 = 1.0 - sum_new;
    let denom = 1.0 - c * w0;
    let coef = 2.0 * c - c * c;
    let mut mu = 1.0;
    for _ in 0..20 {
        let mut sell = 0.0;
        for (prev, new) in weights_prev.iter().zip(weights_new.iter()) {
            sell += (prev - mu * new).max(0.0);
        }
        let mu_next = (1.0 - c * w0 - coef * sell) / denom;
        if (mu_next - mu).abs() < 1e-10 {
            mu = mu_next;
            break;
        }
        mu = mu_next;
    }
    mu
}

/// Execution-robustness profile: a named bundle of a [`CostModel`] plus a logical
/// **decision-to-fill delay** (how many sim-bars an order waits before it becomes
/// eligible to fill). Lets "score this agent under worst-case execution" be a
/// single swappable axis rather than hand-tuned cost fields scattered per test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostProfile {
    /// Reported as **frictionless**: no fees, no slippage, no impact, no delay.
    /// The ceiling case.
    None,
    /// Reported as **typical**: a realistic retail/institutional blend, the
    /// default-ish baseline.
    Typical,
    /// Reported as **stressed**: wide fees + slippage + impact and a multi-bar
    /// fill delay.
    WorstCase,
    /// Reported as **realistic**: typical costs plus seed-driven fill delay,
    /// partial fills and queue-position slippage ([`ExecutionNoise::default`]),
    /// so the seed leg of pass^k resamples execution rather than a few basis
    /// points of slippage.
    Realistic,
}

impl CostProfile {
    /// The name this profile is reported under, in evidence records, the paper
    /// and the documentation. The variant identifiers are the published Rust API
    /// and do not match these names; [`name`](Self::name) is the single place
    /// that binds the two, so a rename cannot silently desynchronize them.
    pub fn name(self) -> &'static str {
        match self {
            CostProfile::None => "frictionless",
            CostProfile::Typical => "typical",
            CostProfile::WorstCase => "stressed",
            CostProfile::Realistic => "realistic",
        }
    }
}

/// A cost profile resolved to a concrete [`CostModel`] and a decision-to-fill
/// delay in sim-bars.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionProfile {
    pub costs: CostModel,
    /// Bars an order waits after the decision before it is eligible to fill.
    pub decision_delay_bars: usize,
}

impl CostProfile {
    /// Resolve this profile to its [`CostModel`] and decision-to-fill delay.
    pub fn resolve(self) -> ExecutionProfile {
        match self {
            CostProfile::None => ExecutionProfile {
                costs: CostModel {
                    fee_bps: 0.0,
                    slippage_bps: 0.0,
                    impact_bps: 0.0,
                    financing_bps: 0.0,
                    max_participation: f64::INFINITY,
                    trf_cost: None,
                    noise: None,
                },
                decision_delay_bars: 0,
            },
            CostProfile::Typical => ExecutionProfile {
                costs: CostModel::default(),
                decision_delay_bars: 0,
            },
            CostProfile::WorstCase => ExecutionProfile {
                costs: CostModel {
                    fee_bps: 10.0,
                    slippage_bps: 15.0,
                    impact_bps: 150.0,
                    financing_bps: 20.0,
                    max_participation: 0.1,
                    trf_cost: None,
                    noise: None,
                },
                decision_delay_bars: 2,
            },
            CostProfile::Realistic => ExecutionProfile {
                costs: CostModel {
                    noise: Some(ExecutionNoise::default()),
                    ..CostModel::default()
                },
                decision_delay_bars: 0,
            },
        }
    }
}

/// Per-step financing cost as a fraction of NAV: `financing_bps` applied to the
/// leveraged portion of gross exposure (everything above 1× NAV). Zero at or below
/// full investment.
pub fn financing_cost_frac(financing_bps: f64, gross_exposure: f64) -> f64 {
    financing_bps / 10_000.0 * (gross_exposure - 1.0).max(0.0)
}

/// Apply the liquidity cap to a desired trade value: an order is clamped to
/// `±max_participation × nav`, modelling a partial fill of the rest.
pub fn liquidity_capped_delta(delta_value: f64, max_participation: f64, nav: f64) -> f64 {
    if !max_participation.is_finite() {
        return delta_value;
    }
    let cap = max_participation * nav.max(0.0);
    delta_value.clamp(-cap, cap)
}

/// Own-order market impact as a return fraction: a concave (square-root law)
/// function of `participation` = |trade value| / portfolio NAV. Concavity is the
/// empirical Almgren shape — the first dollar moves the price more than the last.
pub fn market_impact_frac(impact_bps: f64, participation: f64) -> f64 {
    impact_bps / 10_000.0 * participation.max(0.0).sqrt()
}

/// Minimal deterministic PRNG (SplitMix64) for seeded execution noise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0xA5A5_5A5A_C3C3_3C3C)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [-1, 1].
    pub fn signed_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_noise_validation_rejects_unsafe_public_values() {
        let mut noise = ExecutionNoise::default();
        for value in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            noise.delay_prob = value;
            assert!(noise.validate().is_err());
        }
        noise = ExecutionNoise::default();
        for value in [-0.1, 1.1, f64::NAN, f64::NEG_INFINITY] {
            noise.min_fill_frac = value;
            assert!(noise.validate().is_err());
        }
        noise = ExecutionNoise::default();
        noise.carry_floor = -0.001;
        assert!(noise.validate().is_err());
        noise = ExecutionNoise::default();
        noise.queue_participation_ref = 0.0;
        assert!(noise.validate().is_err());
        assert!(ExecutionNoise::default().validate().is_ok());
    }

    #[test]
    fn reported_profile_names_are_the_names_in_the_evidence_records() {
        // These exact strings are the `cost_profile` field of every record in
        // paper/evidence/final/external-rules.jsonl and the profile names the
        // paper reports under. Renaming one silently invalidates both.
        assert_eq!(CostProfile::None.name(), "frictionless");
        assert_eq!(CostProfile::Typical.name(), "typical");
        assert_eq!(CostProfile::WorstCase.name(), "stressed");
        assert_eq!(CostProfile::Realistic.name(), "realistic");
    }

    #[test]
    fn impact_grows_with_participation() {
        let small = market_impact_frac(50.0, 0.01);
        let big = market_impact_frac(50.0, 0.5);
        assert!(big > small, "bigger trade should cost more");
        assert!(market_impact_frac(50.0, 0.0).abs() < 1e-12);
    }

    #[test]
    fn impact_is_concave() {
        // Square-root law: doubling participation less-than-doubles the impact.
        let a = market_impact_frac(50.0, 0.1);
        let b = market_impact_frac(50.0, 0.2);
        assert!(b < 2.0 * a, "impact must be concave in size");
    }

    #[test]
    fn financing_only_bites_above_full_investment() {
        assert_eq!(financing_cost_frac(50.0, 1.0), 0.0);
        assert_eq!(financing_cost_frac(50.0, 0.5), 0.0);
        assert!(financing_cost_frac(50.0, 2.0) > 0.0);
    }

    #[test]
    fn profile_none_is_frictionless() {
        let p = CostProfile::None.resolve();
        assert_eq!(p.costs.fee_bps, 0.0);
        assert_eq!(p.costs.slippage_bps, 0.0);
        assert_eq!(p.costs.impact_bps, 0.0);
        assert_eq!(p.costs.financing_bps, 0.0);
        assert!(!p.costs.max_participation.is_finite());
        assert_eq!(p.decision_delay_bars, 0);
    }

    #[test]
    fn profile_typical_matches_default_costs_no_delay() {
        let p = CostProfile::Typical.resolve();
        let d = CostModel::default();
        assert_eq!(p.costs.fee_bps, d.fee_bps);
        assert_eq!(p.costs.slippage_bps, d.slippage_bps);
        assert_eq!(p.decision_delay_bars, 0);
    }

    #[test]
    fn worst_case_is_strictly_harsher_with_delay() {
        let none = CostProfile::None.resolve();
        let typ = CostProfile::Typical.resolve();
        let worst = CostProfile::WorstCase.resolve();
        // Monotone friction across the three profiles.
        assert!(none.costs.fee_bps <= typ.costs.fee_bps);
        assert!(typ.costs.fee_bps < worst.costs.fee_bps);
        assert!(typ.costs.slippage_bps < worst.costs.slippage_bps);
        assert!(typ.costs.impact_bps < worst.costs.impact_bps);
        // Worst-case caps liquidity and imposes a fill delay; the others don't.
        assert!(worst.costs.max_participation.is_finite());
        assert!(worst.decision_delay_bars > 0);
        assert_eq!(typ.decision_delay_bars, 0);
    }

    #[test]
    fn trf_factor_matches_hand_computed_fixture() {
        // prev = 50% in asset 0 (50% cash); target = 50% in asset 1 (50% cash).
        // w0 = 1 - 0.5 = 0.5; the only positive sell term is asset 0 (0.5), which
        // is μ-independent here, so the fixed point is reached in one sweep:
        //   μ = (1 - 0.01·0.5 - (0.0199)·0.5) / (1 - 0.01·0.5)
        //     = 0.98505 / 0.995 = 0.99 exactly.
        let mu = trf_factor(&[0.5, 0.0], &[0.0, 0.5], 0.01);
        assert!((mu - 0.99).abs() < 1e-12, "expected μ=0.99, got {mu}");
    }

    #[test]
    fn trf_factor_zero_cost_is_unity() {
        // c = 0 ⇒ nothing is lost to turnover, μ = 1 exactly.
        assert_eq!(trf_factor(&[0.3, 0.7], &[0.6, 0.4], 0.0), 1.0);
    }

    #[test]
    fn trf_factor_converges_within_the_pinned_cap() {
        // A μ-dependent sell term (target keeps weight in a held name): the result
        // must be a fixed point to tolerance — i.e. one more sweep barely moves it,
        // proving convergence happened inside the 20-iteration cap.
        let prev = [0.8, 0.1];
        let new = [0.2, 0.6];
        let c = 0.005;
        let mu = trf_factor(&prev, &new, c);
        let w0 = 1.0 - (new[0] + new[1]);
        let coef = 2.0 * c - c * c;
        let sell: f64 = prev
            .iter()
            .zip(new.iter())
            .map(|(p, n)| (p - mu * n).max(0.0))
            .sum();
        let residual = (1.0 - c * w0 - coef * sell) / (1.0 - c * w0) - mu;
        assert!(residual.abs() < 1e-10, "μ is not a fixed point: {residual}");
        assert!(mu > 0.0 && mu <= 1.0, "μ out of range: {mu}");
    }

    #[test]
    fn realistic_profile_is_typical_costs_plus_noise() {
        let p = CostProfile::Realistic.resolve();
        let d = CostModel::default();
        assert_eq!(p.costs.fee_bps, d.fee_bps);
        assert_eq!(p.costs.slippage_bps, d.slippage_bps);
        assert_eq!(p.costs.impact_bps, d.impact_bps);
        assert_eq!(p.costs.financing_bps, d.financing_bps);
        assert_eq!(p.decision_delay_bars, 0);
        assert_eq!(p.costs.noise, Some(ExecutionNoise::default()));
        assert_eq!(d.noise, None);
    }

    #[test]
    fn noise_field_is_serde_default_so_old_configs_still_parse() {
        let old = r#"{"fee_bps":2.0,"slippage_bps":3.0,"impact_bps":50.0,"financing_bps":5.0,"max_participation":0.5}"#;
        let m: CostModel = serde_json::from_str(old).expect("pre-noise config parses");
        assert_eq!(m.noise, None);
        assert_eq!(m.trf_cost, None);
    }

    #[test]
    fn noise_stream_is_a_pure_function_of_seed_step_symbol() {
        let mut a = ExecutionNoise::stream(7, 3, 1);
        let mut b = ExecutionNoise::stream(7, 3, 1);
        assert_eq!(a.unit(), b.unit());
        let mut c = ExecutionNoise::stream(8, 3, 1);
        let mut d = ExecutionNoise::stream(7, 4, 1);
        let mut e = ExecutionNoise::stream(7, 3, 2);
        let base = ExecutionNoise::stream(7, 3, 1).unit();
        assert_ne!(base, c.unit());
        assert_ne!(base, d.unit());
        assert_ne!(base, e.unit());
    }

    #[test]
    fn trf_cost_defaults_to_none_and_is_byte_neutral() {
        // The new field is opt-in: the default model is unchanged, and an explicit
        // `None` is indistinguishable from the default for every other field.
        assert_eq!(CostModel::default().trf_cost, None);
    }

    #[test]
    fn liquidity_cap_clamps_large_trades() {
        // 5% of a 1000 NAV = 50 cap.
        assert_eq!(liquidity_capped_delta(200.0, 0.05, 1000.0), 50.0);
        assert_eq!(liquidity_capped_delta(-200.0, 0.05, 1000.0), -50.0);
        // Small trades pass through, and an infinite cap never clamps.
        assert_eq!(liquidity_capped_delta(30.0, 0.05, 1000.0), 30.0);
        assert_eq!(liquidity_capped_delta(1e9, f64::INFINITY, 1000.0), 1e9);
    }
}
