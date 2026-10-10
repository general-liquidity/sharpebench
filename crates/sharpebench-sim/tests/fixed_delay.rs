//! Submission timing is separate from execution noise and survives replay/forks.
use sharpebench_core::ProcessEvent;
use sharpebench_protocol::{Action, Decision, DecisionCost, MarketObservation, Order};
use sharpebench_sim::{
    replay_run, run_backtest_capture, Agent, CostModel, CostProfile, Dataset, EnvState,
    ExecutionNoise, FixedDecisionDelay, TradingEnv, Window,
};

fn data() -> Dataset {
    Dataset::from_csv("date,symbol,close\nt0,A,1\nt1,A,2\nt2,A,4\nt3,A,8\nt4,A,16\nt5,A,32\n")
        .unwrap()
}

fn costs(bars: usize) -> CostModel {
    CostModel {
        fixed_delay: Some(FixedDecisionDelay::Fifo { bars }),
        ..CostProfile::None.resolve().costs
    }
}

fn decision(target: Option<f64>) -> Decision {
    Decision {
        orders: target
            .map(|target_weight| Order {
                symbol: "A".into(),
                action: Action::Buy,
                target_weight,
                confidence: Some(0.75),
                rationale: "submitted target".into(),
            })
            .into_iter()
            .collect(),
        reasoning: "test".into(),
        cost: Some(DecisionCost {
            cost_usd: 1.0,
            ..DecisionCost::default()
        }),
    }
}

struct Sequence(std::vec::IntoIter<Decision>);
impl Agent for Sequence {
    fn decide(&mut self, _: &MarketObservation) -> Decision {
        self.0.next().unwrap_or_else(|| decision(None))
    }
}

#[test]
fn diagnostic_lag_is_additional_to_the_bound_execution_delay() {
    use sharpebench_protocol::AgentTrajectory;
    use sharpebench_sim::replay_nulls::{lagged_replay, LaggedRun};
    let prices = Dataset::from_csv(
        "date,symbol,close\nt0,A,1\nt1,A,2\nt2,A,4\nt3,A,8\nt4,A,9\nt5,A,13\nt6,A,21\nt7,A,17\n",
    )
    .unwrap();
    let (_, captured) = run_backtest_capture(
        &prices,
        &mut Sequence(vec![decision(Some(1.0))].into_iter()),
        Window { start: 0, end: 8 },
        7,
        costs(2),
    );
    let trajectory = AgentTrajectory {
        agent_id: "test".into(),
        contract: None,
        in_sample_trials: 0,
        declared_mandate: None,
        runs: vec![captured],
    };
    let report = lagged_replay(&prices, &trajectory, costs(2), &[0, 2]).unwrap();
    let LaggedRun::Available {
        skipped_leading_bars,
        compared_bars,
        undelayed,
        lagged,
        ..
    } = &report.runs[0]
    else {
        panic!("the varied held return stream must be available");
    };
    assert_eq!(
        *skipped_leading_bars, 5,
        "fixed two plus diagnostic two fills at t4, whose trade bar is excluded"
    );
    assert_eq!(*compared_bars, 3);
    assert_eq!(*undelayed, lagged[0], "lag zero retains the bound timing");
    assert!(
        (undelayed.mean_return - lagged[1].mean_return).abs() < 1e-15,
        "both rows hold the same fully invested book over the compared bars"
    );
}

#[test]
fn fifo_uses_eligibility_prices_and_does_not_replace_waiting_decisions() {
    let mut env = TradingEnv::new(data(), Window { start: 0, end: 6 }, costs(2), 7);
    env.reset();
    let a = env.step(decision(Some(1.0)));
    let b = env.step(decision(Some(0.0)));
    assert_eq!(a.observation.portfolio[0].shares, 0.0);
    assert_eq!(b.observation.portfolio[0].shares, 0.0);
    assert!(a.info.events.is_empty() && b.info.events.is_empty());
    let c = env.step(decision(None));
    assert_eq!(
        c.observation.portfolio[0].shares, 0.25,
        "buy uses t2 price 4, not submission price 1"
    );
    let d = env.step(decision(None));
    assert_eq!(
        d.observation.portfolio[0].shares, 0.0,
        "close follows buy in FIFO order"
    );
    assert_eq!(d.observation.cash, 2.0);
    assert_eq!(d.reward, 1.0);
    assert_eq!(env.step(decision(None)).reward, 0.0);
}

#[test]
fn raw_capture_replay_and_env_agree_with_nonzero_window_start_and_tail() {
    let window = Window { start: 1, end: 6 };
    let submissions = vec![
        decision(Some(1.0)),
        decision(Some(0.0)),
        decision(None),
        decision(Some(1.0)),
        decision(Some(-1.0)),
    ];
    let (run, trajectory) = run_backtest_capture(
        &data(),
        &mut Sequence(submissions.clone().into_iter()),
        window,
        11,
        costs(2),
    );
    assert_eq!(
        trajectory.steps.len(),
        5,
        "unfilled tail decisions are still captured"
    );
    assert_eq!(
        run.cost, 5.0,
        "compute is billed at submission, including tail"
    );
    assert_eq!(run.returns, vec![0.0, 0.0, 0.0, 1.0, 0.0]);
    assert_eq!(
        run.trace
            .events
            .iter()
            .filter(|e| matches!(e, ProcessEvent::OrderPlaced { .. }))
            .count(),
        2
    );
    assert_eq!(
        serde_json::to_string(&run).unwrap(),
        serde_json::to_string(&replay_run(&data(), &trajectory, costs(2))).unwrap()
    );
    let mut env = TradingEnv::new(data(), window, costs(2), 11);
    env.reset();
    let mut rewards = Vec::new();
    let mut events = Vec::new();
    for submission in submissions {
        let out = env.step(submission);
        rewards.push(out.reward);
        events.extend(out.info.events);
    }
    assert_eq!(rewards, run.returns);
    assert_eq!(events, run.trace.events);
}

