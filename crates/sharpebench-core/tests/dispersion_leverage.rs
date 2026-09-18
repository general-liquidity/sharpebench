//! The vote that sets the measured deflation bar is disclosed, and nothing else
//! moves.
//!
//! On a measured field the deflation dispersion is the sample standard
//! deviation of one per-period Sharpe per vote. On the paper's own measured
//! panels that number is usually carried by one honest reference agent: on the
//! hourly crypto panel the five luck-floor agents sit within 0.004 of each other
//! and buy-and-hold sits far above them, so its one vote multiplies the
//! dispersion by 4.89. A cap on the vote was measured and rejected, because a
//! cap strict enough to contain one hostile entrant would have lowered that
//! panel's bar by 74.5%. Every measured row now names the vote with the largest
//! leverage instead, and the bar is left exactly where it was.
//!
//! These tests read the disclosure through the serialized score rather than the
//! Rust type, so they compile against the tree that had no disclosure and fail
//! there on their assertions.

use serde_json::Value;
use sharpebench_core::{rank, score_agent, AgentSubmission, Run, ScoreConfig};
use sharpebench_stats::deflated_sharpe::{
    deflated_sharpe_ratio_against_null, expected_max_sharpe, observed_sharpe_ratio,
};
use sharpebench_stats::stats::std_dev;

const KEY: &str = "trials_sr_std_most_influential_vote";

/// The seven votes the hourly crypto panel measures under the current engine,
/// captured from the evidence producer: five luck-floor agents, momentum and
/// buy-and-hold, in that order of Sharpe.
const HOURLY_CRYPTO_VOTES: [(&str, f64); 7] = [
    ("luck-floor-0", -0.28058),
    ("luck-floor-1", -0.27846),
    ("luck-floor-2", -0.27757),
    ("luck-floor-3", -0.27726),
    ("luck-floor-4", -0.27721),
    ("momentum", -0.22467),
    ("buy-and-hold", 0.00916),
];

/// A two-window track whose pooled per-period Sharpe is `sharpe` to rounding.
/// `a + b z` with `z` of mean exactly zero and unit sample standard deviation
/// over the pooled 120 bars has Sharpe `a / b`. Each agent's `z` has its own
/// frequency and phase, so no two agents are near-clones and every vote
/// survives the collapse, and each window varies inside itself.
fn track(seed: usize, sharpe: f64) -> Vec<Vec<f64>> {
    let k = seed as f64;
    let raw: Vec<f64> = (0..120)
        .map(|t| {
            let t = t as f64;
            (t * (0.37 + 0.113 * k) + 0.9 * k).sin() + 0.5 * (t * (1.61 + 0.07 * k) + k).cos()
        })
        .collect();
    let mean = raw.iter().sum::<f64>() / raw.len() as f64;
    let centred: Vec<f64> = raw.iter().map(|x| x - mean).collect();
    let sd = std_dev(&centred);
    let b = 0.01;
    let z: Vec<f64> = centred.iter().map(|x| sharpe * b + b * x / sd).collect();
    vec![z[..60].to_vec(), z[60..].to_vec()]
}

