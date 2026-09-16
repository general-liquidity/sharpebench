//! Returns an entrant supplies after the data reveal are not ranked as forward
//! evidence. A next-bar oracle over the revealed window clears every gate when
//! its returns are ranked directly, so the refusal has to come from intake.
//!
//! This file uses only the arena API that predates bound intake (entries are
//! read from JSON, as `arena score` reads them), so it runs unchanged against
//! the scorer that ranked supplied returns, and fails there.

use std::path::{Path, PathBuf};

use sharpebench_arena::{Arena, RevealedEntry};
use sharpebench_attest::{content_digest, make_commitment};
use sharpebench_core::{rank, AgentSubmission, ScoreConfig};
use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{Agent, CostModel, Dataset, Window};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-hindsight-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A revealed market with daily moves of a few percent, where a next-bar
/// oracle clears the engine's impact costs.
fn market() -> Dataset {
    Dataset::synthetic_parameterized(4, 160, 20_260_916, 3.0, 0.0, 0.0)
}

fn csv(data: &Dataset) -> String {
    let mut out = String::from("date,symbol,close\n");
    for (symbol, closes) in &data.closes {
        for (date, close) in data.dates.iter().zip(closes) {
            out.push_str(&format!("{date},{symbol},{close}\n"));
        }
    }
    out
}

/// Holds the revealed data and puts half its book into the symbol whose next
/// close rises most.
struct Oracle {
    data: Dataset,
}

impl Agent for Oracle {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let now = self.data.dates.iter().position(|d| *d == observation.date);
        let best = now.and_then(|t| {
            self.data
                .closes
                .iter()
                .filter_map(|(symbol, closes)| {
                    let gain = closes.get(t + 1)? / closes[t] - 1.0;
                    (gain > 0.0).then_some((symbol, gain))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(symbol, _)| symbol.clone())
        });
        Decision {
            orders: observation
                .symbols
                .iter()
                .map(|s| {
                    let long = best.as_deref() == Some(s.symbol.as_str());
                    Order {
                        symbol: s.symbol.clone(),
                        action: if long { Action::Buy } else { Action::Close },
                        target_weight: if long { 0.5 } else { 0.0 },
                        confidence: 0.5,
                        rationale: String::new(),
                    }
                })
                .collect(),
            reasoning: String::new(),
            cost: None,
        }
    }
}

/// What the oracle earns over the revealed window, as an entrant holding the
/// data would compute and deliver it.
fn hindsight_returns(data: &Dataset) -> AgentSubmission {
    let windows = [
        Window { start: 16, end: 88 },
        Window {
            start: 88,
            end: 160,
        },
    ];
    let (mut submission, _) = sharpebench_harness::run_agent_capture(
        "oracle",
        data,
        &windows,
        &[0],
        CostModel::default(),
        || Box::new(Oracle { data: data.clone() }),
    );
    submission.agent_id = "oracle".to_string();
    submission
}

fn window_json(dir: &Path) -> serde_json::Value {
    let path = dir.join("windows").join("w1").join("window.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn supplied_hindsight_returns_are_not_ranked_as_forward_evidence() {
    let data = market();
    let submission = hindsight_returns(&data);

    // The statistics alone admit the oracle: this is why intake has to refuse it.
    let direct = rank(std::slice::from_ref(&submission), &ScoreConfig::default());
    assert!(direct[0].rank_eligible, "{:?}", direct[0]);
    assert!(direct[0].deflated_sharpe >= 0.95, "{:?}", direct[0]);

    let dir = temp_dir("supplied");
    let mut arena = Arena::init(&dir).unwrap();
    arena
        .open_window("w1", 10, 20, ScoreConfig::default())
        .unwrap();
    // A valid commitment made before the deadline, to any artifact at all.
    let artifact = content_digest(b"an artifact committed before the reveal");
    arena
        .register_entry("w1", make_commitment("oracle", "w1", &artifact, "salt-o"))
        .unwrap();
    arena.advance(20).unwrap();
    let dataset = dir.join("dataset.csv");
    std::fs::write(&dataset, csv(&data)).unwrap();

    let entries: Vec<RevealedEntry> = serde_json::from_value(serde_json::json!([{
        "submission": submission,
        "artifact_digest": artifact,
        "salt": "salt-o",
    }]))
    .unwrap();
    let scores = arena.reveal_and_score("w1", &dataset, &entries).unwrap();
    assert!(
        scores.iter().all(|s| s.agent_id != "oracle"),
        "supplied hindsight returns were ranked: {scores:?}"
    );

    let window = window_json(&dir);
    let refusals = window["refusals"].as_array().unwrap();
    assert_eq!(refusals.len(), 1, "{window}");
    assert_eq!(refusals[0]["agent_id"], "oracle");
    assert!(
        refusals[0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("supplied returns are not ranked"),
        "{window}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
