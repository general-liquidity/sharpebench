//! The language-agnostic agent ⇄ harness protocol.
//!
//! Agents are **external** — a container or HTTP endpoint, in any language — not
//! Rust code. Each decision step the harness sends a [`MarketObservation`] (JSON)
//! and the agent replies with a [`Decision`] (JSON). Keeping this surface tiny and
//! stable is what lets any vendor compete (and is the whole adoption story).
//!
//! All observations are **point-in-time**: `close_history`, `fundamentals` and
//! `news` only ever contain information available at or before `date`.
//!
//! # The wire contract is closed (breaking for entrants as of 0.11.0)
//!
//! Every wire type carries `#[serde(deny_unknown_fields)]`. An agent that emits
//! a key the contract does not define is rejected at the transport boundary and
//! scored as an agent protocol fault, not silently accepted. This is a
//! deliberate departure from the additive-only discipline the rest of the
//! artifact formats follow: an attested benchmark cannot let an unread field
//! carry meaning the scorer never saw.
//!
//! The authoritative machine-readable definition of the closed contract is
//! published as JSON Schema (draft 2020-12) alongside this crate:
//! `schema/decision.schema.json` and `schema/observation.schema.json`. Both set
//! `additionalProperties: false` to mirror `deny_unknown_fields`, and a
//! bidirectional drift guard (`tests/schema_drift.rs`) fails the build if the
//! schema and the Rust types disagree in either direction.
//!
//! Entrants migrating from 0.10.x: drop any extra keys, or move them under
//! `reasoning` (free text) or `cost` (structured spend). [`decision_from_wire`]
//! produces the diagnostic that names the offending field.
//!
//! # Decisions must be deterministic under re-execution
//!
//! A run may be executed more than once: a runtime failure is retried by
//! restarting the run from its first step with a fresh agent, an interrupted
//! sweep resumes by running its unfinished runs again, and a verifier may
//! re-execute a captured trajectory to check it. Each decision must therefore
//! be a deterministic function of the observations of the same run up to and
//! including the one being answered, and of the agent's own earlier decisions
//! in that run. Behaviour that depends on anything else (wall-clock time,
//! ambient randomness, or state carried in from another run or from outside
//! the run) may diverge on re-execution, and re-execution verification refuses
//! a run that diverges.
//!
//! What must repeat is the score-bearing decision: every order's `symbol`,
//! `action`, `target_weight` and `confidence`, and the `cost` report.
//! `reasoning` and each order's `rationale` are audit text that the scorer
//! never reads, so they may differ. An agent that wants randomness must derive
//! it from the observation stream it was given.
//!
//! The harness checks this in `sharpebench_harness::verify_trajectory_reexecuted`:
//! after the strict artifact checks pass, every captured run is re-executed
//! with a fresh agent on the same frozen data, window and seed, and the first
//! decision that differs is refused as a typed `ReexecutionDivergence` naming
//! the run, the step and the observation, never accepted as a silently
//! different run. Replaying recorded decisions alone
//! (`sharpebench_harness::verify_trajectory_strict`) is exact by construction
//! and cannot see a non-deterministic agent; re-execution is the check that
//! can. A sweep's own retries and resumes do not compare a rerun against the
//! attempt it replaced, so a non-deterministic agent is caught when its
//! trajectory is re-executed, not while the sweep runs. The command line
//! exposes the check as `sharpebench verify-trajectory --reexecute`.
//!
//! # Consistency relaxations a fault plan may declare
//!
//! An operator may run an entrant under a seeded fault plan (`sharpebench run
//! --fault-plan`, `sharpebench_harness::fault_plan`). A plan must declare
//! exactly the relaxations its faults use, or it is refused, and the operator
//! publishes that declaration to the entrant before the sweep. A faulted
//! observation may violate only a declared relaxation, only for a bounded
//! number of decision steps or presentations (at most 64), and never in the
//! book: every fault changes what the entrant is shown or which of its
//! submissions is accepted, never the executed book or the returns the scorer
//! reads. The wire shape is the same under every relaxation, and without a
//! plan none of them applies. Each relaxation is named here by its wire name
//! in the plan's `declared_relaxations`:
//!
//! - `read_your_writes`: after an order executes, `cash` and `portfolio` on
//!   later observations may show their pre-execution values for a bounded
//!   number of decision steps. The order has executed and the book is
//!   authoritative; `date` and `symbols` stay current. The harness records
//!   convergence when an observation is seen to carry the canonical holdings
//!   again, not when a timer expires. Restating the last target is the
//!   idempotent response; pushing the target further in the direction of the
//!   hidden write is graded as escalation.
//! - `position_sign_convention`: a nonzero `portfolio[].shares` may be shown
//!   with the opposite sign for a bounded number of decision steps. Zero has
//!   no sign and is left alone, and the book is unchanged. The response is
//!   graded against the entrant's own last stated target for the symbol.
//! - `submission_acceptance`: a decision that carries orders may be rejected
//!   under a rate limit. Rejection is signalled by presenting the identical
//!   observation again, a bounded number of times; only the decision
//!   answering the first presentation after that deadline executes. A hold is
//!   never rejected. A deterministic entrant restates its decision and loses
//!   nothing.
//! - `complete_results`: declared for completeness only. No plan can arm it,
//!   because the observation contract has no paged read.
//!
//! The grades of the entrant's responses are rank-neutral evidence on the
//! attempt ledger. They never enter a return, a score or a rank.
//!
//! The declared metadata of each wire operation (does it change state, is it
//! safe to repeat, may the harness retry it) is published beside the schema;
//! see [`operations`].
#![forbid(unsafe_code)]

