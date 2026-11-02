//! The fast-signal track: measures that separate agents in days rather than
//! years, reported beside the skill verdict and never read by it.
//!
//! The skill verdict needs a long track because a Sharpe ratio is estimated
//! slowly. Several behaviours the kernel and the harness already record are
//! measured precisely after a few runs: whether an agent keeps its process
//! clean, stays inside its mandate, trades a little or a lot, faults, spends,
//! answers differently when the same data is shown under another name, and
//! decides the same way twice. This module collects them into one separately
//! versioned report ([`FAST_SIGNALS_VERSION`]).
//!
//! Every measure states its scale, its direction, an interval and whether it
//! could be computed. None of them is evidence of trading skill: an agent can
//! score well on every measure here and have no edge at all. The report is a
//! rank-neutral disclosure. No score, rank, ordinal, gate, pass^k pool or
//! headline verdict reads it.
//!
//! Intervals are 95% and come in two kinds:
//!
//! - a share of countable units (runs, cells, steps, replicate pairs) carries
//!   the exact Clopper-Pearson binomial interval;
//! - a mean over runs or windows carries a percentile bootstrap over that unit,
//!   [`BOOTSTRAP_RESAMPLES`] resamples under the fixed seed
//!   [`BOOTSTRAP_SEED`], so the same input gives the same bits everywhere.
//!
//! Both treat their unit as independent. Runs of one window share its bars and
//! the steps of one presentation pair share a book, so where the unit is not
//! independent the interval is narrower than the truth and the measure's
//! `interval.sampling_unit` says what was resampled.
//!
//! Pure: no I/O, no clock.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::composite::{max_drawdown, AgentSubmission, ScoreConfig};
use crate::process::{process_score, ProcessEvent};
use crate::trial_census::{AgentCensus, CensusGate};

/// Schema of [`FastSignalsReport`].
pub const FAST_SIGNALS_VERSION: &str = "sharpebench.fast-signals.v1";

/// Two-sided level of every interval in the report.
pub const INTERVAL_LEVEL: f64 = 0.95;

/// Percentile-bootstrap resamples per interval.
pub const BOOTSTRAP_RESAMPLES: usize = 2000;

/// Seed of every percentile bootstrap (`"SIGNALS1"` in ASCII).
pub const BOOTSTRAP_SEED: u64 = 0x5349_474E_414C_5331;

/// The statement every report carries.
pub const NOT_SKILL_EVIDENCE: &str = "No measure in this report is evidence of trading skill. \
    Each separates agents on behaviour the benchmark records, and none is a rank, gate or \
    verdict input.";

/// Bisection steps for a Clopper-Pearson bound; 2^-100 is below f64 spacing
/// on (0, 1).
const BISECTION_STEPS: usize = 100;

/// Prefix of a census failure reason the harness classed as the entrant's own
/// fault (`sharpebench_harness::trial_reports`).
const AGENT_FAULT_PREFIX: &str = "agent_fault: ";

/// Prefix of a census failure reason the harness classed as its own runtime
/// failure.
const RUNTIME_PREFIX: &str = "runtime: ";

/// Which way a measure reads as better behaviour. Better behaviour is still
/// not skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalDirection {
    HigherIsBetter,
    LowerIsBetter,
}

/// A two-sided interval around a measure's value.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignalInterval {
    pub low: f64,
    pub high: f64,
    pub level: f64,
    /// `clopper_pearson_exact` or `percentile_bootstrap`.
    pub method: &'static str,
    /// What the interval treats as the independent unit.
    pub sampling_unit: &'static str,
}

/// One measure of one agent.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignalMeasure {
    /// Stable identifier, e.g. `process_clean_runs`.
    pub id: &'static str,
    /// The kind of behaviour measured, e.g. `process` or `refusal_and_fault`.
    pub family: &'static str,
    /// What the number is and its range.
    pub scale: String,
    pub direction: SignalDirection,
    /// Where the inputs came from.
    pub source: &'static str,
    /// The value, or `None` when it could not be computed (`unavailable`).
    pub value: Option<f64>,
    /// For a share: the counted units, then the units counted over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// The number of units behind the value.
    pub observations: usize,
    pub interval: Option<SignalInterval>,
    /// Why the value or the interval is missing. Omitted when both exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    /// Breakdown counts, e.g. agent faults by kind. Omitted when empty.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub detail: BTreeMap<String, usize>,
    /// Always `false`.
    pub skill_evidence: bool,
}

