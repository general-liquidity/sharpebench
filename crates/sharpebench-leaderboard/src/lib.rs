//! sb-leaderboard — render a scored field and sign it tamper-evidently.
//!
//! Rendering is cosmetic; the load-bearing part is [`sign_board`], which chains an
//! HMAC signature over each ranked entry (via [`sharpebench_attest`]) so a *published*
//! board can be independently verified — nobody, including the host, can quietly
//! reorder or edit it after the fact.
#![forbid(unsafe_code)]

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use sharpebench_attest::{sign_result, verify_chain, SignedResult, GENESIS};
use sharpebench_core::{CompositeScore, ScoreConfig};

/// The transaction-cost profile a board was scored under — a self-contained mirror
/// of the simulator's cost model, inlined so a published entry is re-derivable
/// without reaching into the sim crate.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CostProfile {
    pub fee_bps: f64,
    pub slippage_bps: f64,
    pub impact_bps: f64,
    pub financing_bps: f64,
    /// `None` encodes an unlimited (`f64::INFINITY`) liquidity cap so the profile
    /// JSON stays finite and portable.
    pub max_participation: Option<f64>,
}

/// The full, self-describing specification a leaderboard was produced from. Inlined
/// into the signed blob so any entry is independently re-derivable: same dataset
/// (by hash), same costs, same scoring config, same seeds and windows.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunSpec {
    /// SHA-256 hex digest of the exact frozen dataset the agents were scored on
    /// (see [`sharpebench_attest::content_digest`]).
    pub dataset_hash: String,
    pub cost_profile: CostProfile,
    pub score_config: ScoreConfig,
    pub seeds: Vec<u64>,
    /// `[start, end)` simulation windows, as index pairs over the dataset axis.
    pub windows: Vec<(usize, usize)>,
}

/// Render a ranked field as a plain-text leaderboard.
pub fn render(board: &[CompositeScore]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<4} {:<18} {:>9} {:>7} {:>9}",
        "#", "agent", "DSR", "elig", "raw_ret"
    );
    for (i, s) in board.iter().enumerate() {
        let pos = if s.rank_eligible {
            (i + 1).to_string()
        } else {
            "-".to_string()
        };
        let _ = writeln!(
            out,
            "{:<4} {:<18} {:>9.4} {:>7} {:>9.5}",
            pos, s.agent_id, s.deflated_sharpe, s.rank_eligible, s.raw_mean_return
        );
    }
    out
}

/// Build a tamper-evident signed chain over the ranked field — each entry signed
/// against the previous, so the published board is independently verifiable.
pub fn sign_board(board: &[CompositeScore], key: &[u8]) -> Vec<SignedResult> {
    let mut chain = Vec::with_capacity(board.len());
    let mut prev = GENESIS.to_string();
    for s in board {
        let payload = serde_json::to_string(s).unwrap_or_default();
        let signed = sign_result(&payload, &prev, key);
        prev = signed.signature.clone();
        chain.push(signed);
    }
    chain
}

/// Verify a signed board chain.
pub fn verify_board(chain: &[SignedResult], key: &[u8]) -> bool {
    verify_chain(chain, key)
}

/// A published board: the ranked scores plus their tamper-evident signature chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishedBoard {
    pub scores: Vec<CompositeScore>,
    pub chain: Vec<SignedResult>,
}

/// Build a published board (scores + signed chain).
pub fn publish(board: &[CompositeScore], key: &[u8]) -> PublishedBoard {
    PublishedBoard {
        scores: board.to_vec(),
        chain: sign_board(board, key),
    }
}

/// A *self-describing* published board: the run-spec, the ranked scores, and a
/// single tamper-evident chain that signs the spec **and** every entry. Because
/// the spec is the chain's first link, altering the dataset hash, costs, scoring
/// config, or seeds after the fact breaks the signature — the published number is
/// inseparable from the inputs that produced it, and re-derivable from them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelfDescribingBoard {
    pub spec: RunSpec,
    pub scores: Vec<CompositeScore>,
    pub chain: Vec<SignedResult>,
}

/// Canonical JSON of the run-spec — the payload of the chain's spec link.
fn spec_payload(spec: &RunSpec) -> String {
    serde_json::to_string(spec).unwrap_or_default()
}

