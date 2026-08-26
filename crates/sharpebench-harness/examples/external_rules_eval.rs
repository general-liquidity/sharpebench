//! Externally specified rule-based strategies, scored through the shipped gates.
//!
//! Every entrant reported in the paper so far was written by the author, so a
//! reader cannot tell a strict gate from a weak field. This example scores four
//! trading rules whose specifications come from the published literature rather
//! than from this repository, beside the existing reference field, on all nine
//! frozen datasets, and does it under each of the three named cost profiles so
//! the cost calibration is a reported axis rather than an unexamined default.
//!
//! The four rules, each implemented from its published statement and not tuned
//! here:
//!
//! 1. `donchian-20-10` — Donchian channel breakout. Enter long when the latest
//!    close exceeds the maximum of the prior 20 closes; exit to cash when it
//!    falls below the minimum of the prior 10. The 20/10 parameterization is
//!    Turtle System 1.
//! 2. `bll-vma-1-50` — the variable-moving-average rule of Brock, Lakonishok and
//!    LeBaron (1992): long when the close is above its trailing 50-bar mean,
//!    flat otherwise, with no band.
//! 3. `faber-10m` — Faber (2007) tactical asset allocation: long when the close
//!    is above its trailing ten-month simple moving average, cash otherwise.
//!    Ten months is converted to the dataset's own bar count.
//! 4. `rsi-14-wilder` — Wilder (1978) RSI(14) with the 30/70 thresholds: enter
//!    long when RSI falls below 30, exit when it rises above 70, hold between.
//!
//! Each rule allocates an equal `1 / n_symbols` weight to every symbol it is
//! currently long, so gross exposure never exceeds one and the rules are
//! comparable with the long-only reference agents.
//!
//! Every agent starts each run with the 20 trailing closes the protocol's
//! observation carries and appends one close per bar, so a rule needing `L`
//! closes produces no signal for the first `L - 20` bars of every window. That
//! quantity is recorded per row as `signal_blind_bars`, because the walk-forward
//! window geometry, not the rule, is what bounds the longest usable lookback.
//!
//! Deterministic: no clock, no ambient RNG. Run with
//!
//!   cargo run --release -p sharpebench-harness --example external_rules_eval -- <out.jsonl> [dataset]

use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use serde::Serialize;
use sharpebench_core::composite::{rank, ScoreConfig, TrialsSrStdSource};
use sharpebench_core::AgentSubmission;
use sharpebench_harness::{luck_floor, run_agent};
use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::agent::RiskManaged;
use sharpebench_sim::{
    tag_regime, walk_forward, Agent, BuyAndHold, CostModel, CostProfile, Dataset, HoldAgent,
    Momentum, Window,
};

/// Frozen datasets, bar size, periods per year, and the number of bars nearest to
/// ten months at that bar size, which is what Faber's rule is stated in.
const DATASETS: &[(&str, &str, f64, usize)] = &[
    ("us-indices-1d", "1d", 252.0, 210),
    ("us-indices-1w", "1w", 52.0, 43),
    ("crypto-majors-1h", "1h", 8760.0, 7300),
    ("crypto-majors-4h", "4h", 2190.0, 1825),
    ("crypto-majors-1d", "1d", 365.0, 304),
    ("crypto-majors-1w", "1w", 52.0, 43),
    ("fx-majors-1d", "1d", 252.0, 210),
    ("commodities-1d", "1d", 252.0, 210),
    ("rates-1d", "1d", 252.0, 210),
];

const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const LUCK_FLOOR_AGENTS: usize = 5;
/// Closes the protocol's observation carries, so the history every rule starts with.
const OBSERVATION_LOOKBACK: usize = 20;

// ---------------------------------------------------------------------------
// Shared per-symbol history accumulator.
// ---------------------------------------------------------------------------

/// Per-symbol close history, seeded from the observation's trailing window on the
/// first bar and extended by one close per bar afterwards. Point-in-time by
/// construction: it only ever holds closes the observation already delivered.
#[derive(Default)]
struct History {
    per_symbol: BTreeMap<String, Vec<f64>>,
}

impl History {
    fn update(&mut self, obs: &MarketObservation) {
        for s in &obs.symbols {
            let entry = self.per_symbol.entry(s.symbol.clone()).or_default();
            if entry.is_empty() {
                entry.extend_from_slice(&s.close_history);
            } else if let Some(&last) = s.close_history.last() {
                entry.push(last);
            }
        }
    }