impl SignalMeasure {
    fn new(
        id: &'static str,
        family: &'static str,
        scale: String,
        direction: SignalDirection,
        source: &'static str,
    ) -> Self {
        Self {
            id,
            family,
            scale,
            direction,
            source,
            value: None,
            count: None,
            observations: 0,
            interval: None,
            unavailable: None,
            detail: BTreeMap::new(),
            skill_evidence: false,
        }
    }

    fn unavailable(mut self, reason: impl Into<String>) -> Self {
        self.unavailable = Some(reason.into());
        self
    }

    /// `hits` of `total` units, with the exact interval.
    fn share(mut self, hits: usize, total: usize, unit: &'static str) -> Self {
        self.observations = total;
        match clopper_pearson(hits, total, INTERVAL_LEVEL) {
            Some((low, high)) => {
                self.value = Some(hits as f64 / total as f64);
                self.count = Some(hits);
                self.interval = Some(SignalInterval {
                    low,
                    high,
                    level: INTERVAL_LEVEL,
                    method: "clopper_pearson_exact",
                    sampling_unit: unit,
                });
                self
            }
            None => self.unavailable(format!("no {unit} to count")),
        }
    }

    /// The mean of `values`, with the percentile bootstrap over them.
    fn mean_of(mut self, values: &[f64], unit: &'static str) -> Self {
        self.observations = values.len();
        if values.is_empty() {
            return self.unavailable(format!("no {unit} to average"));
        }
        self.value = Some(mean(values));
        match percentile_bootstrap(values, INTERVAL_LEVEL) {
            Some((low, high)) => {
                self.interval = Some(SignalInterval {
                    low,
                    high,
                    level: INTERVAL_LEVEL,
                    method: "percentile_bootstrap",
                    sampling_unit: unit,
                });
                self
            }
            None => self.unavailable(format!("fewer than two {unit}: no interval")),
        }
    }
}

/// One agent's measures, in a fixed order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentSignals {
    pub agent_id: String,
    pub runs: usize,
    pub measures: Vec<SignalMeasure>,
}

impl AgentSignals {
    /// The measure with this id, when the report has it.
    pub fn measure(&self, id: &str) -> Option<&SignalMeasure> {
        self.measures.iter().find(|m| m.id == id)
    }
}

/// The bootstrap every mean interval used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SignalBootstrap {
    pub resamples: usize,
    pub seed: u64,
}

/// The fast-signal report of a field.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FastSignalsReport {
    pub schema: &'static str,
    /// Always `false`: no rank, gate or verdict reads this report.
    pub rank_input: bool,
    /// Always `false`.
    pub skill_evidence: bool,
    pub statement: &'static str,
    pub interval_level: f64,
    pub bootstrap: SignalBootstrap,
    /// One entry per submission, in field order.
    pub agents: Vec<AgentSignals>,
    /// Agent ids that extra evidence was supplied for but the field does not
    /// contain, sorted. Their evidence is reported nowhere else.
    pub unmatched_evidence: Vec<String>,
}

/// Host cost a gateway observed for an agent's whole sweep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostObservedCost {
    /// Host-observed spend in USD.
    pub usd: f64,
    /// Runs the spend covers.
    pub runs: usize,
}

/// One presentation pair's step counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationCounts {
    /// The variant shown against the baseline, e.g. `reordered`.
    pub variant: String,
    pub steps: usize,
    pub disagreeing_steps: usize,
}

