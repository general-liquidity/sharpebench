use sharpebench_memory::transition::{
    observe_stage, scenario_transition_report, CarryoverMode, EffectiveDate, FactLog, FactRef,
    FactVersion, Invariant, NamedInvariant, PortfolioState, PreservationViolation,
    ScenarioManifest, StageDecl, StageFailure, StageRecord, TransitionDecl, TransitionError,
};
use sharpebench_memory::SessionScores;
use std::collections::{BTreeMap, BTreeSet};

const D1: &str = "2026-01-05";
const D2: &str = "2026-02-02";

fn date(text: &str) -> EffectiveDate {
    EffectiveDate::parse(text).unwrap()
}

fn book(cash: f64, positions: &[(&str, f64)]) -> PortfolioState {
    PortfolioState::new(
        cash,
        positions
            .iter()
            .map(|&(name, notional)| (name.to_string(), notional))
            .collect(),
    )
    .unwrap()
}

fn stage(id: u64, effective: &str, initial: Option<PortfolioState>) -> StageDecl {
    StageDecl {
        stage_id: id,
        effective_date: date(effective),
        initial_portfolio: initial,
        fact_refs: Vec::new(),
        invariants: Vec::new(),
    }
}

fn transition(from: u64, to: u64, mode: CarryoverMode) -> TransitionDecl {
    TransitionDecl {
        from,
        to,
        carryover_mode: Some(mode),
        allowed_memory: BTreeSet::from(["notes".to_string()]),
        preserve: BTreeSet::new(),
    }
}

fn fact(name: &str, available_from: &str) -> FactRef {
    FactRef {
        name: name.to_string(),
        available_from: date(available_from),
    }
}

fn version(name: &str, available_from: &str, value: &str) -> FactVersion {
    FactVersion {
        name: name.to_string(),
        available_from: date(available_from),
        value: value.to_string(),
    }
}

fn exposure_cap(name: &str, max: f64) -> NamedInvariant {
    NamedInvariant {
        name: name.to_string(),
        invariant: Invariant::GrossExposureCap {
            max_gross_exposure: max,
        },
    }
}

/// Session 2 depends on session 1. Both have positive paired lift.
fn two_session_dag() -> Vec<SessionScores> {
    vec![
        SessionScores::new(1, vec![0.0, 0.0], vec![0.4, 0.4], vec![]),
        SessionScores::new(2, vec![0.0, 0.0], vec![0.6, 0.6], vec![1]),
    ]
}

/// Fresh stage 2 with its own initial portfolio, or continuous without one.
fn two_stage_manifest(mode: CarryoverMode) -> ScenarioManifest {
    let initial_2 = match mode {
        CarryoverMode::FreshEpisodeWithMemory => Some(book(100.0, &[])),
        CarryoverMode::ContinuousPortfolio => None,
    };
    ScenarioManifest::declare(
        vec![
            stage(1, D1, Some(book(100.0, &[]))),
            stage(2, D2, initial_2),
        ],
        vec![transition(1, 2, mode)],
    )
    .unwrap()
}

fn records(stage_2_closing: PortfolioState, stage_2_note: &str) -> Vec<StageRecord> {
    let mut first = StageRecord::new(1, book(40.0, &[("BTC", 70.0)]));
    first
        .memory_written
        .insert("notes".to_string(), "funding flipped".to_string());
    first
        .memory_written
        .insert("scratch".to_string(), "not for export".to_string());
    first.facts_used.push(fact("btc_close", D1));
    let mut second = StageRecord::new(2, stage_2_closing);
    second.memory_read.insert("notes".to_string());
    second
        .memory_written
        .insert("notes".to_string(), stage_2_note.to_string());
    second.facts_used.push(fact("btc_close", D2));
    vec![first, second]
}

fn facts(stage_2_close: &str) -> FactLog {
    FactLog::new(vec![
        version("btc_close", D1, "94000"),
        version("btc_close", D2, stage_2_close),
        version("cpi_print", D2, "3.1"),
    ])
    .unwrap()
}

fn observation_bytes(
    manifest: &ScenarioManifest,
    records: &[StageRecord],
    facts: &FactLog,
    stage: u64,
) -> Vec<u8> {
    serde_json::to_vec(&observe_stage(manifest, records, facts, stage).unwrap()).unwrap()
}