pub mod canonical;
pub mod operations;

pub use operations::{
    operation_contract_preimage, AutomaticRetries, Idempotency, Operation, OperationMetadata,
    OPERATIONS,
};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What the agent sees at one decision point.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketObservation {
    /// ISO-8601 date of the decision point.
    pub date: String,
    pub cash: f64,
    pub symbols: Vec<SymbolSnapshot>,
    pub portfolio: Vec<PositionState>,
}

/// Point-in-time data for one instrument.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolSnapshot {
    pub symbol: String,
    /// Trailing closes up to and including `date` (oldest first).
    pub close_history: Vec<f64>,
    /// Named fundamental fields (e.g. `pe`, `revenue_yoy`). Empty if unavailable.
    #[serde(default)]
    pub fundamentals: BTreeMap<String, f64>,
    /// Headlines published on or before `date`.
    #[serde(default)]
    pub news: Vec<String>,
}

/// The agent's current holding in one instrument.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionState {
    pub symbol: String,
    pub shares: f64,
    pub avg_price: f64,
}

/// What the agent returns.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub orders: Vec<Order>,
    /// Free-text rationale, captured into the trajectory for auditability.
    #[serde(default)]
    pub reasoning: String,
    /// Optional self-reported compute/token spend for producing *this* decision.
    /// The engine accumulates it into the run's `cost`, which drives the
    /// cost-normalized leaderboard columns (`return_per_cost` / `dsr_per_cost` =
    /// skill-per-dollar-of-compute). `None` = not reported, so existing agents
    /// need no change and the cost columns stay `None` (back-compat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<DecisionCost>,
}

/// An agent's self-reported spend to produce one decision. Every field defaults to
/// zero so a partial report (e.g. tokens only, no dollar figure) still deserializes.
/// The engine reduces this to a single scalar via [`DecisionCost::billable_units`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionCost {
    /// Dollar cost of the compute/tokens spent on this decision. The preferred
    /// unit for skill-per-dollar reporting.
    #[serde(default)]
    pub cost_usd: f64,
    /// Prompt/input tokens consumed.
    #[serde(default)]
    pub tokens_in: u64,
    /// Completion/output tokens produced.
    #[serde(default)]
    pub tokens_out: u64,
    /// Reasoning/thinking tokens, reported as a legibility breakdown. Providers
    /// typically already bill these inside `tokens_out`, so they are *not* re-added
    /// into the token total; they are surfaced separately, not double-counted.
    #[serde(default)]
    pub reasoning_tokens: u64,
}