/// Evidence beyond the submissions file, per agent. Every field is optional:
/// a measure whose evidence is absent is reported unavailable with the reason,
/// never left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SignalEvidence {
    /// The agent's row of a trial census (`suite_evidence.trials.per_agent`).
    pub census: Option<AgentCensus>,
    /// Host-observed gateway spend. Supersedes the entrant-reported `cost`.
    pub host_cost: Option<HostObservedCost>,
    /// Contamination probe: real-minus-twin per-bar Sharpe per window, or why
    /// the probe had no result.
    pub contamination_edges: Option<Result<Vec<f64>, String>>,
    /// Presentation stability: one entry per compared pair, or why the suite
    /// had no result.
    pub presentation: Option<Result<Vec<PresentationCounts>, String>>,
    /// Decision stability: `(pairs_compared, differing_pairs)` summed over
    /// windows, or why the rate is unavailable.
    pub decision_stability: Option<Result<(usize, usize), String>>,
}

/// Build the report. `cfg` supplies the per-run mandate cap; `evidence` maps
/// agent ids to optional extra evidence.
pub fn fast_signals(
    subs: &[AgentSubmission],
    cfg: &ScoreConfig,
    evidence: &BTreeMap<String, SignalEvidence>,
) -> FastSignalsReport {
    let none = SignalEvidence::default();
    let agents = subs
        .iter()
        .map(|sub| agent_signals(sub, cfg, evidence.get(&sub.agent_id).unwrap_or(&none)))
        .collect();
    let unmatched_evidence = evidence
        .keys()
        .filter(|id| !subs.iter().any(|s| &&s.agent_id == id))
        .cloned()
        .collect();
    FastSignalsReport {
        schema: FAST_SIGNALS_VERSION,
        rank_input: false,
        skill_evidence: false,
        statement: NOT_SKILL_EVIDENCE,
        interval_level: INTERVAL_LEVEL,
        bootstrap: SignalBootstrap {
            resamples: BOOTSTRAP_RESAMPLES,
            seed: BOOTSTRAP_SEED,
        },
        agents,
        unmatched_evidence,
    }
}

fn agent_signals(
    sub: &AgentSubmission,
    cfg: &ScoreConfig,
    evidence: &SignalEvidence,
) -> AgentSignals {
    let runs = &sub.runs;
    let processes: Vec<_> = runs.iter().map(|r| process_score(&r.trace)).collect();
    let clean = processes.iter().filter(|p| p.is_clean()).count();
    let warns: Vec<f64> = processes.iter().map(|p| p.warn_violations as f64).collect();
    let cap = cfg.mandate.max_run_drawdown;
    let compliant = runs
        .iter()
        .filter(|r| max_drawdown(&r.returns) <= cap)
        .count();
    let orders: Vec<f64> = runs
        .iter()
        .map(|r| {
            r.trace
                .events
                .iter()
                .filter(|e| matches!(e, ProcessEvent::OrderPlaced { .. }))
                .count() as f64
        })
        .collect();
    let measures = vec![
        SignalMeasure::new(
            "process_clean_runs",
            "process",
            "share of runs with no block-severity process violation, in [0, 1]".into(),
            SignalDirection::HigherIsBetter,
            "submission trace",
        )
        .share(clean, runs.len(), "runs"),
        SignalMeasure::new(
            "warn_violations_per_run",
            "process",
            "warn-severity process violations per run, at least 0".into(),
            SignalDirection::LowerIsBetter,
            "submission trace",
        )
        .mean_of(&warns, "runs"),
        SignalMeasure::new(
            "mandate_compliant_runs",
            "mandate",
            format!("share of runs whose own maximum drawdown is at most the per-run cap {cap}, in [0, 1]"),
            SignalDirection::HigherIsBetter,
            "submission returns",
        )
        .share(compliant, runs.len(), "runs"),
        SignalMeasure::new(
            "orders_per_run",
            "turnover",
            "orders placed per run, at least 0 (the board's turnover column)".into(),
            SignalDirection::LowerIsBetter,
            "submission trace",
        )
        .mean_of(&orders, "runs"),
        cost_measure(sub, evidence.host_cost),
        fault_measure(evidence.census.as_ref()),
        contamination_measure(evidence.contamination_edges.as_ref()),
        presentation_measure(evidence.presentation.as_ref()),
        stability_measure(evidence.decision_stability.as_ref()),
    ];
    AgentSignals {
        agent_id: sub.agent_id.clone(),
        runs: runs.len(),
        measures,
    }
}

