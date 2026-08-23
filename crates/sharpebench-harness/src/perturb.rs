//! Perturbed-window generation — making fragility visible.
//!
//! The self-audit's adversarial-input attack records its own limit: a fragility
//! that no submitted run exercises is invisible, because the harness only ever
//! scores agents on the frozen datasets as-is. This module closes that gap by
//! generating **perturbed variants** of a frozen dataset: each bar's close is
//! moved by a small bounded factor drawn from a seeded SplitMix64 stream, with
//! the perturbed bar-to-bar move constrained to stay **inside the empirical
//! range of the original series' own bar-to-bar moves** (asserted, per bar). A
//! perturbed series is therefore always a path the original market could have
//! printed bar-by-bar — nothing exotic is injected, so a large score swing
//! under perturbation indicts the agent, not the generator.
//!
//! [`perturbed_field`] scores a field of agents on the original plus `n`
//! perturbed datasets and reports, per agent, the spread between the best and
//! worst perturbed-run PSR. A robust agent's spread is small; an agent keyed to
//! incidental features of the exact price path shows a large one.
//!
//! Fully deterministic given the seed: the generator is the same SplitMix64
//! ([`sharpebench_sim::costs::Rng`]) the simulator already uses — no `rand`
//! crate, no ambient state.

use serde::{Deserialize, Serialize};
use sharpebench_core::{score_agent, ScoreConfig};
use sharpebench_sim::costs::Rng;
use sharpebench_sim::{CostModel, Dataset, Window};

use crate::{run_agent, TeamMember};

/// Fraction of the empirical bar-to-bar move range used as the perturbation
/// half-width. Small by design: the perturbation nudges each move, it does not
/// replace it.
const MOVE_FRACTION: f64 = 0.25;

/// Produce one perturbed variant of `data`, deterministic in `seed`.
///
/// For each symbol: compute the original bar-to-bar simple returns and their
/// empirical range `[lo, hi]`; nudge each return by a uniform draw in
/// `± MOVE_FRACTION * (hi - lo)`; clamp the result back into `[lo, hi]` (the
/// in-range invariant, asserted); rebuild the close series from the first
/// original close and the perturbed returns. Dates and dividends are carried
/// over unchanged. Symbols are visited in `BTreeMap` order, so the draw stream
/// is stable across runs and platforms.
pub fn perturb_dataset(data: &Dataset, seed: u64) -> Dataset {
    let mut rng = Rng::new(seed ^ 0x7E27_00B5_2026_0001);
    let mut out = data.clone();
    for series in out.closes.values_mut() {
        if series.len() < 2 {
            continue;
        }
        let rets: Vec<f64> = series.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
        let lo = rets.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = rets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let half_width = MOVE_FRACTION * (hi - lo);
        let mut px = series[0];
        for (i, r) in rets.iter().enumerate() {
            let nudged = (r + rng.signed_unit() * half_width).clamp(lo, hi);
            assert!(
                nudged >= lo && nudged <= hi,
                "perturbed move {nudged} escapes the empirical range [{lo}, {hi}]"
            );
            px *= 1.0 + nudged;
            series[i + 1] = px;
        }
    }
    out
}

/// Per-agent robustness verdict from a perturbation sweep.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerturbationSpread {
    pub agent_id: String,
    /// PSR on the unperturbed frozen dataset.
    pub original_psr: f64,
    /// PSR of every perturbed run, in perturbation order.
    pub perturbed_psrs: Vec<f64>,
    pub best_perturbed_psr: f64,
    pub worst_perturbed_psr: f64,
    /// `best - worst` across the perturbed runs — the fragility signal. A
    /// robust agent's spread is small; a path-keyed one's is large.
    pub spread: f64,
}

/// The full report of one perturbation sweep over a field of agents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerturbationReport {
    pub n_perturbations: usize,
    pub seed: u64,
    pub spreads: Vec<PerturbationSpread>,
}