impl DecisionCost {
    /// The single scalar the engine folds into `Run.cost` (any consistent unit,
    /// matching the leaderboard's cost column). Prefers the reported dollar figure;
    /// with no dollars reported it falls back to total billable tokens
    /// (`tokens_in + tokens_out`). Reasoning tokens are a sub-breakdown of the
    /// output and are not added again.
    ///
    /// The counts arrive from the entrant over an untrusted transport and no
    /// semantic rule bounds them, so the total saturates rather than adding
    /// unchecked: an unchecked sum panics the sweep where overflow checks are on
    /// and wraps toward a near-zero cost where they are off, and a wrapped cost
    /// flatters the cost-normalized columns of the agent that reported it.
    pub fn billable_units(&self) -> f64 {
        if self.cost_usd > 0.0 {
            self.cost_usd
        } else {
            self.tokens_in.saturating_add(self.tokens_out) as f64
        }
    }
}

/// A single per-instrument instruction.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Order {
    pub symbol: String,
    pub action: Action,
    /// Target portfolio weight for this symbol in [-1, 1]; negative values are shorts.
    pub target_weight: f64,
    /// Stated conviction in [0, 1]; scored for calibration.
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    /// Optional one-line rationale for *this* order, captured into the run trace
    /// (audit trail). Defaults to empty so existing agents need no change.
    #[serde(default)]
    pub rationale: String,
}

/// Discrete action label (sizing is carried by `target_weight`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Buy,
    Sell,
    Hold,
    Close,
}

fn default_confidence() -> f64 {
    0.5
}

/// Where an entrant finds the authoritative, machine-readable contract. Quoted
/// into every wire-shape diagnostic so a failing agent is one link from the fix.
pub const DECISION_SCHEMA_PATH: &str = "crates/sharpebench-protocol/schema/decision.schema.json";

/// Deserialize a [`Decision`] from the wire, turning a contract violation into a
/// diagnostic an entrant can act on.
///
/// The contract is closed ([`deny_unknown_fields`]), so the most common
/// migration failure is an extra key. `serde` already knows which key that is
/// and which keys were expected; the transports used to throw that away and
/// report only an opaque protocol fault. This function keeps it, and points at
/// the published schema.
///
/// This checks the object *shape* only. Semantic validity against the
/// observation being answered is [`Decision::validate_for`].
///
/// [`deny_unknown_fields`]: https://serde.rs/container-attrs.html#deny_unknown_fields
pub fn decision_from_wire(json: &str) -> Result<Decision, String> {
    serde_json::from_str(json).map_err(|error| {
        format!(
            "decision rejected by the closed wire contract: {error}. \
             Unknown fields are rejected rather than ignored; validate against {DECISION_SCHEMA_PATH} \
             (additionalProperties: false) and move any extra payload into `reasoning` or `cost`."
        )
    })
}

