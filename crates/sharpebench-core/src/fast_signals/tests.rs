use super::*;
use crate::composite::{rank, Run};
use crate::process::Trace;

fn close(actual: f64, expected: f64, tol: f64) -> bool {
    (actual - expected).abs() <= tol
}

fn run(returns: Vec<f64>, events: Vec<ProcessEvent>, cost: f64) -> Run {
    Run {
        returns,
        trace: Trace { events },
        cost,
        ..Run::default()
    }
}

fn order(gated: bool) -> ProcessEvent {
    ProcessEvent::OrderPlaced {
        risk_gate_passed: gated,
    }
}

fn agent(id: &str, runs: Vec<Run>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn measure<'a>(report: &'a FastSignalsReport, agent: &str, id: &str) -> &'a SignalMeasure {
    report
        .agents
        .iter()
        .find(|a| a.agent_id == agent)
        .and_then(|a| a.measure(id))
        .unwrap_or_else(|| panic!("{agent} has {id}"))
}

fn no_evidence() -> BTreeMap<String, SignalEvidence> {
    BTreeMap::new()
}

/// Published Clopper-Pearson bounds at 95%: the closed forms at 0 and n
/// (`1 - 0.025^(1/n)`) and the tabulated 5 of 10 and 1 of 20.
#[test]
fn clopper_pearson_matches_published_exact_bounds() {
    let cases = [
        (0, 10, 0.0, 1.0 - 0.025_f64.powf(0.1)),
        (10, 10, 0.025_f64.powf(0.1), 1.0),
        (5, 10, 0.187_086_028, 0.812_913_972),
        (1, 20, 0.001_265_089, 0.248_732_762),
        (3, 3, 0.025_f64.powf(1.0 / 3.0), 1.0),
    ];
    for (hits, total, low, high) in cases {
        let (l, h) = clopper_pearson(hits, total, 0.95).unwrap();
        assert!(close(l, low, 1e-8), "{hits}/{total} low {l} vs {low}");
        assert!(close(h, high, 1e-8), "{hits}/{total} high {h} vs {high}");
    }
    assert_eq!(clopper_pearson(0, 0, 0.95), None);
    assert_eq!(clopper_pearson(4, 3, 0.95), None);
    // A wider level gives a wider interval.
    let (l90, h90) = clopper_pearson(5, 10, 0.90).unwrap();
    assert!(l90 > 0.187_086_028 && h90 < 0.812_913_972);
}

#[test]
fn binomial_cdf_matches_hand_counts() {
    assert!(close(binomial_cdf(0, 3, 0.5), 0.125, 1e-15));
    assert!(close(binomial_cdf(1, 3, 0.5), 0.5, 1e-15));
    assert!(close(binomial_cdf(2, 3, 0.5), 0.875, 1e-15));
    assert!(close(binomial_cdf(1, 4, 0.25), 0.738_281_25, 1e-15));
    assert!(binomial_cdf(3, 3, 0.5) <= 1.0);
}

#[test]
fn splitmix_draws_stay_in_range_and_reach_every_index() {
    let mut rng = SplitMix64(BOOTSTRAP_SEED);
    let mut seen = [0usize; 7];
    for _ in 0..7000 {
        let i = rng.below(7);
        assert!(i < 7);
        seen[i] += 1;
    }
    assert!(seen.iter().all(|&c| (800..1200).contains(&c)), "{seen:?}");
    // SplitMix64 from 0: the published first output.
    assert_eq!(SplitMix64(0).next_u64(), 0xE220_A839_7B1D_CDAF);
}

/// The bootstrap is a pure function of its input: the bits are pinned, the
/// interval brackets the mean, and a constant input has no spread.
#[test]
fn the_percentile_bootstrap_is_pinned_and_brackets_the_mean() {
    let values: Vec<f64> = (0..10).map(f64::from).collect();
    let (low, high) = percentile_bootstrap(&values, 0.95).unwrap();
    assert_eq!((low, high), percentile_bootstrap(&values, 0.95).unwrap());
    assert!(low < 4.5 && 4.5 < high, "{low} {high}");
    assert!(low > 2.0 && high < 7.0, "{low} {high}");
    assert_eq!((low.to_bits(), high.to_bits()), (PINNED_LOW, PINNED_HIGH));
    let (l90, h90) = percentile_bootstrap(&values, 0.90).unwrap();
    assert!(l90 >= low && h90 <= high && (l90 > low || h90 < high));
    assert_eq!(
        percentile_bootstrap(&[3.0, 3.0, 3.0], 0.95),
        Some((3.0, 3.0))
    );
    assert_eq!(percentile_bootstrap(&[1.0], 0.95), None);
    assert_eq!(percentile_bootstrap(&[], 0.95), None);
}

