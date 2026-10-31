//! Work units, resume and the output directory.
//!
//! A shard is a fixed replicate range of one block. It is a pure function of the
//! campaign, the run tier's seed and its index, so the published bytes do not
//! depend on the worker count or on where a previous attempt stopped. A shard is
//! staged to `<name>.tmp` and renamed into place only once complete, so a killed
//! run leaves whole shards and stray temporaries, never a short shard. Resume
//! reads each present shard's header and footer and skips it only when both
//! agree with this run's identity; a disagreeing shard is a refusal, never an
//! overwrite.

use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use sharpebench_core::composite::{rank, ScoreConfig};
use sharpebench_core::{AgentSubmission, Run};

use crate::campaign::{hex, Campaign, SeedPath, StatBlock};
use crate::controls::{run_control, ControlRow};
use crate::fields::{draw_field, effective_config_digest, score_cell, Cell, Row};

/// Evidence directories a run may never write into. A new campaign's output
/// lives in a directory of its own.
pub const FROZEN_EVIDENCE: &[&str] = &[
    "paper/evidence/final",
    "paper/evidence/after-v0.3.0",
    "paper/evidence/baseline-v0.2.1",
    "paper/evidence/development",
    "paper/evidence/prospective-forecast-field",
];

/// What a run is entitled to call itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// A reduced count on seeds disjoint from the frozen ones. Proves the
    /// producer works. Its rates are not estimates of anything.
    Smoke,
    /// The sealed count on the reserved seeds.
    Frozen,
}

impl Tier {
    /// Salt on the master seed. The frozen stream is the campaign's seed
    /// unchanged, so no smoke or timing run has ever looked at a frozen draw.
    fn seed_salt(self) -> u64 {
        match self {
            Self::Smoke => 0x5EED_0000_D15C_0DE5,
            Self::Frozen => 0,
        }
    }
}

/// The identity every shard carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunIdentity {
    pub tier: Tier,
    pub campaign_digest: String,
    pub engine_fingerprint: String,
    /// Replicates per block when smaller than the campaign's. Absent on a
    /// frozen run, which may not reduce.
    pub replicate_cap: Option<u64>,
}

/// A failure of the run as a whole. Everything here stops the run: no partial
/// result is published under a complete name.
#[derive(Debug)]
pub enum RunError {
    Refused(String),
    Io(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(detail) | Self::Io(detail) => f.write_str(detail),
        }
    }
}

impl std::error::Error for RunError {}

fn io_error(context: &str, error: impl std::fmt::Display) -> RunError {
    RunError::Io(format!("{context}: {error}"))
}

/// Lexically normalize a path to forward-slash components.
fn normalized(path: &Path) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            other => parts.push(other.as_os_str().to_string_lossy().to_lowercase()),
        }
    }
    parts
}

/// Refuse an output directory at or inside frozen evidence, however it is
/// spelled, and one that is not `repo`-relative-safe by the same test.
pub fn check_output_dir(out: &Path, repo: &Path) -> Result<(), RunError> {
    let out_abs = if out.is_absolute() {
        out.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| io_error("current directory", e))?
            .join(out)
    };
    let out_parts = normalized(&out_abs);
    let repo_abs = if repo.is_absolute() {
        repo.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| io_error("current directory", e))?
            .join(repo)
    };
    for frozen in FROZEN_EVIDENCE {
        let protected = normalized(&repo_abs.join(frozen));
        if out_parts.starts_with(&protected) {
            return Err(RunError::Refused(format!(
                "output {} is inside frozen evidence {frozen}",
                out.display()
            )));
        }
    }
    Ok(())
}

/// Every cell of a block, in the campaign's own order.
pub fn cells_of(block: &StatBlock) -> Vec<Cell> {
    let mut cells = Vec::new();
    for dgp in &block.dgps {
        for &history_bars in &block.history_bars {
            for &effect_annual in &block.effects_annual {
                for rule in &block.rules {
                    for &block_prob in &block.block_probs {
                        for &seed_path in &block.seed_paths {
                            cells.push(Cell {
                                block: block.id.clone(),
                                dgp: dgp.clone(),
                                history_bars,
                                effect_annual,
                                rule: rule.clone(),
                                block_prob,
                                seed_path,
                            });
                        }
                    }
                }
            }
        }
    }
    cells
}

/// The cell table of the whole campaign. A row's `cell` indexes it.
pub fn cell_table(campaign: &Campaign) -> Vec<Cell> {
    campaign.statistical.iter().flat_map(cells_of).collect()
}