impl Decision {
    /// Validate the semantic part of the closed wire contract against the
    /// observation this decision answers.  Deserialization enforces the object
    /// shape; this method closes the gaps JSON Schema cannot express cheaply at
    /// the transport boundary: point-in-time symbol membership, one target per
    /// symbol, finite bounded weights/confidence, and nonnegative finite spend.
    ///
    /// `action` is deliberately not inferred from the target sign.  It is an
    /// audit label: selling a long can leave a positive target, and buying to
    /// cover can leave a negative one.  The signed target remains authoritative.
    pub fn validate_for(&self, observation: &MarketObservation) -> Result<(), String> {
        let offered = observation
            .symbols
            .iter()
            .map(|snapshot| snapshot.symbol.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut seen = std::collections::BTreeSet::new();
        for (index, order) in self.orders.iter().enumerate() {
            if !offered.contains(order.symbol.as_str()) {
                return Err(format!(
                    "orders[{index}].symbol {:?} was not observed",
                    order.symbol
                ));
            }
            if !seen.insert(order.symbol.as_str()) {
                return Err(format!("duplicate order for symbol {:?}", order.symbol));
            }
            if !order.target_weight.is_finite() || order.target_weight.abs() > 1.0 {
                return Err(format!(
                    "orders[{index}].target_weight must be finite and in [-1, 1]"
                ));
            }
            if !order.confidence.is_finite() || !(0.0..=1.0).contains(&order.confidence) {
                return Err(format!(
                    "orders[{index}].confidence must be finite and in [0, 1]"
                ));
            }
        }
        if let Some(cost) = self.cost {
            if !cost.cost_usd.is_finite() || cost.cost_usd < 0.0 {
                return Err("cost.cost_usd must be finite and nonnegative".to_string());
            }
        }
        Ok(())
    }
}

/// One captured decision step of a single backtest run: the agent's *raw* output
/// at one point-in-time observation. This is the persisted artifact — it holds the
/// agent's [`Decision`] (orders, sizing, conviction, reasoning) tagged with the
/// observation it was made against, and deliberately stores **no** returns, NAV, or
/// any self-reported metric. The score is recomputed by replaying these decisions
/// through the engine, never read from the agent's word.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionStep {
    /// 0-based step index within the run's window (`window.start + step` is the
    /// dataset index the observation was drawn from).
    pub step: usize,
    /// Stable id of the point-in-time observation this decision answered — the
    /// observation's ISO date. Lets a verifier confirm the decision lines up with
    /// the frozen dataset's bar at the replayed step.
    pub observation_id: String,
    /// The agent's raw decision at this step (orders + reasoning).
    pub decision: Decision,
}

/// One captured backtest run (a single window × seed): the ordered sequence of the
/// agent's raw decision steps, plus the (window, seed) coordinates needed to replay
/// it through the identical point-in-time engine path.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunTrajectory {
    /// Inclusive window start (dataset index of the first decision step).
    pub window_start: usize,
    /// Exclusive window end.
    pub window_end: usize,
    /// Execution seed the run was driven with (governs slippage noise on replay).
    pub seed: u64,
    /// The raw decisions, in step order.
    pub steps: Vec<DecisionStep>,
}

/// Identity of the execution environment that produced a raw-decision
/// trajectory. The score configuration is intentionally absent: a trajectory
/// may be regraded under a newer scorer, but it must not be replayed against
/// different market data, costs, or engine semantics while being described as
/// the original run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryWindow {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryContract {
    pub schema_version: u32,
    pub dataset_sha256: String,
    pub cost_model_sha256: String,
    pub engine_version: String,
    /// Exact market windows the capture planned, in execution order.
    #[serde(default)]
    pub windows: Vec<TrajectoryWindow>,
    /// Exact execution seeds applied to every window, in execution order.
    #[serde(default)]
    pub seeds: Vec<u64>,
    /// Exact CLI executable when capture came through the command line. Library
    /// callers may leave this absent and still bind the semantic inputs above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_artifact_sha256: Option<String>,
}

impl TrajectoryContract {
    pub const SCHEMA_VERSION: u32 = 2;
}

/// The mandate an agent declares at submission: which **reliability verdict** it
/// asks to be judged under. Opt-in and additive: a submission with no
/// declaration is scored exactly as before.
///
/// A declaration selects the per-run series and aggregation of the pass^k gate
/// and, for [`DeclaredMandate::DrawdownCapped`], adds a per-run drawdown bound.
/// It never relaxes anything: the deflated-Sharpe bar, the block bootstrap, the
/// process audit and the host's drawdown mandate are computed on the agent's raw
/// returns under every declaration, and the host board's own verdict is still
/// applied and still decides rank. The declared verdict is reported beside it,
/// labeled, so a reader sees both "meets its declared mandate" and "is not
/// all-weather" on one row. Serialized internally tagged in snake case, e.g.
/// `{"kind":"relative_to","benchmark_id":"buy-and-hold"}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeclaredMandate {
    /// Profitable in every regime: per-run PSR on raw returns, every run must
    /// pass. The verdict the benchmark applies when nothing is declared.
    AbsoluteReturn,
    /// Beats the named benchmark agent in every regime: per-run PSR on the
    /// excess return over `benchmark_id`'s run in the same (window, seed) cell,
    /// every run must pass. The benchmark must be in the field being ranked; a
    /// missing or misaligned benchmark fails every run rather than falling
    /// back to the absolute test.
    RelativeTo { benchmark_id: String },
    /// Never catastrophic in any regime: at least one run clears the per-run
    /// PSR bar, and no single run draws down more than `max_per_run_drawdown`
    /// (in `(0, 1]`; a bound outside that range is a misdeclaration and fails).
    DrawdownCapped { max_per_run_drawdown: f64 },
    /// Beats the field's same-cell buy-and-hold reference in every regime.
    /// This is an excess-return mandate, not a statement that the agent itself
    /// is long-only or beta-tracking. The former `long_only_beta` wire spelling
    /// remains accepted only for backward-compatible reads.
    #[serde(rename = "outperform_buy_and_hold", alias = "long_only_beta")]
    OutperformBuyAndHold,
}