/// Spend per run. Host-observed spend, when supplied, replaces the
/// entrant-reported figure entirely, as the host cost columns do: a measure
/// never mixes the two sources.
fn cost_measure(sub: &AgentSubmission, host: Option<HostObservedCost>) -> SignalMeasure {
    if let Some(host) = host {
        let measure = SignalMeasure::new(
            "cost_per_run",
            "cost",
            "host-observed USD per run, at least 0".into(),
            SignalDirection::LowerIsBetter,
            "host_observed_usd",
        );
        if host.runs == 0 || !host.usd.is_finite() || host.usd < 0.0 {
            return measure
                .unavailable("host-observed spend covers no run or is not a finite amount");
        }
        let mut measure = measure.unavailable(
            "one host-observed total and no per-run breakdown: nothing to resample, so no interval",
        );
        measure.value = Some(host.usd / host.runs as f64);
        measure.observations = host.runs;
        return measure;
    }
    let measure = SignalMeasure::new(
        "cost_per_run",
        "cost",
        "entrant-reported cost per run, in the entrant's unit, at least 0".into(),
        SignalDirection::LowerIsBetter,
        "entrant_reported",
    );
    let costs: Vec<f64> = sub.runs.iter().map(|r| r.cost).collect();
    if costs.is_empty() || costs.iter().any(|&c| c <= 0.0 || !c.is_finite()) {
        return measure
            .unavailable("cost is not reported as a positive finite number on every run");
    }
    measure.mean_of(&costs, "runs")
}

/// Agent faults among the cells the harness could evaluate. Runtime failures,
/// unreported cells and duplicate reports are the harness's, not the agent's,
/// so they leave the denominator and are listed in `detail`.
fn fault_measure(census: Option<&AgentCensus>) -> SignalMeasure {
    let measure = SignalMeasure::new(
        "agent_fault_cells",
        "refusal_and_fault",
        "share of evaluable declared cells that ended in an agent fault, in [0, 1]".into(),
        SignalDirection::LowerIsBetter,
        "trial census",
    );
    let Some(census) = census else {
        return measure.unavailable("no trial census was supplied");
    };
    let mut detail = BTreeMap::new();
    let mut faults = 0;
    for gate in &census.gates {
        let key = match gate {
            CensusGate::TrialFailed { reason, .. } => match reason.strip_prefix(AGENT_FAULT_PREFIX)
            {
                Some(kind) => {
                    faults += 1;
                    format!("agent_fault:{kind}")
                }
                None => match reason.strip_prefix(RUNTIME_PREFIX) {
                    Some(kind) => format!("excluded:runtime:{kind}"),
                    None => format!("excluded:failed:{reason}"),
                },
            },
            CensusGate::TrialUnreported { .. } => "excluded:unreported".to_string(),
            CensusGate::TrialReportedTwice { .. } => "excluded:reported_twice".to_string(),
            CensusGate::OffRosterCell { .. } => "excluded:off_roster".to_string(),
        };
        *detail.entry(key).or_insert(0) += 1;
    }
    let mut measure = measure.share(faults, census.completed + faults, "declared cells");
    measure.detail = detail;
    measure
}

fn contamination_measure(edges: Option<&Result<Vec<f64>, String>>) -> SignalMeasure {
    let measure = SignalMeasure::new(
        "contamination_mean_edge",
        "contamination",
        "mean over windows of the real-minus-twin per-bar Sharpe; above 0 is consistent with recall of the real prices".into(),
        SignalDirection::LowerIsBetter,
        "contamination probe",
    );
    match edges {
        None => measure.unavailable("no contamination probe was supplied"),
        Some(Err(reason)) => measure.unavailable(format!("the probe had no result: {reason}")),
        Some(Ok(edges)) => measure.mean_of(edges, "windows"),
    }
}

fn presentation_measure(pairs: Option<&Result<Vec<PresentationCounts>, String>>) -> SignalMeasure {
    let measure = SignalMeasure::new(
        "presentation_disagreement",
        "presentation",
        "share of compared steps whose decision changed when the same data was presented differently, in [0, 1]".into(),
        SignalDirection::LowerIsBetter,
        "presentation stability",
    );
    let pairs = match pairs {
        None => return measure.unavailable("no presentation-stability suite was supplied"),
        Some(Err(reason)) => {
            return measure.unavailable(format!("the suite had no result: {reason}"))
        }
        Some(Ok(pairs)) => pairs,
    };
    let steps = pairs.iter().map(|p| p.steps).sum();
    let disagreeing = pairs.iter().map(|p| p.disagreeing_steps).sum();
    let mut measure = measure.share(disagreeing, steps, "steps");
    measure.detail = pairs
        .iter()
        .map(|p| (p.variant.clone(), p.disagreeing_steps))
        .collect();
    measure
}

