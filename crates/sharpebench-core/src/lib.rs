//! # sb-core — the SharpeBench scoring kernel
//!
//! A pure, deterministic library that turns a set of agent **trajectories**
//! (per-seed × per-window return series + decision traces) into a **luck-robust,
//! risk-adjusted** score and leaderboard ranking.
//!
//! Design invariants — these are what make a SharpeBench score reproducible forever:
//! - **Pure.** No I/O, no system clock, no ambient randomness. Any randomness
//!   (the significance bootstrap) takes an explicit seed argument.
//! - **Deterministic.** Plain `f64` math, fixed reduction order, no parallel
//!   float sums. The same input yields byte-identical output on any platform.
//! - **No `unsafe`.**
//!
//! The headline idea: an agent does **not** rank on raw return. It ranks only if
//! its edge survives (a) deflation for the number of agents tested
//! ([`deflated_sharpe`]), (b) reliability across every seed×window
//! ([`pass_k`]), and (c) decision-process discipline ([`process`]). Raw return is
//! reported but never the rank key — see [`composite`].
#![forbid(unsafe_code)]

pub mod allocation;
pub mod attribution;
pub mod briefing;
pub mod calibration;
pub mod comparison_sets;
pub mod composite;
pub mod correlation;
pub mod decay;
pub mod deflated_sharpe;
pub mod econrationality;
pub mod greeks;
pub mod oos;
pub mod pass_k;
pub mod percentile;
pub mod process;
pub mod rediscovery;
pub mod roles;
pub mod rolling;
pub mod selection;
pub mod selfaudit;
pub mod significance;
pub mod stats;

pub use allocation::{
    check_weights, score_allocation, turnover, AllocationPolicy, AllocationReport, AllocationStep,
    AllocationTrajectory, WeightValidity, WeightViolation,
};
pub use briefing::{
    audit_briefing, Briefing, BriefingAudit, BriefingPolicy, BriefingSection, BriefingViolation,
};
pub use comparison_sets::{
    comparison_set, qualifies, restrict_field, restrict_to_shared, ComparisonSet, TaggedRun,
    TaggedSubmission,
};
pub use composite::{rank, score_agent, AgentSubmission, CompositeScore, Run, ScoreConfig};
pub use correlation::{crowdedness, Crowdedness};
pub use econrationality::{assess_rationality, DominanceChoice, EconRationalityReport};
pub use greeks::{
    bs_greeks, bs_price, classify_greeks_risk, portfolio_greeks, Greeks, GreeksPolicy, GreeksRisk,
    Leg,
};
pub use oos::{oos_decay, OosDecayReport};
pub use percentile::percentile_of;
pub use process::{ProcessEvent, ProcessScore, Trace};
pub use rediscovery::{
    classify_rediscovery, cosine_similarity, RediscoveryVerdict, DEFAULT_REDISCOVERY_THRESHOLD,
};
pub use rolling::{rolling_sharpe, RollingSharpe};
pub use selection::{selection_robustness, SelectionRobustness};
pub use selfaudit::{run_self_audit, SelfAuditReport};
