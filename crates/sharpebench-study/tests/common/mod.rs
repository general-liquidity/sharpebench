//! A protocol that validates, for the refusal tests to break one field at a
//! time. Built in code rather than read from the shipped example so that a
//! change to the example cannot silently turn a refusal test into a no-op.

use sharpebench_study::protocol::*;

/// Planned runs for the fixture. Sized above the requirement that
/// `required_half_width` and `anticipated_rate` imply; `refusals.rs` asserts
/// the boundary rather than trusting this number.
pub const FIXTURE_PLANNED_RUNS: u64 = 500;

pub fn valid_protocol() -> StudyProtocol {
    StudyProtocol {
        contract_version: STUDY_CONTRACT_VERSION.to_string(),
        protocol_id: "fixture-eligibility-gate".to_string(),
        version: ProtocolVersion {
            major: 1,
            minor: 0,
            patch: 0,
        },
        tier: RunTier::DevelopmentCalibration,
        frozen: false,
        identity: MethodIdentity {
            method: "fixture-gate".to_string(),
            method_version: "1.2.3".to_string(),
            configuration_id: "fixture-default".to_string(),
            configuration_digest: "a".repeat(64),
            scoring_spec_hash: "b".repeat(16),
        },
        claims: vec![
            Claim {
                id: "C1".to_string(),
                statement: "The per-entry false-positive rate stays at or below its limit."
                    .to_string(),
                estimand: "per_entry".to_string(),
                decision_rule: DecisionRule::IntervalUpperBoundAtMost { limit: 0.1 },
            },
            Claim {
                id: "C2".to_string(),
                statement: "Power at the planted effect reaches its bound.".to_string(),
                estimand: "power".to_string(),
                decision_rule: DecisionRule::IntervalLowerBoundAtLeast { bound: 0.5 },
            },
        ],
        estimands: vec![
            Estimand {
                name: "per_entry".to_string(),
                kind: EstimandKind::PerEntryFalsePositive,
                effect_units: EffectUnits::Raw,
                target: EstimandTarget::UpperErrorLimit { limit: 0.05 },
            },
            Estimand {
                name: "whole_field".to_string(),
                kind: EstimandKind::WholeFieldAnyFalseEligibility,
                effect_units: EffectUnits::Raw,
                target: EstimandTarget::UpperErrorLimit { limit: 0.1 },
            },
            Estimand {
                name: "power".to_string(),
                kind: EstimandKind::PowerAtEffect {
                    effect_name: "planted-sharpe-increment".to_string(),
                    effect_size: 0.5,
                },
                effect_units: EffectUnits::Raw,
                target: EstimandTarget::LowerPowerBound { bound: 0.6 },
            },
        ],
        design: Design {
            null_family: "Zero-skill entrants from the simulator's null generator.".to_string(),
            alternative_family: "Planted-skill entrants above and below the benchmark.".to_string(),
            effect_units: EffectUnits::Raw,
            field_composition: FieldComposition {
                null_entrants: 40,
                planted_skill_entrants: 8,
                control_entrants: 2,
            },
            dependence: DependenceStructure {
                serial: "Stationary block dependence.".to_string(),
                cross_entrant: "Correlated through the shared history.".to_string(),
                shared_market_history: true,
            },
            window_geometry: WindowGeometry {
                windows: 12,
                window_length_steps: 252,
                stride_steps: 21,
                overlapping: true,
            },
            search_assumptions: SearchAssumptions {
                trials_searched: 100,
                selection_rule: "Best in-sample Sharpe over the searched trials.".to_string(),
                independent_trials_assumed: false,
            },
            dispersion_policy: DispersionPolicy::Measured {
                estimator: "Cross-entrant Sharpe dispersion.".to_string(),
            },
            tuning_allowed: true,
        },
        replication: Replication {
            independent_unit: ReplicationUnit::SimulatedField,
            nested_observations: vec![
                ReplicationUnit::Entrant,
                ReplicationUnit::Window,
                ReplicationUnit::Seed,
            ],
            independent_units_planned: FIXTURE_PLANNED_RUNS,
        },
        inference: Inference {
            confidence_level: ConfidenceLevel::NinetyFive,
            interval_method: IntervalMethod::WilsonScore,
            required_half_width: 0.02,
            multiplicity: Multiplicity {
                method: MultiplicityMethod::HolmBonferroni,
                reported_conditions: vec![
                    "per-entry error".to_string(),
                    "power at the planted effect".to_string(),
                ],
            },
        },
        accounting: Accounting {
            expected_attempted: 500,
            expected_completed: 490,
            expected_available: 480,
            refusal: FailurePolicy::ExcludeWithReasonCode {
                reason_code: "entrant_refused".to_string(),
                denominator: Denominator::Attempted,
            },
            unavailability: FailurePolicy::ExcludeWithReasonCode {
                reason_code: "result_unavailable".to_string(),
                denominator: Denominator::Completed,
            },
            infrastructure_failure: FailurePolicy::RetryThenExclude {
                max_attempts: 3,
                reason_code: "infrastructure_failure".to_string(),
                denominator: Denominator::Attempted,
            },
        },
        simulation: SimulationPlan {
            planned_runs: FIXTURE_PLANNED_RUNS,
            anticipated_rate: 0.05,
            runtime_estimate_seconds_per_run: 12.0,
        },
        budget: Budget {
            approved: true,
            approver: "study-owner".to_string(),
            spend_cap_usd: 50.0,
            compute_cap_core_hours: 8.0,
            estimated_spend_usd: 0.0,
            estimated_core_hours: 1.67,
        },
        stopping_rule: StoppingRule::FixedRuns {
            runs: FIXTURE_PLANNED_RUNS,
        },
        notes: "Test fixture.".to_string(),
    }
}