/// With values whose resampled means are all distinct, the interval is the
/// order statistics at ranks 50 and 1950 of the 2000 sorted means (0-based,
/// `q * 1999` rounded). Pinned so a shifted rank shows.
#[test]
fn the_bootstrap_bounds_are_the_pinned_order_statistics() {
    let values = [0.31, -1.2, 2.7, 0.05, 1.9, -0.4, 3.3, 0.8, -2.1, 1.17];
    let (low, high) = percentile_bootstrap(&values, 0.95).unwrap();
    assert_eq!(
        (low.to_bits(), high.to_bits()),
        (DISTINCT_LOW, DISTINCT_HIGH),
        "{low} {high}"
    );
}

const DISTINCT_LOW: u64 = 13_821_799_457_979_184_972;
const DISTINCT_HIGH: u64 = 4_610_042_204_563_397_674;

/// Bits of the 95% interval of the mean of 0..10 (about 2.7 and 6.3).
const PINNED_LOW: u64 = 4_613_262_278_296_967_578;
const PINNED_HIGH: u64 = 4_618_779_187_840_496_435;

/// Process, mandate and turnover are counted per run from the submission.
#[test]
fn process_mandate_and_turnover_are_counted_per_run() {
    let field = [agent(
        "a",
        vec![
            // clean, two orders, drawdown 0.5
            run(vec![0.1, -0.5, 0.2], vec![order(true), order(true)], 0.0),
            // block violation, one warn, one order, no drawdown
            run(
                vec![0.1, 0.1],
                vec![order(false), ProcessEvent::ConcentrationBreach],
                0.0,
            ),
            // clean, three warns, no orders
            run(
                vec![0.0, 0.01],
                vec![
                    ProcessEvent::ConcentrationBreach,
                    ProcessEvent::ConcentrationBreach,
                    ProcessEvent::TailSellingExposure { hedged: true },
                ],
                0.0,
            ),
        ],
    )];
    let mut cfg = ScoreConfig::default();
    cfg.mandate.max_run_drawdown = 0.3;
    let report = fast_signals(&field, &cfg, &no_evidence());

    let clean = measure(&report, "a", "process_clean_runs");
    assert_eq!((clean.count, clean.observations), (Some(2), 3));
    assert_eq!(clean.value, Some(2.0 / 3.0));
    assert_eq!(clean.direction, SignalDirection::HigherIsBetter);
    let interval = clean.interval.as_ref().unwrap();
    assert_eq!(interval.method, "clopper_pearson_exact");
    assert_eq!(interval.sampling_unit, "runs");
    assert_eq!(
        (interval.low, interval.high),
        clopper_pearson(2, 3, 0.95).unwrap()
    );

    let warns = measure(&report, "a", "warn_violations_per_run");
    assert_eq!(warns.value, Some(4.0 / 3.0));
    assert_eq!(warns.direction, SignalDirection::LowerIsBetter);
    assert_eq!(
        warns.interval.as_ref().unwrap().method,
        "percentile_bootstrap"
    );

    let mandate = measure(&report, "a", "mandate_compliant_runs");
    assert_eq!(mandate.count, Some(2));
    assert!(mandate.scale.contains("0.3"), "{}", mandate.scale);

    let orders = measure(&report, "a", "orders_per_run");
    assert_eq!(orders.value, Some(1.0));
    let board = rank(&field, &cfg);
    assert_eq!(orders.value, Some(board[0].turnover));

    for m in &report.agents[0].measures {
        assert!(!m.skill_evidence, "{}", m.id);
    }
    assert!(!report.rank_input && !report.skill_evidence);
    assert_eq!(report.schema, FAST_SIGNALS_VERSION);
    assert_eq!(report.agents[0].runs, 3);
}

