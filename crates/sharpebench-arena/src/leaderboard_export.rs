//! Read verified signed boards into a leaderboard export
//! ([`sharpebench_core::leaderboard_export`]).
//!
//! Nothing is exported from an input that does not verify. An arena directory
//! is checked by [`verify_arena_expecting`] first, every board chain, the
//! cross-window anchors and the signed head, and the export is refused unless
//! the whole arena passes. The boards are then read a second time to take
//! their rows, and each one is checked again on the bytes actually read: its
//! chain under the key the verification established, its header's window id
//! and cross-window anchor, and at the end the newest final signature against
//! the head the verification reported. A board replaced between the two reads
//! is refused, not exported. A board file written by `sharpebench sign` is
//! checked under its HMAC key, or under its Ed25519 chain when the caller asks
//! for that instead.
//!
//! Rows come only from signed links: the score payload of each link, never the
//! unsigned `scores` array a board file also carries, and never `window.json`.

use std::path::Path;

use sharpebench_attest::{
    content_digest, verify_public_chain, verify_public_chain_with, PublicChain, SignedResult,
    VerifyingKey,
};
use sharpebench_core::headline::Evidence;
use sharpebench_core::leaderboard_export::{
    refused_row, scored_row, BoardRecord, ExportRow, ExportSource, LeaderboardExport, SourceKind,
    SCHEMA_VERSION,
};
use sharpebench_core::{CompositeScore, DisqualThresholds, ScoreConfig};
use sharpebench_leaderboard::{verify_published, PublishedBoard};

use crate::{
    head_digest, read_json, score_config_digest, verify_arena_expecting, BoardRow, ExpectedHead,
    ReturnsProvenance, WindowHeader, BOARD_FILE, GENESIS_ANCHOR, WINDOWS_DIR, WINDOW_FILE,
    WINDOW_HEADER_KIND,
};

/// The field of a `sharpebench sign --ed25519` board that holds its Ed25519
/// chain.
pub const PUBLIC_CHAIN_FIELD: &str = "public_chain";

/// Noncertifying reason: the board ranked returns the entrant supplied.
pub const SUPPLIED_RETURNS_REASON: &str =
    "the board ranked returns supplied by an entrant after the reveal";
/// Noncertifying reason: the header records that the board certifies nothing.
pub const BOARD_NOT_CERTIFYING_REASON: &str = "the board does not certify its rows";
/// Noncertifying reason: the header predates the certifying flag.
pub const CERTIFYING_UNRECORDED_REASON: &str =
    "the board header records no certifying flag, so it certifies nothing";

/// What `export` states it cannot establish about a board signed under HMAC.
pub const HMAC_BOARD_LIMIT: &str = "an HMAC board verifies only for a holder of its secret key, who could also have signed it; the board binds no dataset, window or score configuration, and `sharpebench sign` scores under the default configuration, which the verdicts assume";
/// What `export` states it cannot establish about an Ed25519 signed board.
pub const ED25519_BOARD_LIMIT: &str = "the Ed25519 chain and its receipt verify, so no link was altered, reordered or removed; the board binds no dataset, window or score configuration, and `sharpebench sign` scores under the default configuration, which the verdicts assume";