/// `kernel rank calls` of one replicate of a block, the unit the budget counts.
pub fn rank_calls_per_replicate(block: &StatBlock) -> u64 {
    cells_of(block)
        .iter()
        .map(|c| match c.seed_path {
            SeedPath::Average => 1,
            SeedPath::PerSeed => block.execution_seeds as u64,
        })
        .sum()
}

/// One unit of work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shard {
    Statistical {
        block: usize,
        first_rep: u64,
        end_rep: u64,
    },
    Integrity {
        block: usize,
        first_rep: u64,
        end_rep: u64,
    },
}

impl Shard {
    pub fn file_name(&self, campaign: &Campaign) -> String {
        match *self {
            Shard::Statistical {
                block, first_rep, ..
            } => {
                format!(
                    "stat-{}-{first_rep:08}.jsonl",
                    campaign.statistical[block].id
                )
            }
            Shard::Integrity {
                block, first_rep, ..
            } => format!(
                "ctrl-{}-{first_rep:08}.jsonl",
                campaign.integrity[block].kind.tag()
            ),
        }
    }
}

fn replicate_count(planned: u64, cap: Option<u64>) -> u64 {
    cap.map_or(planned, |c| c.min(planned))
}

/// Every shard of the run, in canonical order.
pub fn shard_plan(campaign: &Campaign, cap: Option<u64>) -> Vec<Shard> {
    let width = campaign.shard_replicates;
    let mut shards = Vec::new();
    for (block, b) in campaign.statistical.iter().enumerate() {
        let n = replicate_count(b.replicates, cap);
        let mut first_rep = 0;
        while first_rep < n {
            let end_rep = (first_rep + width).min(n);
            shards.push(Shard::Statistical {
                block,
                first_rep,
                end_rep,
            });
            first_rep = end_rep;
        }
    }
    for (block, b) in campaign.integrity.iter().enumerate() {
        let n = replicate_count(b.replicates, cap);
        let mut first_rep = 0;
        while first_rep < n {
            let end_rep = (first_rep + width).min(n);
            shards.push(Shard::Integrity {
                block,
                first_rep,
                end_rep,
            });
            first_rep = end_rep;
        }
    }
    shards
}

/// Fingerprint of the engine actually linked: the default configuration and the
/// board of a fixed field, rounded to nine decimals so a last-bit platform
/// difference does not read as an engine change while any change to a verdict,
/// a default or a statistic does. Sixteen hex characters, the length a protocol's
/// `scoring_spec_hash` carries.
pub fn engine_fingerprint() -> String {
    let mut stream = crate::dgp::Stream::keyed(&[0xF1A6, 2026]);
    let field: Vec<AgentSubmission> = (0..7)
        .map(|e| AgentSubmission {
            agent_id: format!("fixture-{e}"),
            runs: (0..12)
                .map(|_| Run {
                    returns: (0..120)
                        .map(|_| 0.01 * (stream.normal() + 0.12 * e as f64))
                        .collect(),
                    ..Run::default()
                })
                .collect(),
            in_sample_trials: 0,
            candidates: Vec::new(),
        })
        .collect();
    let cfg = ScoreConfig {
        execution_seeds_per_window: 2,
        ..ScoreConfig::default()
    };
    let board = rank(&field, &cfg);
    let mut text = serde_json::to_string(&cfg).expect("a score config serializes");
    for row in &board {
        text.push_str(&format!(
            "|{}|{}|{}|{}|{:.9}|{:.9}|{:.9}|{:.9}",
            row.agent_id,
            row.rank_eligible,
            row.passed_k,
            row.effective_n_trials,
            row.deflated_sharpe,
            row.bootstrap_p,
            row.trials_sr_std,
            row.deflation_bar_per_period,
        ));
    }
    hex(&Sha256::digest(text.as_bytes()))[..16].to_string()
}

#[derive(Serialize, Deserialize)]
struct Footer {
    complete: bool,
    rows: u64,
}

