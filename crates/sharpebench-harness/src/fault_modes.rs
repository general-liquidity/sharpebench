//! The fault modes and their mutable per-trial state (rows 27, 30, 31, 32).
//!
//! [`FaultInjectingAgent`] sits at the boundary where the harness talks to the
//! entrant. It changes only what the entrant is shown and which of its
//! submissions is accepted; the engine keeps building observations from the
//! canonical book and executing the accepted decision, so a fault is something
//! the entrant must handle, never a corruption of the book or of scoring:
//!
//! - projection lag (row 27) presents the pre-write `cash` and `portfolio`
//!   after an order executes, and records convergence when a presentation is
//!   observed to carry the canonical state again;
//! - amount sign (row 30) presents nonzero position quantities with the
//!   opposite sign, on a deep copy of the observation;
//! - rate limit (row 31) rejects an order-bearing decision and re-presents the
//!   identical observation until a seeded deadline counted in presentations,
//!   a monotonic clock that needs no wall time, then executes the first
//!   decision after the deadline.
//!
//! Each fired fault is recorded with a process grade of the entrant's
//! response. The grades are rank-neutral: they live in the attempt ledger and
//! never enter a return, score or rank. The state reads no clock; the only
//! timing, the host time spent on rejected presentations, is measured by the
//! agent wrapper and kept separate from the deterministic events.

use std::collections::BTreeMap;
use std::time::Instant;

use sharpebench_protocol::{Decision, MarketObservation};
use sharpebench_sim::{Agent, CostModel, Dataset, TransportDiagnostics, TransportHealth, Window};

use super::{
    parameter_draw, CellId, FaultEvent, FaultMode, FaultPlan, FaultedObservation, InjectedFaults,
    ReadingGrade, Sign, SignResponse, StaleReadGrade, StaleResponse,
};
use crate::accounting::{RateCard, UsageObservedAgent};
use crate::failure::{AttemptDuration, AttemptObservation};

/// Tolerance under which a restated target counts as the same target.
const TARGET_EPSILON: f64 = 1e-12;

/// A fault armed on one cell, with every parameter already drawn from the plan
/// digest. Fixed when the trial state is built.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ArmedFault {
    ProjectionLag {
        id: String,
        arm_step: u32,
        lag: u32,
    },
    AmountSign {
        id: String,
        arm_step: u32,
        span: u32,
    },
    RateLimit {
        id: String,
        arm_step: u32,
        rejected: u32,
    },
}

/// The book as the entrant sees it: cash and signed quantities.
#[derive(Clone, Debug, PartialEq)]
struct Projection {
    cash: f64,
    shares: Vec<(String, f64)>,
}

impl Projection {
    fn of(observation: &MarketObservation) -> Self {
        Self {
            cash: observation.cash,
            shares: observation
                .portfolio
                .iter()
                .map(|position| (position.symbol.clone(), position.shares))
                .collect(),
        }
    }

    fn apply_to(&self, observation: &mut MarketObservation) {
        observation.cash = self.cash;
        for position in &mut observation.portfolio {
            if let Some((_, shares)) = self.shares.iter().find(|(s, _)| *s == position.symbol) {
                position.shares = *shares;
            }
        }
    }