/// Score `agents` on the original dataset plus `n_perturbations` perturbed
/// variants (each derived deterministically from `seed`), and report per agent
/// the spread between the best and worst perturbed-run PSR. Each agent is run
/// across every `windows` × `seeds` cell per dataset, exactly as a normal
/// submission would be, and scored with `cfg` (which carries the dataset's
/// periods-per-year). Deterministic given `seed`.
#[allow(clippy::too_many_arguments)]
pub fn perturbed_field(
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    agents: &[TeamMember],
    n_perturbations: usize,
    seed: u64,
    cfg: &ScoreConfig,
) -> PerturbationReport {
    let variants: Vec<Dataset> = (0..n_perturbations)
        .map(|k| {
            let vseed = seed ^ (k as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            perturb_dataset(data, vseed)
        })
        .collect();

    let spreads = agents
        .iter()
        .map(|m| {
            let psr_on = |d: &Dataset| {
                let sub = run_agent(&m.name, d, windows, seeds, costs, || (m.make)());
                score_agent(&sub, cfg).psr
            };
            let original_psr = psr_on(data);
            let perturbed_psrs: Vec<f64> = variants.iter().map(&psr_on).collect();
            let best = perturbed_psrs
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let worst = perturbed_psrs.iter().copied().fold(f64::INFINITY, f64::min);
            PerturbationSpread {
                agent_id: m.name.clone(),
                original_psr,
                perturbed_psrs,
                best_perturbed_psr: best,
                worst_perturbed_psr: worst,
                spread: best - worst,
            }
        })
        .collect();

    PerturbationReport {
        n_perturbations,
        seed,
        spreads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
    use sharpebench_sim::{Agent, BuyAndHold};

    #[test]
    fn perturbed_moves_stay_inside_the_empirical_range() {
        let data = Dataset::synthetic(4, 160, 11);
        let p = perturb_dataset(&data, 42);
        assert_ne!(p.closes, data.closes, "perturbation must move the path");
        for (sym, series) in &data.closes {
            let rets: Vec<f64> = series.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
            let lo = rets.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = rets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let pseries = &p.closes[sym];
            assert_eq!(pseries.len(), series.len());
            assert_eq!(pseries[0], series[0], "the first close is anchored");
            for w in pseries.windows(2) {
                let r = w[1] / w[0] - 1.0;
                assert!(
                    r >= lo - 1e-12 && r <= hi + 1e-12,
                    "{sym}: perturbed move {r} outside empirical range [{lo}, {hi}]"
                );
            }
        }
    }

    #[test]
    fn perturbation_is_deterministic_in_the_seed() {
        let data = Dataset::synthetic(3, 120, 7);
        let a = perturb_dataset(&data, 99);
        let b = perturb_dataset(&data, 99);
        assert_eq!(a.closes, b.closes, "same seed, same variant");
        let c = perturb_dataset(&data, 100);
        assert_ne!(a.closes, c.closes, "different seed, different variant");
    }

    /// A deliberately fragile agent: on its first observation it latches onto a
    /// parity bit of the exact close price, then either rides the whole market
    /// or sits in a token position for the rest of the run. Any perturbation
    /// flips the coin — the textbook shape of a strategy keyed to incidental
    /// features of the price path rather than to structure.
    struct ParityLatch {
        active: Option<bool>,
    }
    impl Agent for ParityLatch {
        fn decide(&mut self, obs: &MarketObservation) -> Decision {
            let active = *self.active.get_or_insert_with(|| {
                let px = obs.symbols[0].close_history.last().copied().unwrap_or(1.0);
                ((px * 1e8) as u64).is_multiple_of(2)
            });
            let n = obs.symbols.len().max(1) as f64;
            let orders = obs
                .symbols
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let w = if active {
                        1.0 / n
                    } else if i == 0 {
                        0.02
                    } else {
                        0.0
                    };
                    Order {
                        symbol: s.symbol.clone(),
                        action: if w > 0.0 { Action::Buy } else { Action::Close },
                        target_weight: w,
                        confidence: 0.5,
                        rationale: "parity latch".to_string(),
                    }
                })
                .collect();
            Decision {
                orders,
                reasoning: "fragile parity latch".to_string(),
                cost: None,
            }
        }
    }

    #[test]
    fn fragile_agent_shows_a_large_spread_and_a_robust_one_does_not() {
        let data = Dataset::synthetic(4, 160, 11);
        let windows = [
            Window { start: 30, end: 95 },
            Window {
                start: 95,
                end: 160,
            },
        ];
        let seeds = [1u64, 2, 3];
        let agents = [
            TeamMember::new("fragile", || {
                Box::new(ParityLatch { active: None }) as Box<dyn Agent>
            }),
            TeamMember::new("robust", || Box::new(BuyAndHold) as Box<dyn Agent>),
        ];
        let report = perturbed_field(
            &data,
            &windows,
            &seeds,
            CostModel::default(),
            &agents,
            6,
            42,
            &ScoreConfig::default(),
        );
        let fragile = &report.spreads[0];
        let robust = &report.spreads[1];
        assert_eq!(fragile.agent_id, "fragile");
        assert!(
            fragile.spread > robust.spread,
            "the parity latch must swing more than buy-and-hold: fragile {} vs robust {}",
            fragile.spread,
            robust.spread
        );
        assert!(
            fragile.spread > 2.0 * robust.spread,
            "the fragility signal must be unambiguous: fragile {} vs robust {}",
            fragile.spread,
            robust.spread
        );
    }
}
