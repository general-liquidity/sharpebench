//! BI6: a team's compute cost is the sum of what its members spend.
//!
//! `TeamAgent` keeps only each member's orders and emits `cost: None`, so a team
//! of paid agents used to report zero spend and lose its cost-normalized columns
//! even though every member priced every decision. These regressions pin the
//! aggregation, the refusal when members bill in different denominations, and the
//! absence of a fabricated cost when nobody reports one.

use sharpebench_core::Run;
use sharpebench_harness::{run_team, TeamCostError, TeamCostUnit, TeamMember};
use sharpebench_protocol::{Action, Decision, DecisionCost, MarketObservation, Order};
use sharpebench_sim::{Agent, CostModel, Dataset, Window};

/// A member that always holds a fixed weight in the first symbol and prices every
/// decision it makes. Holding constant keeps the returns identical across members
/// so the assertions are about accounting, not about trading.
struct PricedMember {
    weight: f64,
    cost: DecisionCost,
}

impl Agent for PricedMember {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let orders = obs
            .symbols
            .first()
            .map(|s| {
                vec![Order {
                    symbol: s.symbol.clone(),
                    action: Action::Buy,
                    target_weight: self.weight,
                    confidence: 0.5,
                    rationale: "fixed weight".to_string(),
                }]
            })
            .unwrap_or_default();
        Decision {
            orders,
            reasoning: "fixed weight".to_string(),
            cost: Some(self.cost),
        }
    }
}

/// A member that reports nothing at all, like every in-process reference agent.
struct FreeMember;

impl Agent for FreeMember {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let orders = obs
            .symbols
            .first()
            .map(|s| {
                vec![Order {
                    symbol: s.symbol.clone(),
                    action: Action::Buy,
                    target_weight: 0.5,
                    confidence: 0.5,
                    rationale: "fixed weight".to_string(),
                }]
            })
            .unwrap_or_default();
        Decision {
            orders,
            reasoning: "fixed weight".to_string(),
            cost: None,
        }
    }
}

fn priced(weight: f64, cost: DecisionCost) -> Box<dyn Agent> {
    Box::new(PricedMember { weight, cost })
}

fn fixture() -> (Dataset, Vec<Window>, Vec<u64>) {
    (
        Dataset::synthetic(3, 60, 20_260_908),
        vec![Window { start: 10, end: 40 }],
        vec![0, 1],
    )
}

/// The decision count each run of the fixture makes, which is what a per-decision
/// cost is multiplied by.
fn decisions_per_run(windows: &[Window], data: &Dataset) -> f64 {
    windows
        .iter()
        .map(|w| w.end.min(data.len()) - w.start)
        .sum::<usize>() as f64
}

#[test]
fn team_run_cost_is_the_sum_of_its_members_reported_dollars() {
    let (data, windows, seeds) = fixture();
    let members = [
        TeamMember::new("expensive", || {
            priced(
                0.5,
                DecisionCost {
                    cost_usd: 0.75,
                    tokens_in: 100,
                    tokens_out: 20,
                    reasoning_tokens: 5,
                },
            )
        }),
        TeamMember::new("cheap", || {
            priced(
                0.5,
                DecisionCost {
                    cost_usd: 0.25,
                    tokens_in: 40,
                    tokens_out: 10,
                    reasoning_tokens: 0,
                },
            )
        }),
    ];

    let res = run_team(
        "paid-team",
        &data,
        &windows,
        &seeds,
        CostModel::default(),
        &members,
    )
    .expect("both members bill in dollars");

    assert_eq!(res.cost_unit, TeamCostUnit::Usd);
    let expected = decisions_per_run(&windows, &data);
    assert_eq!(res.team.runs.len(), seeds.len());
    for run in &res.team.runs {
        // 0.75 + 0.25 per decision: the team is charged for every member, once.
        assert!(
            (run.cost - expected).abs() < 1e-9,
            "team run cost {} should be the summed member spend {expected}",
            run.cost
        );
    }
}

#[test]
fn team_run_cost_sums_token_reporters_without_inventing_dollars() {
    let (data, windows, seeds) = fixture();
    let members = [
        TeamMember::new("tokens-a", || {
            priced(
                0.5,
                DecisionCost {
                    cost_usd: 0.0,
                    tokens_in: 30,
                    tokens_out: 10,
                    reasoning_tokens: 4,
                },
            )
        }),
        TeamMember::new("tokens-b", || {
            priced(
                0.5,
                DecisionCost {
                    cost_usd: 0.0,
                    tokens_in: 5,
                    tokens_out: 5,
                    reasoning_tokens: 0,
                },
            )
        }),
    ];

    let res = run_team(
        "token-team",
        &data,
        &windows,
        &seeds,
        CostModel::default(),
        &members,
    )
    .expect("both members bill in tokens");

    assert_eq!(res.cost_unit, TeamCostUnit::BillableTokens);
    // (30 + 10) + (5 + 5) billable tokens per decision; reasoning tokens are a
    // breakdown of the output and are not added again.
    let expected = 50.0 * decisions_per_run(&windows, &data);
    for run in &res.team.runs {
        assert!(
            (run.cost - expected).abs() < 1e-9,
            "team run cost {} should be the summed billable tokens {expected}",
            run.cost
        );
    }
}

#[test]
fn team_run_refuses_members_that_bill_in_different_denominations() {
    let (data, windows, seeds) = fixture();
    let members = [
        TeamMember::new("dollars", || {
            priced(
                0.5,
                DecisionCost {
                    cost_usd: 0.75,
                    ..DecisionCost::default()
                },
            )
        }),
        TeamMember::new("tokens", || {
            priced(
                0.5,
                DecisionCost {
                    tokens_in: 1_000,
                    tokens_out: 250,
                    ..DecisionCost::default()
                },
            )
        }),
    ];

    let err = run_team(
        "mixed-team",
        &data,
        &windows,
        &seeds,
        CostModel::default(),
        &members,
    )
    .expect_err("a dollar total would silently drop the token reporter's spend");

    assert_eq!(
        err,
        TeamCostError::MixedCostUnits {
            usd_member: "dollars".to_string(),
            token_member: "tokens".to_string(),
        }
    );
}

#[test]
fn team_run_without_reported_costs_reports_no_cost() {
    let (data, windows, seeds) = fixture();
    let members = [
        TeamMember::new("free-a", || Box::new(FreeMember) as Box<dyn Agent>),
        TeamMember::new("free-b", || Box::new(FreeMember) as Box<dyn Agent>),
    ];

    let res = run_team(
        "free-team",
        &data,
        &windows,
        &seeds,
        CostModel::default(),
        &members,
    )
    .expect("nobody reported anything to mix");

    assert_eq!(res.cost_unit, TeamCostUnit::NotReported);
    for run in &res.team.runs {
        assert_eq!(run.cost, Run::default().cost);
        assert_eq!(run.cost, 0.0);
    }
}