#[test]
fn a_future_stage_fact_cannot_change_an_earlier_observation() {
    let manifest = two_stage_manifest(CarryoverMode::ContinuousPortfolio);
    let before_records = records(book(30.0, &[("BTC", 90.0)]), "v1");
    let before_facts = facts("98000");
    let stage_1_before = observation_bytes(&manifest, &before_records, &before_facts, 1);
    let stage_2_before = observation_bytes(&manifest, &before_records, &before_facts, 2);

    // Change only stage-2 facts (a revision of a name stage 1 also sees) and the
    // stage-2 record, then recompute.
    let after_records = records(book(-500.0, &[("ETH", 9.0)]), "v2");
    let after_facts = facts("12345");
    let stage_1_after = observation_bytes(&manifest, &after_records, &after_facts, 1);
    let stage_2_after = observation_bytes(&manifest, &after_records, &after_facts, 2);

    assert_eq!(
        String::from_utf8(stage_1_before.clone()).unwrap(),
        String::from_utf8(stage_1_after.clone()).unwrap(),
        "a stage-2 fact or record leaked into the stage-1 observation"
    );
    assert_eq!(stage_1_before, stage_1_after);
    // Sensitivity control: the edit is visible where it is in scope, so identity
    // above is not produced by an observation that ignores facts altogether.
    assert_ne!(stage_2_before, stage_2_after);
    let stage_1 = observe_stage(&manifest, &before_records, &before_facts, 1).unwrap();
    assert_eq!(stage_1.facts, vec![version("btc_close", D1, "94000")]);
}

#[test]
fn an_earlier_safety_failure_stays_visible_after_a_later_success() {
    let mut stages = vec![
        stage(1, D1, Some(book(100.0, &[]))),
        stage(2, D2, Some(book(100.0, &[]))),
    ];
    stages[0].invariants.push(exposure_cap("gross_cap", 50.0));
    let manifest = ScenarioManifest::declare(
        stages,
        vec![transition(1, 2, CarryoverMode::FreshEpisodeWithMemory)],
    )
    .unwrap();
    // Stage 1 closes 70 gross over a 50 cap. Stage 2 is clean and earns credit.
    let mut recs = records(book(90.0, &[("BTC", 20.0)]), "ok");
    recs[1].facts_used.clear();
    let report =
        scenario_transition_report(&two_session_dag(), &manifest, &recs, &facts("98000"), 0.05)
            .unwrap();

    let (first, second) = (&report.stages[0], &report.stages[1]);
    assert_eq!(
        first.own_failures,
        vec![StageFailure::InvariantViolated {
            name: "gross_cap".to_string()
        }]
    );
    assert!(!first.clean);
    assert!(second.clean);
    assert!(second.qualified_retention && second.conditioned_lift > 0.0);
    assert_eq!(report.failed_stages, vec![1]);
    assert!(!report.scenario_clean);
}

#[test]
fn a_later_preservation_breach_is_charged_to_the_later_stage_only() {
    let mut stages = vec![stage(1, D1, Some(book(100.0, &[]))), stage(2, D2, None)];
    stages[0].invariants.push(exposure_cap("gross_cap", 100.0));
    let mut edge = transition(1, 2, CarryoverMode::ContinuousPortfolio);
    edge.preserve.insert("gross_cap".to_string());
    let manifest = ScenarioManifest::declare(stages, vec![edge]).unwrap();
    // Stage 1 closes 70 gross (within cap); stage 2 closes 150 gross.
    let recs = records(book(0.0, &[("BTC", 150.0)]), "ok");
    let report =
        scenario_transition_report(&two_session_dag(), &manifest, &recs, &facts("98000"), 0.05)
            .unwrap();

    let (first, second) = (&report.stages[0], &report.stages[1]);
    assert!(first.clean, "the earlier stage's own record was rewritten");
    assert!(first.own_failures.is_empty() && first.preservation_violations.is_empty());
    assert_eq!(
        second.preservation_violations,
        vec![PreservationViolation {
            declared_by: 1,
            obligation: "gross_cap".to_string()
        }]
    );
    assert!(second.own_failures.is_empty());
    assert_eq!(report.failed_stages, vec![2]);
}

