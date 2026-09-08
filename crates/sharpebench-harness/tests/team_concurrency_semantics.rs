//! BI6: the concurrency semantics the team cost aggregation depends on.
//!
//! Summing member spend onto one consensus decision is only correct if every
//! member is polled exactly once per decision, sequentially, in declaration
//! order, against the same point-in-time observation, with none of them seeing
//! another's orders. That is what `TeamAgent` does today and what the aggregation
//! documents; these regressions pin it, so a future parallel or re-polling team
//! cannot silently turn the documented sum into a double count.

use sharpebench_harness::{run_team, TeamMember};
use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{Agent, CostModel, Dataset, Window};
use std::cell::RefCell;
use std::rc::Rc;

/// What one member saw at the moment it was asked to decide.
#[derive(Clone, Debug, PartialEq)]
struct Poll {
    member: String,
    date: String,
    cash: f64,
    portfolio: Vec<(String, f64)>,
}

type PollLog = Rc<RefCell<Vec<Poll>>>;

/// A member that records the observation it was handed before trading.
struct RecordingMember {
    name: String,
    weight: f64,
    log: PollLog,
}

impl Agent for RecordingMember {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.log.borrow_mut().push(Poll {
            member: self.name.clone(),
            date: obs.date.clone(),
            cash: obs.cash,
            portfolio: obs
                .portfolio
                .iter()
                .map(|p| (p.symbol.clone(), p.shares))
                .collect(),
        });
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
            cost: None,
        }
    }
}

fn recording(name: &str, weight: f64, log: &PollLog) -> TeamMember {
    let name = name.to_string();
    let log = Rc::clone(log);
    TeamMember::new(&name.clone(), move || {
        Box::new(RecordingMember {
            name: name.clone(),
            weight,
            log: Rc::clone(&log),
        }) as Box<dyn Agent>
    })
}

#[test]
fn team_members_are_polled_once_each_sequentially_on_one_shared_observation() {
    let data = Dataset::synthetic(3, 60, 20_260_908);
    let windows = [Window { start: 10, end: 40 }];
    let seeds = [7u64];
    let log: PollLog = Rc::new(RefCell::new(Vec::new()));

    let members = [
        recording("first", 0.4, &log),
        recording("second", 0.2, &log),
    ];

    let res = run_team(
        "sequential-team",
        &data,
        &windows,
        &seeds,
        CostModel::default(),
        &members,
    )
    .expect("no member reports a cost, so there is no denomination to mix");
    assert_eq!(res.role_returns.len(), 2);

    let decisions = windows[0].end.min(data.len()) - windows[0].start;
    let polls = log.borrow();

    // run_team runs the team once per window x seed, then each member solo over
    // the same grid. Exactly one poll per member per decision in each phase: a
    // member polled twice would have its spend counted twice.
    assert_eq!(
        polls.len(),
        4 * decisions,
        "expected {decisions} team polls per member plus {decisions} solo polls per member"
    );

    let team_phase = &polls[..2 * decisions];
    for (index, poll) in team_phase.iter().enumerate() {
        let expected = if index % 2 == 0 { "first" } else { "second" };
        assert_eq!(
            poll.member, expected,
            "poll {index} broke declaration order: members must be polled sequentially, \
             first then second"
        );
    }

    for step in 0..decisions {
        let first = &team_phase[2 * step];
        let second = &team_phase[2 * step + 1];
        assert_eq!(
            first.date, second.date,
            "both members must decide against the same point in time"
        );
        assert_eq!(
            first.cash, second.cash,
            "the second member must not see the first member's orders applied"
        );
        assert_eq!(
            first.portfolio, second.portfolio,
            "the second member must not see the first member's orders applied"
        );
    }

    let dates: Vec<&str> = (0..decisions)
        .map(|step| team_phase[2 * step].date.as_str())
        .collect();
    let mut sorted = dates.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        dates, sorted,
        "decisions must advance strictly forward in time, with no interleaving"
    );
}