    fn get(&self, symbol: &str) -> &[f64] {
        self.per_symbol
            .get(symbol)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Build the decision from the set of symbols the rule currently wants to be long.
fn equal_weight_decision(obs: &MarketObservation, long: &[bool], reasoning: &str) -> Decision {
    let n = obs.symbols.len().max(1) as f64;
    let w = 1.0 / n;
    let orders = obs
        .symbols
        .iter()
        .zip(long)
        .map(|(s, &is_long)| Order {
            symbol: s.symbol.clone(),
            action: if is_long { Action::Buy } else { Action::Close },
            target_weight: if is_long { w } else { 0.0 },
            confidence: 0.5,
            rationale: reasoning.to_string(),
        })
        .collect();
    Decision {
        orders,
        reasoning: reasoning.to_string(),
        cost: None,
    }
}

// ---------------------------------------------------------------------------
// Rule 1: Donchian channel breakout, 20-bar entry and 10-bar exit.
// ---------------------------------------------------------------------------

struct Donchian {
    entry: usize,
    exit: usize,
    history: History,
    long: BTreeMap<String, bool>,
}

impl Donchian {
    fn new(entry: usize, exit: usize) -> Self {
        Self {
            entry,
            exit,
            history: History::default(),
            long: BTreeMap::new(),
        }
    }
}

impl Agent for Donchian {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.history.update(obs);
        let mut long_flags = Vec::with_capacity(obs.symbols.len());
        for s in &obs.symbols {
            let h = self.history.get(&s.symbol);
            let l = h.len();
            let held = *self.long.get(&s.symbol).unwrap_or(&false);
            let latest = h.last().copied().unwrap_or(0.0);
            let next = if !held && l > self.entry {
                let prior = &h[l - 1 - self.entry..l - 1];
                latest > prior.iter().copied().fold(f64::MIN, f64::max)
            } else if held && l > self.exit {
                let prior = &h[l - 1 - self.exit..l - 1];
                latest >= prior.iter().copied().fold(f64::MAX, f64::min)
            } else {
                held
            };
            self.long.insert(s.symbol.clone(), next);
            long_flags.push(next);
        }
        equal_weight_decision(obs, &long_flags, "donchian channel breakout")
    }
}

// ---------------------------------------------------------------------------
// Rule 2 and 3: trailing simple-moving-average filters at two published lengths.
// ---------------------------------------------------------------------------

struct SmaFilter {
    lookback: usize,
    history: History,
    label: &'static str,
}

impl SmaFilter {
    fn new(lookback: usize, label: &'static str) -> Self {
        Self {
            lookback,
            history: History::default(),
            label,
        }
    }
}

impl Agent for SmaFilter {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.history.update(obs);
        let mut long_flags = Vec::with_capacity(obs.symbols.len());
        for s in &obs.symbols {
            let h = self.history.get(&s.symbol);
            let l = h.len();
            let is_long = if l >= self.lookback {
                let tail = &h[l - self.lookback..];
                let mean = tail.iter().sum::<f64>() / self.lookback as f64;
                h[l - 1] > mean
            } else {
                false
            };
            long_flags.push(is_long);
        }
        equal_weight_decision(obs, &long_flags, self.label)
    }
}

// ---------------------------------------------------------------------------
// Rule 4: Wilder RSI(14) with 30/70 thresholds.
// ---------------------------------------------------------------------------

struct RsiWilder {
    period: usize,
    history: History,
    long: BTreeMap<String, bool>,
}

impl RsiWilder {
    fn new(period: usize) -> Self {
        Self {
            period,
            history: History::default(),
            long: BTreeMap::new(),
        }
    }

    /// Wilder's RSI over the whole accumulated series: a simple average of the
    /// first `period` gains and losses, then Wilder smoothing thereafter.
    fn rsi(&self, h: &[f64]) -> Option<f64> {
        if h.len() < self.period + 1 {
            return None;
        }
        let p = self.period as f64;
        let mut gain = 0.0;
        let mut loss = 0.0;
        for i in 1..=self.period {
            let d = h[i] - h[i - 1];
            if d > 0.0 {
                gain += d;
            } else {
                loss -= d;
            }
        }
        gain /= p;
        loss /= p;
        for i in self.period + 1..h.len() {
            let d = h[i] - h[i - 1];
            let (g, l) = if d > 0.0 { (d, 0.0) } else { (0.0, -d) };
            gain = (gain * (p - 1.0) + g) / p;
            loss = (loss * (p - 1.0) + l) / p;
        }
        if loss <= 0.0 {
            return Some(100.0);
        }
        let rs = gain / loss;
        Some(100.0 - 100.0 / (1.0 + rs))
    }
}