#[test]
fn both_failures_remain_when_earlier_and_later_stages_fail() {
    let mut stages = vec![stage(1, D1, Some(book(100.0, &[]))), stage(2, D2, None)];
    stages[0].invariants.push(exposure_cap("gross_cap", 60.0));
    let mut edge = transition(1, 2, CarryoverMode::ContinuousPortfolio);
    edge.preserve.insert("gross_cap".to_string());
    let manifest = ScenarioManifest::declare(stages, vec![edge]).unwrap();
    let recs = records(book(0.0, &[("BTC", 150.0)]), "ok");
    let report =
        scenario_transition_report(&two_session_dag(), &manifest, &recs, &facts("98000"), 0.05)
            .unwrap();
    assert_eq!(report.failed_stages, vec![1, 2]);
    assert!(report.stages[0].preservation_violations.is_empty());
    assert!(report.stages[1].own_failures.is_empty());
}

#[test]
fn fresh_and_continuous_modes_score_the_same_dag_differently() {
    let facts = facts("98000");
    let recs = records(book(30.0, &[("BTC", 90.0)]), "ok");

    let fresh = scenario_transition_report(
        &two_session_dag(),
        &two_stage_manifest(CarryoverMode::FreshEpisodeWithMemory),
        &recs,
        &facts,
        0.05,
    )
    .unwrap();
    let continuous = scenario_transition_report(
        &two_session_dag(),
        &two_stage_manifest(CarryoverMode::ContinuousPortfolio),
        &recs,
        &facts,
        0.05,
    )
    .unwrap();

    let fresh_2 = &fresh.stages[1].observation;
    let continuous_2 = &continuous.stages[1].observation;
    // Fresh resets to the declared 100 cash; continuous opens on stage 1's close.
    assert_eq!(fresh_2.opening_portfolio, book(100.0, &[]));
    assert_eq!(continuous_2.opening_portfolio, book(40.0, &[("BTC", 70.0)]));
    assert_eq!(fresh.stages[1].stage_pnl, 20.0);
    assert_eq!(continuous.stages[1].stage_pnl, 10.0);
    assert_eq!(
        fresh_2.incoming[0].mode,
        CarryoverMode::FreshEpisodeWithMemory
    );
    assert_eq!(
        continuous_2.incoming[0].mode,
        CarryoverMode::ContinuousPortfolio
    );
    // Memory crosses only through the allowed set in both modes.
    for observation in [fresh_2, continuous_2] {
        let names: Vec<_> = observation.carried_memory.iter().map(|a| &a.name).collect();
        assert_eq!(names, ["notes"]);
        assert_eq!(observation.carried_memory[0].content, "funding flipped");
    }
    assert_eq!(fresh.chain, continuous.chain);
}

#[test]
fn a_valid_manifest_scores_successfully() {
    let manifest = two_stage_manifest(CarryoverMode::ContinuousPortfolio);
    let report = scenario_transition_report(
        &two_session_dag(),
        &manifest,
        &records(book(30.0, &[("BTC", 90.0)]), "ok"),
        &facts("98000"),
        0.05,
    )
    .unwrap();
    assert!(report.scenario_clean);
    assert!(report.failed_stages.is_empty());
    let ids: Vec<_> = report.stages.iter().map(|s| s.stage_id).collect();
    assert_eq!(ids, [1, 2]);
    assert_eq!(report.stages[0].stage_pnl, 10.0);
    assert_eq!(report.chain.dependency_satisfaction_rate, 1.0);
    assert!((report.stages[1].conditioned_lift - 0.6).abs() < 1e-12);
}

// Validation: each refusal names its own cause.

#[test]
fn non_increasing_dates_along_an_edge_are_refused() {
    for later in [D1, "2025-12-31"] {
        let error = ScenarioManifest::declare(
            vec![
                stage(1, D1, Some(book(100.0, &[]))),
                stage(2, later, Some(book(100.0, &[]))),
            ],
            vec![transition(1, 2, CarryoverMode::FreshEpisodeWithMemory)],
        )
        .unwrap_err();
        assert_eq!(
            error,
            TransitionError::NonIncreasingEffectiveDate {
                from: 1,
                to: 2,
                from_date: date(D1),
                to_date: date(later),
            }
        );
    }
}

