//! Check an explicit board certification claim against its signed row evidence.

use std::collections::BTreeMap;

use sharpebench_attest::PublicChain;

use crate::{board_certifies, BoardRow, WindowHeader};

pub(crate) fn certification_error(header: &WindowHeader, board: &PublicChain) -> Option<String> {
    // Before this field existed a signed board certified nothing. Preserve
    // that meaning, rather than upgrading old documents from incidental rows.
    let claimed = header.certifying?;
    let mut provenance = BTreeMap::new();
    for (index, link) in board.chain.iter().skip(1).enumerate() {
        let row: BoardRow = match serde_json::from_str(&link.payload) {
            Ok(row) => row,
            Err(error) => return Some(format!("board row {index} is not a scored row: {error}")),
        };
        let Some(source) = row.returns_provenance else {
            return Some(format!(
                "board row `{}` has no returns provenance for an explicit certification claim",
                row.score.agent_id
            ));
        };
        if provenance
            .insert(row.score.agent_id.clone(), source)
            .is_some()
        {
            return Some(format!("board repeats agent `{}`", row.score.agent_id));
        }
    }
    let derived = board_certifies(&provenance, header.supplied_returns_accepted);
    (claimed != derived)
        .then(|| format!("signed certification is {claimed}, but its signed rows derive {derived}"))
}
