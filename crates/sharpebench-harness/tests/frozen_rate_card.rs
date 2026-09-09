use sharpebench_harness::accounting::{
    summarize_usage, AttemptUsage, RateCard, MAX_RATE_CARD_BYTES,
};
use sharpebench_harness::{
    failing_sentinel_run, run_external_backtest_observed, run_resumable_sweep_observed,
    run_with_observed_retries, AttemptObservation, FailureKind, ResumePolicy, SweepCheckpoint,
    SweepContract, SweepIdentity,
};
use sharpebench_protocol::{Decision, DecisionCost, MarketObservation};
use sharpebench_sim::{Agent, CostModel, Dataset, TransportDiagnostics, TransportHealth, Window};

fn wire() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "sharpebench.token-rate-card.v1",
        "provider": "fixture", "model": "model-a", "revision": "test-1",
        "input_usd_nanos_per_token": 125, "output_usd_nanos_per_token": 500,
    })
}

fn card() -> RateCard {
    serde_json::from_value(wire()).unwrap()
}

fn usage() -> AttemptUsage {
    AttemptUsage {
        input_tokens: 3,
        output_tokens: 5,
        observed_decisions: 1,
        ..AttemptUsage::new(card())
    }
}

#[test]
fn card_contract_rejects_missing_unknown_noninteger_and_invalid_identity_fields() {
    let original = wire();
    for key in original.as_object().unwrap().keys() {
        let mut invalid = original.clone();
        invalid.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<RateCard>(invalid).is_err(),
            "missing {key}"
        );
    }
    for (key, value) in [
        ("schema_version", serde_json::json!("v2")),
        ("provider", serde_json::json!("")),
        ("model", serde_json::json!(" model-a")),
        ("revision", serde_json::json!("test\n1")),
        ("model", serde_json::json!("x".repeat(257))),
        ("input_usd_nanos_per_token", serde_json::json!(-1)),
        ("input_usd_nanos_per_token", serde_json::json!(0.25)),
        ("output_usd_nanos_per_token", serde_json::json!(null)),
        ("usd_per_token", serde_json::json!(1)),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        assert!(
            serde_json::from_value::<RateCard>(invalid).is_err(),
            "invalid {key}"
        );
    }
    for number in ["1e999", "NaN", "Infinity", "18446744073709551616"] {
        let raw = serde_json::to_string(&original)
            .unwrap()
            .replace(":125", &format!(":{number}"));
        assert!(
            RateCard::from_json(raw.as_bytes()).is_err(),
            "invalid numeric token {number}"
        );
    }
    let duplicate =
        serde_json::to_string(&original)
            .unwrap()
            .replacen('{', "{\"model\":\"other\",", 1);
    assert!(RateCard::from_json(duplicate.as_bytes()).is_err());
    let mut oversized = serde_json::to_vec(&original).unwrap();
    oversized.resize(MAX_RATE_CARD_BYTES + 1, b' ');
    assert!(RateCard::from_json(&oversized).is_err());
}

#[test]
fn identity_binds_all_rates_and_model_fields_but_not_json_formatting() {
    let original = card();
    let pretty = serde_json::to_vec_pretty(&wire()).unwrap();
    assert_eq!(
        RateCard::from_json(&pretty).unwrap().digest(),
        original.digest()
    );
    for key in [
        "provider",
        "model",
        "revision",
        "input_usd_nanos_per_token",
        "output_usd_nanos_per_token",
    ] {
        let mut changed = wire();
        changed[key] = if key.ends_with("per_token") {
            serde_json::json!(126)
        } else {
            serde_json::json!("different")
        };
        let changed: RateCard = serde_json::from_value(changed).unwrap();
        assert_ne!(changed.digest(), original.digest(), "{key} must be frozen");
    }
    assert_eq!(original.quote_nanos(3, 5), Some(2875));
    assert_eq!(original.quote_nanos(5, 3), Some(2125));
    let value = serde_json::to_value(summarize_usage([Some(&usage())])).unwrap();
    assert_eq!(value["status"], "estimated");
    assert_eq!(value["usage_source"], "entrant_reported");
    assert_eq!(value["usd_nanos"], "2875");
}

