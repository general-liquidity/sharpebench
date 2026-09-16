//! The Pareto diagnostic is taken among the agents whose pooled track has a
//! Sharpe ratio.
//!
//! `pareto_optimal` marks the agents no other agent dominates on (return up,
//! drawdown down, turnover down). A track that never trades is all zeros: its
//! drawdown is zero and, with no orders, so is its turnover, so nothing could
//! dominate it and it was always on the front, as the committed synthetic
//! golden showed for `hold`. The same held for every track the kernel refuses
//! as having no Sharpe ratio and that has no drawdown or orders, and each such
//! track also pushed the losing agents it dominated off the front. A refused
//! track is now neither a Pareto candidate nor a dominator.
//!
//! The refusal is read off the row itself: with a reference population
//! configured, `dsr_percentile` is absent exactly when the pooled track has no
//! Sharpe ratio, so the tests check the flag against that statement of the
//! kernel rather than against a list kept here.

use std::path::Path;

use sharpebench_core::{rank, AgentSubmission, CompositeScore, ProcessEvent, Run, ScoreConfig};

fn agent(id: &str, returns: Vec<f64>, orders: usize) -> AgentSubmission {
    let mut run = Run {
        returns,
        ..Run::default()
    };
    run.trace.events = (0..orders)
        .map(|_| ProcessEvent::OrderPlaced {
            risk_gate_passed: true,
        })
        .collect();
    AgentSubmission {
        agent_id: id.into(),
        runs: vec![run],
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn cycle(pattern: &[f64]) -> Vec<f64> {
    (0..60).map(|i| pattern[i % pattern.len()]).collect()
}

/// A dispersed agent that loses on average, with drawdown and 20 orders.
fn loser() -> AgentSubmission {
    agent("loser", cycle(&[0.01, -0.02, 0.005, -0.01]), 20)
}

/// A dispersed agent with a higher return than `loser` but twice its
/// turnover, so neither dominates the other.
fn trader() -> AgentSubmission {
    agent("trader", cycle(&[0.05, -0.04, 0.03, -0.035]), 40)
}

/// `loser` shifted up by 0.001 a bar with the same orders: a higher return and
/// no more drawdown at equal turnover, so it dominates `loser`.
fn better() -> AgentSubmission {
    let mut sub = loser();
    sub.agent_id = "better".into();
    for x in &mut sub.runs[0].returns {
        *x += 0.001;
    }
    sub
}

/// Refused tracks with no orders: all zero (`hold`), constant positive,
/// constant negative, and a non-constant track whose computed standard
/// deviation underflows to zero. Each has a return above `loser`'s, less
/// drawdown and no turnover, so each dominated `loser` before.
fn refused_tracks() -> Vec<AgentSubmission> {
    vec![
        agent("hold", vec![0.0; 60], 0),
        agent("constant 0.001", vec![0.001; 60], 0),
        agent("constant -0.002", vec![-0.002; 60], 0),
        agent(
            "underflowing",
            (0..60).map(|i| (i % 2) as f64 * 1e-170).collect(),
            0,
        ),
    ]
}

/// A reference population makes `dsr_percentile` the row's own statement of
/// whether its pooled track has a Sharpe ratio.
fn config() -> ScoreConfig {
    ScoreConfig {
        reference_dsr_population: vec![0.1, 0.5, 0.9],
        ..ScoreConfig::default()
    }
}

fn row<'a>(board: &'a [CompositeScore], id: &str) -> &'a CompositeScore {
    board
        .iter()
        .find(|r| r.agent_id == id)
        .unwrap_or_else(|| panic!("{id} is on the board"))
}

fn refused(row: &CompositeScore) -> bool {
    row.dsr_percentile.is_none()
}

/// The front recomputed from the published row fields, over the rows the
/// kernel did not refuse.
fn expected_front(board: &[CompositeScore]) -> Vec<bool> {
    let dominates = |a: &CompositeScore, b: &CompositeScore| {
        a.raw_mean_return >= b.raw_mean_return
            && a.max_drawdown <= b.max_drawdown
            && a.turnover <= b.turnover
            && (a.raw_mean_return > b.raw_mean_return
                || a.max_drawdown < b.max_drawdown
                || a.turnover < b.turnover)
    };
    (0..board.len())
        .map(|i| {
            !refused(&board[i])
                && !(0..board.len())
                    .any(|j| j != i && !refused(&board[j]) && dominates(&board[j], &board[i]))
        })
        .collect()
}

