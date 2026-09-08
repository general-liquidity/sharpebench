//! BM2/BS6 regression: a published board must be verified as displayed.
//!
//! `verify_board` recomputes the signature chain and nothing else. It never
//! looks at the `scores` array a reader is actually shown, and it accepts any
//! prefix of the signed chain. So a board could be re-scored, reordered, padded
//! or truncated and still be reported as verified. These cases pin the two
//! bindings that close that: score content/count/order against the signed
//! links, and chain length against a signed terminal receipt.

use sharpebench_core::{rank, AgentSubmission, Run, ScoreConfig};
use sharpebench_leaderboard::{
    publish, publish_self_describing, verify_board, verify_published, verify_self_describing,
    CostProfile, PublishedBoard, RunSpec, SelfDescribingBoard,
};

const KEY: &[u8] = b"host-board-key";

fn sub(id: &str, m: f64) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.to_string(),
        runs: vec![Run {
            returns: (0..30).map(|i| m + 0.0003 * (i as f64).sin()).collect(),
            trace: Default::default(),
            confidences: Vec::new(),
            outcomes: Vec::new(),
            cost: 0.0,
        }],
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn board() -> Vec<sharpebench_core::CompositeScore> {
    rank(
        &[sub("a", 0.002), sub("b", 0.001), sub("c", 0.0)],
        &ScoreConfig::default(),
    )
}

fn spec() -> RunSpec {
    RunSpec {
        dataset_hash: sharpebench_attest::content_digest(b"frozen-dataset-bytes"),
        cost_profile: CostProfile {
            fee_bps: 2.0,
            slippage_bps: 3.0,
            impact_bps: 50.0,
            financing_bps: 5.0,
            max_participation: None,
        },
        score_config: ScoreConfig::default(),
        seeds: vec![0, 1, 2, 3],
        windows: vec![(20, 100), (100, 180)],
    }
}

fn published() -> PublishedBoard {
    publish(&board(), KEY)
}

fn self_describing() -> SelfDescribingBoard {
    publish_self_describing(spec(), &board(), KEY)
}

#[test]
fn an_honest_board_verifies_through_both_surfaces_and_a_json_round_trip() {
    let pb = published();
    assert!(verify_board(&pb.chain, KEY));
    assert!(verify_published(&pb, KEY));
    assert!(!verify_published(&pb, b"wrong-key"));
    let back: PublishedBoard = serde_json::from_str(&serde_json::to_string(&pb).unwrap()).unwrap();
    assert!(verify_published(&back, KEY));

    let sdb = self_describing();
    assert!(verify_self_describing(&sdb, KEY));
    let back: SelfDescribingBoard =
        serde_json::from_str(&serde_json::to_string(&sdb).unwrap()).unwrap();
    assert!(verify_self_describing(&back, KEY));
}

#[test]
fn rewriting_a_displayed_score_is_caught_even_though_the_chain_still_recomputes() {
    let mut pb = published();
    pb.scores[2].deflated_sharpe = 9.0;
    pb.scores[2].composite = 9.0;
    pb.scores[2].rank_eligible = true;
    // The chain is untouched, so the chain-only verifier is satisfied. That is
    // exactly the reported defect.
    assert!(verify_board(&pb.chain, KEY));
    assert!(!verify_published(&pb, KEY));

    let mut sdb = self_describing();
    sdb.scores[0].agent_id = "impostor".to_string();
    assert!(!verify_self_describing(&sdb, KEY));
}

#[test]
fn reordering_the_displayed_scores_is_caught() {
    let mut pb = published();
    pb.scores.swap(0, 2);
    assert!(verify_board(&pb.chain, KEY));
    assert!(!verify_published(&pb, KEY));

    let mut sdb = self_describing();
    sdb.scores.swap(0, 1);
    assert!(!verify_self_describing(&sdb, KEY));
}

#[test]
fn dropping_or_padding_the_displayed_scores_is_caught_by_count() {
    let mut short = published();
    short.scores.remove(1);
    assert!(verify_board(&short.chain, KEY));
    assert!(!verify_published(&short, KEY));

    let mut padded = published();
    let extra = padded.scores[0].clone();
    padded.scores.push(extra);
    assert!(!verify_published(&padded, KEY));

    let mut sdb = self_describing();
    sdb.scores.pop();
    assert!(!verify_self_describing(&sdb, KEY));
}

#[test]
fn deleting_the_last_entry_from_both_the_scores_and_the_chain_is_caught() {
    // The case neither the chain nor the score comparison can see on its own: a
    // prefix of a valid chain is a valid chain, and the shortened score array
    // still matches the shortened links one for one.
    let mut pb = published();
    pb.scores.pop();
    pb.chain.pop();
    assert!(verify_board(&pb.chain, KEY), "the prefix still recomputes");
    assert!(
        !verify_published(&pb, KEY),
        "the receipt anchors the length"
    );

    let mut sdb = self_describing();
    sdb.scores.pop();
    sdb.chain.pop();
    assert!(!verify_self_describing(&sdb, KEY));

    // Deleting every entry is the same failure, not a vacuous pass.
    let mut emptied = published();
    emptied.scores.clear();
    emptied.chain.clear();
    assert!(verify_board(&emptied.chain, KEY));
    assert!(!verify_published(&emptied, KEY));
}

#[test]
fn a_board_with_no_terminal_anchor_is_refused_not_reported_as_verified() {
    let mut pb = published();
    pb.receipt = None;
    assert!(!verify_published(&pb, KEY));

    let mut sdb = self_describing();
    sdb.receipt = None;
    assert!(!verify_self_describing(&sdb, KEY));

    // A document written before the anchor existed still loads, and fails closed.
    let honest = published();
    let legacy = serde_json::json!({ "scores": honest.scores, "chain": honest.chain });
    let parsed: PublishedBoard = serde_json::from_value(legacy).unwrap();
    assert!(parsed.receipt.is_none());
    assert!(!verify_published(&parsed, KEY));
}

#[test]
fn swapping_the_spec_is_still_caught_alongside_the_new_bindings() {
    let mut sdb = self_describing();
    sdb.spec.dataset_hash = sharpebench_attest::content_digest(b"a-different-dataset");
    assert!(!verify_self_describing(&sdb, KEY));
}