#[test]
fn unavailable_and_mixed_usage_never_become_a_complete_total() {
    let known = usage();
    let missing = serde_json::to_value(summarize_usage([Some(&known), None])).unwrap();
    assert_eq!(missing["status"], "unavailable");
    assert_eq!(missing["reason"], "incomplete_usage_evidence");
    assert_eq!(missing["known_subtotal_usd_nanos"], "2875");
    assert!(missing.get("usd_nanos").is_none());
    let empty = serde_json::to_value(summarize_usage([])).unwrap();
    assert_eq!(empty["reason"], "attempt_ledger_has_no_usage_evidence");
    assert!(empty.get("known_subtotal_usd_nanos").is_none());
    let mut invalid = known.clone();
    invalid.unpriced_decisions = 1;
    assert_eq!(
        serde_json::to_value(summarize_usage([Some(&invalid)])).unwrap()["status"],
        "unavailable"
    );
    invalid.unpriced_decisions = 0;
    invalid.observed_decisions = 0;
    assert_eq!(
        serde_json::to_value(summarize_usage([Some(&invalid)])).unwrap()["status"],
        "unavailable"
    );
    let mut changed = wire();
    changed["revision"] = serde_json::json!("new");
    let mixed = AttemptUsage {
        rate_card: serde_json::from_value(changed).unwrap(),
        ..known.clone()
    };
    let mixed = serde_json::to_value(summarize_usage([Some(&known), Some(&mixed)])).unwrap();
    assert_eq!(mixed["reason"], "mixed_rate_card_identities");
    assert!(mixed.get("known_subtotal_usd_nanos").is_none());
}

#[test]
fn exact_integer_quotes_refuse_overflow_and_allow_explicit_free_rates() {
    let mut large = wire();
    large["input_usd_nanos_per_token"] = serde_json::json!(u64::MAX);
    large["output_usd_nanos_per_token"] = serde_json::json!(u64::MAX);
    let large: RateCard = serde_json::from_value(large).unwrap();
    assert_eq!(large.quote_nanos(u64::MAX, u64::MAX), None);
    let one = AttemptUsage {
        rate_card: large,
        input_tokens: u64::MAX,
        output_tokens: 0,
        ..usage()
    };
    let overflow = serde_json::to_value(summarize_usage([Some(&one), Some(&one)])).unwrap();
    assert_eq!(overflow["reason"], "monetary_arithmetic_overflow");
    assert!(overflow.get("known_subtotal_usd_nanos").is_none());
    let mut free = wire();
    free["input_usd_nanos_per_token"] = serde_json::json!(0);
    free["output_usd_nanos_per_token"] = serde_json::json!(0);
    let free = AttemptUsage {
        rate_card: serde_json::from_value(free).unwrap(),
        ..usage()
    };
    let priced = serde_json::to_value(summarize_usage([Some(&free)])).unwrap();
    assert_eq!(priced["status"], "estimated");
    assert_eq!(priced["usd_nanos"], "0");
}

struct Fixture {
    cost: Option<DecisionCost>,
    health: TransportHealth,
}
impl Agent for Fixture {
    fn decide(&mut self, _: &MarketObservation) -> Decision {
        Decision {
            orders: vec![],
            reasoning: String::new(),
            cost: self.cost,
        }
    }
}
impl TransportDiagnostics for Fixture {
    fn health(&self) -> &TransportHealth {
        &self.health
    }
}