#[test]
fn a_stage_referencing_a_later_fact_is_refused() {
    let mut first = stage(1, D1, Some(book(100.0, &[])));
    first.fact_refs.push(fact("cpi_print", D2));
    let error = ScenarioManifest::declare(
        vec![first, stage(2, D2, None)],
        vec![transition(1, 2, CarryoverMode::ContinuousPortfolio)],
    )
    .unwrap_err();
    assert_eq!(
        error,
        TransitionError::FutureFactReference {
            stage: 1,
            fact: fact("cpi_print", D2),
            effective_date: date(D1),
        }
    );
}

#[test]
fn reading_an_artifact_outside_the_allowed_set_is_refused() {
    let manifest = two_stage_manifest(CarryoverMode::FreshEpisodeWithMemory);
    let mut recs = records(book(30.0, &[("BTC", 90.0)]), "ok");
    // Stage 1 wrote "scratch", but the edge allows only "notes".
    recs[1].memory_read.insert("scratch".to_string());
    let error =
        scenario_transition_report(&two_session_dag(), &manifest, &recs, &facts("98000"), 0.05)
            .unwrap_err();
    assert_eq!(
        error,
        TransitionError::DisallowedCarryover {
            stage: 2,
            artifact: "scratch".to_string(),
        }
    );
}

#[test]
fn reading_an_allowed_artifact_nobody_wrote_is_refused() {
    let manifest = two_stage_manifest(CarryoverMode::FreshEpisodeWithMemory);
    let mut recs = records(book(30.0, &[("BTC", 90.0)]), "ok");
    recs[0].memory_written.remove("notes");
    let error =
        scenario_transition_report(&two_session_dag(), &manifest, &recs, &facts("98000"), 0.05)
            .unwrap_err();
    assert_eq!(
        error,
        TransitionError::UnproducedCarryover {
            stage: 2,
            artifact: "notes".to_string(),
        }
    );
}

#[test]
fn a_manifest_stage_outside_the_dag_is_refused() {
    let manifest = ScenarioManifest::declare(
        vec![
            stage(1, D1, Some(book(100.0, &[]))),
            stage(2, D2, None),
            stage(7, D2, Some(book(1.0, &[]))),
        ],
        vec![transition(1, 2, CarryoverMode::ContinuousPortfolio)],
    )
    .unwrap();
    let mut recs = records(book(30.0, &[("BTC", 90.0)]), "ok");
    recs.push(StageRecord::new(7, book(1.0, &[])));
    let error =
        scenario_transition_report(&two_session_dag(), &manifest, &recs, &facts("98000"), 0.05)
            .unwrap_err();
    assert_eq!(error, TransitionError::StageNotInDag { stage: 7 });
}

#[test]
fn a_dag_session_without_a_stage_is_refused() {
    let manifest = two_stage_manifest(CarryoverMode::ContinuousPortfolio);
    let mut sessions = two_session_dag();
    sessions.push(SessionScores::new(3, vec![0.0], vec![0.1], vec![]));
    let recs = records(book(30.0, &[("BTC", 90.0)]), "ok");
    let error =
        scenario_transition_report(&sessions, &manifest, &recs, &facts("98000"), 0.05).unwrap_err();
    assert_eq!(error, TransitionError::SessionWithoutStage { session: 3 });
}

#[test]
fn a_transition_that_is_not_a_dag_edge_is_refused() {
    let manifest = two_stage_manifest(CarryoverMode::ContinuousPortfolio);
    let sessions = vec![
        SessionScores::new(1, vec![0.0], vec![0.4], vec![]),
        SessionScores::new(2, vec![0.0], vec![0.6], vec![]),
    ];
    let recs = records(book(30.0, &[("BTC", 90.0)]), "ok");
    let error =
        scenario_transition_report(&sessions, &manifest, &recs, &facts("98000"), 0.05).unwrap_err();
    assert_eq!(
        error,
        TransitionError::TransitionNotInDag { from: 1, to: 2 }
    );
}

#[test]
fn a_dag_edge_without_a_transition_is_refused() {
    let manifest = ScenarioManifest::declare(
        vec![
            stage(1, D1, Some(book(100.0, &[]))),
            stage(2, D2, Some(book(100.0, &[]))),
        ],
        vec![],
    )
    .unwrap();
    let mut recs = records(book(30.0, &[("BTC", 90.0)]), "ok");
    recs[1].memory_read.clear();
    let error =
        scenario_transition_report(&two_session_dag(), &manifest, &recs, &facts("98000"), 0.05)
            .unwrap_err();
    assert_eq!(error, TransitionError::MissingTransition { from: 1, to: 2 });
}

