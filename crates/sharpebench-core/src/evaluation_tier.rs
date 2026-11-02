//! The evaluation tier of a board or a row: which of the two kinds of evidence
//! it is.
//!
//! - [`EvaluationTier::HistoricalAdvisory`]: the row was scored on bars that
//!   existed before the entrant was fixed, or on returns nothing ties to a
//!   signed forward window. It is advisory and never certifies skill, whatever
//!   its verdict. A submissions file, `sharpebench run` (historical windows
//!   only) and an arena board that does not certify its rows are all in this
//!   tier.
//! - [`EvaluationTier::ForwardCertifying`]: the row was scored on a forward
//!   window, whose bars were revealed after the entrant was committed, under
//!   evidence that certifies it: the harness evidence mode `forward` with
//!   `certifying` true, or a signed arena board whose header states
//!   `certifying: true`.
//!
//! The tier is derived from facts the host already holds (the evidence mode and
//! the signed arena header). It is a label: it never changes a score, a rank,
//! an ordinal, a gate or the headline verdict. Anything that does not prove the
//! forward-certifying conditions is labelled historical-advisory, so the label
//! fails closed.

use serde::{Deserialize, Serialize};

use crate::headline::Evidence;

/// Schema of the tier label wherever it is published as an object.
pub const EVALUATION_TIER_VERSION: &str = "sharpebench.evaluation-tier.v1";

/// The harness evidence mode of a forward window (`evidence_mode.mode`).
const FORWARD_MODE: &str = "forward";

/// The evaluation tier. Serialized in kebab case: `historical-advisory`,
/// `forward-certifying`.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum EvaluationTier {
    /// Non-forward or noncertifying evidence. Never certifies skill.
    #[default]
    HistoricalAdvisory,
    /// A signed forward window whose evidence certifies the row.
    ForwardCertifying,
}

impl EvaluationTier {
    /// The serialized name, for tables.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HistoricalAdvisory => "historical-advisory",
            Self::ForwardCertifying => "forward-certifying",
        }
    }

    /// Whether a verdict in this tier may be read as evidence of skill.
    pub const fn certifies_skill(self) -> bool {
        matches!(self, Self::ForwardCertifying)
    }

    /// One sentence saying what the tier means, for human output.
    pub const fn meaning(self) -> &'static str {
        match self {
            Self::HistoricalAdvisory => {
                "advisory only: no signed forward window certifies these rows, so no verdict here certifies skill"
            }
            Self::ForwardCertifying => {
                "scored on a forward window whose evidence certifies the rows"
            }
        }
    }

    /// The tier of a row whose headline evidence is `evidence`: forward
    /// certifying only for a certifying evidence mode of `forward`.
    pub fn of_evidence(evidence: &Evidence) -> Self {
        match evidence {
            Evidence::Certifying { mode } if mode == FORWARD_MODE => Self::ForwardCertifying,
            _ => Self::HistoricalAdvisory,
        }
    }

    /// The tier of a signed arena board from its header's `certifying` field.
    /// Arena windows are forward windows; the board certifies its rows only
    /// when the signed header says `true`. A header without the field
    /// certifies nothing.
    pub const fn of_signed_forward_board(certifying: Option<bool>) -> Self {
        match certifying {
            Some(true) => Self::ForwardCertifying,
            _ => Self::HistoricalAdvisory,
        }
    }

    /// The human line printed first on a board.
    pub fn board_line(self) -> String {
        format!("evaluation tier: {} ({})", self.as_str(), self.meaning())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_certifying_forward_evidence_is_forward_certifying() {
        let cases = [
            (Evidence::Unknown, EvaluationTier::HistoricalAdvisory),
            (
                Evidence::Certifying {
                    mode: "forward".into(),
                },
                EvaluationTier::ForwardCertifying,
            ),
            (
                Evidence::Certifying {
                    mode: "backtest".into(),
                },
                EvaluationTier::HistoricalAdvisory,
            ),
            (
                Evidence::Noncertifying {
                    mode: "forward".into(),
                    reasons: vec!["supplied returns".into()],
                },
                EvaluationTier::HistoricalAdvisory,
            ),
        ];
        for (evidence, tier) in cases {
            assert_eq!(EvaluationTier::of_evidence(&evidence), tier, "{evidence:?}");
        }
    }

    #[test]
    fn an_arena_board_certifies_only_when_its_signed_header_says_true() {
        assert_eq!(
            EvaluationTier::of_signed_forward_board(Some(true)),
            EvaluationTier::ForwardCertifying
        );
        for header in [Some(false), None] {
            assert_eq!(
                EvaluationTier::of_signed_forward_board(header),
                EvaluationTier::HistoricalAdvisory
            );
        }
    }

    #[test]
    fn the_tier_serializes_in_kebab_case_and_only_forward_certifies_skill() {
        for (tier, name, certifies) in [
            (
                EvaluationTier::HistoricalAdvisory,
                "historical-advisory",
                false,
            ),
            (
                EvaluationTier::ForwardCertifying,
                "forward-certifying",
                true,
            ),
        ] {
            assert_eq!(serde_json::to_value(tier).unwrap(), name);
            assert_eq!(tier.as_str(), name);
            assert_eq!(tier.certifies_skill(), certifies);
            assert!(tier
                .board_line()
                .starts_with(&format!("evaluation tier: {name} (")));
            assert!(tier.board_line().contains(tier.meaning()));
        }
        assert_ne!(
            EvaluationTier::HistoricalAdvisory.meaning(),
            EvaluationTier::ForwardCertifying.meaning()
        );
        assert_eq!(
            EvaluationTier::default(),
            EvaluationTier::HistoricalAdvisory
        );
    }
}