impl Agent for RsiWilder {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.history.update(obs);
        let mut long_flags = Vec::with_capacity(obs.symbols.len());
        for s in &obs.symbols {
            let h = self.history.get(&s.symbol);
            let held = *self.long.get(&s.symbol).unwrap_or(&false);
            let next = match self.rsi(h) {
                Some(v) if v < 30.0 => true,
                Some(v) if v > 70.0 => false,
                _ => held,
            };
            self.long.insert(s.symbol.clone(), next);
            long_flags.push(next);
        }
        equal_weight_decision(obs, &long_flags, "wilder rsi(14) 30/70")
    }
}

// ---------------------------------------------------------------------------
// Runner.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct Record<'a> {
    command: &'static str,
    dataset: &'a str,
    timeframe: &'a str,
    periods_per_year: f64,
    n_bars: usize,
    n_symbols: usize,
    n_windows: usize,
    window_len: usize,
    n_seeds: usize,
    regimes: Vec<String>,
    cost_profile: &'static str,
    field_size: usize,
    agent_id: String,
    agent_origin: &'static str,
    /// Published source of the rule, for the externally specified entrants.
    rule_source: &'static str,
    /// Closes the rule needs before it can emit a signal.
    required_history: usize,
    /// Bars of every window in which the rule cannot form its signal, given that
    /// each run starts with the observation's 20 trailing closes.
    signal_blind_bars: usize,
    effective_n_trials: u32,
    trials_sr_std_used: f64,
    trials_sr_std_annualized_equivalent: f64,
    trials_sr_std_source: String,
    deflation_bar_per_period: f64,
    deflation_bar_annualized_equivalent: f64,
    deflated_sharpe: f64,
    psr: f64,
    passed_k: bool,
    process_ok: bool,
    bootstrap_p: f64,
    raw_mean_return: f64,
    worst_run_drawdown: f64,
    rank_eligible: bool,
}

type AgentFactory = Box<dyn Fn() -> Box<dyn Agent>>;

/// `(id, origin, published source, required closes, factory)`.
fn entrants(
    faber_bars: usize,
) -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    usize,
    AgentFactory,
)> {
    vec![
        (
            "buy-and-hold",
            "reference",
            "this repository",
            1,
            Box::new(|| Box::new(BuyAndHold) as Box<dyn Agent>) as AgentFactory,
        ),
        (
            "momentum",
            "reference",
            "this repository",
            2,
            Box::new(|| Box::new(Momentum::default()) as Box<dyn Agent>),
        ),
        (
            "hold",
            "reference",
            "this repository",
            1,
            Box::new(|| Box::new(HoldAgent) as Box<dyn Agent>),
        ),
        (
            "risk-managed",
            "reference",
            "this repository",
            16,
            Box::new(|| Box::new(RiskManaged::default()) as Box<dyn Agent>),
        ),
        (
            "donchian-20-10",
            "external",
            "Donchian channel breakout, Turtle System 1 parameters",
            21,
            Box::new(|| Box::new(Donchian::new(20, 10)) as Box<dyn Agent>),
        ),
        (
            "bll-vma-1-50",
            "external",
            "Brock, Lakonishok and LeBaron (1992), variable moving average (1, 50)",
            50,
            Box::new(|| {
                Box::new(SmaFilter::new(50, "bll vma(1,50) trend filter")) as Box<dyn Agent>
            }),
        ),
        (
            "faber-10m",
            "external",
            "Faber (2007), ten-month simple moving average timing rule",
            faber_bars,
            Box::new(move || {
                Box::new(SmaFilter::new(faber_bars, "faber 10-month sma filter")) as Box<dyn Agent>
            }),
        ),
        (
            "rsi-14-wilder",
            "external",
            "Wilder (1978), RSI(14) with 30/70 thresholds",
            15,
            Box::new(|| Box::new(RsiWilder::new(14)) as Box<dyn Agent>),
        ),
    ]
}

fn windows_for(n: usize) -> (Vec<Window>, usize) {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    (walk_forward(n, warmup, test, test), test)
}