#[test]
fn an_undeclared_carryover_mode_is_refused() {
    let mut edge = transition(1, 2, CarryoverMode::ContinuousPortfolio);
    edge.carryover_mode = None;
    let error = ScenarioManifest::declare(
        vec![
            stage(1, D1, Some(book(100.0, &[]))),
            stage(2, D2, Some(book(100.0, &[]))),
        ],
        vec![edge],
    )
    .unwrap_err();
    assert_eq!(
        error,
        TransitionError::UndeclaredCarryoverMode { from: 1, to: 2 }
    );
}

#[test]
fn portfolio_entry_rules_are_refused_by_their_own_variant() {
    let fresh_without_initial = ScenarioManifest::declare(
        vec![stage(1, D1, Some(book(100.0, &[]))), stage(2, D2, None)],
        vec![transition(1, 2, CarryoverMode::FreshEpisodeWithMemory)],
    );
    assert_eq!(
        fresh_without_initial.unwrap_err(),
        TransitionError::MissingInitialPortfolio { stage: 2 }
    );
    let continuous_with_initial = ScenarioManifest::declare(
        vec![
            stage(1, D1, Some(book(100.0, &[]))),
            stage(2, D2, Some(book(100.0, &[]))),
        ],
        vec![transition(1, 2, CarryoverMode::ContinuousPortfolio)],
    );
    assert_eq!(
        continuous_with_initial.unwrap_err(),
        TransitionError::UnexpectedInitialPortfolio { stage: 2 }
    );
    let two_continuous = ScenarioManifest::declare(
        vec![
            stage(1, D1, Some(book(100.0, &[]))),
            stage(2, "2026-01-20", Some(book(100.0, &[]))),
            stage(3, D2, None),
        ],
        vec![
            transition(1, 3, CarryoverMode::ContinuousPortfolio),
            transition(2, 3, CarryoverMode::ContinuousPortfolio),
        ],
    );
    assert_eq!(
        two_continuous.unwrap_err(),
        TransitionError::MultipleContinuousPredecessors { stage: 3 }
    );
}

#[test]
fn declaration_bookkeeping_errors_are_refused_by_their_own_variant() {
    let root = || stage(1, D1, Some(book(100.0, &[])));
    let fresh = || transition(1, 2, CarryoverMode::FreshEpisodeWithMemory);
    let second = || stage(2, D2, Some(book(100.0, &[])));

    assert_eq!(
        ScenarioManifest::declare(vec![root(), root()], vec![]).unwrap_err(),
        TransitionError::DuplicateStage { stage: 1 }
    );
    assert_eq!(
        ScenarioManifest::declare(vec![root(), second()], vec![fresh(), fresh()]).unwrap_err(),
        TransitionError::DuplicateTransition { from: 1, to: 2 }
    );
    assert_eq!(
        ScenarioManifest::declare(vec![root()], vec![fresh()]).unwrap_err(),
        TransitionError::UnknownStage { stage: 2 }
    );
    let mut preserving = fresh();
    preserving.preserve.insert("gross_cap".to_string());
    assert_eq!(
        ScenarioManifest::declare(vec![root(), second()], vec![preserving]).unwrap_err(),
        TransitionError::UnknownObligation {
            from: 1,
            to: 2,
            name: "gross_cap".to_string()
        }
    );
    let mut doubled = root();
    doubled.invariants = vec![exposure_cap("cap", 1.0), exposure_cap("cap", 2.0)];
    assert_eq!(
        ScenarioManifest::declare(vec![doubled], vec![]).unwrap_err(),
        TransitionError::DuplicateInvariant {
            stage: 1,
            name: "cap".to_string()
        }
    );
    for bad in [-1.0, f64::NAN, f64::INFINITY] {
        let mut capped = root();
        capped.invariants = vec![exposure_cap("cap", bad)];
        assert_eq!(
            ScenarioManifest::declare(vec![capped], vec![]).unwrap_err(),
            TransitionError::InvalidInvariant {
                stage: 1,
                name: "cap".to_string()
            }
        );
    }
}

