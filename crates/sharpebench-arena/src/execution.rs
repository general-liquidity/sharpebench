//! The execution a forward window replays its captures under, as the signed
//! header binds it.
//!
//! A replayed row's returns are a function of the recorded decisions, the
//! revealed data and the execution model: the cost model (fixed decision delay
//! included), the engine that fills orders, and the window-by-seed matrix. The
//! header already binds the data and the scorer artifact. [`ExecutionIdentity`]
//! binds the rest, so a board scored under other execution settings than the
//! ones it states fails verification instead of reading as comparable.
//!
//! The identity is the replay contract every admitted capture was checked
//! against ([`sharpebench_harness::verify_trajectory_strict`] compares a
//! capture's cost model digest, engine version, windows and seeds with it), plus
//! the cost model in readable form: the leaderboard's [`CostProfile`] mirror.
//!
//! Versioned. A header without an identity keeps its legacy meaning (see
//! [`ExecutionStatus`]); an identity of another schema is refused, never read
//! as this one.

use serde::{Deserialize, Serialize};

use sharpebench_attest::content_digest;
use sharpebench_core::ScoreConfig;
use sharpebench_harness::{cost_model_digest, trajectory_contract};
use sharpebench_leaderboard::{CostProfile, ExecutionNoiseProfile, FixedDelayProfile};
use sharpebench_sim::{CostModel, ExecutionNoise, FixedDecisionDelay};

use crate::intake::{execution_matrix, ReplayWindow};
use crate::validate_sha256;

/// `schema_version` of an [`ExecutionIdentity`].
pub const EXECUTION_IDENTITY_SCHEMA: &str = "sharpebench.arena-execution.v1";

/// What the arena replayed a window's captures under. Recorded in the window
/// at scoring and signed into its header at publish.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionIdentity {
    /// Always [`EXECUTION_IDENTITY_SCHEMA`].
    pub schema_version: String,
    /// The replay cost model, fixed decision delay included, in the
    /// leaderboard's mirror form.
    pub cost_profile: CostProfile,
    /// Bit-exact digest of the same cost model
    /// ([`sharpebench_harness::cost_model_digest`]), the value every admitted
    /// capture's contract names. It must recompute from `cost_profile`.
    pub cost_model_sha256: String,
    /// The fill engine's version, as a capture's contract names it. The
    /// engine's bytes are the scorer artifact the header binds beside this.
    pub engine_version: String,
    /// `[start, end)` market windows of the execution matrix, in order.
    pub windows: Vec<(usize, usize)>,
    /// Execution seeds applied to every window, in order.
    pub seeds: Vec<u64>,
    /// See [`execution_digest`]: one value naming the scorer artifact and
    /// everything above, to compare two boards' execution by.
    pub execution_sha256: String,
}

/// How a published window's execution is established.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// The signed header binds an [`ExecutionIdentity`] that recomputes and
    /// matches the window file.
    Bound,
    /// No capture was replayed: the window ranked only supplied returns, or
    /// nothing, and executed nothing. Same meaning in every header version.
    NotReplayed,
    /// A header signed before the identity existed, over a window that
    /// replayed captures. Its stated meaning is the one it had: captures were
    /// replayed under the default cost model of the scorer artifact the header
    /// names, which is the only cost model that scorer replayed under. The
    /// documents do not bind it, so two such boards are not shown comparable.
    LegacyUnbound,
}

/// The mirror form of `costs`. Destructured, so a cost field the mirror lacks
/// is a compile error here rather than a board that cannot say how it ran.
pub fn cost_profile(costs: CostModel) -> CostProfile {
    let CostModel {
        fee_bps,
        slippage_bps,
        impact_bps,
        financing_bps,
        max_participation,
        trf_cost,
        noise,
        short_borrow_bps,
        fixed_delay,
    } = costs;
    CostProfile {
        fee_bps,
        slippage_bps,
        impact_bps,
        financing_bps,
        max_participation: max_participation.is_finite().then_some(max_participation),
        trf_cost,
        noise: noise.map(
            |ExecutionNoise {
                 delay_prob,
                 min_fill_frac,
                 carry_floor,
                 queue_participation_ref,
             }| ExecutionNoiseProfile {
                delay_prob,
                min_fill_frac,
                carry_floor,
                queue_participation_ref,
            },
        ),
        short_borrow_bps,
        fixed_delay: fixed_delay.map(|delay| match delay {
            FixedDecisionDelay::Fifo { bars } => FixedDelayProfile::Fifo { bars },
        }),
    }
}

/// The cost model `profile` mirrors. A `None` cap is unlimited liquidity.
pub fn cost_model(profile: &CostProfile) -> CostModel {
    let CostProfile {
        fee_bps,
        slippage_bps,
        impact_bps,
        financing_bps,
        max_participation,
        trf_cost,
        noise,
        short_borrow_bps,
        fixed_delay,
    } = profile.clone();
    CostModel {
        fee_bps,
        slippage_bps,
        impact_bps,
        financing_bps,
        max_participation: max_participation.unwrap_or(f64::INFINITY),
        trf_cost,
        noise: noise.map(
            |ExecutionNoiseProfile {
                 delay_prob,
                 min_fill_frac,
                 carry_floor,
                 queue_participation_ref,
             }| ExecutionNoise {
                delay_prob,
                min_fill_frac,
                carry_floor,
                queue_participation_ref,
            },
        ),
        short_borrow_bps,
        fixed_delay: fixed_delay.map(|delay| match delay {
            FixedDelayProfile::Fifo { bars } => FixedDecisionDelay::Fifo { bars },
        }),
    }
}