#[test]
fn actual_backtest_observer_keeps_decisions_and_scores_unchanged_and_does_not_trust_dollars() {
    let data = Dataset::synthetic(1, 10, 7);
    let window = Window { start: 2, end: 5 };
    let cost = DecisionCost {
        cost_usd: 999.0,
        tokens_in: 3,
        tokens_out: 5,
        reasoning_tokens: 4,
    };
    let drive = |cost, card: Option<&RateCard>| {
        let mut agent = Fixture {
            cost,
            health: TransportHealth::default(),
        };
        run_external_backtest_observed(&data, &mut agent, window, 7, CostModel::default(), card)
    };
    let plain = drive(Some(cost), None);
    let observed = drive(Some(cost), Some(&card()));
    assert_eq!(
        serde_json::to_value(plain.result.unwrap()).unwrap(),
        serde_json::to_value(observed.result.unwrap()).unwrap()
    );
    let usage = observed.usage.unwrap();
    assert_eq!(
        (
            usage.input_tokens,
            usage.output_tokens,
            usage.observed_decisions
        ),
        (9, 15, 3)
    );
    assert_eq!(
        serde_json::to_value(summarize_usage([Some(&usage)])).unwrap()["usd_nanos"],
        "8625"
    );
    for cost in [
        None,
        Some(DecisionCost::default()),
        Some(DecisionCost {
            reasoning_tokens: 6,
            ..cost
        }),
    ] {
        let usage = drive(cost, Some(&card())).usage.unwrap();
        assert!(!usage.complete);
        assert_eq!(usage.unpriced_decisions, 3);
    }
    let usage = drive(
        Some(DecisionCost {
            tokens_in: u64::MAX,
            ..cost
        }),
        Some(&card()),
    )
    .usage
    .unwrap();
    assert!(
        !usage.complete,
        "token count overflow must not be a complete estimate"
    );
}

#[test]
fn failed_attempt_usage_survives_retries_and_remains_a_partial_subtotal() {
    let mut calls = 0;
    let driven = run_with_observed_retries(2, || {
        calls += 1;
        AttemptObservation {
            result: if calls == 3 {
                Ok(failing_sentinel_run(3))
            } else {
                Err(FailureKind::TransportError)
            },
            usage: Some(usage()),
        }
    });
    assert_eq!(driven.ledger.len(), 3);
    assert!(!driven.ledger.attempts[0].usage.as_ref().unwrap().complete);
    let cost = serde_json::to_value(driven.ledger.monetary_summary()).unwrap();
    assert_eq!(cost["known_subtotal_usd_nanos"], "8625");
    assert_eq!(cost["status"], "unavailable");
}

#[test]
fn bound_checkpoint_recovery_keeps_usage_and_refuses_changed_rate_identity() {
    let path = std::env::temp_dir().join(format!(
        "sharpe-rate-checkpoint-{}.json",
        std::process::id()
    ));
    let windows = [Window { start: 0, end: 3 }];
    let contract = SweepContract::new(
        SweepIdentity {
            dataset_sha256: "aa".repeat(32),
            cost_model_sha256: "bb".repeat(32),
            score_config_sha256: "cc".repeat(32),
            runner_artifact_sha256: "dd".repeat(32),
            entrant_sha256: "ee".repeat(32),
            invocation_sha256: card().digest(),
        },
        &windows,
        &[7],
        0,
    );
    let first = run_resumable_sweep_observed(
        &path,
        "fixture",
        &contract,
        &windows,
        ResumePolicy::UnfinishedOnly,
        |_, _| AttemptObservation {
            result: Err(FailureKind::Timeout),
            usage: Some(usage()),
        },
    )
    .unwrap();
    assert_eq!(first.attempts.attempts, 1);
    let before = std::fs::read(&path).unwrap();
    let mut other = contract.clone();
    other.invocation_sha256 = "ff".repeat(32);
    assert!(run_resumable_sweep_observed(
        &path,
        "fixture",
        &other,
        &windows,
        ResumePolicy::RetryRuntimeFailures,
        |_, _| panic!("changed rate must refuse before executing")
    )
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let resumed = run_resumable_sweep_observed(
        &path,
        "fixture",
        &contract,
        &windows,
        ResumePolicy::RetryRuntimeFailures,
        |_, _| AttemptObservation {
            result: Ok(failing_sentinel_run(3)),
            usage: Some(usage()),
        },
    )
    .unwrap();
    let persisted = SweepCheckpoint::load(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(persisted.tasks[0].attempts.len(), 2);
    assert_eq!(resumed.attempts.attempts, 2);
    let subtotal = serde_json::to_value(resumed.monetary_cost).unwrap();
    assert_eq!(subtotal["status"], "unavailable");
    assert_eq!(subtotal["known_subtotal_usd_nanos"], "5750");
}