fn stability_measure(counts: Option<&Result<(usize, usize), String>>) -> SignalMeasure {
    let measure = SignalMeasure::new(
        "decision_disagreement",
        "decision_stability",
        "share of replicate pairs on a shared observation history that decided differently, in [0, 1]".into(),
        SignalDirection::LowerIsBetter,
        "decision stability",
    );
    match counts {
        None => measure.unavailable("no decision-stability report was supplied"),
        Some(Err(reason)) => measure.unavailable(format!("the rate is unavailable: {reason}")),
        Some(Ok((pairs, differing))) => measure.share(*differing, *pairs, "replicate pairs"),
    }
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

/// The binomial probability of at most `k` successes in `n` trials, for `p`
/// strictly inside (0, 1). Each term is formed in log space so no factorial
/// overflows.
fn binomial_cdf(k: usize, n: usize, p: f64) -> f64 {
    let (ln_p, ln_q) = (p.ln(), (1.0 - p).ln());
    let mut ln_choose = 0.0;
    let mut total = 0.0;
    for i in 0..=k {
        if i > 0 {
            ln_choose += ((n - i + 1) as f64).ln() - (i as f64).ln();
        }
        total += (ln_choose + i as f64 * ln_p + (n - i) as f64 * ln_q).exp();
    }
    total.min(1.0)
}

/// The `p` in (0, 1) at which the decreasing function `f` crosses `target`.
fn bisect_decreasing(target: f64, f: impl Fn(f64) -> f64) -> f64 {
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..BISECTION_STEPS {
        let mid = 0.5 * (lo + hi);
        if f(mid) > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The exact (Clopper-Pearson) two-sided interval for `hits` of `total`.
/// `None` when `total` is 0 or `hits > total`.
pub(crate) fn clopper_pearson(hits: usize, total: usize, level: f64) -> Option<(f64, f64)> {
    if total == 0 || hits > total {
        return None;
    }
    let tail = 0.5 * (1.0 - level);
    // P(X >= hits) = 1 - P(X <= hits - 1) increases with p; the lower bound is
    // where it reaches `tail`, i.e. where P(X <= hits - 1) falls to 1 - tail.
    let low = if hits == 0 {
        0.0
    } else {
        bisect_decreasing(1.0 - tail, |p| binomial_cdf(hits - 1, total, p))
    };
    let high = if hits == total {
        1.0
    } else {
        bisect_decreasing(tail, |p| binomial_cdf(hits, total, p))
    };
    Some((low, high))
}

/// Minimal deterministic PRNG (SplitMix64) for the bootstrap.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform integer in `[0, n)`, `n > 0`, by the high bits of a 128-bit
    /// product.
    fn below(&mut self, n: usize) -> usize {
        ((u128::from(self.next_u64()) * n as u128) >> 64) as usize
    }
}

/// The percentile-bootstrap interval of the mean of `values`. `None` below two
/// values, where resampling cannot show any spread.
fn percentile_bootstrap(values: &[f64], level: f64) -> Option<(f64, f64)> {
    let n = values.len();
    if n < 2 {
        return None;
    }
    let mut rng = SplitMix64(BOOTSTRAP_SEED);
    let mut means: Vec<f64> = (0..BOOTSTRAP_RESAMPLES)
        .map(|_| (0..n).map(|_| values[rng.below(n)]).sum::<f64>() / n as f64)
        .collect();
    means.sort_by(f64::total_cmp);
    let tail = 0.5 * (1.0 - level);
    let last = (BOOTSTRAP_RESAMPLES - 1) as f64;
    let at = |q: f64| means[(q * last).round() as usize];
    Some((at(tail), at(1.0 - tail)))
}

#[cfg(test)]
mod tests;
