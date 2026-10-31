//! One statistical cell: draw a field, build its submissions, score it with the
//! shipped `rank`, read the configuration back.
//!
//! Nothing in this module computes a verdict. `rank_eligible` comes off the
//! kernel's own board, field effects included: the host-N floor at the field
//! size, measured dispersion with its clone collapse, vote fence and floor, and
//! the shared-cell restriction. The legs are copied beside it for attribution
//! and are never recombined here.

use serde::{Deserialize, Serialize};

use sharpebench_core::composite::{rank, CompositeScore, Run, ScoreConfig, TrialsSrStdSource};
use sharpebench_core::deflated_sharpe::expected_max_sharpe;
use sharpebench_core::AgentSubmission;

use crate::campaign::{hex, Campaign, Dgp, Rule, SeedPath, StatBlock};
use crate::dgp::{blend_shared, zero_skill_path, Stream};
use sha2::{Digest, Sha256};

/// Id of the entrant the per-entry and power estimands read: one fixed entrant
/// per field, so its rate is a binomial over independent fields rather than a
/// pooled rate over entrants that share a history.
pub const FOCAL: &str = "focal";

/// A readback the kernel disagreed with. The run stops: a cell scored under a
/// configuration other than the declared one is not a measurement of it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadbackMismatch(pub String);

impl std::fmt::Display for ReadbackMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "effective-config readback mismatch: {}", self.0)
    }
}

impl std::error::Error for ReadbackMismatch {}

/// One scored cell of one replicate. Reduced to what a rate needs, with the
/// legs kept for attribution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// Index into the cell table of [`crate::shards::cell_table`].
    pub cell: usize,
    pub rep: u64,
    pub focal_eligible: bool,
    /// Any zero-skill entrant admitted, the focal one included when it is
    /// zero-skill. The whole-field false-eligibility event.
    pub any_false: bool,
    pub eligible_count: u32,
    pub focal_dsr_leg: bool,
    pub focal_reliability_leg: bool,
    pub focal_bootstrap_leg: bool,
    /// Annualized deflation benchmark the focal entrant faced.
    pub bar_annual: f64,
    /// `configured`, `measured` or `measured_floored` of the focal entrant.
    pub dispersion_source: u8,
    /// Entrants the kernel returned a typed refusal for. Unavailable stays
    /// typed: such an entrant is ineligible, and this says how often.
    pub unavailable: u32,
}

/// The drawn zero-skill field of one (block, dgp, history, replicate), before
/// any effect is planted. Effects, rules and block lengths share these draws,
/// so a comparison between two rules is paired within a replicate.
pub struct DrawnField {
    /// `entrants x bars`.
    pub paths: Vec<Vec<f64>>,
    /// `entrants x seeds x bars`, unit variance, before `eta`.
    pub seed_noise: Vec<Vec<Vec<f64>>>,
}

/// The draws of one field.
pub fn draw_field(
    dgp: &Dgp,
    key: [u64; 5],
    field_size: usize,
    bars: usize,
    seeds: usize,
    eta: f64,
) -> DrawnField {
    let [master, block, dgp_index, bars_key, rep] = key;
    let stream = |role: u64, entrant: u64, seed: u64| {
        Stream::keyed(&[master, block, dgp_index, bars_key, rep, role, entrant, seed])
    };
    let mut paths: Vec<Vec<f64>> = (0..field_size)
        .map(|e| zero_skill_path(dgp, &mut stream(1, e as u64, 0), bars))
        .collect();
    if dgp.shared_variance > 0.0 {
        let common = zero_skill_path(dgp, &mut stream(2, 0, 0), bars);
        blend_shared(&mut paths, &common, dgp.shared_variance);
    }
    let seed_noise = if eta > 0.0 {
        (0..field_size)
            .map(|e| {
                (0..seeds)
                    .map(|s| {
                        let mut noise = stream(3, e as u64, s as u64);
                        (0..bars).map(|_| noise.normal()).collect()
                    })
                    .collect()
            })
            .collect()
    } else {
        Vec::new()
    };
    DrawnField { paths, seed_noise }
}

