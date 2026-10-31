//! The integrity legs: planted violations and negative controls, measured as
//! detection rates. A power curve has no meaning here, and a failure here is a
//! different finding from a power shortfall, so these rows never mix with the
//! statistical ones.
//!
//! `detected` means the shipped path refused the control. Where the statistics
//! are expected to admit it and a separate mechanism is the defence (the
//! look-ahead oracle and the forged capture), `statistics_admitted` records
//! what the scorer alone would have done, so the reader can see which layer did
//! the work.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use sharpebench_core::composite::{rank, Run, ScoreConfig};
use sharpebench_core::process::{ProcessEvent, Trace};
use sharpebench_core::AgentSubmission;
use sharpebench_harness::{
    luck_floor, run_agent, run_agent_capture, verify_trajectory_reexecuted, ReexecutionError,
};
use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{Agent, BuyAndHold, CostModel, CostProfile, Dataset, Momentum, Window};

use crate::campaign::{Campaign, ControlKind, Dgp, Garch, IntegrityBlock, Rule, Tail};
use crate::dgp::{zero_skill_path, Stream};
use crate::fields::{agent_id, FOCAL};

/// One control on one replicate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlRow {
    pub kind: ControlKind,
    pub rep: u64,
    /// The shipped path refused it.
    pub detected: bool,
    /// The scorer alone, with no re-execution, admitted it.
    pub statistics_admitted: bool,
    /// A twin without the violation behaved as the control needs: admitted
    /// where the control is a high-Sharpe track, so a refusal is the
    /// violation's doing and not a track that would have failed anyway.
    pub twin_ok: bool,
    /// Control-specific number, named per kind in the protocol document.
    pub measure: f64,
}

/// Peers in a tape-level control field.
const PEERS: usize = 7;
/// Symbols in an engine tape.
const SYMBOLS: usize = 4;
/// Luck-floor monkeys in an engine field.
const MONKEYS: usize = 3;
/// Annualized Sharpe of the planted high-skill track behind the process and
/// mandate controls: far past every bar, so the violation is what refuses it.
const OBVIOUS_SKILL: f64 = 8.0;
/// Annualized market Sharpe on a premium tape, the top of the 0.69 to 0.87 the
/// frozen US index panels realize.
const PREMIUM_SHARPE: f64 = 0.87;
/// Per-bar volatility of an engine tape, large enough for an oracle to clear
/// costs. The controls read eligibility, and Sharpe does not depend on this.
const ENGINE_VOL: f64 = 0.02;

fn normal_dgp() -> Dgp {
    Dgp {
        id: "control-normal".into(),
        rho: 0.0,
        tail: Tail::Normal,
        garch: None,
        regime: None,
        shared_variance: 0.0,
    }
}

fn clustered_dgp() -> Dgp {
    Dgp {
        id: "control-garch".into(),
        rho: 0.0,
        tail: Tail::Normal,
        garch: Some(Garch {
            alpha: 0.08,
            beta: 0.9,
        }),
        regime: None,
        shared_variance: 0.0,
    }
}

fn window_major(track: &[f64], windows: usize, seeds: usize, vol: f64) -> Vec<Run> {
    let len = track.len() / windows;
    (0..windows)
        .flat_map(|w| (0..seeds).map(move |_| w))
        .map(|w| Run {
            returns: track[w * len..(w + 1) * len]
                .iter()
                .map(|x| vol * x)
                .collect(),
            ..Run::default()
        })
        .collect()
}