/// An agent's full captured trajectory: every (window × seed) run's raw decisions.
/// Serde-(de)serializable to JSON; this is the on-disk artifact a separate verifier
/// ingests to recompute the score from raw decisions alone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTrajectory {
    pub agent_id: String,
    /// The data, costs, and engine that produced the decisions. Absent only on
    /// legacy or deliberately unbound artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<TrajectoryContract>,
    /// In-sample search budget the agent declared (mirrors `AgentSubmission`), so a
    /// recomputed submission carries the same deflation footprint.
    #[serde(default)]
    pub in_sample_trials: u32,
    /// The mandate the agent declared at submission (see [`DeclaredMandate`]).
    /// `None` = undeclared, the default; the artifact's bytes are unchanged for
    /// every existing trajectory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_mandate: Option<DeclaredMandate>,
    /// One captured run per (window, seed), in the same order the harness produced
    /// them (window-major: all seeds of window 0, then window 1, …).
    pub runs: Vec<RunTrajectory>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_and_decision_roundtrip() {
        let obs = MarketObservation {
            date: "2025-01-01".to_string(),
            cash: 1.0,
            symbols: vec![SymbolSnapshot {
                symbol: "A".to_string(),
                close_history: vec![1.0, 2.0],
                fundamentals: Default::default(),
                news: vec!["headline".to_string()],
            }],
            portfolio: vec![PositionState {
                symbol: "A".to_string(),
                shares: 1.0,
                avg_price: 2.0,
            }],
        };
        let back: MarketObservation =
            serde_json::from_str(&serde_json::to_string(&obs).unwrap()).unwrap();
        assert_eq!(back.symbols[0].symbol, "A");

        let d = Decision {
            orders: vec![Order {
                symbol: "A".to_string(),
                action: Action::Buy,
                target_weight: 0.5,
                confidence: 0.9,
                rationale: "trailing breakout".to_string(),
            }],
            reasoning: "r".to_string(),
            cost: None,
        };
        let db: Decision = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(db.orders[0].action, Action::Buy);
        // The per-order rationale survives the JSON round-trip into the trajectory.
        assert_eq!(db.orders[0].rationale, "trailing breakout");

        // Older agents that omit `rationale` still deserialize (default empty).
        let legacy = r#"{"orders":[{"symbol":"A","action":"buy","target_weight":0.5}]}"#;
        let parsed: Decision = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.orders[0].rationale, "");
        assert!((parsed.orders[0].confidence - 0.5).abs() < 1e-12);
        // A legacy decision omits `cost` entirely (back-compat → None).
        assert!(parsed.cost.is_none());
    }

    /// An entrant supplies its own token counts over an untrusted transport, so
    /// the reduction has to survive the extremes of the declared `u64` type. An
    /// unchecked `tokens_in + tokens_out` panics the sweep where overflow checks
    /// are on and wraps to a near-zero cost where they are off, which is the
    /// favorable direction for the cost-normalized columns.
    #[test]
    fn extreme_token_counts_saturate_instead_of_wrapping() {
        let wire = format!(
            r#"{{"orders":[],"reasoning":"","cost":{{"cost_usd":0.0,
            "tokens_in":{},"tokens_out":1,"reasoning_tokens":0}}}}"#,
            u64::MAX
        );
        let decision: Decision = serde_json::from_str(&wire).expect("extremes are valid u64 wire");
        let observation = MarketObservation {
            date: "2026-01-01".to_string(),
            cash: 0.0,
            symbols: Vec::new(),
            portfolio: Vec::new(),
        };
        assert!(
            decision.validate_for(&observation).is_ok(),
            "no semantic rule rejects extreme token counts, so the reduction must hold"
        );
        let cost = decision.cost.expect("cost channel present");
        assert_eq!(
            cost.billable_units(),
            u64::MAX as f64,
            "the token total saturates at the type maximum, never wraps toward cheap"
        );
    }

    #[test]
    fn decision_cost_channel_parses_and_reduces() {
        // An agent self-reporting spend: dollars present → billable = dollars.
        let with_cost = r#"{"orders":[],"reasoning":"","cost":{"cost_usd":0.42,
            "tokens_in":1200,"tokens_out":300,"reasoning_tokens":180}}"#;
        let d: Decision = serde_json::from_str(with_cost).unwrap();
        let c = d.cost.expect("cost channel present");
        assert!((c.cost_usd - 0.42).abs() < 1e-12);
        assert_eq!(c.tokens_in, 1200);
        assert!((c.billable_units() - 0.42).abs() < 1e-12);

        // Tokens-only report (no dollars) → billable = tokens_in + tokens_out;
        // reasoning tokens are a sub-breakdown of the output, not re-added.
        let tokens_only = DecisionCost {
            cost_usd: 0.0,
            tokens_in: 1000,
            tokens_out: 250,
            reasoning_tokens: 200,
        };
        assert!((tokens_only.billable_units() - 1250.0).abs() < 1e-12);

        // `cost` round-trips through JSON.
        let d2 = Decision {
            orders: Vec::new(),
            reasoning: String::new(),
            cost: Some(tokens_only),
        };
        let back: Decision = serde_json::from_str(&serde_json::to_string(&d2).unwrap()).unwrap();
        assert_eq!(back.cost, Some(tokens_only));
    }

    #[test]
    fn closed_decision_contract_rejects_drift_and_semantic_faults() {
        let obs = MarketObservation {
            date: "2026-01-01".to_string(),
            cash: 1.0,
            symbols: vec![SymbolSnapshot {
                symbol: "A".to_string(),
                close_history: vec![1.0],
                fundamentals: Default::default(),
                news: Vec::new(),
            }],
            portfolio: Vec::new(),
        };
        assert!(serde_json::from_str::<Decision>(r#"{"orders":[],"typo":true}"#).is_err());

        let order = |symbol: &str, weight: f64| Order {
            symbol: symbol.to_string(),
            action: Action::Sell,
            target_weight: weight,
            confidence: 0.5,
            rationale: String::new(),
        };
        let valid = Decision {
            orders: vec![order("A", -0.5)],
            reasoning: String::new(),
            cost: None,
        };
        assert!(valid.validate_for(&obs).is_ok());

        for invalid in [
            Decision {
                orders: vec![order("UNKNOWN", 0.0)],
                reasoning: String::new(),
                cost: None,
            },
            Decision {
                orders: vec![order("A", 0.1), order("A", 0.2)],
                reasoning: String::new(),
                cost: None,
            },
            Decision {
                orders: vec![order("A", 1.01)],
                reasoning: String::new(),
                cost: None,
            },
        ] {
            assert!(invalid.validate_for(&obs).is_err());
        }
    }

    #[test]
    fn unknown_field_diagnostic_names_the_offending_field() {
        let error = decision_from_wire(r#"{"orders":[],"latency_ms":12}"#)
            .expect_err("the closed contract rejects an undefined key");
        assert!(
            error.contains("latency_ms"),
            "the diagnostic must name the offending field, got: {error}"
        );
        assert!(
            error.contains("orders") && error.contains("reasoning") && error.contains("cost"),
            "the diagnostic must list the accepted fields, got: {error}"
        );
        assert!(
            error.contains(DECISION_SCHEMA_PATH),
            "the diagnostic must point at the published schema, got: {error}"
        );

        // A shape fault that is not an unknown field still gets a diagnostic
        // rather than an opaque failure.
        let malformed = decision_from_wire("not json").expect_err("malformed input is rejected");
        assert!(malformed.contains("closed wire contract"));

        // The happy path is unchanged: a conforming decision parses.
        let ok =
            decision_from_wire(r#"{"orders":[{"symbol":"A","action":"buy","target_weight":0.5}]}"#)
                .expect("a conforming decision parses");
        assert_eq!(ok.orders[0].symbol, "A");
    }

    #[test]
    fn trajectory_roundtrips_through_json() {
        let traj = AgentTrajectory {
            agent_id: "a".to_string(),
            contract: None,
            in_sample_trials: 7,
            declared_mandate: None,
            runs: vec![RunTrajectory {
                window_start: 20,
                window_end: 30,
                seed: 3,
                steps: vec![DecisionStep {
                    step: 0,
                    observation_id: "2025-001".to_string(),
                    decision: Decision {
                        orders: vec![Order {
                            symbol: "A".to_string(),
                            action: Action::Buy,
                            target_weight: 0.25,
                            confidence: 0.8,
                            rationale: String::new(),
                        }],
                        reasoning: "r".to_string(),
                        cost: None,
                    },
                }],
            }],
        };
        let back: AgentTrajectory =
            serde_json::from_str(&serde_json::to_string(&traj).unwrap()).unwrap();
        assert_eq!(back.agent_id, "a");
        assert_eq!(back.in_sample_trials, 7);
        assert_eq!(back.runs[0].seed, 3);
        assert_eq!(back.runs[0].steps[0].observation_id, "2025-001");
        assert_eq!(back.runs[0].steps[0].decision.orders[0].target_weight, 0.25);
        // An undeclared mandate is absent from the bytes, not serialized as null.
        assert!(!serde_json::to_string(&traj)
            .unwrap()
            .contains("declared_mandate"));
        assert!(back.declared_mandate.is_none());
    }

    #[test]
    fn declared_mandate_is_additive_and_round_trips() {
        // Every trajectory written before the field existed still parses.
        let legacy = r#"{"agent_id":"a","runs":[]}"#;
        let t: AgentTrajectory = serde_json::from_str(legacy).unwrap();
        assert!(t.declared_mandate.is_none());

        for (m, json) in [
            (
                DeclaredMandate::AbsoluteReturn,
                r#"{"kind":"absolute_return"}"#,
            ),
            (
                DeclaredMandate::RelativeTo {
                    benchmark_id: "buy-and-hold".to_string(),
                },
                r#"{"kind":"relative_to","benchmark_id":"buy-and-hold"}"#,
            ),
            (
                DeclaredMandate::DrawdownCapped {
                    max_per_run_drawdown: 0.2,
                },
                r#"{"kind":"drawdown_capped","max_per_run_drawdown":0.2}"#,
            ),
            (
                DeclaredMandate::OutperformBuyAndHold,
                r#"{"kind":"outperform_buy_and_hold"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&m).unwrap(), json);
            assert_eq!(serde_json::from_str::<DeclaredMandate>(json).unwrap(), m);
        }

        let declared = AgentTrajectory {
            agent_id: "a".to_string(),
            contract: None,
            in_sample_trials: 0,
            declared_mandate: Some(DeclaredMandate::OutperformBuyAndHold),
            runs: Vec::new(),
        };
        let back: AgentTrajectory =
            serde_json::from_str(&serde_json::to_string(&declared).unwrap()).unwrap();
        assert_eq!(
            back.declared_mandate,
            Some(DeclaredMandate::OutperformBuyAndHold)
        );
        assert_eq!(
            serde_json::from_str::<DeclaredMandate>(r#"{"kind":"long_only_beta"}"#).unwrap(),
            DeclaredMandate::OutperformBuyAndHold,
            "old artifacts remain readable but are re-emitted under the honest name"
        );
    }
}
