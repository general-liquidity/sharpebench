//! Versioned, opt-in lifecycle certification beside the host rank.
//!
//! The process gate in [`crate::composite`] counts events; the ordering check in
//! [`crate::process::check_lifecycle`] is additive and, by the shipped protocol,
//! not a conjunct of eligibility. A host that wants the ordering leg to bind can
//! select a **rank mode** by its versioned identifier. The mode never changes
//! the host board: `rank_eligible`, the sort and `rank_ordinal` are the ones
//! [`crate::composite::rank_declared`] produces, byte for byte. Certification is
//! a second, labeled verdict on each row, the same shape as a declared mandate.
//!
//! Certification is withheld, never coerced. Each property the record cannot
//! establish is named as a [`CertificationGap`], so a row says *why* it is not
//! certified rather than reporting a favourable value it did not earn.

use serde::{Deserialize, Serialize};

use crate::composite::{
    rank_declared, AgentSubmission, CompositeScore, MandateDeclarations, ScoreConfig,
};
use crate::process::{check_lifecycle, ProcessEvent};

/// The identifier of the first lifecycle-certified rank mode.
pub const LIFECYCLE_CERTIFIED_V1: &str = "lifecycle-certified/v1";

/// A rank mode a host opts into by identifier. Absent means the legacy
/// protocol, which is [`rank_declared`] unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankMode {
    /// Host eligibility, plus lifecycle evidence in every submitted run, plus
    /// zero block-severity ordering violations across all of them.
    LifecycleCertifiedV1,
}

/// Why a rank-mode identifier was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RankModeError {
    /// Not a mode this kernel implements. Carries the identifier as supplied.
    UnknownMode(String),
}

impl std::fmt::Display for RankModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownMode(id) => {
                write!(
                    f,
                    "unknown rank mode `{id}`; known: {LIFECYCLE_CERTIFIED_V1}"
                )
            }
        }
    }
}

impl std::error::Error for RankModeError {}

impl RankMode {
    /// Resolve an identifier. Anything but a known versioned identifier is
    /// refused, including a later version this kernel does not implement.
    pub fn parse(id: &str) -> Result<Self, RankModeError> {
        match id {
            LIFECYCLE_CERTIFIED_V1 => Ok(Self::LifecycleCertifiedV1),
            other => Err(RankModeError::UnknownMode(other.to_string())),
        }
    }

    /// The identifier this mode is selected and reported by.
    pub fn identifier(self) -> &'static str {
        match self {
            Self::LifecycleCertifiedV1 => LIFECYCLE_CERTIFIED_V1,
        }
    }
}

/// One property the record could not establish. Each variant names the
/// property; the run-scoped ones name the submitted run (window-major index,
/// before any shared-cell restriction, so a peer cannot hide a run by omitting
/// its cell).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "property", rename_all = "snake_case")]
pub enum CertificationGap {
    /// The row is not `rank_eligible` under the host verdict. Certification
    /// is a strengthening of eligibility, never a substitute for it.
    HostIneligible,
    /// The run's trace carries no [`ProcessEvent::Lifecycle`] transition, so
    /// the ordering check has nothing to certify: a vacuously clean report is
    /// not evidence that a lifecycle ran.
    LifecycleEvidenceAbsent { run: usize },
    /// The run's lifecycle has block-severity ordering violations.
    LifecycleOrderingBlocked { run: usize, block_violations: usize },
}

/// The certification verdict on one row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Certification {
    /// The mode identifier the verdict was produced under.
    pub mode: String,
    /// `true` only when `withheld` is empty.
    pub certified: bool,
    /// Warn-severity ordering violations across all runs. Reported, never
    /// withholding: the control ran and the record is incomplete.
    pub lifecycle_warnings: usize,
    /// Every property that withheld certification, host first, then by run.
    pub withheld: Vec<CertificationGap>,
}

impl Certification {
    /// Board-row wording, e.g. `certified (lifecycle-certified/v1)` or
    /// `withheld (lifecycle-certified/v1): host_ineligible, lifecycle_evidence_absent[run 1]`.
    pub fn describe(&self) -> String {
        if self.certified {
            return format!("certified ({})", self.mode);
        }
        let gaps: Vec<String> = self
            .withheld
            .iter()
            .map(|gap| match gap {
                CertificationGap::HostIneligible => "host_ineligible".to_string(),
                CertificationGap::LifecycleEvidenceAbsent { run } => {
                    format!("lifecycle_evidence_absent[run {run}]")
                }
                CertificationGap::LifecycleOrderingBlocked {
                    run,
                    block_violations,
                } => format!("lifecycle_ordering_blocked[run {run}: {block_violations}]"),
            })
            .collect();
        format!("withheld ({}): {}", self.mode, gaps.join(", "))
    }
}