/// How a board file written by `sharpebench sign` is verified.
#[derive(Clone, Copy, Debug)]
pub enum BoardVerifier<'a> {
    /// The HMAC key the board was signed with.
    Hmac(&'a [u8]),
    /// The board's Ed25519 chain, under this pinned key or, with `None`, the
    /// key the document embeds.
    Ed25519(Option<&'a VerifyingKey>),
}

/// The evidence mode of every arena row: an arena window is committed before
/// its data exists.
pub const ARENA_EVIDENCE_MODE: &str = "forward";

/// The evidence qualifier of a row on an arena board, in the headline's sense
/// of the word: a forward row, certifying exactly when the signed header says
/// the board certifies. The board's flag, when it does not, and its supplied
/// returns are the noncertifying reasons.
fn row_evidence(header: &WindowHeader) -> Evidence {
    let mode = ARENA_EVIDENCE_MODE.to_string();
    let mut reasons = Vec::new();
    match header.certifying {
        Some(true) => return Evidence::Certifying { mode },
        Some(false) => reasons.push(BOARD_NOT_CERTIFYING_REASON.to_string()),
        None => reasons.push(CERTIFYING_UNRECORDED_REASON.to_string()),
    }
    if header.supplied_returns_accepted {
        reasons.push(SUPPLIED_RETURNS_REASON.to_string());
    }
    Evidence::Noncertifying { mode, reasons }
}

fn final_signature(chain: &[SignedResult]) -> String {
    chain
        .last()
        .map_or_else(String::new, |link| link.signature.clone())
}

/// The rows of one verified arena board, and its record.
fn arena_board(
    window_id: &str,
    board: &PublicChain,
) -> Result<(BoardRecord, Vec<ExportRow>), String> {
    let (first, links) = board
        .chain
        .split_first()
        .ok_or_else(|| format!("the board of `{window_id}` has an empty chain"))?;
    let header: WindowHeader = serde_json::from_str(&first.payload)
        .map_err(|e| format!("the board of `{window_id}` has no window header: {e}"))?;
    let record = BoardRecord {
        board_id: window_id.to_string(),
        board_path: format!("{WINDOWS_DIR}/{window_id}/{BOARD_FILE}"),
        window_path: Some(format!("{WINDOWS_DIR}/{window_id}/{WINDOW_FILE}")),
        final_signature_sha256: head_digest(&final_signature(&board.chain)),
        dataset_hash: Some(header.dataset_hash.clone()),
        score_config_sha256: header.score_config_sha256.clone(),
        scorer_artifact_sha256: Some(header.scorer_artifact_sha256.clone()),
        certifying: header.certifying,
        supplied_returns_accepted: header.supplied_returns_accepted,
        rows: links.len(),
        refusals: header.refusals.len(),
    };
    let thresholds = DisqualThresholds::from_score_config(&header.score_config);
    let mut rows = Vec::with_capacity(links.len() + header.refusals.len());
    for (offset, link) in links.iter().enumerate() {
        let index = offset + 1;
        let row: BoardRow = serde_json::from_str(&link.payload).map_err(|e| {
            format!("link {index} of the board of `{window_id}` is not a board row: {e}")
        })?;
        rows.push(scored_row(
            &record,
            index,
            &link.signature,
            &row.score,
            &thresholds,
            row_evidence(&header),
            row.returns_provenance.map(ReturnsProvenance::as_str),
            header
                .live_capture
                .get(&row.score.agent_id)
                .map(|root| root.stalled_holds),
        ));
    }
    for refusal in &header.refusals {
        rows.push(refused_row(
            &record,
            &first.signature,
            &refusal.agent_id,
            &refusal.reason,
            &thresholds,
            Evidence::Unknown,
        ));
    }
    Ok((record, rows))
}

/// Export every published board of the arena at `dir`.
///
/// The arena must verify under `pinned` (or the key its boards embed) and,
/// when given, pass through `expected`; otherwise nothing is exported and the
/// error lists what failed. An arena with no published window exports no
/// rows.
pub fn export_arena(
    dir: &Path,
    pinned: Option<&VerifyingKey>,
    expected: Option<&ExpectedHead>,
) -> Result<LeaderboardExport, String> {
    let report = verify_arena_expecting(dir, pinned, expected)?;
    if !report.ok {
        let mut failures: Vec<String> = report
            .windows
            .iter()
            .filter(|w| w.detail != "ok")
            .map(|w| format!("{}: {}", w.window_id, w.detail))
            .collect();
        failures.push(format!(
            "completeness {}: {}",
            serde_json::to_value(report.completeness.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            report.completeness.detail
        ));
        return Err(format!(
            "the arena does not verify, so nothing is exported: {}",
            failures.join("; ")
        ));
    }
    let key = report
        .verifying_key
        .as_deref()
        .map(|hex| {
            VerifyingKey::from_hex(hex)
                .ok_or_else(|| format!("the arena key `{hex}` is not an Ed25519 key"))
        })
        .transpose()?;
    let mut boards = Vec::with_capacity(report.windows.len());
    let mut rows = Vec::new();
    let mut anchor = GENESIS_ANCHOR.to_string();
    for window in &report.windows {
        let id = &window.window_id;
        let board: PublicChain = read_json(&dir.join(WINDOWS_DIR).join(id).join(BOARD_FILE))?;
        let header_ok = board
            .chain
            .first()
            .and_then(|link| serde_json::from_str::<WindowHeader>(&link.payload).ok())
            .is_some_and(|h| {
                h.kind == WINDOW_HEADER_KIND
                    && h.window_id == *id
                    && h.prev_final_signature == anchor
            });
        let signed = key
            .as_ref()
            .is_some_and(|key| verify_public_chain_with(&board, key));
        if !signed || !header_ok {
            return Err(format!(
                "the board of `{id}` changed after the arena verified, so nothing is exported"
            ));
        }
        anchor = final_signature(&board.chain);
        let (record, board_rows) = arena_board(id, &board)?;
        boards.push(record);
        rows.extend(board_rows);
    }
    if let Some(digest) = &report.completeness.head_digest {
        if head_digest(&anchor) != *digest {
            return Err(
                "the newest board changed after the arena verified, so nothing is exported"
                    .to_string(),
            );
        }
    }
    let completeness = &report.completeness;
    Ok(LeaderboardExport {
        schema: SCHEMA_VERSION.to_string(),
        source: ExportSource {
            kind: SourceKind::Arena,
            scheme: sharpebench_attest::ED25519_SCHEME.to_string(),
            verifying_key: report.verifying_key.clone(),
            key_pinned: pinned.is_some(),
            completeness: serde_json::to_value(completeness.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            completeness_limit: completeness.detail.clone(),
            head: completeness
                .head_digest
                .as_ref()
                .map(|digest| format!("{}:{digest}", completeness.published_count)),
        },
        boards,
        rows,
    })
}

/// Export the board file `file_name` holding `document`, written by
/// `sharpebench sign`, after verifying it with `verifier`.
///
/// The board records no score configuration; `sign` always scores under
/// [`ScoreConfig::default`], so the verdicts are read under that, and the
/// export says so in its stated limit.
pub fn export_signed_board(
    file_name: &str,
    document: &str,
    verifier: BoardVerifier<'_>,
) -> Result<LeaderboardExport, String> {
    let (links, scheme, verifying_key, key_pinned, limit) = match verifier {
        BoardVerifier::Hmac(key) => {
            let board: PublishedBoard = serde_json::from_str(document)
                .map_err(|e| format!("{file_name} is not a signed board: {e}"))?;
            if !verify_published(&board, key) {
                return Err(format!(
                    "{file_name} does not verify under the HMAC key, so nothing is exported"
                ));
            }
            (board.chain, "hmac-sha256", None, true, HMAC_BOARD_LIMIT)
        }
        BoardVerifier::Ed25519(pinned) => {
            let value: serde_json::Value = serde_json::from_str(document)
                .map_err(|e| format!("{file_name} is not JSON: {e}"))?;
            let public: PublicChain = value
                .get(PUBLIC_CHAIN_FIELD)
                .cloned()
                .ok_or_else(|| format!("{file_name} carries no `{PUBLIC_CHAIN_FIELD}`"))
                .and_then(|chain| {
                    serde_json::from_value(chain).map_err(|e| {
                        format!("{file_name} has a malformed `{PUBLIC_CHAIN_FIELD}`: {e}")
                    })
                })?;
            let ok = match pinned {
                Some(key) => verify_public_chain_with(&public, key),
                None => verify_public_chain(&public),
            };
            if !ok {
                return Err(format!(
                    "the Ed25519 chain of {file_name} does not verify, so nothing is exported"
                ));
            }
            (
                public.chain,
                sharpebench_attest::ED25519_SCHEME,
                Some(public.verifying_key),
                pinned.is_some(),
                ED25519_BOARD_LIMIT,
            )
        }
    };
    let config = ScoreConfig::default();
    let record = BoardRecord {
        board_id: file_name.to_string(),
        board_path: file_name.to_string(),
        window_path: None,
        final_signature_sha256: content_digest(final_signature(&links).as_bytes()),
        dataset_hash: None,
        score_config_sha256: score_config_digest(&config)?,
        scorer_artifact_sha256: None,
        certifying: None,
        supplied_returns_accepted: false,
        rows: links.len(),
        refusals: 0,
    };
    let thresholds = DisqualThresholds::from_score_config(&config);
    let rows = links
        .iter()
        .enumerate()
        .map(|(index, link)| {
            serde_json::from_str::<CompositeScore>(&link.payload)
                .map_err(|e| format!("link {index} of {file_name} is not a score: {e}"))
                .map(|score| {
                    scored_row(
                        &record,
                        index,
                        &link.signature,
                        &score,
                        &thresholds,
                        Evidence::Unknown,
                        None,
                        None,
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LeaderboardExport {
        schema: SCHEMA_VERSION.to_string(),
        source: ExportSource {
            kind: SourceKind::SignedBoard,
            scheme: scheme.to_string(),
            verifying_key,
            key_pinned,
            completeness: "receipt_anchored".to_string(),
            completeness_limit: limit.to_string(),
            head: None,
        },
        boards: vec![record],
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Refusal;

    fn header(certifying: Option<bool>, supplied: bool) -> WindowHeader {
        WindowHeader {
            kind: WINDOW_HEADER_KIND.to_string(),
            window_id: "w".to_string(),
            commit_deadline: 1,
            data_reveal_epoch: 2,
            dataset_hash: "d".to_string(),
            schema_version: 2,
            score_config: ScoreConfig::default(),
            score_config_sha256: "c".to_string(),
            scorer_artifact_sha256: "s".to_string(),
            sealed_eval_salt_sha256: None,
            fault_plan_sha256: None,
            replay_dataset_sha256: None,
            supplied_returns_accepted: supplied,
            certifying,
            refusals: vec![Refusal {
                agent_id: "x".to_string(),
                reason: "r".to_string(),
            }],
            prev_final_signature: GENESIS_ANCHOR.to_string(),
            head_anchored: false,
            execution: None,
            forward_evidence: None,
            live_capture: Default::default(),
        }
    }

    #[test]
    fn evidence_certifies_only_under_a_certifying_header() {
        assert_eq!(
            row_evidence(&header(Some(true), false)),
            Evidence::Certifying {
                mode: "forward".to_string()
            }
        );
        assert_eq!(
            row_evidence(&header(Some(false), true)),
            Evidence::Noncertifying {
                mode: "forward".to_string(),
                reasons: vec![
                    BOARD_NOT_CERTIFYING_REASON.to_string(),
                    SUPPLIED_RETURNS_REASON.to_string()
                ],
            }
        );
        assert_eq!(
            row_evidence(&header(None, false)),
            Evidence::Noncertifying {
                mode: "forward".to_string(),
                reasons: vec![CERTIFYING_UNRECORDED_REASON.to_string()],
            }
        );
    }
}