fn entrant(id: String, runs: Vec<Run>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id,
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

/// `PEERS` zero-skill peers on independent normal paths.
fn peers(stream: &mut Stream, block: &IntegrityBlock, vol: f64) -> Vec<AgentSubmission> {
    let dgp = normal_dgp();
    (1..=PEERS)
        .map(|e| {
            let path = zero_skill_path(&dgp, stream, block.history_bars);
            entrant(
                agent_id(e),
                window_major(&path, block.windows, block.execution_seeds, vol),
            )
        })
        .collect()
}

fn planted_track(stream: &mut Stream, bars: usize, sharpe_annual: f64, ppy: f64) -> Vec<f64> {
    let dgp = normal_dgp();
    let shift = sharpe_annual / ppy.sqrt();
    zero_skill_path(&dgp, stream, bars)
        .into_iter()
        .map(|x| x + shift)
        .collect()
}

fn focal_eligible(board: &[sharpebench_core::composite::CompositeScore]) -> bool {
    board.iter().any(|s| s.agent_id == FOCAL && s.rank_eligible)
}

/// A synthetic multi-symbol tape: a common factor at the stated Sharpe plus
/// independent noise, as closes, so the engine and its costs run on a tape with
/// a known population rather than on one realized history.
fn engine_tape(stream: &mut Stream, bars: usize, sharpe_annual: f64, ppy: f64) -> Dataset {
    let dgp = normal_dgp();
    let market = zero_skill_path(&dgp, stream, bars);
    let drift = sharpe_annual / ppy.sqrt();
    let mut closes = BTreeMap::new();
    for s in 0..SYMBOLS {
        let own = zero_skill_path(&dgp, stream, bars);
        let mut price = 100.0;
        let series = (0..bars)
            .map(|t| {
                let shock = (0.5_f64).sqrt() * market[t] + (0.5_f64).sqrt() * own[t];
                let ret = ENGINE_VOL * (drift + shock);
                price *= (1.0 + ret).max(1e-6);
                price
            })
            .collect();
        closes.insert(format!("SYM{s:02}"), series);
    }
    Dataset {
        dates: (0..bars).map(|d| format!("S{d:06}")).collect(),
        closes,
        dividends: BTreeMap::new(),
        marks: None,
    }
}

fn engine_windows(bars: usize, windows: usize) -> Vec<Window> {
    let len = bars / windows;
    (0..windows)
        .map(|w| Window {
            start: w * len,
            end: (w + 1) * len,
        })
        .collect()
}

/// Buys the symbol that rises most on the next bar. It reads the future from
/// the tape it was constructed with, which no point-in-time observation
/// carries. Rule modelled on `forward_hindsight_oracle_case`.
struct HindsightOracle {
    data: Dataset,
    next: usize,
}

impl Agent for HindsightOracle {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let t = self.next;
        self.next += 1;
        let best = self
            .data
            .closes
            .iter()
            .filter_map(|(symbol, closes)| {
                let gain = closes.get(t + 1)? / closes[t] - 1.0;
                (gain > 0.0).then_some((symbol.clone(), gain))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(symbol, _)| symbol);
        let orders = observation
            .symbols
            .iter()
            .map(|snapshot| {
                let long = best.as_deref() == Some(snapshot.symbol.as_str());
                Order {
                    symbol: snapshot.symbol.clone(),
                    action: if long { Action::Buy } else { Action::Close },
                    target_weight: if long { 0.5 } else { 0.0 },
                    confidence: None,
                    rationale: String::new(),
                }
            })
            .collect();
        Decision {
            orders,
            reasoning: String::new(),
            cost: None,
        }
    }
}

const RUNNER: &str = "joint-gate-control-scorer";

fn seeds_of(block: &IntegrityBlock, rep: u64) -> Vec<u64> {
    (0..block.execution_seeds as u64)
        .map(|s| rep.wrapping_mul(1_000).wrapping_add(s + 1))
        .collect()
}

/// A look-ahead oracle's capture claims the committed image produced it;
/// `forged` instead overwrites one recorded decision of an honest capture. Both
/// meet the same defence, re-executing the committed image.
fn reexecution_control(
    block: &IntegrityBlock,
    rule: &Rule,
    campaign: &Campaign,
    stream: &mut Stream,
    rep: u64,
    forged: bool,
) -> ControlRow {
    let ppy = campaign.periods_per_year;
    let data = engine_tape(stream, block.history_bars, 0.0, ppy);
    let windows = engine_windows(block.history_bars, block.windows);
    let seeds = seeds_of(block, rep);
    let costs: CostModel = CostProfile::Typical.resolve().costs;
    let cfg = rule.score_config(ppy, block.execution_seeds, 0.1);
    let (submission, mut trajectory) = if forged {
        run_agent_capture(FOCAL, &data, &windows, &seeds, costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        })
    } else {
        let mut window = 0;
        let starts: Vec<usize> = windows
            .iter()
            .flat_map(|w| seeds.iter().map(move |_| w.start))
            .collect();
        run_agent_capture(FOCAL, &data, &windows, &seeds, costs, || {
            let next = starts[window];
            window += 1;
            Box::new(HindsightOracle {
                data: data.clone(),
                next,
            }) as Box<dyn Agent>
        })
    };
    if let Some(contract) = &mut trajectory.contract {
        contract.runner_artifact_sha256 = Some(RUNNER.to_string());
    }
    if forged {
        let run = stream.below(trajectory.runs.len());
        let step = stream.below(trajectory.runs[run].steps.len());
        let decision = &mut trajectory.runs[run].steps[step].decision;
        decision.orders = vec![Order {
            symbol: "SYM00".to_string(),
            action: Action::Buy,
            target_weight: 0.987_654_321,
            confidence: None,
            rationale: String::new(),
        }];
    }
    let board = rank(std::slice::from_ref(&submission), &cfg);
    let reexecute = |trajectory: &sharpebench_protocol::AgentTrajectory| {
        verify_trajectory_reexecuted(&data, trajectory, costs, &cfg, Some(RUNNER), || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        })
    };
    // Only a divergence is a detection. Any other refusal means the capture never
    // reached the comparison, and the honest twin must clear the same path, or a
    // verifier refusing everything would read as a perfect detection rate.
    let verdict = reexecute(&trajectory);
    let (_, mut honest) = run_agent_capture(FOCAL, &data, &windows, &seeds, costs, || {
        Box::new(Momentum::default()) as Box<dyn Agent>
    });
    if let Some(contract) = &mut honest.contract {
        contract.runner_artifact_sha256 = Some(RUNNER.to_string());
    }
    ControlRow {
        kind: block.kind,
        rep,
        detected: matches!(verdict, Err(ReexecutionError::Diverged(_))),
        statistics_admitted: focal_eligible(&board),
        twin_ok: reexecute(&honest).is_ok(),
        measure: board[0].deflated_sharpe,
    }
}