/// Build a self-describing signed board: chain = [spec] ++ [scores…], each link
/// signed against the previous so the spec and the ranking are bound together.
pub fn publish_self_describing(
    spec: RunSpec,
    board: &[CompositeScore],
    key: &[u8],
) -> SelfDescribingBoard {
    let mut chain = Vec::with_capacity(board.len() + 1);
    let spec_link = sign_result(&spec_payload(&spec), GENESIS, key);
    let mut prev = spec_link.signature.clone();
    chain.push(spec_link);
    for s in board {
        let payload = serde_json::to_string(s).unwrap_or_default();
        let signed = sign_result(&payload, &prev, key);
        prev = signed.signature.clone();
        chain.push(signed);
    }
    SelfDescribingBoard {
        spec,
        scores: board.to_vec(),
        chain,
    }
}

/// Verify a self-describing board: the HMAC chain must recompute end-to-end *and*
/// the chain's spec link must still equal the inlined spec (so the spec can't be
/// swapped without re-signing the whole chain).
pub fn verify_self_describing(b: &SelfDescribingBoard, key: &[u8]) -> bool {
    if !verify_chain(&b.chain, key) {
        return false;
    }
    match b.chain.first() {
        Some(first) => first.payload == spec_payload(&b.spec),
        None => false,
    }
}

/// Persist a self-describing board to a JSON file.
pub fn save_self_describing(b: &SelfDescribingBoard, path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(b).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Persist a published board to a JSON file.
pub fn save(board: &PublishedBoard, path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(board).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Load a published board from a JSON file.
pub fn load(path: &str) -> std::io::Result<PublishedBoard> {
    let s = std::fs::read_to_string(path)?;
    serde_json::from_str(&s).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_core::{rank, AgentSubmission, Run, ScoreConfig};

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

    #[test]
    fn render_and_sign_roundtrip() {
        let board = rank(&[sub("a", 0.002), sub("b", 0.0)], &ScoreConfig::default());
        assert!(render(&board).contains("agent"));
        let chain = sign_board(&board, b"key");
        assert!(verify_board(&chain, b"key"));
        assert!(!verify_board(&chain, b"wrong-key"));
    }

    fn demo_spec() -> RunSpec {
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

    #[test]
    fn self_describing_board_inlines_spec_and_verifies() {
        let board = rank(&[sub("a", 0.002), sub("b", 0.0)], &ScoreConfig::default());
        let sdb = publish_self_describing(demo_spec(), &board, b"key");
        // The spec is inlined and re-derivable: dataset hash, seeds, costs all present.
        assert_eq!(sdb.spec.seeds, vec![0, 1, 2, 3]);
        assert_eq!(sdb.spec.dataset_hash.len(), 64);
        // Chain has one spec link + one per entry, and verifies HMAC-signed.
        assert_eq!(sdb.chain.len(), board.len() + 1);
        assert!(verify_self_describing(&sdb, b"key"));
        assert!(!verify_self_describing(&sdb, b"wrong-key"));

        // Survives a JSON round-trip (it is a self-contained, portable blob).
        let back: SelfDescribingBoard =
            serde_json::from_str(&serde_json::to_string(&sdb).unwrap()).unwrap();
        assert!(verify_self_describing(&back, b"key"));
    }

    #[test]
    fn tampering_with_the_spec_breaks_the_signature() {
        let board = rank(&[sub("a", 0.002)], &ScoreConfig::default());
        let mut sdb = publish_self_describing(demo_spec(), &board, b"key");
        // Forge a different dataset hash without re-signing → verification fails,
        // both because the spec link no longer matches and the chain is broken.
        sdb.spec.dataset_hash = sharpebench_attest::content_digest(b"a-different-dataset");
        assert!(!verify_self_describing(&sdb, b"key"));
    }

    #[test]
    fn publish_roundtrips_and_verifies() {
        let board = rank(&[sub("a", 0.002), sub("b", 0.0)], &ScoreConfig::default());
        let pb = publish(&board, b"key");
        let back: PublishedBoard =
            serde_json::from_str(&serde_json::to_string(&pb).unwrap()).unwrap();
        assert_eq!(back.scores.len(), 2);
        assert!(verify_board(&back.chain, b"key"));
        assert!(!verify_board(&back.chain, b"wrong"));
    }
}