#[test]
fn record_and_fact_errors_are_refused_by_their_own_variant() {
    let manifest = two_stage_manifest(CarryoverMode::ContinuousPortfolio);
    let dag = two_session_dag();
    let good = || records(book(30.0, &[("BTC", 90.0)]), "ok");
    let log = facts("98000");

    let mut missing = good();
    missing.pop();
    assert_eq!(
        scenario_transition_report(&dag, &manifest, &missing, &log, 0.05).unwrap_err(),
        TransitionError::MissingStageRecord { stage: 2 }
    );
    let mut duplicated = good();
    duplicated.push(duplicated[0].clone());
    assert_eq!(
        scenario_transition_report(&dag, &manifest, &duplicated, &log, 0.05).unwrap_err(),
        TransitionError::DuplicateStageRecord { stage: 1 }
    );
    let mut stray = good();
    stray.push(StageRecord::new(9, book(0.0, &[])));
    assert_eq!(
        scenario_transition_report(&dag, &manifest, &stray, &log, 0.05).unwrap_err(),
        TransitionError::UnknownStageRecord { stage: 9 }
    );
    let mut unknown_use = good();
    unknown_use[1].facts_used.push(fact("gdp", D1));
    assert_eq!(
        scenario_transition_report(&dag, &manifest, &unknown_use, &log, 0.05).unwrap_err(),
        TransitionError::UnknownFactReference {
            stage: 2,
            fact: fact("gdp", D1)
        }
    );
    assert_eq!(
        FactLog::new(vec![version("x", D1, "1"), version("x", D1, "2")]).unwrap_err(),
        TransitionError::DuplicateFactVersion {
            fact: fact("x", D1)
        }
    );
    assert!(matches!(
        scenario_transition_report(&dag, &manifest, &good(), &log, 1.5).unwrap_err(),
        TransitionError::Chain(_)
    ));
    assert_eq!(
        PortfolioState::new(f64::NAN, BTreeMap::new()).unwrap_err(),
        TransitionError::NonFinitePortfolio
    );
    for text in [
        "2026-02-30",
        "2026-13-01",
        "0000-01-01",
        "2026-1-05",
        "2026/01/05",
    ] {
        assert_eq!(
            EffectiveDate::parse(text).unwrap_err(),
            TransitionError::InvalidDate {
                text: text.to_string()
            }
        );
    }
    assert!(EffectiveDate::parse("2024-02-29").is_ok());
}

#[test]
fn a_declared_fact_reference_missing_from_the_log_is_refused() {
    let mut first = stage(1, D1, Some(book(100.0, &[])));
    first.fact_refs.push(fact("btc_close", "2026-01-01"));
    let manifest = ScenarioManifest::declare(
        vec![first, stage(2, D2, None)],
        vec![transition(1, 2, CarryoverMode::ContinuousPortfolio)],
    )
    .unwrap();
    let recs = records(book(30.0, &[("BTC", 90.0)]), "ok");
    assert_eq!(
        observe_stage(&manifest, &recs, &facts("98000"), 1).unwrap_err(),
        TransitionError::UnknownFactReference {
            stage: 1,
            fact: fact("btc_close", "2026-01-01")
        }
    );
}

#[test]
fn lookahead_use_and_point_in_time_obligations_are_scored_not_hidden() {
    let mut stages = vec![stage(1, D1, Some(book(100.0, &[]))), stage(2, D2, None)];
    stages[0].invariants.push(NamedInvariant {
        name: "january_embargo".to_string(),
        invariant: Invariant::PointInTime { cutoff: date(D1) },
    });
    let mut edge = transition(1, 2, CarryoverMode::ContinuousPortfolio);
    edge.preserve.insert("january_embargo".to_string());
    let manifest = ScenarioManifest::declare(stages, vec![edge]).unwrap();
    let mut recs = records(book(30.0, &[("BTC", 90.0)]), "ok");
    recs[0].facts_used.push(fact("cpi_print", D2));
    let report =
        scenario_transition_report(&two_session_dag(), &manifest, &recs, &facts("98000"), 0.05)
            .unwrap();
    assert_eq!(
        report.stages[0].own_failures,
        vec![
            StageFailure::LookaheadUse {
                fact: fact("cpi_print", D2)
            },
            StageFailure::InvariantViolated {
                name: "january_embargo".to_string()
            },
        ]
    );
    // Stage 2 legitimately used a D2 fact, which breaks stage 1's embargo.
    assert_eq!(
        report.stages[1].preservation_violations,
        vec![PreservationViolation {
            declared_by: 1,
            obligation: "january_embargo".to_string()
        }]
    );
    assert!(report.stages[1].own_failures.is_empty());
    let mut reported = records(book(30.0, &[("BTC", 90.0)]), "ok");
    reported[1]
        .reported_safety_failures
        .push("constitution:max_leverage".to_string());
    let manifest = two_stage_manifest(CarryoverMode::ContinuousPortfolio);
    let report = scenario_transition_report(
        &two_session_dag(),
        &manifest,
        &reported,
        &facts("98000"),
        0.05,
    )
    .unwrap();
    assert_eq!(
        report.stages[1].own_failures,
        vec![StageFailure::Reported {
            label: "constitution:max_leverage".to_string()
        }]
    );
}

