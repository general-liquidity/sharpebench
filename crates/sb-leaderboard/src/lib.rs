//! sb-leaderboard — render a scored field and sign it tamper-evidently.
//!
//! Rendering is cosmetic; the load-bearing part is [`sign_board`], which chains an
//! HMAC signature over each ranked entry (via [`sb_attest`]) so a *published*
//! board can be independently verified — nobody, including the host, can quietly
//! reorder or edit it after the fact.
#![forbid(unsafe_code)]

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use sb_attest::{sign_result, verify_chain, SignedResult, GENESIS};
use sb_core::CompositeScore;

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
    use sb_core::{rank, AgentSubmission, Run, ScoreConfig};

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