/// Compute a shard's rows.
fn compute(
    campaign: &Campaign,
    identity: &RunIdentity,
    shard: &Shard,
    table_offsets: &[usize],
) -> Result<Vec<String>, RunError> {
    let master = campaign.master_seed ^ identity.tier.seed_salt();
    let mut lines = Vec::new();
    match *shard {
        Shard::Statistical {
            block,
            first_rep,
            end_rep,
        } => {
            let b = &campaign.statistical[block];
            let cells = cells_of(b);
            for rep in first_rep..end_rep {
                let mut offset = table_offsets[block];
                for (dgp_index, dgp_id) in b.dgps.iter().enumerate() {
                    let dgp = campaign.dgp(dgp_id).expect("validated");
                    for &bars in &b.history_bars {
                        let field = draw_field(
                            dgp,
                            [master, block as u64, dgp_index as u64, bars as u64, rep],
                            b.field_size,
                            bars,
                            b.execution_seeds,
                            b.seed_noise_eta,
                        );
                        let group = b.effects_annual.len()
                            * b.rules.len()
                            * b.block_probs.len()
                            * b.seed_paths.len();
                        let start = offset - table_offsets[block];
                        for (i, cell) in cells[start..start + group].iter().enumerate() {
                            let mut row = score_cell(campaign, b, cell, &field)
                                .map_err(|e| RunError::Refused(e.to_string()))?;
                            row.cell = offset + i;
                            row.rep = rep;
                            lines.push(serde_json::to_string(&row).expect("a row serializes"));
                        }
                        offset += group;
                    }
                }
            }
        }
        Shard::Integrity {
            block,
            first_rep,
            end_rep,
        } => {
            let b = &campaign.integrity[block];
            for rep in first_rep..end_rep {
                let row = run_control(campaign, b, master, rep);
                lines.push(serde_json::to_string(&row).expect("a row serializes"));
            }
        }
    }
    Ok(lines)
}

/// Is this shard already complete under this identity? A shard written by a
/// different identity, or one without its footer, is a refusal.
fn shard_is_complete(path: &Path, identity: &RunIdentity) -> Result<bool, RunError> {
    if !path.exists() {
        return Ok(false);
    }
    let file = fs::File::open(path).map_err(|e| io_error("open shard", e))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .transpose()
        .map_err(|e| io_error("read shard", e))?
        .unwrap_or_default();
    let found: RunIdentity = serde_json::from_str(&header).map_err(|_| {
        RunError::Refused(format!("shard {} has no identity header", path.display()))
    })?;
    if &found != identity {
        return Err(RunError::Refused(format!(
            "shard {} belongs to a different run: {found:?} against {identity:?}",
            path.display()
        )));
    }
    let mut last = String::new();
    let mut rows = 0_u64;
    for line in lines {
        last = line.map_err(|e| io_error("read shard", e))?;
        rows += 1;
    }
    let footer: Footer = serde_json::from_str(&last)
        .map_err(|_| RunError::Refused(format!("shard {} has no footer", path.display())))?;
    if !footer.complete || footer.rows + 1 != rows {
        return Err(RunError::Refused(format!(
            "shard {} is short",
            path.display()
        )));
    }
    Ok(true)
}

fn write_shard(path: &Path, identity: &RunIdentity, lines: &[String]) -> Result<(), RunError> {
    let tmp = path.with_extension("tmp");
    let mut out = BufWriter::new(fs::File::create(&tmp).map_err(|e| io_error("create shard", e))?);
    let header = serde_json::to_string(identity).expect("an identity serializes");
    writeln!(out, "{header}").map_err(|e| io_error("write shard", e))?;
    for line in lines {
        writeln!(out, "{line}").map_err(|e| io_error("write shard", e))?;
    }
    let footer = Footer {
        complete: true,
        rows: lines.len() as u64,
    };
    writeln!(
        out,
        "{}",
        serde_json::to_string(&footer).expect("a footer serializes")
    )
    .map_err(|e| io_error("write shard", e))?;
    out.flush().map_err(|e| io_error("flush shard", e))?;
    drop(out);
    fs::rename(&tmp, path).map_err(|e| io_error("publish shard", e))
}

/// Runtime of one run. Written beside the shards and never into them, so the
/// evidence stays byte-reproducible.
#[derive(Debug, Default, Serialize)]
pub struct RunTiming {
    pub shards_run: usize,
    pub shards_skipped: usize,
    pub workers: usize,
    pub wall_seconds: f64,
    /// Core seconds per shard file, the input to the budget projection.
    pub shard_core_seconds: Vec<(String, f64)>,
}

/// The configuration every rule, seed count and block length of the run scores
/// under, with its digest: the readback a reader compares with the protocol.
pub fn effective_config_document(campaign: &Campaign) -> serde_json::Value {
    let mut rows = Vec::new();
    for block in &campaign.statistical {
        for rule_id in &block.rules {
            let rule = campaign.rule(rule_id).expect("validated");
            for &p in &block.block_probs {
                let cfg = rule.score_config(campaign.periods_per_year, block.execution_seeds, p);
                rows.push(serde_json::json!({
                    "block": block.id,
                    "rule": rule.id,
                    "block_prob": p,
                    "digest": effective_config_digest(&cfg),
                    "config": cfg,
                }));
            }
        }
    }
    serde_json::Value::Array(rows)
}