/// Certify one scored row under [`RankMode::LifecycleCertifiedV1`]. Reads the
/// host verdict from `score` and the lifecycle from every run of `sub`.
pub fn certify_lifecycle_v1(sub: &AgentSubmission, score: &CompositeScore) -> Certification {
    let mut withheld = Vec::new();
    if !score.rank_eligible {
        withheld.push(CertificationGap::HostIneligible);
    }
    let mut lifecycle_warnings = 0;
    for (run, r) in sub.runs.iter().enumerate() {
        let has_evidence = r
            .trace
            .events
            .iter()
            .any(|e| matches!(e, ProcessEvent::Lifecycle(_)));
        if !has_evidence {
            withheld.push(CertificationGap::LifecycleEvidenceAbsent { run });
            continue;
        }
        let report = check_lifecycle(&r.trace);
        lifecycle_warnings += report.warn_violations;
        if !report.is_clean() {
            withheld.push(CertificationGap::LifecycleOrderingBlocked {
                run,
                block_violations: report.block_violations,
            });
        }
    }
    Certification {
        mode: LIFECYCLE_CERTIFIED_V1.to_string(),
        certified: withheld.is_empty(),
        lifecycle_warnings,
        withheld,
    }
}

/// [`rank_declared`] with a certification verdict attached to every row.
///
/// The board is the one [`rank_declared`] produces, byte for byte, except that
/// each row carries `certification`. Certification is computed over the runs
/// as submitted, not over the shared-cell restriction, for the same reason the
/// process gate is.
pub fn rank_certified(
    subs: &[AgentSubmission],
    declarations: &MandateDeclarations,
    cfg: &ScoreConfig,
    mode: RankMode,
) -> Vec<CompositeScore> {
    let mut board = rank_declared(subs, declarations, cfg);
    for score in &mut board {
        let sub = subs
            .iter()
            .find(|s| s.agent_id == score.agent_id)
            .expect("every ranked row comes from a submitted agent");
        score.certification = Some(match mode {
            RankMode::LifecycleCertifiedV1 => certify_lifecycle_v1(sub, score),
        });
    }
    board
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{rank, Run};
    use crate::process::{LifecycleStep, OrderId, Phase, Subject, Trace};

    fn step(phase: Phase) -> ProcessEvent {
        ProcessEvent::Lifecycle(LifecycleStep::new(
            Subject::Instrument("BTC".to_string()),
            phase,
        ))
    }

    fn oid() -> OrderId {
        OrderId("o1".to_string())
    }

    fn full_cycle() -> Trace {
        Trace {
            events: vec![
                step(Phase::Observation),
                step(Phase::Decision),
                step(Phase::RiskEvaluation { passed: true }),
                step(Phase::Submission { order: oid() }),
                step(Phase::Acknowledgment { order: oid() }),
                step(Phase::Fill { order: oid() }),
                step(Phase::Reconciliation { order: oid() }),
            ],
        }
    }

    fn strong(id: &str, traces: Vec<Trace>) -> AgentSubmission {
        AgentSubmission {
            agent_id: id.to_string(),
            runs: traces
                .into_iter()
                .map(|trace| Run {
                    returns: (0..60).map(|i| 0.01 + 0.001 * (i as f64).sin()).collect(),
                    trace,
                    ..Run::default()
                })
                .collect(),
            in_sample_trials: 0,
            candidates: Vec::new(),
        }
    }

    fn only(board: &[CompositeScore]) -> &Certification {
        assert_eq!(board.len(), 1);
        board[0]
            .certification
            .as_ref()
            .expect("mode attaches a verdict")
    }

    #[test]
    fn mode_absent_is_byte_identical_and_carries_no_certification_key() {
        let subs = [
            strong("a", vec![full_cycle()]),
            strong("b", vec![Trace::default()]),
        ];
        let cfg = ScoreConfig::default();
        let legacy = serde_json::to_string(&rank(&subs, &cfg)).unwrap();
        assert!(!legacy.contains("certification"));
        assert_eq!(
            serde_json::to_string(&rank_declared(&subs, &MandateDeclarations::new(), &cfg))
                .unwrap(),
            legacy
        );
        // The certified board is the legacy board plus the verdict: stripping
        // the verdict restores the legacy bytes, so the host rank is untouched.
        let mut certified = rank_certified(
            &subs,
            &MandateDeclarations::new(),
            &cfg,
            RankMode::LifecycleCertifiedV1,
        );
        for row in &mut certified {
            row.certification = None;
        }
        assert_eq!(serde_json::to_string(&certified).unwrap(), legacy);
    }

    #[test]
    fn fully_certified_entrant() {
        let subs = [strong("a", vec![full_cycle(), full_cycle()])];
        let board = rank_certified(
            &subs,
            &MandateDeclarations::new(),
            &ScoreConfig::default(),
            RankMode::LifecycleCertifiedV1,
        );
        assert!(board[0].rank_eligible);
        let c = only(&board);
        assert_eq!(
            *c,
            Certification {
                mode: LIFECYCLE_CERTIFIED_V1.to_string(),
                certified: true,
                lifecycle_warnings: 0,
                withheld: vec![],
            }
        );
        assert_eq!(c.describe(), "certified (lifecycle-certified/v1)");
    }

    #[test]
    fn host_ineligible_withholds_and_names_the_host_gate() {
        let mut sub = strong("a", vec![full_cycle()]);
        sub.runs[0].returns = vec![0.0; 60];
        let board = rank_certified(
            &[sub],
            &MandateDeclarations::new(),
            &ScoreConfig::default(),
            RankMode::LifecycleCertifiedV1,
        );
        assert!(!board[0].rank_eligible);
        let c = only(&board);
        assert!(!c.certified);
        assert_eq!(c.withheld, vec![CertificationGap::HostIneligible]);
        assert_eq!(
            c.describe(),
            "withheld (lifecycle-certified/v1): host_ineligible"
        );
    }

    #[test]
    fn missing_lifecycle_evidence_withholds_and_names_the_run() {
        let subs = [strong("a", vec![full_cycle(), Trace::default()])];
        let board = rank_certified(
            &subs,
            &MandateDeclarations::new(),
            &ScoreConfig::default(),
            RankMode::LifecycleCertifiedV1,
        );
        assert!(board[0].rank_eligible, "the host gate is untouched");
        let c = only(&board);
        assert!(!c.certified);
        assert_eq!(
            c.withheld,
            vec![CertificationGap::LifecycleEvidenceAbsent { run: 1 }]
        );
    }

    #[test]
    fn blocked_ordering_withholds_and_names_the_run() {
        // A fill for an order never submitted: block severity, and invisible to
        // the counting gate, so the host row stays eligible.
        let bad = Trace {
            events: vec![step(Phase::Fill { order: oid() })],
        };
        let subs = [strong("a", vec![bad, full_cycle()])];
        let board = rank_certified(
            &subs,
            &MandateDeclarations::new(),
            &ScoreConfig::default(),
            RankMode::LifecycleCertifiedV1,
        );
        assert!(board[0].rank_eligible && board[0].process_ok);
        let c = only(&board);
        assert!(!c.certified);
        assert_eq!(
            c.withheld,
            vec![CertificationGap::LifecycleOrderingBlocked {
                run: 0,
                block_violations: 1
            }]
        );
        assert_eq!(
            c.describe(),
            "withheld (lifecycle-certified/v1): lifecycle_ordering_blocked[run 0: 1]"
        );
    }

    #[test]
    fn warn_severity_ordering_is_reported_not_withholding() {
        let mut trace = full_cycle();
        // Submission without a recorded decision: warn severity.
        trace.events.remove(1);
        let subs = [strong("a", vec![trace])];
        let board = rank_certified(
            &subs,
            &MandateDeclarations::new(),
            &ScoreConfig::default(),
            RankMode::LifecycleCertifiedV1,
        );
        let c = only(&board);
        assert!(c.certified);
        assert_eq!(c.lifecycle_warnings, 1);
    }

    #[test]
    fn unknown_mode_version_is_refused() {
        assert_eq!(
            RankMode::parse(LIFECYCLE_CERTIFIED_V1),
            Ok(RankMode::LifecycleCertifiedV1)
        );
        // The identifier a mode reports is the one that selects it, so a
        // report naming a mode can be fed back to `parse` unchanged.
        let mode = RankMode::LifecycleCertifiedV1;
        assert_eq!(mode.identifier(), "lifecycle-certified/v1");
        assert_eq!(RankMode::parse(mode.identifier()), Ok(mode));
        for id in [
            "lifecycle-certified/v2",
            "lifecycle-certified",
            "",
            "legacy",
        ] {
            let err = RankMode::parse(id).unwrap_err();
            assert_eq!(err, RankModeError::UnknownMode(id.to_string()));
            assert_eq!(
                err.to_string(),
                format!("unknown rank mode `{id}`; known: lifecycle-certified/v1")
            );
        }
    }

    #[test]
    fn certification_roundtrips_through_json() {
        let subs = [strong("a", vec![Trace::default()])];
        let board = rank_certified(
            &subs,
            &MandateDeclarations::new(),
            &ScoreConfig::default(),
            RankMode::LifecycleCertifiedV1,
        );
        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains("\"property\":\"lifecycle_evidence_absent\""));
        let back: Vec<CompositeScore> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, board);
    }
}
