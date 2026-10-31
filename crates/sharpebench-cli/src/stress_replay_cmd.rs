//! `sharpebench run --stress-replay`: replay every admitted row through the
//! declared synthetic stress set and report the result beside its score.
//!
//! This is a **mitigation** for the `crash_unseen_in_sample` tail seller, not a
//! closure. The record is rank-neutral: it is attached after the board is
//! ranked and sealed, and it marks a row without moving its rank, score or
//! eligibility. The rule, its limit and what it cannot catch are documented in
//! [`sharpebench_core::stress_replay`]; the scenarios are
//! [`sharpebench_harness::stress_replay::stress_scenarios`], the same ones
//! `sharpebench stress` runs. The replay is opt-in because it launches an
//! external entrant again, once per stress cell.

use std::collections::BTreeMap;

use sharpebench_core::stress_replay::StressReplay;
use sharpebench_core::CompositeScore;
use sharpebench_harness::stress_replay::{replay_in_process, replay_under_stress};
use sharpebench_sim::{Agent, BuyAndHold, ExternalAgent, HttpAgent, Momentum};

/// The flag that turns the replay on.
pub const FLAG: &str = "--stress-replay";

/// How the external entrant, if any, was driven. Mirrors the precedence of
/// `run`: `--http`, then `--image`, then `--cmd`.
pub enum ExternalTransport {
    Http(String),
    Image,
    Cmd(String, Vec<String>),
}

impl ExternalTransport {
    pub fn from_args(args: &[String]) -> Option<Self> {
        if let Some(addr) = crate::flag_value(args, "--http") {
            return Some(Self::Http(addr.to_string()));
        }
        if crate::flag_value(args, "--image").is_some() {
            return Some(Self::Image);
        }
        let cmd = crate::flag_value(args, "--cmd")?;
        let mut parts = cmd.split_whitespace().map(String::from);
        let prog = parts.next()?;
        Some(Self::Cmd(prog, parts.collect()))
    }
}

/// Replay one row's agent. `external` names the external entrant's row label.
fn replay_row(
    agent_id: &str,
    external: Option<(&str, &ExternalTransport)>,
    luck_floor_agents: usize,
) -> StressReplay {
    if let Some((label, transport)) = external {
        if label == agent_id {
            return replay_external(transport);
        }
    }
    match agent_id {
        "buy-and-hold" => replay_in_process(|_| Box::new(BuyAndHold) as Box<dyn Agent>),
        "momentum" => replay_in_process(|_| Box::new(Momentum::default()) as Box<dyn Agent>),
        _ => match (0..luck_floor_agents)
            .find(|&k| sharpebench_harness::luck_floor_agent_id(k) == agent_id)
        {
            Some(k) => replay_in_process(|seed| sharpebench_harness::luck_floor_agent(k, seed)),
            None => StressReplay::unavailable("no replayable agent stands behind this row"),
        },
    }
}

fn replay_external(transport: &ExternalTransport) -> StressReplay {
    match transport {
        // A second sandboxed launch per stress cell is a separate, audited
        // launch path; it is not taken implicitly.
        ExternalTransport::Image => StressReplay::unavailable(
            "an --image entrant is not replayed: each stress cell would be a new sandboxed launch",
        ),
        ExternalTransport::Http(addr) => replay_under_stress(|scenario, window, seed| {
            let mut agent = HttpAgent::new(addr.clone());
            sharpebench_harness::run_external_backtest(
                &scenario.data,
                &mut agent,
                window,
                seed,
                scenario.costs,
            )
            .map_err(|kind| format!("external agent failed: {kind:?}"))
        }),
        ExternalTransport::Cmd(prog, rest) => replay_under_stress(|scenario, window, seed| {
            let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
            let mut agent =
                ExternalAgent::spawn(prog, &rest).map_err(|_| format!("cannot spawn `{prog}`"))?;
            sharpebench_harness::run_external_backtest(
                &scenario.data,
                &mut agent,
                window,
                seed,
                scenario.costs,
            )
            .map_err(|kind| format!("external agent failed: {kind:?}"))
        }),
    }
}

/// The stress record of every rank-eligible row, keyed by agent id. Rows that
/// are not admitted are not replayed.
pub fn replay_admitted(
    board: &[CompositeScore],
    external: Option<(&str, &ExternalTransport)>,
    luck_floor_agents: usize,
) -> BTreeMap<String, StressReplay> {
    board
        .iter()
        .filter(|row| row.rank_eligible)
        .map(|row| {
            (
                row.agent_id.clone(),
                replay_row(&row.agent_id, external, luck_floor_agents),
            )
        })
        .collect()
}

/// Attach each record to its row of a sealed board JSON array, after the seal.
pub fn attach(board_json: &mut serde_json::Value, replays: &BTreeMap<String, StressReplay>) {
    let Some(rows) = board_json.as_array_mut() else {
        return;
    };
    for row in rows {
        let Some(id) = row["agent_id"].as_str() else {
            continue;
        };
        if let Some(replay) = replays.get(id) {
            row["stress_replay"] =
                serde_json::to_value(replay).expect("a stress replay record serializes");
        }
    }
}

/// The text report, printed after the board.
pub fn print(replays: &BTreeMap<String, StressReplay>) {
    println!(
        "\nstress replay (mitigation, rank-neutral; limit: stress loss > {} x the unlevered market's loss)",
        sharpebench_core::stress_replay::STRESS_LOSS_LIMIT
    );
    if replays.is_empty() {
        println!("  no admitted row to replay");
        return;
    }
    for (id, replay) in replays {
        match replay {
            StressReplay::Replayed {
                scenarios, marked, ..
            } => {
                let detail: Vec<String> = scenarios
                    .iter()
                    .map(|s| {
                        format!(
                            "{} {:.4} vs {:.4}{}",
                            s.scenario,
                            s.agent_loss,
                            s.benchmark_loss,
                            if s.exceeds_limit { " EXCEEDS" } else { "" }
                        )
                    })
                    .collect();
                println!(
                    "  {id:<20} {}  {}",
                    if *marked { "MARKED" } else { "clean " },
                    detail.join("; ")
                );
            }
            StressReplay::Unavailable { reason, .. } => {
                println!("  {id:<20} unavailable: {reason}");
            }
        }
    }
    println!(
        "  a clean replay says only that the agent survived these synthetic scenarios; crashes shaped unlike them are not detected"
    );
}