fn flags(board: &[CompositeScore]) -> Vec<bool> {
    board.iter().map(|r| r.pareto_optimal).collect()
}

#[test]
fn hold_in_the_committed_synthetic_field_is_not_pareto_optimal() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/synthetic_field.input.json");
    let raw = std::fs::read_to_string(&path).expect("read the committed synthetic input");
    let subs: Vec<AgentSubmission> = serde_json::from_str(&raw).expect("input parses");
    let board = rank(&subs, &ScoreConfig::default());
    let hold = row(&board, "hold");
    assert_eq!(
        (hold.raw_mean_return, hold.max_drawdown, hold.turnover),
        (0.0, 0.0, 0.0)
    );
    assert!(hold.deflation_error.is_some(), "hold has no Sharpe ratio");
    assert!(!hold.pareto_optimal, "a never-trading track is not optimal");
    assert!(row(&board, "buy-and-hold").pareto_optimal);
    assert!(row(&board, "momentum").pareto_optimal);
    // `random` is dominated by buy-and-hold as well as by hold, so it stays off.
    assert!(!row(&board, "random").pareto_optimal);
}

#[test]
fn a_refused_track_is_never_a_candidate_and_never_dominates() {
    let cfg = config();
    for refused_sub in refused_tracks() {
        let id = refused_sub.agent_id.clone();
        let board = rank(&[refused_sub, loser(), trader()], &cfg);
        let r = row(&board, &id);
        assert!(refused(r), "{id} is refused as having no Sharpe ratio");
        assert!(r.deflation_error.is_some(), "{id} says why");
        assert_eq!(r.turnover, 0.0, "{id}");
        assert!(
            r.max_drawdown < row(&board, "loser").max_drawdown,
            "{id} would dominate loser"
        );
        assert!(!r.pareto_optimal, "{id} is not a candidate");
        // `loser` was dominated only by the refused track; `trader` trades
        // twice as much, so nothing left dominates it.
        assert!(row(&board, "loser").pareto_optimal, "{id}");
        assert!(row(&board, "trader").pareto_optimal, "{id}");
        assert_eq!(flags(&board), expected_front(&board), "{id}");
    }
}

#[test]
fn an_agent_dominated_by_a_refused_track_is_optimal_only_if_nothing_else_dominates_it() {
    let cfg = config();
    for refused_sub in refused_tracks() {
        let id = refused_sub.agent_id.clone();
        let without = rank(&[refused_sub.clone(), loser(), trader()], &cfg);
        assert!(row(&without, "loser").pareto_optimal, "{id}");

        let with = rank(&[refused_sub, loser(), better(), trader()], &cfg);
        assert!(!refused(row(&with, "better")));
        assert!(
            !row(&with, "loser").pareto_optimal,
            "better dominates loser"
        );
        assert!(row(&with, "better").pareto_optimal, "{id}");
        assert!(row(&with, "trader").pareto_optimal, "{id}");
        assert!(!row(&with, &id).pareto_optimal, "{id}");
        assert_eq!(flags(&with), expected_front(&with), "{id}");
    }
}

#[test]
fn a_field_of_dispersed_tracks_keeps_the_front_it_had() {
    let cfg = config();
    let board = rank(&[loser(), better(), trader()], &cfg);
    assert!(board.iter().all(|r| !refused(r)));
    assert!(!row(&board, "loser").pareto_optimal);
    assert!(row(&board, "better").pareto_optimal);
    assert!(row(&board, "trader").pareto_optimal);
    assert_eq!(flags(&board), expected_front(&board));
    // A field of refused tracks alone has an empty front.
    let refused_only = rank(&refused_tracks(), &cfg);
    assert!(refused_only.iter().all(refused));
    assert!(refused_only.iter().all(|r| !r.pareto_optimal));
}