/// A track far past every bar, with the violation planted on one run.
fn planted_violation(
    block: &IntegrityBlock,
    rule: &Rule,
    campaign: &Campaign,
    stream: &mut Stream,
    rep: u64,
) -> ControlRow {
    let ppy = campaign.periods_per_year;
    let vol = campaign.vol_per_period;
    let cfg_base = rule.score_config(ppy, block.execution_seeds, 0.1);
    let mut field = peers(stream, block, vol);
    let track = planted_track(stream, block.history_bars, OBVIOUS_SKILL, ppy);
    let clean = entrant(
        FOCAL.to_string(),
        window_major(&track, block.windows, block.execution_seeds, vol),
    );
    let mut violating = clean.clone();
    let run = stream.below(violating.runs.len());
    let (detected, twin_ok) = match block.kind {
        ControlKind::ProcessViolation => {
            violating.runs[run].trace = Trace {
                events: vec![ProcessEvent::DenylistBypass],
            };
            field.push(clean);
            let twin = focal_eligible(&rank(&field, &cfg_base));
            field.pop();
            field.push(violating);
            let board = rank(&field, &cfg_base);
            let focal = board.iter().find(|s| s.agent_id == FOCAL);
            (
                focal.is_some_and(|s| !s.rank_eligible && !s.process_ok),
                twin,
            )
        }
        _ => {
            // A drawdown inside one run, sized off the track's own volatility,
            // past a per-run cap the clean track respects.
            let cap = 0.5;
            let cfg = ScoreConfig {
                mandate: sharpebench_core::composite::Mandate {
                    max_run_drawdown: cap,
                    ..sharpebench_core::composite::Mandate::default()
                },
                ..cfg_base.clone()
            };
            let mid = violating.runs[run].returns.len() / 2;
            violating.runs[run].returns[mid] = -0.75;
            field.push(clean);
            let twin = focal_eligible(&rank(&field, &cfg));
            field.pop();
            field.push(violating);
            let board = rank(&field, &cfg);
            let focal = board.iter().find(|s| s.agent_id == FOCAL);
            (
                focal.is_some_and(|s| !s.rank_eligible && !s.mandate_ok),
                twin,
            )
        }
    };
    ControlRow {
        kind: block.kind,
        rep,
        detected,
        statistics_admitted: false,
        twin_ok,
        measure: 0.0,
    }
}

