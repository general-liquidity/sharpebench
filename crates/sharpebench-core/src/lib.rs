//! # sb-core — the SharpeBench scoring kernel
//!
//! A pure, deterministic library that turns a set of agent **trajectories**
//! (per-seed × per-window return series + decision traces) into a **luck-robust,
//! risk-adjusted** score and leaderboard ranking.
//!
//! Design invariants — these make a score reproducible for a pinned release and input:
//! - **Pure.** No I/O, no system clock, no ambient randomness. Any randomness
//!   (the significance bootstrap) takes an explicit seed argument.
//! - **Deterministic.** Plain `f64` math, fixed reduction order, no parallel
//!   float sums. CI pins two committed golden fields on Linux, macOS, and
//!   Windows; that check does not cover every input or platform.
//! - **No `unsafe`.**
//!
//! The headline idea: an agent does **not** rank on raw return. It ranks only if
//! its edge survives (a) deflation for the number of agents tested
//! ([`deflated_sharpe`]), (b) reliability across every seed×window
//! ([`mod@pass_k`]), and (c) decision-process discipline ([`process`]). Raw return is
//! reported but never the rank key — see [`composite`].
#![forbid(unsafe_code)]

// The pure statistics core lives in `sharpebench-stats`; re-export the modules so
// every existing path (`crate::stats::…`, `sharpebench_core::deflated_sharpe::…`,
// `…::significance::*`, `…::selection::*`) keeps resolving byte-for-byte.
pub use sharpebench_stats::{deflated_sharpe, selection, significance, stats, stylized_facts};

// The dataset-realism validator (Cont's stylized facts) — certifies that a frozen
// or synthetic dataset actually behaves like a market before any score is trusted.
// The measurement fn stays reachable as `stylized_facts::stylized_facts`.
pub use sharpebench_stats::{
    validate_dataset, validate_dataset_with, RealismFailure, RealismThresholds, RealismVerdict,
    StylizedFactsReport,
};

pub mod allocation;
pub mod attribution;
pub mod briefing;
pub mod budget_curve;
pub mod calibration;
pub mod candidate_lineage;
pub mod comparison_sets;
pub mod composite;
pub mod correlation;
pub mod decay;
pub mod disqualification;
pub mod econrationality;
pub mod entrants;
pub mod evidence_coverage;
pub mod forecast;
pub mod greeks;
pub mod oos;
pub mod pass_k;
pub mod percentile;
pub mod process;
pub mod rediscovery;
pub mod regime_compare;
pub mod roles;
pub mod rolling;
pub mod selfaudit;

pub use allocation::{
    check_weights, score_allocation, turnover, AllocationPolicy, AllocationReport, AllocationStep,
    AllocationTrajectory, WeightValidity, WeightViolation,
};
pub use briefing::{
    audit_briefing, Briefing, BriefingAudit, BriefingPolicy, BriefingSection, BriefingViolation,
};
pub use budget_curve::{budget_curve, BudgetCurveOpts, BudgetCurveReport, BudgetPoint};
pub use candidate_lineage::{
    verify_candidate_lineage, CandidateAncestry, CandidateFamilyRobustness, CandidateLineageError,
    CandidateLineageLedger, CandidateLineageReport, CandidateLineageScore,
};
pub use comparison_sets::{
    comparison_set, qualifies, restrict_field, restrict_to_shared, ComparisonSet, TaggedRun,
    TaggedSubmission,
};
pub use composite::{
    per_period_sr_std, per_run_passes, per_run_psr_benchmark, rank, rank_declared, score_agent,
    score_agent_declared, split_declarations, AgentSubmission, CompositeScore, DeclaredMandate,
    DeclaredSubmission, Mandate, MandateDeclarations, MandateVerdict, Run, ScoreConfig,
    TrialsSrStdSource,
};
pub use correlation::{crowdedness, Crowdedness};
pub use disqualification::{classify_disqualification, rollup, DisqualThresholds, FailReason};
pub use econrationality::{
    assess_rationality, elicit_revealed_selection, DominanceChoice, EconRationalityReport,
};
pub use entrants::{
    atr_breakout, bounce_counter, distribution_days, follow_through_day, max_exposure_timeout,
    regime_rsi, signal_gate, AtrBreakoutParams, AtrBreakoutSignal, AtrBreakoutVerdict,
    BounceParams, BounceState, DistributionCount, DistributionParams, DistributionSeverity,
    ExposureDecision, ExposureState, FollowThroughParams, FollowThroughPhase, FollowThroughSignal,
    GateDecision, GateState, GateStatus, IndexBar, RegimeRsiParams, RegimeRsiSignal, RsiBands,
    Side, TrendRegime,
};
pub use evidence_coverage::{
    Coverage, DigestId, EvidenceInventory, InventoryAudit, PreimageError, RunProvenance,
    COMPOSITE_SCORE_INVENTORY, REDACTED, RUN_PROVENANCE_INVENTORY,
};
pub use forecast::{
    analyze_forecast_quality, parse_forecast_evidence, AgentForecastSummary, BinaryCalibration,
    CalibrationBin, CommonSupport, ConfidenceCalibration, DistributionCalibration,
    ForecastAnalysisConfig, ForecastError, ForecastEvidence, ForecastQualityReport, MetricMean,
    PairwiseForecastComparison,
};
pub use greeks::{
    bs_greeks, bs_price, classify_greeks_risk, portfolio_greeks, Greeks, GreeksPolicy, GreeksRisk,
    Leg,
};
pub use oos::{oos_decay, OosDecayReport};
pub use pass_k::{pass_k, PassMode};
pub use percentile::{percentile_of, BaselineBand, HumanBaseline};
pub use process::{
    check_lifecycle, process_score_with_ordering, LifecycleReport, LifecycleStep, OrderId,
    OrderingViolation, Phase, PhaseKind, ProcessEvent, ProcessScore, Subject, Trace,
};
pub use rediscovery::{
    classify_rediscovery, clone_clusters, cosine_similarity, RediscoveryVerdict,
    CLONE_COLLAPSE_COSINE, DEFAULT_REDISCOVERY_THRESHOLD,
};
pub use regime_compare::{
    compare_by_regime, RegimeCompareOpts, RegimeComparison, RegimeDistributionReport, ZagaSplit,
};
pub use roles::{
    attribute_behavior_roles, attribute_roles, elicit_behavior_roles, RoleContribution,
};
pub use rolling::{rolling_sharpe, RollingSharpe};
pub use selection::{selection_robustness, SelectionRobustness};
pub use selfaudit::{run_self_audit, SelfAuditReport};