    fn same_holdings(&self, other: &Projection) -> bool {
        self.shares == other.shares
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ActiveLag {
    armed: usize,
    snapshot: Projection,
    remaining: u32,
    presented: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingConvergence {
    armed: usize,
    stale_presentations: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct ActiveLimit {
    armed: usize,
    step: u32,
    remaining: u32,
    rejected_presentations: u32,
    resubmissions: u32,
    waits: u32,
}

/// A symbol a stale read hid a write on: its symbol, the shown quantity and
/// the canonical quantity.
type HiddenWrite = (String, f64, f64);

/// What the current presentation showed, kept until its decision is accepted
/// so the response can be graded.
#[derive(Clone, Debug, Default, PartialEq)]
struct Shown {
    stale: Option<(usize, Vec<HiddenWrite>)>,
    inverted: Vec<(usize, Vec<(String, Sign)>)>,
}

/// Whether the harness accepts a submission or re-presents the observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Reject,
}

/// Mutable per-trial fault state (row 32): built from a frozen plan for one
/// cell, confined to this object, and never written back to the plan.
/// [`TrialFaultState::reset`] returns it to exactly its freshly built value.
#[derive(Clone, Debug, PartialEq)]
pub struct TrialFaultState {
    plan_sha256: String,
    cell: CellId,
    assigned: Vec<String>,
    armed: Vec<ArmedFault>,
    step: u32,
    targets: BTreeMap<String, f64>,
    previous: Option<Projection>,
    fired: Vec<bool>,
    lag: Option<ActiveLag>,
    converging: Option<PendingConvergence>,
    limit: Option<ActiveLimit>,
    shown: Shown,
    events: Vec<FaultEvent>,
}

impl TrialFaultState {
    /// Arm the plan's faults for `cell`, drawing every parameter from the
    /// plan digest.
    pub fn new(plan: &FaultPlan, cell: CellId) -> Self {
        let digest = plan.digest();
        let steps = cell.steps();
        let assigned = plan.assigned(cell);
        let draw = |id: &str, parameter: &str, bound: u32| {
            parameter_draw(&digest, id, parameter, plan.seed(), cell, bound)
        };
        let armed = if steps == 0 {
            Vec::new()
        } else {
            assigned
                .iter()
                .map(|spec| {
                    let id = spec.id.clone();
                    let arm_step = draw(&id, "arm_step", steps);
                    match spec.fault {
                        FaultMode::ProjectionLag { max_lag_steps } => ArmedFault::ProjectionLag {
                            lag: 1 + draw(&id, "lag", max_lag_steps),
                            id,
                            arm_step,
                        },
                        FaultMode::AmountSign { max_span_steps } => ArmedFault::AmountSign {
                            span: 1 + draw(&id, "span", max_span_steps),
                            id,
                            arm_step,
                        },
                        FaultMode::RateLimit {
                            max_rejected_presentations,
                        } => ArmedFault::RateLimit {
                            rejected: 1 + draw(&id, "rejected", max_rejected_presentations),
                            id,
                            arm_step,
                        },
                        FaultMode::LimitBeforeSort { .. } => {
                            unreachable!("plan validation refuses limit_before_sort")
                        }
                    }
                })
                .collect()
        };
        Self::fresh(
            digest,
            cell,
            assigned.iter().map(|spec| spec.id.clone()).collect(),
            armed,
        )
    }

    fn fresh(
        plan_sha256: String,
        cell: CellId,
        assigned: Vec<String>,
        armed: Vec<ArmedFault>,
    ) -> Self {
        let fired = vec![false; armed.len()];
        Self {
            plan_sha256,
            cell,
            assigned,
            armed,
            step: 0,
            targets: BTreeMap::new(),
            previous: None,
            fired,
            lag: None,
            converging: None,
            limit: None,
            shown: Shown::default(),
            events: Vec::new(),
        }
    }

    /// Discard everything the trial has done, keeping only what the plan armed.
    pub fn reset(&mut self) {
        *self = Self::fresh(
            std::mem::take(&mut self.plan_sha256),
            self.cell,
            std::mem::take(&mut self.assigned),
            std::mem::take(&mut self.armed),
        );
    }

    /// Fault ids assigned to this cell.
    pub fn assigned(&self) -> &[String] {
        &self.assigned
    }

    /// The observation to present for the canonical `observation` at the
    /// current step. The canonical value is only read; the result is a copy.
    pub fn prepare(&mut self, observation: &MarketObservation) -> MarketObservation {
        let canonical = Projection::of(observation);
        let wrote = self
            .previous
            .as_ref()
            .is_some_and(|previous| !previous.same_holdings(&canonical));
        let mut presented = observation.clone();
        self.shown = Shown::default();

        if self.lag.is_none() {
            let due = self.armed.iter().enumerate().find(|(index, fault)| {
                matches!(fault, ArmedFault::ProjectionLag { arm_step, .. }
                    if !self.fired[*index] && wrote && self.step >= *arm_step)
            });
            if let (Some((index, ArmedFault::ProjectionLag { lag, .. })), Some(previous)) =
                (due, self.previous.as_ref())
            {
                self.fired[index] = true;
                self.converging = None;
                self.lag = Some(ActiveLag {
                    armed: index,
                    snapshot: previous.clone(),
                    remaining: *lag,
                    presented: 0,
                });
            }
        }
        if let Some(lag) = &mut self.lag {
            lag.snapshot.apply_to(&mut presented);
            lag.remaining -= 1;
            lag.presented += 1;
            let hidden = presented
                .portfolio
                .iter()
                .zip(&observation.portfolio)
                .filter(|(shown, real)| shown.shares != real.shares)
                .map(|(shown, real)| (real.symbol.clone(), shown.shares, real.shares))
                .collect();
            self.shown.stale = Some((lag.armed, hidden));
            if lag.remaining == 0 {
                self.converging = Some(PendingConvergence {
                    armed: lag.armed,
                    stale_presentations: lag.presented,
                });
                self.lag = None;
            }
        }

        for (index, fault) in self.armed.iter().enumerate() {
            let ArmedFault::AmountSign { arm_step, span, .. } = fault else {
                continue;
            };
            if self.step < *arm_step || self.step - *arm_step >= *span {
                continue;
            }
            let mut inverted = Vec::new();
            for position in &mut presented.portfolio {
                if position.shares != 0.0 {
                    position.shares = -position.shares;
                    inverted.push((position.symbol.clone(), Sign::of(position.shares)));
                }
            }
            if !inverted.is_empty() {
                self.fired[index] = true;
                self.shown.inverted.push((index, inverted));
            }
        }

        if self.shown.stale.is_none() {
            if let Some(pending) = &self.converging {
                if Projection::of(&presented) == canonical {
                    let ArmedFault::ProjectionLag { id, .. } = &self.armed[pending.armed] else {
                        unreachable!("a convergence is only ever pending for a projection lag")
                    };
                    self.events.push(FaultEvent::ProjectionConverged {
                        fault_id: id.clone(),
                        step: self.step,
                        stale_presentations: pending.stale_presentations,
                    });
                    self.converging = None;
                }
            }
        }

        self.previous = Some(canonical);
        presented
    }

    /// Rule on one submission for the current step. `Reject` means the harness
    /// must present the same observation again and submit its answer here.
    pub fn submit(&mut self, decision: &Decision) -> Verdict {
        if let Some(limit) = &mut self.limit {
            if limit.remaining == 0 {
                let armed = limit.armed;
                let ArmedFault::RateLimit { id, .. } = &self.armed[armed] else {
                    unreachable!("an active limit is only ever a rate limit")
                };
                self.events.push(FaultEvent::RateLimited {
                    fault_id: id.clone(),
                    step: limit.step,
                    rejected_presentations: limit.rejected_presentations,
                    resubmissions_under_limit: limit.resubmissions,
                    waits_under_limit: limit.waits,
                    resubmitted_after_deadline: !decision.orders.is_empty(),
                });
                self.limit = None;
                return Verdict::Accept;
            }
            if decision.orders.is_empty() {
                limit.waits += 1;
            } else {
                limit.resubmissions += 1;
            }
            limit.rejected_presentations += 1;
            limit.remaining -= 1;
            return Verdict::Reject;
        }
        if decision.orders.is_empty() {
            return Verdict::Accept;
        }
        let due = self.armed.iter().enumerate().find(|(index, fault)| {
            matches!(fault, ArmedFault::RateLimit { arm_step, .. }
                if !self.fired[*index] && self.step >= *arm_step)
        });
        match due {
            Some((index, ArmedFault::RateLimit { rejected, .. })) => {
                self.fired[index] = true;
                self.limit = Some(ActiveLimit {
                    armed: index,
                    step: self.step,
                    remaining: *rejected - 1,
                    rejected_presentations: 1,
                    resubmissions: 0,
                    waits: 0,
                });
                Verdict::Reject
            }
            _ => Verdict::Accept,
        }
    }

    /// Record the accepted decision: grade the response to what this step
    /// showed, adopt its targets as the entrant's stated reading, and advance.
    pub fn accept(&mut self, decision: &Decision) {
        let target = |symbol: &str| {
            decision
                .orders
                .iter()
                .find(|order| order.symbol == symbol)
                .map(|order| order.target_weight)
        };
        if let Some((armed, hidden)) = self.shown.stale.take() {
            let responses = hidden
                .into_iter()
                .map(|(symbol, shown, real)| {
                    let grade = match (target(&symbol), self.targets.get(&symbol)) {
                        (None, _) => StaleReadGrade::NoOrder,
                        (Some(_), None) => StaleReadGrade::NoPriorStatement,
                        (Some(new), Some(&prior)) => {
                            let direction = (real - shown).signum();
                            if (new - prior).abs() <= TARGET_EPSILON {
                                StaleReadGrade::Reaffirmed
                            } else if (new - prior) * direction > 0.0 {
                                StaleReadGrade::Escalated
                            } else {
                                StaleReadGrade::Revised
                            }
                        }
                    };
                    StaleResponse { symbol, grade }
                })
                .collect();
            let ArmedFault::ProjectionLag { id, .. } = &self.armed[armed] else {
                unreachable!("a stale presentation is only ever a projection lag")
            };
            self.events.push(FaultEvent::ProjectionStale {
                fault_id: id.clone(),
                step: self.step,
                responses,
            });
        }
        for (armed, inverted) in std::mem::take(&mut self.shown.inverted) {
            let responses = inverted
                .into_iter()
                .map(|(symbol, presented)| {
                    let grade = match (target(&symbol), self.targets.get(&symbol)) {
                        (None, _) => ReadingGrade::NoOrder,
                        (Some(_), None) => ReadingGrade::NoPriorStatement,
                        (Some(new), Some(&prior)) => {
                            if Sign::of(new) == Sign::of(prior) {
                                ReadingGrade::ConsistentWithOwnStatement
                            } else if Sign::of(new) == presented {
                                ReadingGrade::FollowedPresentation
                            } else {
                                ReadingGrade::Revised
                            }
                        }
                    };
                    SignResponse {
                        symbol,
                        presented,
                        grade,
                    }
                })
                .collect();
            let ArmedFault::AmountSign { id, .. } = &self.armed[armed] else {
                unreachable!("an inversion is only ever an amount-sign fault")
            };
            self.events.push(FaultEvent::SignInverted {
                fault_id: id.clone(),
                step: self.step,
                responses,
            });
        }
        for order in &decision.orders {
            self.targets
                .insert(order.symbol.clone(), order.target_weight);
        }
        self.step = self.step.saturating_add(1);
    }

    /// The evidence so far, closing any convergence no read observed.
    pub fn evidence(&self, rate_limited: AttemptDuration) -> InjectedFaults {
        let mut events = self.events.clone();
        let unconverged = self
            .lag
            .as_ref()
            .map(|lag| (lag.armed, lag.presented))
            .or_else(|| {
                self.converging
                    .as_ref()
                    .map(|pending| (pending.armed, pending.stale_presentations))
            });
        if let Some((armed, stale_presentations)) = unconverged {
            if let ArmedFault::ProjectionLag { id, .. } = &self.armed[armed] {
                events.push(FaultEvent::ProjectionUnconverged {
                    fault_id: id.clone(),
                    stale_presentations,
                });
            }
        }
        InjectedFaults {
            plan_sha256: self.plan_sha256.clone(),
            cell: self.cell,
            assigned: self.assigned.clone(),
            events,
            rate_limited,
        }
    }
}

/// Wraps the entrant at the harness boundary and injects the cell's faults.
/// Transport health is the entrant's own, unchanged: an injected fault is
/// never reported as a transport or protocol fault.
pub struct FaultInjectingAgent<'a, A> {
    inner: &'a mut A,
    state: TrialFaultState,
    rate_limited_nanos: u64,
}

impl<'a, A: Agent + TransportDiagnostics> FaultInjectingAgent<'a, A> {
    pub fn new(inner: &'a mut A, plan: &FaultPlan, cell: CellId) -> Self {
        Self {
            inner,
            state: TrialFaultState::new(plan, cell),
            rate_limited_nanos: 0,
        }
    }

    pub fn state(&self) -> &TrialFaultState {
        &self.state
    }

    pub fn evidence(&self) -> InjectedFaults {
        self.state.evidence(AttemptDuration::HostClock {
            nanos: self.rate_limited_nanos,
        })
    }
}

impl<A: Agent + TransportDiagnostics> Agent for FaultInjectingAgent<'_, A> {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let presented = self.state.prepare(observation);
        loop {
            let started = Instant::now();
            let decision = self.inner.decide(&presented);
            match self.state.submit(&decision) {
                Verdict::Accept => {
                    self.state.accept(&decision);
                    return decision;
                }
                Verdict::Reject => {
                    let spent = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    self.rate_limited_nanos = self.rate_limited_nanos.saturating_add(spent);
                }
            }
        }
    }
}