fn main() {
    // Required positional; see evidence_sweep for why there is no default.
    let out = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: external_rules_eval <out.jsonl> [dataset]");
        std::process::exit(2);
    });
    let only = env::args().nth(2);
    let mut w = BufWriter::new(File::create(&out).expect("create output"));
    let mut n_records = 0usize;

    for (name, tf, ppy, faber_bars) in DATASETS {
        if let Some(ref o) = only {
            if o != name {
                continue;
            }
        }
        let path = format!("data/{name}.csv");
        let data = match Dataset::from_csv_file(&path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip {name}: {e}");
                continue;
            }
        };
        let n = data.len();
        let (windows, window_len) = windows_for(n);
        let regimes: Vec<String> = windows
            .iter()
            .map(|win| format!("{:?}", tag_regime(&data, *win)))
            .collect();
        eprintln!(
            "{name}: {n} bars, {} symbols, {} windows of {window_len}",
            data.symbols().len(),
            windows.len()
        );

        for profile in [
            CostProfile::None,
            CostProfile::Typical,
            CostProfile::WorstCase,
        ] {
            let profile_name = match profile {
                CostProfile::None => "frictionless",
                CostProfile::Typical => "typical",
                CostProfile::WorstCase => "stressed",
                CostProfile::Realistic => "realistic",
            };
            let costs: CostModel = profile.resolve().costs;

            let specs = entrants(*faber_bars);
            let mut meta: BTreeMap<String, (&'static str, &'static str, usize)> = BTreeMap::new();
            let mut subs: Vec<AgentSubmission> = Vec::new();
            for (id, origin, source, need, make) in specs {
                meta.insert(id.to_string(), (origin, source, need));
                subs.push(run_agent(id, &data, &windows, &EXEC_SEEDS, costs, || {
                    make()
                }));
            }
            subs.extend(luck_floor(
                &data,
                &windows,
                &EXEC_SEEDS,
                costs,
                LUCK_FLOOR_AGENTS,
            ));
            let field_size = subs.len();

            let cfg = ScoreConfig {
                execution_seeds_per_window: EXEC_SEEDS.len(),
                ..ScoreConfig::for_periods_per_year(*ppy)
            };
            for s in rank(&subs, &cfg) {
                let (origin, source, need) =
                    meta.get(&s.agent_id)
                        .copied()
                        .unwrap_or(("luck-floor", "this repository", 1));
                let blind = need.saturating_sub(OBSERVATION_LOOKBACK).min(window_len);
                let rec = Record {
                    command:
                        "cargo run --release -p sharpebench-harness --example external_rules_eval",
                    dataset: name,
                    timeframe: tf,
                    periods_per_year: *ppy,
                    n_bars: n,
                    n_symbols: data.symbols().len(),
                    n_windows: windows.len(),
                    window_len,
                    n_seeds: EXEC_SEEDS.len(),
                    regimes: regimes.clone(),
                    cost_profile: profile_name,
                    field_size,
                    agent_id: s.agent_id.clone(),
                    agent_origin: origin,
                    rule_source: source,
                    required_history: need,
                    signal_blind_bars: blind,
                    effective_n_trials: s.effective_n_trials,
                    trials_sr_std_used: s.trials_sr_std,
                    trials_sr_std_annualized_equivalent: s.trials_sr_std_annualized_equivalent,
                    trials_sr_std_source: match s.trials_sr_std_source {
                        TrialsSrStdSource::Measured => "measured".into(),
                        TrialsSrStdSource::MeasuredFloored => "measured_floored".into(),
                        TrialsSrStdSource::Configured => "configured".into(),
                    },
                    deflation_bar_per_period: s.deflation_bar_per_period,
                    deflation_bar_annualized_equivalent: s.deflation_bar_annualized_equivalent,
                    deflated_sharpe: s.deflated_sharpe,
                    psr: s.psr,
                    passed_k: s.passed_k,
                    process_ok: s.process_ok,
                    bootstrap_p: s.bootstrap_p,
                    raw_mean_return: s.raw_mean_return,
                    worst_run_drawdown: s.worst_run_drawdown,
                    rank_eligible: s.rank_eligible,
                };
                serde_json::to_writer(&mut w, &rec).expect("write record");
                w.write_all(b"\n").expect("newline");
                n_records += 1;
            }
        }
    }
    w.flush().expect("flush");
    eprintln!("wrote {n_records} records to {out}");
}
