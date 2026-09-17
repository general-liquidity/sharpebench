//! The Pareto diagnostic takes its members among the agents whose pooled track
//! has a Sharpe ratio, and its dominators among every agent with an
//! observation.
//!
//! `pareto_optimal` marks the agents no other agent dominates on (return up,
//! drawdown down, turnover down). A track that never trades is all zeros: its
//! drawdown is zero and, with no orders, so is its turnover, so nothing could
//! dominate it and it was always on the front, as the committed synthetic
//! golden showed for `hold`. A track the kernel refuses as having no Sharpe
//! ratio is therefore never a member. It still has a return, a drawdown and a
//! turnover, so it still dominates: an agent that loses money with drawdown and
//! orders is beaten on all three by doing nothing, and is not optimal.
//!
//! "No Sharpe ratio" is the kernel's own statement of it,
//! `observed_sharpe_ratio`: every observation equal, a Sharpe that does not
//! stay finite, or fewer than two observations. A one-observation track was a
//! member before this rule, because the checked PSR keeps a 0.0 convention for
//! a short track instead of refusing it. An empty track has no return to
//! compare and dominates nobody.
//!
//! The tests read the refusal off each row, not off a list kept here: under a
//! valid configuration `deflation_error` is present exactly when the pooled
//! track has no Sharpe ratio, a one-observation track included, and
//! `pooled_observations` says whether the track can dominate.

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
/// constant negative, a non-constant track whose computed standard deviation
/// underflows to zero, and two one-observation tracks. Each has a return above
/// `loser`'s, less drawdown and no turnover, so each dominates `loser`.
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
        agent("one 0.0", vec![0.0], 0),
        agent("one 0.001", vec![0.001], 0),
    ]
}

/// No observation at all: no Sharpe ratio and no return to compare.
fn empty() -> AgentSubmission {
    agent("empty", Vec::new(), 0)
}

fn row<'a>(board: &'a [CompositeScore], id: &str) -> &'a CompositeScore {
    board
        .iter()
        .find(|r| r.agent_id == id)
        .unwrap_or_else(|| panic!("{id} is on the board"))
}

fn refused(row: &CompositeScore) -> bool {
    row.deflation_error.is_some()
}

fn can_dominate(row: &CompositeScore) -> bool {
    row.pooled_observations > 0
}

/// The front recomputed from the published row fields: members are the rows
/// the kernel did not refuse, dominators every row with an observation.
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
                    .any(|j| j != i && can_dominate(&board[j]) && dominates(&board[j], &board[i]))
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
    assert!(refused(hold), "hold has no Sharpe ratio");
    assert!(!hold.pareto_optimal, "a never-trading track is not optimal");
    assert!(row(&board, "buy-and-hold").pareto_optimal);
    assert!(row(&board, "momentum").pareto_optimal);
    // `random` is dominated by buy-and-hold as well as by hold, so it stays
    // off; `hold` dominates no member that nothing else dominates, so the
    // committed front is the same under either reading of a refused track.
    assert!(!row(&board, "random").pareto_optimal);
    assert_eq!(flags(&board), expected_front(&board));
}

#[test]
fn a_refused_track_is_never_a_member_but_still_dominates() {
    let cfg = ScoreConfig::default();
    for refused_sub in refused_tracks() {
        let id = refused_sub.agent_id.clone();
        let board = rank(&[refused_sub, loser(), trader()], &cfg);
        let r = row(&board, &id);
        assert!(refused(r), "{id} is refused as having no Sharpe ratio");
        assert!(can_dominate(r), "{id}");
        assert_eq!(r.turnover, 0.0, "{id}");
        assert!(
            r.raw_mean_return > row(&board, "loser").raw_mean_return
                && r.max_drawdown < row(&board, "loser").max_drawdown,
            "{id} dominates loser"
        );
        assert!(!r.pareto_optimal, "{id} is not a member");
        assert!(
            !row(&board, "loser").pareto_optimal,
            "{id} beats loser on return, drawdown and turnover"
        );
        // `trader` returns more than every refused track, so nothing
        // dominates it.
        assert!(row(&board, "trader").pareto_optimal, "{id}");
        assert_eq!(flags(&board), expected_front(&board), "{id}");
    }
}