/// Run every shard not already complete, on `workers` threads.
pub fn run_all(
    campaign: &Campaign,
    identity: &RunIdentity,
    out: &Path,
    workers: usize,
    stop_after: Option<usize>,
) -> Result<RunTiming, RunError> {
    let shards_dir: PathBuf = out.join("shards");
    fs::create_dir_all(&shards_dir).map_err(|e| io_error("create output", e))?;
    let plan = shard_plan(campaign, identity.replicate_cap);
    let mut offsets = Vec::with_capacity(campaign.statistical.len());
    let mut offset = 0;
    for block in &campaign.statistical {
        offsets.push(offset);
        offset += cells_of(block).len();
    }

    let identity_path = out.join("run-identity.json");
    let identity_text =
        serde_json::to_string_pretty(identity).expect("an identity serializes") + "\n";
    if identity_path.exists() {
        let found = fs::read_to_string(&identity_path).map_err(|e| io_error("read identity", e))?;
        if found != identity_text {
            return Err(RunError::Refused(format!(
                "{} holds another run's identity",
                out.display()
            )));
        }
    } else {
        fs::write(&identity_path, &identity_text).map_err(|e| io_error("write identity", e))?;
    }
    let effective = serde_json::to_string_pretty(&effective_config_document(campaign))
        .expect("the readback serializes")
        + "\n";
    fs::write(out.join("effective-config.json"), effective)
        .map_err(|e| io_error("write readback", e))?;
    let table = serde_json::to_string_pretty(&cell_table(campaign)).expect("cells serialize");
    fs::write(out.join("cells.json"), table + "\n").map_err(|e| io_error("write cells", e))?;

    let mut pending = Vec::new();
    let mut skipped = 0;
    for shard in &plan {
        let path = shards_dir.join(shard.file_name(campaign));
        if shard_is_complete(&path, identity)? {
            skipped += 1;
        } else {
            pending.push((shard.clone(), path));
        }
    }
    if let Some(limit) = stop_after {
        pending.truncate(limit);
    }

    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let failure: Mutex<Option<RunError>> = Mutex::new(None);
    let core_seconds: Mutex<Vec<(String, f64)>> = Mutex::new(Vec::new());
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..workers.max(1) {
            scope.spawn(|| loop {
                if failed.load(Ordering::SeqCst) {
                    return;
                }
                let index = next.fetch_add(1, Ordering::SeqCst);
                let Some((shard, path)) = pending.get(index) else {
                    return;
                };
                let begin = Instant::now();
                let result = compute(campaign, identity, shard, &offsets)
                    .and_then(|lines| write_shard(path, identity, &lines));
                match result {
                    Ok(()) => core_seconds
                        .lock()
                        .expect("no worker panics holding the lock")
                        .push((shard.file_name(campaign), begin.elapsed().as_secs_f64())),
                    Err(error) => {
                        failed.store(true, Ordering::SeqCst);
                        *failure.lock().expect("no worker panics holding the lock") = Some(error);
                        return;
                    }
                }
            });
        }
    });
    if let Some(error) = failure.into_inner().expect("workers have stopped") {
        return Err(error);
    }
    let mut shard_core_seconds = core_seconds.into_inner().expect("workers have stopped");
    shard_core_seconds.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(RunTiming {
        shards_run: shard_core_seconds.len(),
        shards_skipped: skipped,
        workers: workers.max(1),
        wall_seconds: started.elapsed().as_secs_f64(),
        shard_core_seconds,
    })
}

/// All statistical and control rows, in canonical shard order. Refuses a
/// missing or foreign shard: a report over part of a run is not available.
pub fn load_rows(
    campaign: &Campaign,
    identity: &RunIdentity,
    out: &Path,
) -> Result<(Vec<Row>, Vec<ControlRow>), RunError> {
    let mut rows = Vec::new();
    let mut controls = Vec::new();
    for shard in shard_plan(campaign, identity.replicate_cap) {
        let path = out.join("shards").join(shard.file_name(campaign));
        if !shard_is_complete(&path, identity)? {
            return Err(RunError::Refused(format!(
                "shard {} is missing",
                path.display()
            )));
        }
        let text = fs::read_to_string(&path).map_err(|e| io_error("read shard", e))?;
        let lines: Vec<&str> = text.lines().collect();
        let body = &lines[1..lines.len() - 1];
        for line in body {
            match shard {
                Shard::Statistical { .. } => {
                    rows.push(serde_json::from_str(line).map_err(|e| io_error("parse row", e))?)
                }
                Shard::Integrity { .. } => {
                    controls.push(serde_json::from_str(line).map_err(|e| io_error("parse row", e))?)
                }
            }
        }
    }
    Ok((rows, controls))
}