/// A griefer whose every window is constant at its own level. It must be
/// refused and must not move anybody else's bar by a bit.
fn griefer(
    block: &IntegrityBlock,
    rule: &Rule,
    campaign: &Campaign,
    stream: &mut Stream,
    rep: u64,
) -> ControlRow {
    let ppy = campaign.periods_per_year;
    let vol = campaign.vol_per_period;
    let cfg = rule.score_config(ppy, block.execution_seeds, 0.1);
    let mut field = peers(stream, block, vol);
    let track = planted_track(stream, block.history_bars, 3.0, ppy);
    field.push(entrant(
        FOCAL.to_string(),
        window_major(&track, block.windows, block.execution_seeds, vol),
    ));
    let without = rank(&field, &cfg);
    let bar_of = |board: &[sharpebench_core::composite::CompositeScore]| {
        board
            .iter()
            .find(|s| s.agent_id == FOCAL)
            .map(|s| (s.trials_sr_std, s.deflated_sharpe))
    };
    let len = block.history_bars / block.windows;
    let runs: Vec<Run> = (0..block.windows)
        .flat_map(|w| (0..block.execution_seeds).map(move |_| w))
        .map(|w| Run {
            returns: vec![vol * (w as f64 + 1.0) * 0.5; len],
            ..Run::default()
        })
        .collect();
    field.push(entrant("griefer".to_string(), runs));
    let with = rank(&field, &cfg);
    let refused = with
        .iter()
        .find(|s| s.agent_id == "griefer")
        .is_some_and(|s| !s.rank_eligible);
    let same = match (bar_of(&without), bar_of(&with)) {
        (Some(a), Some(b)) => a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits(),
        _ => false,
    };
    let unchanged_n = rule.host_n as usize >= field.len();
    ControlRow {
        kind: block.kind,
        rep,
        detected: refused && (same || !unchanged_n),
        statistics_admitted: !refused,
        twin_ok: unchanged_n,
        measure: 0.0,
    }
}

/// SR004: leverage bounded to [0.5, 1.5], set from trailing volatility, on a
/// passive premium path. No information about the next return is used, so any
/// admission is a manipulation of the ratio and not skill.
fn dynamic_leverage(
    block: &IntegrityBlock,
    rule: &Rule,
    campaign: &Campaign,
    stream: &mut Stream,
    rep: u64,
) -> ControlRow {
    let ppy = campaign.periods_per_year;
    let vol = campaign.vol_per_period;
    let cfg = rule.score_config(ppy, block.execution_seeds, 0.1);
    let path = zero_skill_path(&clustered_dgp(), stream, block.history_bars);
    let drift = PREMIUM_SHARPE / ppy.sqrt();
    let market: Vec<f64> = path.iter().map(|x| x + drift).collect();
    let mut sigma2: f64 = 1.0;
    let levered: Vec<f64> = market
        .iter()
        .map(|&r| {
            let leverage = (1.0 / sigma2.sqrt()).clamp(0.5, 1.5);
            sigma2 = 0.94 * sigma2 + 0.06 * r * r;
            leverage * r
        })
        .collect();
    let sharpe = |x: &[f64]| {
        let n = x.len() as f64;
        let m = x.iter().sum::<f64>() / n;
        let v = x.iter().map(|a| (a - m).powi(2)).sum::<f64>() / (n - 1.0);
        m / v.sqrt()
    };
    let mut field = peers(stream, block, vol);
    field.push(entrant(
        FOCAL.to_string(),
        window_major(&levered, block.windows, block.execution_seeds, vol),
    ));
    let admitted = focal_eligible(&rank(&field, &cfg));
    ControlRow {
        kind: block.kind,
        rep,
        detected: !admitted,
        statistics_admitted: admitted,
        twin_ok: true,
        measure: (sharpe(&levered) - sharpe(&market)) * ppy.sqrt(),
    }
}