fn agent(id: &str, windows: Vec<Vec<f64>>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.into(),
        runs: windows
            .into_iter()
            .map(|returns| Run {
                returns,
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn hourly_crypto_field() -> Vec<AgentSubmission> {
    HOURLY_CRYPTO_VOTES
        .iter()
        .enumerate()
        .map(|(i, &(id, sharpe))| agent(id, track(i, sharpe)))
        .collect()
}

fn hourly_cfg() -> ScoreConfig {
    ScoreConfig::for_periods_per_year(8760.0)
}

fn board_json(subs: &[AgentSubmission], cfg: &ScoreConfig) -> Vec<Value> {
    rank(subs, cfg)
        .iter()
        .map(|s| serde_json::to_value(s).expect("a score serializes"))
        .collect()
}

/// The votes as the kernel computes them, sorted as the dispersion sorts them.
fn votes(subs: &[AgentSubmission]) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = subs
        .iter()
        .map(|s| {
            let pooled: Vec<f64> = s.runs.iter().flat_map(|r| r.returns.clone()).collect();
            (
                s.agent_id.clone(),
                observed_sharpe_ratio(&pooled).expect("every track varies"),
            )
        })
        .collect();
    v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    v
}

#[test]
fn the_fixture_reproduces_the_hourly_crypto_votes() {
    for ((id, got), (want_id, want)) in votes(&hourly_crypto_field())
        .into_iter()
        .zip(HOURLY_CRYPTO_VOTES)
    {
        assert_eq!(id, want_id);
        assert!((got - want).abs() < 1e-12, "{id}: {got} against {want}");
    }
}

#[test]
fn an_hourly_crypto_shaped_field_names_the_honest_vote_that_sets_the_bar() {
    let field = hourly_crypto_field();
    let board = board_json(&field, &hourly_cfg());
    assert_eq!(board[0]["trials_sr_std_source"], "measured");

    let sharpes: Vec<f64> = votes(&field).into_iter().map(|(_, s)| s).collect();
    let with = std_dev(&sharpes);
    let without_bh = std_dev(&sharpes[..6]);
    let expected_leverage = with / without_bh;
    assert!(
        (expected_leverage - 4.89).abs() < 0.01,
        "the fixture carries the panel's leverage: {expected_leverage}"
    );

    for row in &board {
        let vote = &row[KEY];
        assert!(
            vote.is_object(),
            "{}: no disclosure on a measured row",
            row["agent_id"]
        );
        assert_eq!(vote["agent_id"], "buy-and-hold", "{row}");
        assert_eq!(vote["agents_in_vote"], 1);
        assert_eq!(vote["votes"], 7);
        assert_eq!(vote["vote_sharpe"].as_f64(), Some(sharpes[6]));
        assert_eq!(
            vote["measured_dispersion_without_it"].as_f64(),
            Some(without_bh)
        );
        assert_eq!(vote["leverage"].as_f64(), Some(expected_leverage));
        // One disclosure for the whole field: every row carries the same one.
        assert_eq!(vote, &board[0][KEY]);
    }
}

/// The disclosure reads the dispersion and never recomputes it: the measured
/// value is still the plain standard deviation of the sorted votes, bit for
/// bit, and every row's bar and deflated Sharpe are what the deflation formula
/// gives at that dispersion, with every vote in it.
#[test]
fn the_disclosure_moves_no_bar() {
    let field = hourly_crypto_field();
    let cfg = hourly_cfg();
    let board = rank(&field, &cfg);
    let sharpes: Vec<f64> = votes(&field).into_iter().map(|(_, s)| s).collect();
    let measured = std_dev(&sharpes);
    let floor = cfg.min_measured_trials_sr_std / cfg.periods_per_year.sqrt();
    assert!(measured > floor, "the fixture measures above the floor");
    for row in &board {
        assert_eq!(
            row.trials_sr_std.to_bits(),
            measured.to_bits(),
            "{}: nothing clipped",
            row.agent_id
        );
    }
    for (row, sub) in board.iter().map(|row| {
        (
            row,
            field.iter().find(|s| s.agent_id == row.agent_id).unwrap(),
        )
    }) {
        let pooled: Vec<f64> = sub.runs.iter().flat_map(|r| r.returns.clone()).collect();
        let dsr =
            deflated_sharpe_ratio_against_null(&pooled, row.effective_n_trials, 0.0, measured)
                .unwrap();
        let bar = expected_max_sharpe(measured, row.effective_n_trials).unwrap();
        assert_eq!(
            row.deflated_sharpe.to_bits(),
            dsr.to_bits(),
            "{}",
            row.agent_id
        );
        assert_eq!(
            row.deflation_bar_per_period.to_bits(),
            bar.to_bits(),
            "{}",
            row.agent_id
        );
    }
}

/// A field whose votes are all equal has a measured dispersion of zero, so no
/// vote raises it and nothing is named. Each track is the same dyadic values
/// in its own order, so every Sharpe is bitwise the same and the tracks are far
/// from clones of each other.
#[test]
fn a_field_of_equal_votes_names_no_vote() {
    // A mean of exactly 1/64 and deviations of j/64: every sum, mean and square
    // is exact, so the order the values come in cannot move a bit.
    let base: Vec<f64> = (1..=30)
        .flat_map(|j| [(1 + j) as f64 / 64.0, (1 - j) as f64 / 64.0])
        .collect();
    let strides = [1, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43];
    let field: Vec<AgentSubmission> = (0..6)
        .map(|k| {
            let windows: Vec<Vec<f64>> = (0..2)
                .map(|w| {
                    let stride = strides[2 * k + w];
                    (0..60).map(|t| base[(t * stride + k) % 60]).collect()
                })
                .collect();
            agent(&format!("equal-{k}"), windows)
        })
        .collect();
    let sharpes: Vec<f64> = votes(&field).into_iter().map(|(_, s)| s).collect();
    assert!(sharpes.iter().all(|s| s.to_bits() == sharpes[0].to_bits()));
    let board = board_json(&field, &hourly_cfg());
    for row in &board {
        assert_eq!(row["trials_sr_std_source"], "measured_floored", "{row}");
        assert!(row.get(KEY).is_none(), "{}", row[KEY]);
    }
}

/// Below the floor the bar is the floor. The disclosure still names the vote
/// with the largest leverage over the measured dispersion, and the source says
/// that removing it would not lower the bar.
#[test]
fn a_floored_board_still_names_its_vote() {
    let field: Vec<AgentSubmission> = HOURLY_CRYPTO_VOTES
        .iter()
        .enumerate()
        .map(|(i, &(id, sharpe))| agent(id, track(i, sharpe / 100.0)))
        .collect();
    let sharpes: Vec<f64> = votes(&field).into_iter().map(|(_, s)| s).collect();
    let cfg = hourly_cfg();
    let floor = cfg.min_measured_trials_sr_std / cfg.periods_per_year.sqrt();
    assert!(std_dev(&sharpes) < floor);
    for row in board_json(&field, &cfg) {
        assert_eq!(row["trials_sr_std_source"], "measured_floored");
        assert!((row["trials_sr_std"].as_f64().unwrap() - floor).abs() < 1e-15);
        assert_eq!(row[KEY]["agent_id"], "buy-and-hold");
        assert!(row[KEY]["leverage"].as_f64().unwrap() > 4.0);
    }
}

/// A field too small to measure, and a lone agent, use the configured prior:
/// no field voted, so there is nothing to disclose and no key is written. The
/// committed goldens are fields of this kind, which is why their bytes cannot
/// move.
#[test]
fn the_configured_path_writes_no_disclosure() {
    let field: Vec<AgentSubmission> = hourly_crypto_field().into_iter().take(4).collect();
    for row in board_json(&field, &hourly_cfg()) {
        assert_eq!(row["trials_sr_std_source"], "configured");
        assert!(row.get(KEY).is_none(), "{row}");
    }
    let lone = serde_json::to_value(score_agent(&hourly_crypto_field()[6], &hourly_cfg())).unwrap();
    assert!(lone.get(KEY).is_none());
}

/// The committed golden score files are fields below the measuring floor, so
/// they carry no disclosure and recompute to their committed bytes.
#[test]
fn the_committed_goldens_stay_byte_identical() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (input, scores) in [
        (
            root.join("golden/synthetic_field.input.json"),
            root.join("golden/synthetic_field.scores.json"),
        ),
        (
            root.join("../../suites/example_submissions.json"),
            root.join("golden/example_submissions.scores.json"),
        ),
    ] {
        let committed = std::fs::read_to_string(&scores).expect("golden scores");
        assert!(!committed.contains(KEY), "{}", scores.display());
        let parsed: Vec<Value> = serde_json::from_str(&committed).unwrap();
        for row in &parsed {
            assert_eq!(
                row["trials_sr_std_source"],
                "configured",
                "{}",
                scores.display()
            );
        }
        let raw = std::fs::read_to_string(&input).expect("golden input");
        let subs: Vec<AgentSubmission> = serde_json::from_str(&raw).expect("input parses");
        let recomputed =
            serde_json::to_string_pretty(&rank(&subs, &ScoreConfig::default())).unwrap() + "\n";
        assert!(
            recomputed == committed,
            "{} no longer recomputes to its committed bytes",
            scores.display()
        );
    }
}

/// Order of submission cannot change which vote is named.
#[test]
fn the_named_vote_does_not_depend_on_submission_order() {
    let mut field = hourly_crypto_field();
    let forward = board_json(&field, &hourly_cfg())[0][KEY].clone();
    field.reverse();
    let reverse = board_json(&field, &hourly_cfg())[0][KEY].clone();
    assert_eq!(forward, reverse);
}