// Wire format: every manifest goes through `declare`.

#[test]
fn manifest_json_round_trips_through_the_validating_constructor() {
    let mut stages = vec![
        stage(1, D1, Some(book(100.0, &[("BTC", 5.0)]))),
        stage(2, D2, None),
    ];
    stages[0].invariants.push(exposure_cap("gross_cap", 100.0));
    stages[1].fact_refs.push(fact("cpi_print", D2));
    let mut edge = transition(1, 2, CarryoverMode::ContinuousPortfolio);
    edge.preserve.insert("gross_cap".to_string());
    let manifest = ScenarioManifest::declare(stages, vec![edge]).unwrap();

    let json = serde_json::to_string(&manifest).unwrap();
    assert!(json.contains(r#""carryover_mode":"continuous_portfolio""#));
    assert!(json.contains(r#""effective_date":"2026-01-05""#));
    let back: ScenarioManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(back, manifest);
    assert_eq!(serde_json::to_string(&back).unwrap(), json);
}

fn wire(first_date: &str, second_date: &str, mode: &str) -> String {
    format!(
        r#"{{
          "stages": [
            {{"stage_id": 1, "effective_date": "{first_date}", "initial_portfolio": {{"cash": 100.0}}}},
            {{"stage_id": 2, "effective_date": "{second_date}", "initial_portfolio": {{"cash": 100.0}}}}
          ],
          "transitions": [{{"from": 1, "to": 2 {mode} }}]
        }}"#
    )
}

#[test]
fn json_manifests_that_violate_the_contract_fail_to_deserialize() {
    let mode = r#", "carryover_mode": "fresh_episode_with_memory""#;
    assert!(serde_json::from_str::<ScenarioManifest>(&wire(D1, D2, mode)).is_ok());

    let non_increasing = serde_json::from_str::<ScenarioManifest>(&wire(D2, D1, mode))
        .unwrap_err()
        .to_string();
    let expected = TransitionError::NonIncreasingEffectiveDate {
        from: 1,
        to: 2,
        from_date: date(D2),
        to_date: date(D1),
    }
    .to_string();
    assert!(non_increasing.contains(&expected), "{non_increasing}");

    let undeclared = serde_json::from_str::<ScenarioManifest>(&wire(D1, D2, ""))
        .unwrap_err()
        .to_string();
    let expected = TransitionError::UndeclaredCarryoverMode { from: 1, to: 2 }.to_string();
    assert!(undeclared.contains(&expected), "{undeclared}");

    let bad_date = serde_json::from_str::<ScenarioManifest>(&wire("2026-02-30", D2, mode))
        .unwrap_err()
        .to_string();
    assert!(bad_date.contains("invalid effective date"), "{bad_date}");

    let typo = wire(D1, D2, r#", "carryover_mod": "continuous_portfolio""#);
    let typo = serde_json::from_str::<ScenarioManifest>(&typo)
        .unwrap_err()
        .to_string();
    assert!(typo.contains("unknown field"), "{typo}");

    let bad_book = r#"{"cash": 1.0, "positions": {"BTC": 1e308, "ETH": 1e308}}"#;
    let bad_book = serde_json::from_str::<PortfolioState>(bad_book)
        .unwrap_err()
        .to_string();
    assert!(
        bad_book.contains(&TransitionError::NonFinitePortfolio.to_string()),
        "{bad_book}"
    );
}