#[test]
fn a_one_observation_track_is_refused_by_the_kernel_predicate() {
    // The short tracks carry a deflation error and are not members, where the
    // PSR-based predicate kept them on the front at a PSR of 0.0.
    let cfg = ScoreConfig::default();
    for returns in [vec![0.0], vec![0.001], vec![-0.05]] {
        let board = rank(&[agent("one", returns.clone(), 0), trader()], &cfg);
        let one = row(&board, "one");
        assert_eq!(one.pooled_observations, 1);
        assert!(refused(one), "{returns:?}");
        assert!(!one.pareto_optimal, "{returns:?}");
        assert!(row(&board, "trader").pareto_optimal, "{returns:?}");
    }
}

#[test]
fn an_empty_track_is_neither_a_member_nor_a_dominator() {
    let cfg = ScoreConfig::default();
    let board = rank(&[empty(), loser(), trader()], &cfg);
    let e = row(&board, "empty");
    assert_eq!(e.pooled_observations, 0);
    assert!(refused(e));
    assert!(!e.pareto_optimal);
    // Its reported return, drawdown and turnover are the 0.0 conventions of
    // an empty sample, which would beat `loser` on all three.
    assert_eq!(
        (e.raw_mean_return, e.max_drawdown, e.turnover),
        (0.0, 0.0, 0.0)
    );
    assert!(row(&board, "loser").pareto_optimal);
    assert!(row(&board, "trader").pareto_optimal);
    assert_eq!(flags(&board), expected_front(&board));
}

#[test]
fn a_member_is_optimal_only_if_no_track_with_an_observation_dominates_it() {
    let cfg = ScoreConfig::default();
    for refused_sub in refused_tracks() {
        let id = refused_sub.agent_id.clone();
        let with = rank(&[refused_sub, loser(), better(), trader()], &cfg);
        assert!(!refused(row(&with, "better")));
        assert!(!row(&with, "loser").pareto_optimal, "{id}");
        assert!(row(&with, "trader").pareto_optimal, "{id}");
        assert!(!row(&with, &id).pareto_optimal, "{id}");
        // `better` is optimal exactly when the refused track does not beat it.
        let r = row(&with, &id);
        let b = row(&with, "better");
        let beaten = r.raw_mean_return >= b.raw_mean_return && r.max_drawdown <= b.max_drawdown;
        assert_eq!(b.pareto_optimal, !beaten, "{id}");
        assert_eq!(flags(&with), expected_front(&with), "{id}");
    }
}

#[test]
fn a_field_of_dispersed_tracks_keeps_the_front_it_had() {
    let cfg = ScoreConfig::default();
    let board = rank(&[loser(), better(), trader()], &cfg);
    assert!(board.iter().all(|r| !refused(r)));
    assert!(!row(&board, "loser").pareto_optimal);
    assert!(row(&board, "better").pareto_optimal);
    assert!(row(&board, "trader").pareto_optimal);
    assert_eq!(flags(&board), expected_front(&board));
    // A field of refused tracks alone has an empty front.
    let mut all_refused = refused_tracks();
    all_refused.push(empty());
    let refused_only = rank(&all_refused, &cfg);
    assert!(refused_only.iter().all(refused));
    assert!(refused_only.iter().all(|r| !r.pareto_optimal));
}

#[test]
fn the_front_is_the_front_of_every_observed_track_restricted_to_members() {
    // Every subset of two refused tracks and the three dispersed agents: the
    // flag equals the non-dominated set over all tracks with an observation,
    // with the refused rows then dropped.
    let cfg = ScoreConfig::default();
    let refused = refused_tracks();
    for (a, first) in refused.iter().enumerate() {
        for second in &refused[a + 1..] {
            let field = vec![
                first.clone(),
                second.clone(),
                loser(),
                better(),
                trader(),
                empty(),
            ];
            let board = rank(&field, &cfg);
            let label = format!("{} + {}", first.agent_id, second.agent_id);
            assert_eq!(flags(&board), expected_front(&board), "{label}");
            assert!(row(&board, "trader").pareto_optimal, "{label}");
            assert!(!row(&board, "loser").pareto_optimal, "{label}");
        }
    }
}
