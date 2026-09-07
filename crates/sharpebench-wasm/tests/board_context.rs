use sharpebench_core::{
    classify_disqualification, parse_declared_field, rank_declared, score_agent, DisqualThresholds,
    ScoreConfig,
};
use sharpebench_wasm::{classify_disqualification_json, score_json};

fn field() -> serde_json::Value {
    let track = |mean: f64| {
        (0..60)
            .map(|i| mean + 0.001 * (i as f64 * 0.7).sin())
            .collect::<Vec<_>>()
    };
    serde_json::json!([
        {"agent_id":"candidate", "runs":[{"returns":track(0.01)}], "declared_mandate":{"kind":"relative_to", "benchmark_id":"reference"}},
        {"agent_id":"reference", "runs":[{"returns":track(0.02)}]}
    ])
}

#[test]
fn declared_verdict_survives_the_board_wire_boundary_without_changing_host_rank() {
    let raw = field().to_string();
    let (subs, declarations) = parse_declared_field(&raw).unwrap();
    let native = rank_declared(&subs, &declarations, &ScoreConfig::default());
    assert_eq!(
        score_json(&raw, "").unwrap(),
        serde_json::to_string(&native).unwrap()
    );
    let candidate = native.iter().find(|s| s.agent_id == "candidate").unwrap();
    assert!(candidate.passed_k);
    assert_eq!(candidate.declared_passed_k, Some(false));
    let undeclared = sharpebench_core::rank(&subs, &ScoreConfig::default());
    assert_eq!(
        native
            .iter()
            .map(|s| (&s.agent_id, s.rank_eligible, s.rank_ordinal))
            .collect::<Vec<_>>(),
        undeclared
            .iter()
            .map(|s| (&s.agent_id, s.rank_eligible, s.rank_ordinal))
            .collect::<Vec<_>>()
    );
}

#[test]
fn explanations_are_classified_from_the_ranked_field_not_individual_rescores() {
    let raw = field().to_string();
    let (subs, declarations) = parse_declared_field(&raw).unwrap();
    for cfg in [
        ScoreConfig {
            pass_mode: sharpebench_core::pass_k::PassMode::RelativeToBenchmark,
            benchmark_agent_id: "candidate".into(),
            ..ScoreConfig::default()
        },
        ScoreConfig {
            n_trials: 1,
            min_field_for_measured_sr_std: 2,
            ..ScoreConfig::default()
        },
    ] {
        let board = rank_declared(&subs, &declarations, &cfg);
        let explanations: Vec<serde_json::Value> = serde_json::from_str(
            &classify_disqualification_json(&raw, &serde_json::to_string(&cfg).unwrap()).unwrap(),
        )
        .unwrap();
        let thresholds = DisqualThresholds::from_score_config(&cfg);
        for (score, explanation) in board.iter().zip(explanations) {
            assert_eq!(explanation["agent_id"], score.agent_id);
            assert_eq!(explanation["rank_eligible"], score.rank_eligible);
            assert_eq!(
                explanation["reasons"],
                serde_json::to_value(classify_disqualification(score, &thresholds, None, None))
                    .unwrap()
            );
        }
        if cfg.pass_mode == sharpebench_core::pass_k::PassMode::RelativeToBenchmark {
            assert!(subs.iter().all(|s| !score_agent(s, &cfg).passed_k));
            assert!(
                board
                    .iter()
                    .find(|s| s.agent_id == "reference")
                    .unwrap()
                    .passed_k
            );
        }
    }
}

#[test]
fn malformed_declarations_and_duplicate_or_blank_ids_are_refused() {
    for raw in [
        r#"[{"agent_id":"a","runs":[],"declared_mandate":{"kind":"relative_typo"}}]"#,
        r#"[{"agent_id":"a","runs":[]},{"agent_id":"a","runs":[]}]"#,
        r#"[{"agent_id":"  ","runs":[]}]"#,
    ] {
        assert!(score_json(raw, "").is_err());
        assert!(classify_disqualification_json(raw, "").is_err());
    }
}