/// A run whose drawdown equals the cap is within it: the bound is inclusive,
/// as the kernel's own mandate check is.
#[test]
fn a_run_at_the_cap_is_compliant() {
    let mut cfg = ScoreConfig::default();
    cfg.mandate.max_run_drawdown = 0.25;
    let field = [agent(
        "a",
        vec![run(vec![-0.25], vec![], 0.0), run(vec![-0.5], vec![], 0.0)],
    )];
    let report = fast_signals(&field, &cfg, &no_evidence());
    assert_eq!(
        measure(&report, "a", "mandate_compliant_runs").count,
        Some(1)
    );
}

/// The same field ranks the same with or without a report built beside it,
/// and an agent can have perfect signals and no edge.
#[test]
fn signals_are_rank_neutral_and_not_evidence_of_skill() {
    let noise: Vec<Run> = (0..5)
        .map(|k| {
            run(
                (0..40)
                    .map(|i| 0.01 * ((i + k) as f64 * 1.3).sin())
                    .collect(),
                vec![order(true)],
                1.0,
            )
        })
        .collect();
    let field = [agent("noise", noise)];
    let cfg = ScoreConfig::default();
    let before = serde_json::to_string(&rank(&field, &cfg)).unwrap();
    let report = fast_signals(&field, &cfg, &no_evidence());
    assert_eq!(serde_json::to_string(&rank(&field, &cfg)).unwrap(), before);
    let board = rank(&field, &cfg);
    assert!(!board[0].rank_eligible);
    assert_eq!(
        measure(&report, "noise", "process_clean_runs").value,
        Some(1.0)
    );
    assert_eq!(
        measure(&report, "noise", "mandate_compliant_runs").value,
        Some(1.0)
    );
    assert!(report
        .statement
        .starts_with("No measure in this report is evidence of trading skill"));
}

#[test]
fn entrant_cost_needs_every_run_and_host_cost_replaces_it() {
    let paid = agent(
        "paid",
        vec![run(vec![0.0], vec![], 2.0), run(vec![0.0], vec![], 4.0)],
    );
    let partial = agent(
        "partial",
        vec![run(vec![0.0], vec![], 2.0), run(vec![0.0], vec![], 0.0)],
    );
    let host = agent("host", vec![run(vec![0.0], vec![], 9.0)]);
    let mut evidence = BTreeMap::new();
    evidence.insert(
        "host".to_string(),
        SignalEvidence {
            host_cost: Some(HostObservedCost { usd: 3.0, runs: 4 }),
            ..SignalEvidence::default()
        },
    );
    let report = fast_signals(&[paid, partial, host], &ScoreConfig::default(), &evidence);

    let paid = measure(&report, "paid", "cost_per_run");
    assert_eq!((paid.value, paid.source), (Some(3.0), "entrant_reported"));
    assert!(paid.interval.is_some() && paid.unavailable.is_none());

    let infinite = fast_signals(
        &[agent(
            "inf",
            vec![
                run(vec![0.0], vec![], 1.0),
                run(vec![0.0], vec![], f64::INFINITY),
            ],
        )],
        &ScoreConfig::default(),
        &no_evidence(),
    );
    assert_eq!(measure(&infinite, "inf", "cost_per_run").value, None);
    let empty = fast_signals(
        &[agent("e", vec![])],
        &ScoreConfig::default(),
        &no_evidence(),
    );
    assert!(measure(&empty, "e", "cost_per_run")
        .unavailable
        .as_deref()
        .unwrap()
        .contains("every run"));

    let partial = measure(&report, "partial", "cost_per_run");
    assert_eq!(partial.value, None);
    assert!(partial
        .unavailable
        .as_deref()
        .unwrap()
        .contains("every run"));

    let host = measure(&report, "host", "cost_per_run");
    assert_eq!((host.value, host.source), (Some(0.75), "host_observed_usd"));
    assert_eq!(host.observations, 4);
    assert!(host.interval.is_none());
    assert!(host.unavailable.as_deref().unwrap().contains("no interval"));
}