impl<A: Agent + TransportDiagnostics> TransportDiagnostics for FaultInjectingAgent<'_, A> {
    fn health(&self) -> &TransportHealth {
        self.inner.health()
    }
}

/// One external attempt under an optional fault plan. With `None` this is
/// exactly [`crate::run_external_backtest_observed`] with no evidence, so an
/// unfaulted sweep is unchanged. With a plan, usage (when a rate card is set)
/// is observed inside the injector, so decisions a rate limit rejected are
/// still counted as spend; the run itself only ever sees accepted decisions.
pub fn run_faulted_backtest_observed<A: Agent + TransportDiagnostics>(
    data: &Dataset,
    agent: &mut A,
    window: Window,
    seed: u64,
    costs: CostModel,
    card: Option<&RateCard>,
    plan: Option<&FaultPlan>,
) -> FaultedObservation {
    let Some(plan) = plan else {
        return crate::run_external_backtest_observed(data, agent, window, seed, costs, card)
            .into();
    };
    let cell = CellId::new(window, seed);
    match card {
        None => {
            let mut faulted = FaultInjectingAgent::new(agent, plan, cell);
            let result = crate::run_external_backtest(data, &mut faulted, window, seed, costs);
            FaultedObservation {
                observation: result.into(),
                injected_faults: Some(faulted.evidence()),
            }
        }
        Some(card) => {
            let mut observed = UsageObservedAgent::new(agent, card);
            let (result, evidence) = {
                let mut faulted = FaultInjectingAgent::new(&mut observed, plan, cell);
                let result = crate::run_external_backtest(data, &mut faulted, window, seed, costs);
                (result, faulted.evidence())
            };
            FaultedObservation {
                observation: AttemptObservation {
                    result,
                    usage: Some(observed.into_usage()),
                },
                injected_faults: Some(evidence),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fault_plan::{ContractRelaxation, FaultSpec, COHORT_SCALE};
    use sharpebench_protocol::{Action, Order, RunTrajectory};
    use sharpebench_sim::run_backtest_capture;

    const WINDOW: Window = Window { start: 5, end: 45 };

    /// An in-process entrant with clean transport health that records what it
    /// was shown and answers through `policy(presentation_index, observation)`.
    struct Entrant<P> {
        policy: P,
        seen: Vec<MarketObservation>,
        health: TransportHealth,
    }

    impl<P: FnMut(usize, &MarketObservation) -> Decision> Entrant<P> {
        fn new(policy: P) -> Self {
            Self {
                policy,
                seen: Vec::new(),
                health: TransportHealth::default(),
            }
        }
    }

    impl<P: FnMut(usize, &MarketObservation) -> Decision> Agent for Entrant<P> {
        fn decide(&mut self, observation: &MarketObservation) -> Decision {
            let decision = (self.policy)(self.seen.len(), observation);
            self.seen.push(observation.clone());
            decision
        }
    }

    impl<P> TransportDiagnostics for Entrant<P> {
        fn health(&self) -> &TransportHealth {
            &self.health
        }
    }

    fn decision(targets: &[(&str, f64)]) -> Decision {
        Decision {
            orders: targets
                .iter()
                .map(|(symbol, target)| Order {
                    symbol: symbol.to_string(),
                    action: if *target > 0.0 {
                        Action::Buy
                    } else {
                        Action::Sell
                    },
                    target_weight: *target,
                    confidence: 0.5,
                    rationale: String::new(),
                })
                .collect(),
            reasoning: String::new(),
            cost: None,
        }
    }

    fn symbol(observation: &MarketObservation) -> String {
        observation.symbols[0].symbol.clone()
    }

    /// Trades on the date alone, never on what the book looks like, so its
    /// decisions cannot depend on a perturbed presentation.
    fn by_date(_: usize, observation: &MarketObservation) -> Decision {
        let day: u32 = observation
            .date
            .bytes()
            .filter(u8::is_ascii_digit)
            .map(|b| u32::from(b - b'0'))
            .sum();
        let target = if day.is_multiple_of(2) { 0.4 } else { -0.3 };
        decision(&[(&symbol(observation), target)])
    }

    fn plan(fault: FaultMode) -> FaultPlan {
        FaultPlan::new(
            42,
            vec![fault.relaxation()],
            vec![FaultSpec {
                id: "only".to_string(),
                group: None,
                cohort_ppm: COHORT_SCALE,
                fault,
            }],
        )
        .unwrap()
    }

    fn data() -> Dataset {
        Dataset::synthetic(2, 60, 9)
    }

    type Faulted = (
        sharpebench_core::Run,
        RunTrajectory,
        InjectedFaults,
        Vec<MarketObservation>,
    );

    fn faulted_in<P: FnMut(usize, &MarketObservation) -> Decision>(
        plan: &FaultPlan,
        policy: P,
        window: Window,
        seed: u64,
    ) -> Faulted {
        let mut entrant = Entrant::new(policy);
        let (run, trajectory, evidence) = {
            let mut faulted =
                FaultInjectingAgent::new(&mut entrant, plan, CellId::new(WINDOW, seed));
            let (run, trajectory) =
                run_backtest_capture(&data(), &mut faulted, window, seed, CostModel::default());
            (run, trajectory, faulted.evidence())
        };
        (run, trajectory, evidence, entrant.seen)
    }

    fn faulted_run<P: FnMut(usize, &MarketObservation) -> Decision>(
        plan: &FaultPlan,
        policy: P,
        seed: u64,
    ) -> Faulted {
        faulted_in(plan, policy, WINDOW, seed)
    }

    fn unfaulted_run<P: FnMut(usize, &MarketObservation) -> Decision>(
        policy: P,
        seed: u64,
    ) -> (sharpebench_core::Run, Vec<MarketObservation>) {
        let mut entrant = Entrant::new(policy);
        let (run, _) =
            run_backtest_capture(&data(), &mut entrant, WINDOW, seed, CostModel::default());
        (run, entrant.seen)
    }

    fn json<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string(value).unwrap()
    }

    fn deterministic(evidence: &InjectedFaults) -> String {
        json(&(
            &evidence.plan_sha256,
            evidence.cell,
            &evidence.assigned,
            &evidence.events,
        ))
    }

    fn lag_plan() -> FaultPlan {
        plan(FaultMode::ProjectionLag { max_lag_steps: 4 })
    }

    fn sign_plan() -> FaultPlan {
        plan(FaultMode::AmountSign { max_span_steps: 3 })
    }

    fn limit_plan() -> FaultPlan {
        plan(FaultMode::RateLimit {
            max_rejected_presentations: 5,
        })
    }

    fn stale_grades(evidence: &InjectedFaults) -> Vec<StaleReadGrade> {
        evidence
            .events
            .iter()
            .flat_map(|event| match event {
                FaultEvent::ProjectionStale { responses, .. } => {
                    responses.iter().map(|r| r.grade).collect()
                }
                _ => Vec::new(),
            })
            .collect()
    }

    fn reading_grades(evidence: &InjectedFaults) -> Vec<ReadingGrade> {
        evidence
            .events
            .iter()
            .flat_map(|event| match event {
                FaultEvent::SignInverted { responses, .. } => {
                    responses.iter().map(|r| r.grade).collect()
                }
                _ => Vec::new(),
            })
            .collect()
    }

    /// Row 32: the same plan digest and seed give byte-identical traces,
    /// presentations and evidence, for every mode.
    #[test]
    fn every_mode_fires_reproducibly_from_the_plan() {
        for plan in [lag_plan(), sign_plan(), limit_plan()] {
            let first = faulted_run(&plan, by_date, 3);
            let second = faulted_run(&plan, by_date, 3);
            assert!(!first.2.events.is_empty(), "{plan:?} must fire");
            assert_eq!(json(&first.0), json(&second.0));
            assert_eq!(json(&first.1), json(&second.1));
            assert_eq!(deterministic(&first.2), deterministic(&second.2));
            assert_eq!(json(&first.3), json(&second.3), "same presentations");
        }
    }

    /// No plan, no fault: the helper returns no evidence and exactly the run
    /// the existing path returns, and the entrant sees canonical observations.
    #[test]
    fn no_mode_fires_without_a_plan() {
        let data = data();
        let costs = CostModel::default();
        let mut entrant = Entrant::new(by_date);
        let faulted =
            run_faulted_backtest_observed(&data, &mut entrant, WINDOW, 3, costs, None, None);
        assert!(faulted.injected_faults.is_none());
        assert!(faulted.observation.usage.is_none());
        let mut reference = Entrant::new(by_date);
        let expected = crate::run_external_backtest(&data, &mut reference, WINDOW, 3, costs);
        assert_eq!(
            json(&faulted.observation.result.unwrap()),
            json(&expected.unwrap())
        );
        assert_eq!(json(&entrant.seen), json(&reference.seen));
        assert_eq!(entrant.seen.len(), WINDOW.end - WINDOW.start);
    }

    /// Write-authoritative: the stale read changes what the entrant sees but
    /// never the book, so an entrant that ignores the book scores identically.
    #[test]
    fn projection_lag_is_stale_on_reads_and_authoritative_on_writes() {
        let (run, _, evidence, seen) = faulted_run(&lag_plan(), by_date, 3);
        let (reference, canonical) = unfaulted_run(by_date, 3);
        assert_eq!(json(&run), json(&reference), "the book is authoritative");
        assert_eq!(seen.len(), canonical.len());
        let stale: Vec<u32> = evidence
            .events
            .iter()
            .filter_map(|event| match event {
                FaultEvent::ProjectionStale { step, .. } => Some(*step),
                _ => None,
            })
            .collect();
        assert!((1..=4).contains(&stale.len()), "{evidence:?}");
        let arm = stale[0] as usize;
        for &step in &stale {
            let step = step as usize;
            assert_eq!(json(&seen[step].symbols), json(&canonical[step].symbols));
            assert_eq!(
                json(&seen[step].portfolio),
                json(&canonical[arm - 1].portfolio),
                "a stale read shows the pre-write holdings"
            );
        }
        assert_ne!(json(&seen[arm].portfolio), json(&canonical[arm].portfolio));
        let converged = evidence.events.iter().find_map(|event| match event {
            FaultEvent::ProjectionConverged {
                step,
                stale_presentations,
                ..
            } => Some((*step, *stale_presentations)),
            _ => None,
        });
        let (step, stale_presentations) = converged.expect("the window outlasts the lag");
        assert_eq!(stale_presentations as usize, stale.len());
        assert_eq!(step, stale.last().unwrap() + 1);
        assert_eq!(
            json(&seen[step as usize]),
            json(&canonical[step as usize]),
            "convergence is recorded on the read that observed the canonical state"
        );
    }

    /// Convergence is observed, not timed: when the window ends inside the
    /// lag, the attempt is recorded unconverged.
    #[test]
    fn a_lag_the_window_outlasts_is_recorded_unconverged() {
        let plan = lag_plan();
        let (_, _, evidence, _) = faulted_run(&plan, by_date, 3);
        let arm = evidence
            .events
            .iter()
            .find_map(|event| match event {
                FaultEvent::ProjectionStale { step, .. } => Some(*step),
                _ => None,
            })
            .unwrap();
        let short = Window {
            start: WINDOW.start,
            end: WINDOW.start + arm as usize + 1,
        };
        let (_, _, evidence, _) = faulted_in(&plan, by_date, short, 3);
        assert!(
            matches!(
                evidence.events.last(),
                Some(FaultEvent::ProjectionUnconverged { .. })
            ),
            "{evidence:?}"
        );
        assert!(!evidence
            .events
            .iter()
            .any(|event| matches!(event, FaultEvent::ProjectionConverged { .. })));
    }

    fn book(date: &str, cash: f64, shares: f64) -> MarketObservation {
        MarketObservation {
            date: date.to_string(),
            cash,
            symbols: vec![sharpebench_protocol::SymbolSnapshot {
                symbol: "A".to_string(),
                close_history: vec![10.0],
                fundamentals: Default::default(),
                news: Vec::new(),
            }],
            portfolio: vec![sharpebench_protocol::PositionState {
                symbol: "A".to_string(),
                shares,
                avg_price: 0.0,
            }],
        }
    }

    fn armed(fault: ArmedFault) -> TrialFaultState {
        let cell = CellId {
            window_start: 0,
            window_end: 10,
            seed: 0,
        };
        TrialFaultState::fresh("00".repeat(32), cell, vec!["f".to_string()], vec![fault])
    }

    fn step(
        state: &mut TrialFaultState,
        observation: &MarketObservation,
        target: Option<f64>,
    ) -> MarketObservation {
        let presented = state.prepare(observation);
        let answer = target.map_or_else(|| decision(&[]), |t| decision(&[("A", t)]));
        assert_eq!(state.submit(&answer), Verdict::Accept);
        state.accept(&answer);
        presented
    }

    /// An entrant that reads the stale book as "my buy did not fill" and
    /// chases with a larger target escalates the hidden write: the
    /// resubmission failure the lag exists to catch. Restating the target is
    /// the idempotent response. Convergence is recorded on the first read that
    /// carries the canonical book.
    #[test]
    fn projection_lag_grades_escalation_against_restatement() {
        let lag = |lag| ArmedFault::ProjectionLag {
            id: "f".to_string(),
            arm_step: 0,
            lag,
        };
        let mut state = armed(lag(2));
        step(&mut state, &book("d0", 1.0, 0.0), Some(0.2));
        let shown = step(&mut state, &book("d1", 0.8, 2.0), Some(0.4));
        assert_eq!((shown.cash, shown.portfolio[0].shares), (1.0, 0.0));
        let shown = step(&mut state, &book("d2", 0.8, 2.0), Some(0.2));
        assert_eq!(shown.portfolio[0].shares, 0.0, "still inside the lag");
        let shown = step(&mut state, &book("d3", 0.8, 2.0), None);
        assert_eq!(shown.portfolio[0].shares, 2.0);
        assert_eq!(
            stale_grades(&state.evidence(AttemptDuration::Unavailable)),
            vec![StaleReadGrade::Escalated, StaleReadGrade::Revised]
        );
        assert!(matches!(
            state.evidence(AttemptDuration::Unavailable).events.last(),
            Some(FaultEvent::ProjectionConverged {
                step: 3,
                stale_presentations: 2,
                ..
            })
        ));

        let mut state = armed(lag(1));
        step(&mut state, &book("d0", 1.0, 0.0), Some(0.2));
        step(&mut state, &book("d1", 0.8, 2.0), Some(0.2));
        assert_eq!(
            stale_grades(&state.evidence(AttemptDuration::Unavailable)),
            vec![StaleReadGrade::Reaffirmed]
        );

        let mut state = armed(lag(1));
        step(&mut state, &book("d0", 1.0, 0.0), Some(0.2));
        step(&mut state, &book("d1", 0.8, 2.0), None);
        assert_eq!(
            stale_grades(&state.evidence(AttemptDuration::Unavailable)),
            vec![StaleReadGrade::NoOrder]
        );
    }

    /// No write, no stale read: the lag arms on the first write at or after
    /// its step, never on a quiet book.
    #[test]
    fn projection_lag_waits_for_a_write() {
        let mut state = armed(ArmedFault::ProjectionLag {
            id: "f".to_string(),
            arm_step: 0,
            lag: 3,
        });
        for day in 0..5 {
            let shown = step(&mut state, &book(&format!("d{day}"), 1.0, 0.0), None);
            assert_eq!(shown.portfolio[0].shares, 0.0);
        }
        assert!(state
            .evidence(AttemptDuration::Unavailable)
            .events
            .is_empty());
    }

    /// Following the inverted report against one's own stated target is
    /// graded as such; restating the target is consistent; a flat book is
    /// never inverted.
    #[test]
    fn amount_sign_grades_the_response_to_the_inverted_report() {
        let sign = || ArmedFault::AmountSign {
            id: "f".to_string(),
            arm_step: 0,
            span: 3,
        };
        let mut state = armed(sign());
        let shown = step(&mut state, &book("d0", 1.0, 0.0), Some(0.3));
        assert_eq!(shown.portfolio[0].shares, 0.0, "zero has no sign to invert");
        let shown = step(&mut state, &book("d1", 0.7, 3.0), Some(-0.3));
        assert_eq!(shown.portfolio[0].shares, -3.0);
        let shown = step(&mut state, &book("d2", 1.3, -3.0), Some(-0.3));
        assert_eq!(shown.portfolio[0].shares, 3.0);
        let shown = step(&mut state, &book("d3", 1.3, -3.0), Some(0.0));
        assert_eq!(shown.portfolio[0].shares, -3.0, "the span has ended");
        assert_eq!(
            reading_grades(&state.evidence(AttemptDuration::Unavailable)),
            vec![
                ReadingGrade::FollowedPresentation,
                ReadingGrade::ConsistentWithOwnStatement,
            ]
        );

        let mut state = armed(sign());
        step(&mut state, &book("d0", 1.0, 0.0), None);
        step(&mut state, &book("d1", 1.0, 3.0), Some(0.0));
        assert_eq!(
            reading_grades(&state.evidence(AttemptDuration::Unavailable)),
            vec![ReadingGrade::NoPriorStatement]
        );
    }

    /// The rate limit needs an order to reject: a hold is never limited, and
    /// the first order-bearing submission at or after the armed step is.
    #[test]
    fn rate_limit_fires_on_the_first_order_bearing_call_only() {
        let mut state = armed(ArmedFault::RateLimit {
            id: "f".to_string(),
            arm_step: 1,
            rejected: 2,
        });
        let observation = book("d0", 1.0, 0.0);
        state.prepare(&observation);
        assert_eq!(
            state.submit(&decision(&[("A", 0.2)])),
            Verdict::Accept,
            "before the armed step"
        );
        state.accept(&decision(&[("A", 0.2)]));
        state.prepare(&observation);
        assert_eq!(
            state.submit(&decision(&[])),
            Verdict::Accept,
            "a hold is not a call"
        );
        state.accept(&decision(&[]));
        state.prepare(&observation);
        assert_eq!(state.submit(&decision(&[("A", 0.2)])), Verdict::Reject);
        assert_eq!(state.submit(&decision(&[("A", 0.2)])), Verdict::Reject);
        assert_eq!(state.submit(&decision(&[("A", 0.2)])), Verdict::Accept);
        state.accept(&decision(&[("A", 0.2)]));
        state.prepare(&observation);
        assert_eq!(
            state.submit(&decision(&[("A", 0.2)])),
            Verdict::Accept,
            "fires once"
        );
        assert_eq!(
            state.evidence(AttemptDuration::Unavailable).events,
            vec![FaultEvent::RateLimited {
                fault_id: "f".to_string(),
                step: 2,
                rejected_presentations: 2,
                resubmissions_under_limit: 1,
                waits_under_limit: 0,
                resubmitted_after_deadline: true,
            }]
        );
    }

    /// Row 30: perturb the projection, never the book.
    #[test]
    fn amount_sign_inverts_the_projection_only() {
        let (run, _, evidence, seen) = faulted_run(&sign_plan(), by_date, 3);
        let (reference, canonical) = unfaulted_run(by_date, 3);
        assert_eq!(json(&run), json(&reference), "the book is unchanged");
        let mut inverted_steps = 0;
        for event in &evidence.events {
            let FaultEvent::SignInverted { step, .. } = event else {
                panic!("only the sign fault is armed: {event:?}")
            };
            inverted_steps += 1;
            let step = *step as usize;
            for (shown, real) in seen[step].portfolio.iter().zip(&canonical[step].portfolio) {
                assert_eq!(shown.shares, -real.shares);
            }
        }
        assert!((1..=3).contains(&inverted_steps));
        assert!(!reading_grades(&evidence).is_empty());
    }

    /// Row 31: the limit re-presents the identical observation until the
    /// seeded deadline and then executes; the book sees one decision a step.
    #[test]
    fn rate_limit_rejects_until_the_seeded_deadline() {
        let (run, _, evidence, seen) = faulted_run(&limit_plan(), by_date, 3);
        let (reference, canonical) = unfaulted_run(by_date, 3);
        assert_eq!(
            json(&run),
            json(&reference),
            "an entrant that restates its decision loses nothing to the limit"
        );
        let [FaultEvent::RateLimited {
            step,
            rejected_presentations,
            resubmissions_under_limit,
            waits_under_limit,
            resubmitted_after_deadline,
            ..
        }] = evidence.events.as_slice()
        else {
            panic!("exactly one limit: {:?}", evidence.events)
        };
        assert!((1..=5).contains(rejected_presentations));
        assert_eq!(*resubmissions_under_limit, rejected_presentations - 1);
        assert_eq!(*waits_under_limit, 0);
        assert!(*resubmitted_after_deadline);
        assert_eq!(
            seen.len(),
            canonical.len() + *rejected_presentations as usize
        );
        let step = *step as usize;
        for repeat in 0..=*rejected_presentations as usize {
            assert_eq!(json(&seen[step + repeat]), json(&canonical[step]));
        }
        assert_eq!(
            json(&seen[step + *rejected_presentations as usize + 1]),
            json(&canonical[step + 1])
        );
        assert!(matches!(
            evidence.rate_limited,
            AttemptDuration::HostClock { .. }
        ));
    }

    /// Backing off versus hammering is graded, not folded into the return.
    #[test]
    fn rate_limit_distinguishes_waiting_from_hammering() {
        let mut last_date = String::new();
        let waiter = move |_: usize, observation: &MarketObservation| {
            let repeat = observation.date == last_date;
            last_date = observation.date.clone();
            if repeat {
                decision(&[])
            } else {
                by_date(0, observation)
            }
        };
        let (_, _, evidence, _) = faulted_run(&limit_plan(), waiter, 3);
        let Some(FaultEvent::RateLimited {
            rejected_presentations,
            resubmissions_under_limit,
            waits_under_limit,
            resubmitted_after_deadline,
            ..
        }) = evidence.events.first()
        else {
            panic!("the limit fires: {:?}", evidence.events)
        };
        assert_eq!(*resubmissions_under_limit, 0);
        assert_eq!(*waits_under_limit, rejected_presentations - 1);
        assert!(!*resubmitted_after_deadline);
    }

    /// Row 32: trial state after `reset` is indistinguishable from fresh.
    #[test]
    fn reset_trial_state_equals_fresh_state() {
        for plan in [lag_plan(), sign_plan(), limit_plan()] {
            let cell = CellId::new(WINDOW, 3);
            let mut entrant = Entrant::new(by_date);
            let mut faulted = FaultInjectingAgent::new(&mut entrant, &plan, cell);
            run_backtest_capture(&data(), &mut faulted, WINDOW, 3, CostModel::default());
            let mut state = faulted.state().clone();
            let fresh = TrialFaultState::new(&plan, cell);
            assert_ne!(state, fresh, "the trial changed its state");
            state.reset();
            assert_eq!(state, fresh);
        }
    }

    /// The plan is an input only: running trials never changes it.
    #[test]
    fn a_trial_never_feeds_back_into_the_plan() {
        let plan = limit_plan();
        let before = serde_json::to_vec(&plan).unwrap();
        let _ = faulted_run(&plan, by_date, 3);
        let _ = faulted_run(&plan, by_date, 4);
        assert_eq!(serde_json::to_vec(&plan).unwrap(), before);
    }

    /// The plan fixes the bounds, the cell selects the draw within them.
    #[test]
    fn the_seeded_parameters_vary_across_cells() {
        let plan = limit_plan();
        let draws: std::collections::BTreeSet<String> = (0..16u64)
            .map(|seed| {
                format!(
                    "{:?}",
                    TrialFaultState::new(&plan, CellId::new(WINDOW, seed)).armed
                )
            })
            .collect();
        assert!(draws.len() > 1);
    }

    #[test]
    fn relaxation_declarations_are_published_per_mode() {
        let plan = FaultPlan::new(
            1,
            vec![
                ContractRelaxation::ReadYourWrites,
                ContractRelaxation::PositionSignConvention,
            ],
            vec![
                FaultSpec {
                    id: "lag".to_string(),
                    group: None,
                    cohort_ppm: 1,
                    fault: FaultMode::ProjectionLag { max_lag_steps: 1 },
                },
                FaultSpec {
                    id: "sign".to_string(),
                    group: None,
                    cohort_ppm: 1,
                    fault: FaultMode::AmountSign { max_span_steps: 1 },
                },
            ],
        )
        .unwrap();
        let text = plan.entrant_declaration();
        assert!(text.contains("read-your-writes is relaxed"));
        assert!(text.contains("sign convention is relaxed"));
        assert!(!text.contains("rate limit"));
    }
}