/// One entrant's submission in window-major layout: every seed of window 0,
/// then window 1, the layout the kernel documents on [`Run`].
fn submission(
    field: &DrawnField,
    entrant: usize,
    shift: f64,
    windows: usize,
    seeds: &[usize],
    eta: f64,
    vol: f64,
) -> AgentSubmission {
    let path = &field.paths[entrant];
    let window_len = path.len() / windows;
    let mut runs = Vec::with_capacity(windows * seeds.len());
    for w in 0..windows {
        let bars = w * window_len..(w + 1) * window_len;
        for &seed in seeds {
            let returns = bars
                .clone()
                .map(|t| {
                    let noise = if eta > 0.0 {
                        eta * field.seed_noise[entrant][seed][t]
                    } else {
                        0.0
                    };
                    vol * (path[t] + shift + noise)
                })
                .collect();
            runs.push(Run {
                returns,
                ..Run::default()
            });
        }
    }
    AgentSubmission {
        agent_id: agent_id(entrant),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

/// The id of field position `entrant`; position 0 is the focal entrant.
pub fn agent_id(entrant: usize) -> String {
    if entrant == 0 {
        FOCAL.to_string()
    } else {
        format!("peer-{entrant:03}")
    }
}

/// The whole field as submissions.
pub fn submissions(
    field: &DrawnField,
    effect_per_period: f64,
    windows: usize,
    seeds: &[usize],
    eta: f64,
    vol: f64,
) -> Vec<AgentSubmission> {
    (0..field.paths.len())
        .map(|e| {
            let shift = if e == 0 { effect_per_period } else { 0.0 };
            submission(field, e, shift, windows, seeds, eta, vol)
        })
        .collect()
}

/// Lowercase hex digest of the configuration as serialized, the readback that
/// `effective-config.json` publishes for every cell.
pub fn effective_config_digest(cfg: &ScoreConfig) -> String {
    let bytes = serde_json::to_vec(cfg).expect("a score config serializes");
    hex(&Sha256::digest(&bytes))
}

/// Check that the board reports the configuration the cell declared.
pub fn read_back(
    board: &[CompositeScore],
    rule: &Rule,
    cfg: &ScoreConfig,
    field_size: usize,
    bars: usize,
    runs: usize,
) -> Result<(), ReadbackMismatch> {
    let mismatch = |what: String| Err(ReadbackMismatch(format!("rule {}: {what}", rule.id)));
    if board.len() != field_size {
        return mismatch(format!("{} rows for a field of {field_size}", board.len()));
    }
    let n_expected = rule
        .host_n
        .max(u32::try_from(field_size).unwrap_or(u32::MAX));
    let prior_per_period = rule.dispersion_prior / cfg.periods_per_year.sqrt();
    for score in board {
        if score.deflation_error.is_some() {
            continue;
        }
        if score.effective_n_trials != n_expected {
            return mismatch(format!(
                "effective N {} is not {n_expected}",
                score.effective_n_trials
            ));
        }
        if score.pooled_observations != bars {
            return mismatch(format!(
                "pooled {} observations, declared {bars}",
                score.pooled_observations
            ));
        }
        if score.runs_scored != runs {
            return mismatch(format!(
                "scored {} runs, declared {runs}",
                score.runs_scored
            ));
        }
        let used = score.trials_sr_std;
        let consistent = match score.trials_sr_std_source {
            TrialsSrStdSource::Configured | TrialsSrStdSource::MeasuredFloored => {
                (used - prior_per_period).abs() <= 1e-12 * prior_per_period.max(1e-300)
            }
            TrialsSrStdSource::Measured => used >= prior_per_period * (1.0 - 1e-12),
        };
        if !consistent {
            return mismatch(format!(
                "dispersion {used} under source {:?} contradicts prior {prior_per_period}",
                score.trials_sr_std_source
            ));
        }
        let bar = expected_max_sharpe(used, score.effective_n_trials)
            .map_err(|e| ReadbackMismatch(format!("rule {}: {e}", rule.id)))?;
        if (bar - score.deflation_bar_per_period).abs() > 1e-12 * bar.abs().max(1e-300) {
            return mismatch(format!(
                "bar {} is not the {bar} the dispersion implies",
                score.deflation_bar_per_period
            ));
        }
    }
    Ok(())
}

fn source_code(source: TrialsSrStdSource) -> u8 {
    match source {
        TrialsSrStdSource::Configured => 0,
        TrialsSrStdSource::Measured => 1,
        TrialsSrStdSource::MeasuredFloored => 2,
    }
}

/// Reduce a board to a row. `planted` says the focal entrant is not zero-skill,
/// so its admission is a detection and not a false eligibility.
pub fn reduce(board: &[CompositeScore], cfg: &ScoreConfig, planted: bool) -> (Row, usize) {
    let focal = board
        .iter()
        .find(|s| s.agent_id == FOCAL)
        .expect("the focal entrant is on every board");
    let any_false = board
        .iter()
        .any(|s| s.rank_eligible && (!planted || s.agent_id != FOCAL));
    let eligible_count = board.iter().filter(|s| s.rank_eligible).count() as u32;
    let unavailable = board
        .iter()
        .filter(|s| s.deflation_error.is_some() || s.bootstrap_error.is_some())
        .count();
    (
        Row {
            cell: 0,
            rep: 0,
            focal_eligible: focal.rank_eligible,
            any_false,
            eligible_count,
            focal_dsr_leg: focal.deflation_error.is_none() && focal.deflated_sharpe >= cfg.dsr_bar,
            focal_reliability_leg: focal.passed_k,
            focal_bootstrap_leg: focal.bootstrap_error.is_none() && focal.bootstrap_p < cfg.alpha,
            bar_annual: focal.deflation_bar_annualized_equivalent,
            dispersion_source: source_code(focal.trials_sr_std_source),
            unavailable: unavailable as u32,
        },
        unavailable,
    )
}

/// One concrete cell of a block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub block: String,
    pub dgp: String,
    pub history_bars: usize,
    pub effect_annual: f64,
    pub rule: String,
    pub block_prob: f64,
    pub seed_path: SeedPath,
}

/// Score one cell on already drawn paths.
pub fn score_cell(
    campaign: &Campaign,
    block: &StatBlock,
    cell: &Cell,
    field: &DrawnField,
) -> Result<Row, ReadbackMismatch> {
    let rule = campaign
        .rule(&cell.rule)
        .expect("validated campaigns name known rules");
    let shift = cell.effect_annual / campaign.periods_per_year.sqrt();
    let planted = cell.effect_annual != 0.0;
    let bars = cell.history_bars;
    let all_seeds: Vec<usize> = (0..block.execution_seeds).collect();
    match cell.seed_path {
        SeedPath::Average => {
            let subs = submissions(
                field,
                shift,
                rule.windows,
                &all_seeds,
                block.seed_noise_eta,
                campaign.vol_per_period,
            );
            let cfg = rule.score_config(
                campaign.periods_per_year,
                block.execution_seeds,
                cell.block_prob,
            );
            let board = rank(&subs, &cfg);
            read_back(
                &board,
                rule,
                &cfg,
                block.field_size,
                bars,
                rule.windows * block.execution_seeds,
            )?;
            Ok(reduce(&board, &cfg, planted).0)
        }
        SeedPath::PerSeed => {
            // Each execution seed is scored as a path of its own; an entrant is
            // eligible only when every one admits it. No pooled verdict exists
            // to recombine, so the row is built from the per-seed boards.
            let cfg = rule.score_config(campaign.periods_per_year, 1, cell.block_prob);
            let mut eligible = vec![true; block.field_size];
            let mut first: Option<Row> = None;
            let mut unavailable = 0;
            for &seed in &all_seeds {
                let subs = submissions(
                    field,
                    shift,
                    rule.windows,
                    &[seed],
                    block.seed_noise_eta,
                    campaign.vol_per_period,
                );
                let board = rank(&subs, &cfg);
                read_back(&board, rule, &cfg, block.field_size, bars, rule.windows)?;
                for (position, flag) in eligible.iter_mut().enumerate() {
                    let id = agent_id(position);
                    let admitted = board.iter().any(|s| s.agent_id == id && s.rank_eligible);
                    *flag = *flag && admitted;
                }
                let (row, missing) = reduce(&board, &cfg, planted);
                unavailable += missing;
                first.get_or_insert(row);
            }
            let mut row = first.expect("a block has at least one execution seed");
            row.focal_eligible = eligible[0];
            row.any_false = eligible
                .iter()
                .enumerate()
                .any(|(position, &flag)| flag && (!planted || position != 0));
            row.eligible_count = eligible.iter().filter(|&&flag| flag).count() as u32;
            row.unavailable = unavailable as u32;
            Ok(row)
        }
    }
}