#[test]
fn unusable_host_cost_is_unavailable() {
    for cost in [
        HostObservedCost { usd: 1.0, runs: 0 },
        HostObservedCost { usd: -1.0, runs: 2 },
        HostObservedCost {
            usd: f64::NAN,
            runs: 2,
        },
    ] {
        let mut evidence = BTreeMap::new();
        evidence.insert(
            "a".to_string(),
            SignalEvidence {
                host_cost: Some(cost),
                ..SignalEvidence::default()
            },
        );
        let report = fast_signals(
            &[agent("a", vec![run(vec![0.0], vec![], 1.0)])],
            &ScoreConfig::default(),
            &evidence,
        );
        let m = measure(&report, "a", "cost_per_run");
        assert_eq!((m.value, m.source), (None, "host_observed_usd"), "{cost:?}");
    }
    // A zero host-observed spend is a measured zero.
    let mut evidence = BTreeMap::new();
    evidence.insert(
        "a".to_string(),
        SignalEvidence {
            host_cost: Some(HostObservedCost { usd: 0.0, runs: 2 }),
            ..SignalEvidence::default()
        },
    );
    let report = fast_signals(&[agent("a", vec![])], &ScoreConfig::default(), &evidence);
    assert_eq!(measure(&report, "a", "cost_per_run").value, Some(0.0));
}

fn failed(reason: &str, seed: u64) -> CensusGate {
    CensusGate::TrialFailed {
        window: "0-10".into(),
        seed,
        reason: reason.into(),
    }
}