/// Engine-level controls: random allocators that pay costs, and passive beta,
/// through `run_backtest` on a tape of known population.
fn engine_field(
    block: &IntegrityBlock,
    rule: &Rule,
    campaign: &Campaign,
    stream: &mut Stream,
    rep: u64,
) -> ControlRow {
    let ppy = campaign.periods_per_year;
    let cfg = rule.score_config(ppy, block.execution_seeds, 0.1);
    let premium = match block.kind {
        ControlKind::PassiveBeta => PREMIUM_SHARPE,
        _ => 0.0,
    };
    let data = engine_tape(stream, block.history_bars, premium, ppy);
    let windows = engine_windows(block.history_bars, block.windows);
    let seeds = seeds_of(block, rep);
    let costs: CostModel = CostProfile::Typical.resolve().costs;
    let score = |costs: CostModel| {
        let mut field = luck_floor(&data, &windows, &seeds, costs, MONKEYS);
        field.push(run_agent(FOCAL, &data, &windows, &seeds, costs, || {
            Box::new(BuyAndHold) as Box<dyn Agent>
        }));
        rank(&field, &cfg)
    };
    let board = score(costs);
    let (admitted, measure) = match block.kind {
        ControlKind::PassiveBeta => (focal_eligible(&board), 0.0),
        _ => {
            // The frictionless arm: the cost model is what holds a random
            // allocator's mean below zero, so the same field is scored without
            // it and the rate that survives is in `measure`.
            let free = score(CostProfile::None.resolve().costs);
            let any = |b: &[sharpebench_core::composite::CompositeScore]| {
                b.iter().any(|s| s.agent_id != FOCAL && s.rank_eligible)
            };
            (any(&board), if any(&free) { 1.0 } else { 0.0 })
        }
    };
    ControlRow {
        kind: block.kind,
        rep,
        detected: !admitted,
        statistics_admitted: admitted,
        twin_ok: true,
        measure,
    }
}

/// Run one control replicate.
pub fn run_control(
    campaign: &Campaign,
    block: &IntegrityBlock,
    master_seed: u64,
    rep: u64,
) -> ControlRow {
    let rule = campaign
        .rule(&block.rule)
        .expect("validated campaigns name known rules");
    let mut stream = Stream::keyed(&[master_seed, 0xC0, block.kind as u64, rep]);
    match block.kind {
        ControlKind::LookAheadOracle => {
            reexecution_control(block, rule, campaign, &mut stream, rep, false)
        }
        ControlKind::Fabrication => {
            reexecution_control(block, rule, campaign, &mut stream, rep, true)
        }
        ControlKind::ProcessViolation | ControlKind::MandateBreach => {
            planted_violation(block, rule, campaign, &mut stream, rep)
        }
        ControlKind::ConstantWindowGriefer => griefer(block, rule, campaign, &mut stream, rep),
        ControlKind::DynamicLeverage => dynamic_leverage(block, rule, campaign, &mut stream, rep),
        ControlKind::CostLosingRandom | ControlKind::PassiveBeta => {
            engine_field(block, rule, campaign, &mut stream, rep)
        }
    }
}