#[test]
fn queued_decisions_survive_json_checkpoint_and_reset_clears_them() {
    let mut env = TradingEnv::new(data(), Window { start: 0, end: 6 }, costs(2), 7);
    env.reset();
    env.step(decision(Some(1.0)));
    let json = serde_json::to_string(&env.clone_state()).unwrap();
    assert!(json.contains("delayed_decisions"));
    let saved: EnvState = serde_json::from_str(&json).unwrap();
    env.step(decision(Some(0.0)));
    let first = env.step(decision(None));
    env.restore_state(saved);
    env.step(decision(Some(0.0)));
    let second = env.step(decision(None));
    assert_eq!(
        serde_json::to_string(&first.observation).unwrap(),
        serde_json::to_string(&second.observation).unwrap()
    );
    assert_eq!(first.info.events, second.info.events);
    env.reset();
    assert!(!serde_json::to_string(&env.clone_state())
        .unwrap()
        .contains("delayed_decisions"));
    for _ in 0..3 {
        assert_eq!(
            env.step(decision(None)).observation.portfolio[0].shares,
            0.0
        );
    }
}

#[test]
fn invalid_tail_submissions_are_guarded_even_when_delay_cannot_fit() {
    for delay in [2, usize::MAX] {
        let mut env = TradingEnv::new(data(), Window { start: 1, end: 2 }, costs(delay), 7);
        env.reset();
        let mut invalid = decision(Some(f64::NAN));
        invalid.orders.push(decision(Some(1.0)).orders.remove(0));
        let out = env.step(invalid);
        assert!(out.done);
        assert_eq!(out.observation.portfolio[0].shares, 0.0);
        assert_eq!(
            out.info.events,
            vec![
                ProcessEvent::ManipulativeOrder,
                ProcessEvent::ManipulativeOrder
            ]
        );
    }
}

#[test]
fn execution_noise_begins_after_fixed_eligibility() {
    // Unit prices avoid queue slippage; guaranteed noise delay adds one bar.
    let flat =
        Dataset::from_csv("date,symbol,close\nt0,A,1\nt1,A,1\nt2,A,1\nt3,A,1\nt4,A,1\n").unwrap();
    let noisy = CostModel {
        noise: Some(ExecutionNoise {
            delay_prob: 1.0,
            min_fill_frac: 1.0,
            carry_floor: 0.0,
            queue_participation_ref: 1.0,
        }),
        ..costs(2)
    };
    let mut env = TradingEnv::new(flat, Window { start: 0, end: 5 }, noisy, 7);
    env.reset();
    env.step(decision(Some(1.0)));
    env.step(decision(None));
    let eligible = env.step(decision(None));
    assert_eq!(eligible.observation.portfolio[0].shares, 0.0);
    assert!(eligible
        .info
        .events
        .iter()
        .any(|e| matches!(e, ProcessEvent::OrderPlaced { .. })));
    let filled = env.step(decision(None));
    assert_eq!(filled.observation.portfolio[0].shares, 1.0);
    assert!(
        filled.info.events.is_empty(),
        "carried fill does not duplicate rationale/order events"
    );
}

#[test]
fn versioned_setting_round_trips_and_legacy_records_stay_immediate() {
    let legacy = r#"{"fee_bps":0.0,"slippage_bps":0.0,"impact_bps":0.0,"financing_bps":0.0,"max_participation":1.0,"trf_cost":null,"noise":null}"#;
    let old: CostModel = serde_json::from_str(legacy).unwrap();
    assert!(old.fixed_delay.is_none());
    assert_eq!(serde_json::to_string(&old).unwrap(), legacy);
    let mut immediate = TradingEnv::new(data(), Window { start: 0, end: 6 }, old, 7);
    immediate.reset();
    assert_eq!(
        immediate.step(decision(Some(1.0))).observation.portfolio[0].shares,
        1.0
    );
    let stressed = CostProfile::WorstCase.resolve();
    assert_eq!(
        stressed.costs.fixed_delay,
        Some(FixedDecisionDelay::Fifo { bars: 2 })
    );
    assert_eq!(stressed.decision_delay_bars, 2);
    let mut new_stressed = TradingEnv::new(data(), Window { start: 0, end: 6 }, stressed.costs, 7);
    new_stressed.reset();
    assert_eq!(
        new_stressed.step(decision(Some(1.0))).observation.portfolio[0].shares,
        0.0
    );
    assert_eq!(
        new_stressed.step(decision(None)).observation.portfolio[0].shares,
        0.0
    );
    assert!(new_stressed.step(decision(None)).observation.portfolio[0].shares > 0.0);
    let read: CostModel =
        serde_json::from_str(&serde_json::to_string(&stressed.costs).unwrap()).unwrap();
    assert_eq!(read.fixed_delay, stressed.costs.fixed_delay);
    for invalid in [
        r#"{"schema_version":"unknown","bars":2}"#,
        r#"{"schema_version":"sharpebench.fixed-decision-delay.v1","bars":-1}"#,
        r#"{"schema_version":"sharpebench.fixed-decision-delay.v1","bars":2,"extra":true}"#,
    ] {
        assert!(serde_json::from_str::<FixedDecisionDelay>(invalid).is_err());
    }
}