/// Agent faults count against the agent; harness failures leave the
/// denominator and are listed.
#[test]
fn agent_faults_count_and_harness_failures_are_excluded() {
    let census = AgentCensus {
        agent_id: "a".into(),
        expected: 10,
        completed: 5,
        failed: 4,
        unreported: 1,
        gates: vec![
            failed("agent_fault: decision_budget_exceeded", 1),
            failed("agent_fault: decision_budget_exceeded", 2),
            failed("agent_fault: agent_protocol_violation", 3),
            failed("runtime: timeout", 4),
            failed("refused by operator", 5),
            CensusGate::TrialUnreported {
                window: "0-10".into(),
                seed: 6,
            },
            CensusGate::TrialReportedTwice {
                window: "0-10".into(),
                seed: 7,
            },
            CensusGate::OffRosterCell {
                window: "9-19".into(),
                seed: 8,
            },
        ],
    };
    let mut evidence = BTreeMap::new();
    evidence.insert(
        "a".to_string(),
        SignalEvidence {
            census: Some(census),
            ..SignalEvidence::default()
        },
    );
    let report = fast_signals(&[agent("a", vec![])], &ScoreConfig::default(), &evidence);
    let m = measure(&report, "a", "agent_fault_cells");
    assert_eq!((m.count, m.observations), (Some(3), 8));
    assert_eq!(m.value, Some(3.0 / 8.0));
    assert_eq!(m.direction, SignalDirection::LowerIsBetter);
    let expected: BTreeMap<String, usize> = [
        ("agent_fault:decision_budget_exceeded", 2),
        ("agent_fault:agent_protocol_violation", 1),
        ("excluded:runtime:timeout", 1),
        ("excluded:failed:refused by operator", 1),
        ("excluded:unreported", 1),
        ("excluded:reported_twice", 1),
        ("excluded:off_roster", 1),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    assert_eq!(m.detail, expected);

    let none = fast_signals(
        &[agent("a", vec![])],
        &ScoreConfig::default(),
        &no_evidence(),
    );
    let m = measure(&none, "a", "agent_fault_cells");
    assert_eq!(m.value, None);
    assert!(m.unavailable.as_deref().unwrap().contains("census"));
}

#[test]
fn probe_and_stability_evidence_become_measures_or_typed_unavailability() {
    let mut evidence = BTreeMap::new();
    evidence.insert(
        "a".to_string(),
        SignalEvidence {
            contamination_edges: Some(Ok(vec![0.1, 0.3])),
            presentation: Some(Ok(vec![
                PresentationCounts {
                    variant: "reordered".into(),
                    steps: 10,
                    disagreeing_steps: 1,
                },
                PresentationCounts {
                    variant: "blinded:full".into(),
                    steps: 10,
                    disagreeing_steps: 3,
                },
            ])),
            decision_stability: Some(Ok((20, 5))),
            ..SignalEvidence::default()
        },
    );
    evidence.insert(
        "b".to_string(),
        SignalEvidence {
            contamination_edges: Some(Err("score_unavailable".into())),
            presentation: Some(Err("mask".into())),
            decision_stability: Some(Err("single_replicate".into())),
            ..SignalEvidence::default()
        },
    );
    evidence.insert("ghost".to_string(), SignalEvidence::default());
    let field = [agent("a", vec![]), agent("b", vec![]), agent("c", vec![])];
    let report = fast_signals(&field, &ScoreConfig::default(), &evidence);
    assert_eq!(report.unmatched_evidence, vec!["ghost".to_string()]);

    let edge = measure(&report, "a", "contamination_mean_edge");
    assert!(close(edge.value.unwrap(), 0.2, 1e-15));
    assert_eq!(edge.interval.as_ref().unwrap().sampling_unit, "windows");
    let shown = measure(&report, "a", "presentation_disagreement");
    assert_eq!((shown.count, shown.observations), (Some(4), 20));
    assert_eq!(shown.detail.get("blinded:full"), Some(&3));
    assert_eq!(shown.detail.get("reordered"), Some(&1));
    let stable = measure(&report, "a", "decision_disagreement");
    assert_eq!(stable.value, Some(0.25));
    assert_eq!(
        stable.interval.as_ref().unwrap().sampling_unit,
        "replicate pairs"
    );

    for (id, reason) in [
        ("contamination_mean_edge", "score_unavailable"),
        ("presentation_disagreement", "mask"),
        ("decision_disagreement", "single_replicate"),
    ] {
        let b = measure(&report, "b", id);
        assert_eq!(b.value, None, "{id}");
        assert!(b.unavailable.as_deref().unwrap().ends_with(reason), "{id}");
        let c = measure(&report, "c", id);
        assert!(c.unavailable.as_deref().unwrap().starts_with("no "), "{id}");
    }
}

/// An agent with no runs reports every submission measure unavailable rather
/// than a zero.
#[test]
fn an_agent_with_no_runs_has_no_submission_measures() {
    let report = fast_signals(
        &[agent("empty", vec![])],
        &ScoreConfig::default(),
        &no_evidence(),
    );
    for id in [
        "process_clean_runs",
        "warn_violations_per_run",
        "mandate_compliant_runs",
        "orders_per_run",
        "cost_per_run",
    ] {
        let m = measure(&report, "empty", id);
        assert_eq!((m.value, m.observations), (None, 0), "{id}");
        assert!(m.unavailable.is_some(), "{id}");
    }
    // One run: a value, but no interval.
    let report = fast_signals(
        &[agent("one", vec![run(vec![0.0], vec![order(true)], 0.0)])],
        &ScoreConfig::default(),
        &no_evidence(),
    );
    let m = measure(&report, "one", "orders_per_run");
    assert_eq!((m.value, m.interval.as_ref()), (Some(1.0), None));
    assert!(m.unavailable.as_deref().unwrap().contains("fewer than two"));
}

#[test]
fn the_report_serializes_its_contract() {
    let report = fast_signals(
        &[agent("a", vec![run(vec![0.0, 0.1], vec![], 0.0)])],
        &ScoreConfig::default(),
        &no_evidence(),
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["schema"], FAST_SIGNALS_VERSION);
    assert_eq!(json["rank_input"], false);
    assert_eq!(json["interval_level"], 0.95);
    assert_eq!(json["bootstrap"]["resamples"], BOOTSTRAP_RESAMPLES);
    let m = &json["agents"][0]["measures"][0];
    assert_eq!(m["id"], "process_clean_runs");
    assert_eq!(m["direction"], "higher_is_better");
    assert_eq!(m["skill_evidence"], false);
    assert!(m.get("detail").is_none());
    let ids: Vec<&str> = json["agents"][0]["measures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "process_clean_runs",
            "warn_violations_per_run",
            "mandate_compliant_runs",
            "orders_per_run",
            "cost_per_run",
            "agent_fault_cells",
            "contamination_mean_edge",
            "presentation_disagreement",
            "decision_disagreement",
        ]
    );
}