/// SHA-256 of the execution: the scorer artifact (the engine's bytes), the cost
/// model digest, the engine version and the matrix. The pre-image is
/// `sharpebench-arena/execution/v1`, then one line each for the scorer artifact,
/// the cost model digest and the engine version, then the windows as
/// `start..end` joined by `,`, then the seeds joined by `,`.
pub fn execution_digest(
    scorer_artifact_sha256: &str,
    cost_model_sha256: &str,
    engine_version: &str,
    windows: &[(usize, usize)],
    seeds: &[u64],
) -> String {
    let windows: Vec<String> = windows
        .iter()
        .map(|(start, end)| format!("{start}..{end}"))
        .collect();
    let seeds: Vec<String> = seeds.iter().map(u64::to_string).collect();
    content_digest(
        format!(
            "sharpebench-arena/execution/v1\n{scorer_artifact_sha256}\n{cost_model_sha256}\n{engine_version}\n{}\n{}",
            windows.join(","),
            seeds.join(",")
        )
        .as_bytes(),
    )
}

impl ExecutionIdentity {
    /// The identity of `replay` as scored by `scorer_artifact_sha256`: the
    /// contract a capture must name to be admitted.
    pub fn of(replay: &ReplayWindow, scorer_artifact_sha256: &str) -> Self {
        let contract =
            trajectory_contract(&replay.data, replay.costs, &replay.windows, &replay.seeds);
        let windows: Vec<(usize, usize)> = contract
            .windows
            .iter()
            .map(|window| (window.start, window.end))
            .collect();
        let execution_sha256 = execution_digest(
            scorer_artifact_sha256,
            &contract.cost_model_sha256,
            &contract.engine_version,
            &windows,
            &contract.seeds,
        );
        Self {
            schema_version: EXECUTION_IDENTITY_SCHEMA.to_string(),
            cost_profile: cost_profile(replay.costs),
            cost_model_sha256: contract.cost_model_sha256,
            engine_version: contract.engine_version,
            windows,
            seeds: contract.seeds,
            execution_sha256,
        }
    }

    /// Everything the documents can recompute: the schema, the cost digest
    /// from the profile, a matrix of the shape [`execution_matrix`] builds
    /// under `config`, and the execution digest under `scorer_artifact_sha256`.
    pub fn check(&self, scorer_artifact_sha256: &str, config: &ScoreConfig) -> Result<(), String> {
        if self.schema_version != EXECUTION_IDENTITY_SCHEMA {
            return Err(format!(
                "execution identity schema `{}` is not {EXECUTION_IDENTITY_SCHEMA}",
                self.schema_version
            ));
        }
        validate_sha256("execution cost model", &self.cost_model_sha256)?;
        validate_sha256("execution", &self.execution_sha256)?;
        let costs = cost_model(&self.cost_profile);
        costs.validate().map_err(|error| {
            format!("execution cost profile is not a valid cost model: {error}")
        })?;
        let recomputed = cost_model_digest(costs);
        if recomputed != self.cost_model_sha256 {
            return Err(format!(
                "execution cost profile digests to {recomputed}, but the identity records {}",
                self.cost_model_sha256
            ));
        }
        let len = self
            .windows
            .last()
            .map(|(_, end)| *end)
            .ok_or("execution identity records no market window")?;
        let (windows, seeds) = execution_matrix(len, config)?;
        let windows: Vec<(usize, usize)> = windows.iter().map(|w| (w.start, w.end)).collect();
        if windows != self.windows || seeds != self.seeds {
            return Err(format!(
                "execution matrix {:?} x seeds {:?} is not the matrix a forward window scores ({windows:?} x {seeds:?})",
                self.windows, self.seeds
            ));
        }
        let digest = execution_digest(
            scorer_artifact_sha256,
            &self.cost_model_sha256,
            &self.engine_version,
            &self.windows,
            &self.seeds,
        );
        if digest != self.execution_sha256 {
            return Err(format!(
                "execution digest recomputes to {digest}, but the identity records {}",
                self.execution_sha256
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cost field, a fixed delay included, survives the mirror both
    /// ways, so the profile a header shows is the model the digest names.
    #[test]
    fn every_cost_field_round_trips_through_the_signed_profile() {
        let costs = CostModel {
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
        let back = cost_model(&cost_profile(costs));
        assert_eq!(cost_model_digest(back), cost_model_digest(costs));
        let unlimited = CostModel::default();
        assert_eq!(cost_profile(unlimited).max_participation, None);
        assert_eq!(
            cost_model_digest(cost_model(&cost_profile(unlimited))),
            cost_model_digest(unlimited)
        );
    }
}
