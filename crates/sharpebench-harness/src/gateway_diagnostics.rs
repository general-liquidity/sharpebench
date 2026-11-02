//! How an entrant struggled at the model gateway, decision by decision.
//!
//! Every finished sweep-cell attempt files its decisions in the money journal
//! ([`DecisionLog`]): how many request lines the entrant sent, how many the host
//! handed to the broker, and which reservations they started. The journal also
//! holds every reservation's route alias and settlement. [`gateway_diagnostics`]
//! joins the two into counters an operator can read without reading the
//! journal: calls, retries, requests refused by the per-decision limit, faulted
//! calls by cause, truncated answers and tokens per decision, plus a tool-use
//! breakdown of how many requests and calls each decision made and which route
//! aliases carried them.
//!
//! The report is rank-neutral. It says how an entrant used the gateway, not how
//! well it traded, and no score, rank, gate or pass^k pool reads it. It needs
//! nothing but the journal, so it can be produced for any sweep after the fact,
//! including one still running.
//!
//! Retries are not filed per call, so they are bounded rather than counted.
//! Every served request starts at most one call that is not a retry, so a
//! decision retried at least `calls - served` times. The broker retries only a
//! dispatch that failed, and a failed dispatch is never settled as priced, so
//! every retry follows a faulted call of the same decision: a decision retried
//! at most as many times as it has faulted calls before its last one. When the
//! two bounds agree, as they do for a decision with one served request, the
//! count is exact.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::gateway_journal::{
    FinishClass, GatewayJournal, JournalRecord, ReleaseReason, Settlement, UnknownCostReason,
};

/// Schema of the emitted report.
pub const GATEWAY_DIAGNOSTICS_VERSION: &str = "sharpebench.gateway-diagnostics.v1";

/// How one reservation ended, as the journal says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallEnd {
    Priced { input: u64, output: u64 },
    Unknown(UnknownCostReason),
    Released(ReleaseReason),
    Unsettled,
}

#[derive(Clone, Debug)]
struct CallFacts {
    alias: String,
    end: CallEnd,
    truncated: bool,
}

impl CallFacts {
    fn faulted(&self) -> bool {
        !matches!(self.end, CallEnd::Priced { .. })
    }

    /// Unknown cost or no settlement: what the call used is not measured.
    fn unmeasured(&self) -> bool {
        matches!(self.end, CallEnd::Unknown(_) | CallEnd::Unsettled)
    }
}

fn call_facts(journal: &GatewayJournal) -> Vec<CallFacts> {
    let mut calls: Vec<CallFacts> = Vec::new();
    // inv: calls[o] describes reservation o for every reservation visited so
    // far, with the settlement of each settled one. Ordinals are dense from 0
    // (a loaded journal is validated so), so a reservation's ordinal is its
    // index; a settlement naming no visited reservation is skipped.
    for record in journal.records() {
        match record {
            JournalRecord::Reserved { alias, .. } => calls.push(CallFacts {
                alias: alias.clone(),
                end: CallEnd::Unsettled,
                truncated: false,
            }),
            JournalRecord::Settled {
                ordinal,
                finish_reason,
                settlement,
            } => {
                if let Some(call) = calls.get_mut(*ordinal as usize) {
                    call.truncated = *finish_reason == Some(FinishClass::Length);
                    call.end = match settlement {
                        Settlement::Priced {
                            input_tokens,
                            output_tokens,
                            ..
                        } => CallEnd::Priced {
                            input: *input_tokens,
                            output: *output_tokens,
                        },
                        Settlement::Unknown { reason } => CallEnd::Unknown(*reason),
                        Settlement::Released { reason } => CallEnd::Released(*reason),
                    };
                }
            }
        }
    }
    calls
}

/// Faulted calls by cause: every call that did not end priced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct FaultCounts {
    /// The provider answered without reporting usage.
    pub usage_absent: u64,
    /// The exchange failed after the request was committed.
    pub ambiguous_after_commit: u64,
    /// The adapter returned after the host's deadline.
    pub adapter_deadline_exceeded: u64,
    /// A reservation found unsettled on resume.
    pub interrupted_before_settlement: u64,
    /// Reserved and never dispatched (a bound or a shutdown).
    pub never_dispatched: u64,
    /// Refused by the provider before billable work (a 429 or another 4xx).
    pub provider_refused_before_work: u64,
    /// No settlement at all: a call in flight, or one the process died around.
    pub unsettled: u64,
}

impl FaultCounts {
    fn add(&mut self, end: CallEnd) {
        let slot = match end {
            CallEnd::Priced { .. } => return,
            CallEnd::Unknown(UnknownCostReason::UsageAbsent) => &mut self.usage_absent,
            CallEnd::Unknown(UnknownCostReason::AmbiguousAfterCommit) => {
                &mut self.ambiguous_after_commit
            }
            CallEnd::Unknown(UnknownCostReason::AdapterDeadlineExceeded) => {
                &mut self.adapter_deadline_exceeded
            }
            CallEnd::Unknown(UnknownCostReason::InterruptedBeforeSettlement) => {
                &mut self.interrupted_before_settlement
            }
            CallEnd::Released(ReleaseReason::NeverDispatched) => &mut self.never_dispatched,
            CallEnd::Released(ReleaseReason::ProviderRefusedBeforeWork) => {
                &mut self.provider_refused_before_work
            }
            CallEnd::Unsettled => &mut self.unsettled,
        };
        *slot += 1;
    }
}

/// One decision's struggle counters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DecisionStruggle {
    pub window_index: usize,
    pub seed: u64,
    pub attempt: u32,
    /// The decision's ordinal within its attempt, from 0.
    pub decision: u32,
    /// Gateway request lines the entrant sent.
    pub requests: u32,
    /// Of those, the requests handed to the broker.
    pub served: u32,
    /// Requests refused for exceeding the per-decision request limit.
    pub refused_by_limit: u32,
    /// Provider calls started, retries included.
    pub calls: u32,
    pub priced_calls: u32,
    /// Calls that did not end priced.
    pub faulted_calls: u32,
    /// Answers the output token bound cut off.
    pub truncated_answers: u32,
    pub retries_at_least: u32,
    pub retries_at_most: u32,
    /// Provider-reported tokens, present only when at least one call was
    /// priced and none is unmeasured. Absent means unreported, never zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

/// Tokens per decision over the decisions whose tokens are measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct TokensPerDecision {
    /// Decisions that started a call and whose tokens are measured.
    pub measured_decisions: u64,
    /// Decisions that started a call and whose tokens are not measured.
    pub unmeasured_decisions: u64,
    pub input_total: u64,
    pub output_total: u64,
    pub input_max: u64,
    pub output_max: u64,
    /// `input_total / measured_decisions`, absent when nothing is measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_mean: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_mean: Option<f64>,
}

/// One route alias's share of the calls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AliasUse {
    pub alias: String,
    pub calls: u64,
    pub priced_calls: u64,
    pub faulted_calls: u64,
    pub truncated_answers: u64,
    /// Present only when at least one call was priced and none is unmeasured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

/// The struggle counters and tool-use breakdown of one journal.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GatewayDiagnostics {
    pub schema_version: &'static str,
    /// The sweep the journal pays for, when a sweep owns it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sweep_sha256: Option<String>,
    /// Sweep-cell attempts the journal has a decision log for.
    pub attempts: u64,
    pub decisions: u64,
    /// Decisions that sent at least one gateway request.
    pub decisions_with_requests: u64,
    pub requests: u64,
    pub served: u64,
    pub refused_by_limit: u64,
    pub decisions_refused_by_limit: u64,
    pub calls: u64,
    pub priced_calls: u64,
    pub faulted_calls: u64,
    pub decisions_with_faults: u64,
    pub faults: FaultCounts,
    pub retries_at_least: u64,
    pub retries_at_most: u64,
    /// Decisions certainly retried (`retries_at_least > 0`).
    pub decisions_retried_at_least: u64,
    /// Decisions possibly retried (`retries_at_most > 0`).
    pub decisions_retried_at_most: u64,
    pub truncated_answers: u64,
    pub max_requests_in_a_decision: u32,
    pub max_calls_in_a_decision: u32,
    /// Decisions by how many gateway requests they sent.
    pub requests_per_decision: BTreeMap<u32, u64>,
    /// Decisions by how many provider calls they started.
    pub calls_per_decision: BTreeMap<u32, u64>,
    pub tokens_per_decision: TokensPerDecision,
    /// Calls by route alias, in alias order.
    pub by_alias: Vec<AliasUse>,
    /// Reservations no decision log names: an attempt still running or never
    /// logged, or calls made outside a sweep. Counted in `by_alias`, not in
    /// any decision.
    pub unlogged_calls: u64,
    pub per_decision: Vec<DecisionStruggle>,
    pub rank_neutral: bool,
}

/// Provider tokens over `calls`, on the journal's measurement rule: present
/// only when at least one call was priced and none is unmeasured.
fn measured_tokens<'a>(calls: impl Iterator<Item = &'a CallFacts>) -> (Option<u64>, Option<u64>) {
    let (mut priced, mut unmeasured, mut input, mut output) = (false, false, 0_u64, 0_u64);
    for call in calls {
        unmeasured |= call.unmeasured();
        if let CallEnd::Priced {
            input: i,
            output: o,
        } = call.end
        {
            priced = true;
            input = input.saturating_add(i);
            output = output.saturating_add(o);
        }
    }
    if priced && !unmeasured {
        (Some(input), Some(output))
    } else {
        (None, None)
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The struggle counters and tool-use breakdown of `journal`.
pub fn gateway_diagnostics(journal: &GatewayJournal) -> GatewayDiagnostics {
    let calls = call_facts(journal);
    let mut logged = vec![false; calls.len()];
    let mut per_decision = Vec::new();
    for log in journal.decision_logs() {
        for (decision, entry) in (0_u32..).zip(&log.decisions) {
            let start = (entry.first_ordinal as usize).min(calls.len());
            let end = start.saturating_add(entry.calls as usize).min(calls.len());
            let own = &calls[start..end];
            for flag in &mut logged[start..end] {
                *flag = true;
            }
            let faulted = count(own.iter().filter(|c| c.faulted()).count());
            // A retry follows a faulted call of the same decision, so the last
            // call's fault never leads to one.
            let faulted_before_last = match own.split_last() {
                Some((_, before)) => count(before.iter().filter(|c| c.faulted()).count()),
                None => 0,
            };
            let (input_tokens, output_tokens) = measured_tokens(own.iter());
            per_decision.push(DecisionStruggle {
                window_index: log.window_index,
                seed: log.seed,
                attempt: log.attempt,
                decision,
                requests: entry.requests,
                served: entry.served,
                refused_by_limit: entry.requests.saturating_sub(entry.served),
                calls: entry.calls,
                priced_calls: count(own.len()) - faulted,
                faulted_calls: faulted,
                truncated_answers: count(own.iter().filter(|c| c.truncated).count()),
                retries_at_least: entry.calls.saturating_sub(entry.served),
                retries_at_most: faulted_before_last,
                input_tokens,
                output_tokens,
            });
        }
    }

    let mut faults = FaultCounts::default();
    for call in &calls {
        faults.add(call.end);
    }
    let mut aliases: BTreeMap<&str, Vec<&CallFacts>> = BTreeMap::new();
    for call in &calls {
        aliases.entry(call.alias.as_str()).or_default().push(call);
    }
    let by_alias = aliases
        .into_iter()
        .map(|(alias, own)| {
            let (input_tokens, output_tokens) = measured_tokens(own.iter().copied());
            let faulted = own.iter().filter(|c| c.faulted()).count() as u64;
            AliasUse {
                alias: alias.to_string(),
                calls: own.len() as u64,
                priced_calls: own.len() as u64 - faulted,
                faulted_calls: faulted,
                truncated_answers: own.iter().filter(|c| c.truncated).count() as u64,
                input_tokens,
                output_tokens,
            }
        })
        .collect();

    let mut requests_per_decision = BTreeMap::new();
    let mut calls_per_decision = BTreeMap::new();
    let mut tokens = TokensPerDecision::default();
    let sum = |f: fn(&DecisionStruggle) -> u32| -> u64 {
        per_decision.iter().map(|d| u64::from(f(d))).sum()
    };
    let decisions_where = |f: fn(&DecisionStruggle) -> bool| -> u64 {
        per_decision.iter().filter(|d| f(d)).count() as u64
    };
    for d in &per_decision {
        *requests_per_decision.entry(d.requests).or_insert(0) += 1;
        *calls_per_decision.entry(d.calls).or_insert(0) += 1;
        match (d.calls, d.input_tokens, d.output_tokens) {
            (0, _, _) => {}
            (_, Some(input), Some(output)) => {
                tokens.measured_decisions += 1;
                tokens.input_total = tokens.input_total.saturating_add(input);
                tokens.output_total = tokens.output_total.saturating_add(output);
                tokens.input_max = tokens.input_max.max(input);
                tokens.output_max = tokens.output_max.max(output);
            }
            _ => tokens.unmeasured_decisions += 1,
        }
    }
    if tokens.measured_decisions > 0 {
        let n = tokens.measured_decisions as f64;
        tokens.input_mean = Some(tokens.input_total as f64 / n);
        tokens.output_mean = Some(tokens.output_total as f64 / n);
    }

    GatewayDiagnostics {
        schema_version: GATEWAY_DIAGNOSTICS_VERSION,
        sweep_sha256: journal.identity.sweep_sha256.clone(),
        attempts: journal.decision_logs().len() as u64,
        decisions: per_decision.len() as u64,
        decisions_with_requests: decisions_where(|d| d.requests > 0),
        requests: sum(|d| d.requests),
        served: sum(|d| d.served),
        refused_by_limit: sum(|d| d.refused_by_limit),
        decisions_refused_by_limit: decisions_where(|d| d.refused_by_limit > 0),
        calls: calls.len() as u64,
        priced_calls: calls.iter().filter(|c| !c.faulted()).count() as u64,
        faulted_calls: calls.iter().filter(|c| c.faulted()).count() as u64,
        decisions_with_faults: decisions_where(|d| d.faulted_calls > 0),
        faults,
        retries_at_least: sum(|d| d.retries_at_least),
        retries_at_most: sum(|d| d.retries_at_most),
        decisions_retried_at_least: decisions_where(|d| d.retries_at_least > 0),
        decisions_retried_at_most: decisions_where(|d| d.retries_at_most > 0),
        truncated_answers: calls.iter().filter(|c| c.truncated).count() as u64,
        max_requests_in_a_decision: per_decision.iter().map(|d| d.requests).max().unwrap_or(0),
        max_calls_in_a_decision: per_decision.iter().map(|d| d.calls).max().unwrap_or(0),
        requests_per_decision,
        calls_per_decision,
        tokens_per_decision: tokens,
        by_alias,
        unlogged_calls: logged.iter().filter(|seen| !**seen).count() as u64,
        per_decision,
        rank_neutral: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::RateCard;
    use crate::gateway_journal::{DecisionEntry, DecisionLog, GatewayBudget, JournalIdentity};

    fn card() -> RateCard {
        RateCard::from_json(
            br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
        )
        .expect("a valid test rate card")
    }

    fn journal() -> GatewayJournal {
        GatewayJournal::new(JournalIdentity::new(
            "r".repeat(64),
            GatewayBudget {
                max_usd_nanos: 1_000_000,
                max_calls: 100,
                per_cell: None,
            },
        ))
    }

    fn priced(input_tokens: u64, output_tokens: u64) -> Settlement {
        Settlement::Priced {
            input_tokens,
            output_tokens,
            usd_nanos: (input_tokens + output_tokens).to_string(),
            cache_read_input_tokens: None,
            cache_write_input_tokens: None,
        }
    }

    fn refused() -> Settlement {
        Settlement::Released {
            reason: ReleaseReason::ProviderRefusedBeforeWork,
        }
    }

    /// Reserve and settle one call per settlement, `None` left unsettled, and
    /// return the first ordinal.
    fn calls(journal: &mut GatewayJournal, alias: &str, ends: Vec<Option<Settlement>>) -> u32 {
        let first = journal.reservation_count();
        for end in ends {
            let ordinal = journal.reserve(alias, &card(), 100);
            if let Some(settlement) = end {
                journal.settle(ordinal, settlement);
            }
        }
        first
    }

    fn entry(requests: u32, served: u32, first_ordinal: u32, calls: u32) -> DecisionEntry {
        DecisionEntry {
            requests,
            served,
            first_ordinal,
            calls,
        }
    }

    /// Two or three served requests, three calls: which call faulted decides
    /// whether the retry count is exact or only bounded.
    #[test]
    fn retries_are_bounded_by_served_requests_and_by_faults_before_the_last_call() {
        let mut j = journal();
        // A refused first dispatch, retried and priced, then a second request
        // priced first time: exactly one retry.
        let a = calls(
            &mut j,
            "m",
            vec![Some(refused()), Some(priced(1, 1)), Some(priced(1, 1))],
        );
        // Three requests, the second refused and not retried: no retry, but
        // the record cannot tell that apart from a retry of it.
        let b = calls(
            &mut j,
            "m",
            vec![Some(priced(1, 1)), Some(refused()), Some(priced(1, 1))],
        );
        // One served request whose only call faulted: no retry, exactly.
        let c = calls(&mut j, "m", vec![Some(refused())]);
        j.log_decisions(DecisionLog {
            window_index: 0,
            seed: 7,
            attempt: 0,
            decisions: vec![entry(2, 2, a, 3), entry(3, 3, b, 3), entry(1, 1, c, 1)],
        });
        let d = gateway_diagnostics(&j);
        let bounds: Vec<(u32, u32)> = d
            .per_decision
            .iter()
            .map(|x| (x.retries_at_least, x.retries_at_most))
            .collect();
        assert_eq!(bounds, [(1, 1), (0, 1), (0, 0)]);
        assert_eq!((d.retries_at_least, d.retries_at_most), (1, 2));
        assert_eq!(
            (d.decisions_retried_at_least, d.decisions_retried_at_most),
            (1, 2)
        );
        assert_eq!(d.faults.provider_refused_before_work, 3);
        assert_eq!(d.decisions_with_faults, 3);
        // A released call used nothing, so the first two decisions' tokens are
        // measured; the third started a call and priced none.
        assert_eq!(d.per_decision[0].input_tokens, Some(2));
        assert_eq!(d.per_decision[2].input_tokens, None);
        assert_eq!(d.tokens_per_decision.measured_decisions, 2);
        assert_eq!(d.tokens_per_decision.unmeasured_decisions, 1);
    }

    /// Calls no decision log names, an unsettled call and every fault cause
    /// are counted where they belong; tokens per alias follow the journal's
    /// measurement rule.
    #[test]
    fn unlogged_unsettled_and_per_alias_calls_are_counted_apart() {
        let mut j = journal();
        let first = calls(
            &mut j,
            "a",
            vec![
                Some(priced(10, 5)),
                Some(Settlement::Unknown {
                    reason: UnknownCostReason::UsageAbsent,
                }),
                Some(Settlement::Unknown {
                    reason: UnknownCostReason::AmbiguousAfterCommit,
                }),
                Some(Settlement::Unknown {
                    reason: UnknownCostReason::AdapterDeadlineExceeded,
                }),
                Some(Settlement::Unknown {
                    reason: UnknownCostReason::InterruptedBeforeSettlement,
                }),
                Some(Settlement::Released {
                    reason: ReleaseReason::NeverDispatched,
                }),
            ],
        );
        let truncated = j.reserve("b", &card(), 100);
        j.settle_answered(truncated, priced(4, 9), FinishClass::Length);
        calls(&mut j, "b", vec![Some(priced(3, 2)), None]);
        // Only the first call of alias a is in a decision.
        j.log_decisions(DecisionLog {
            window_index: 1,
            seed: 2,
            attempt: 3,
            decisions: vec![entry(1, 1, first, 1), entry(0, 0, first + 1, 0)],
        });
        let d = gateway_diagnostics(&j);
        assert_eq!(
            d.faults,
            FaultCounts {
                usage_absent: 1,
                ambiguous_after_commit: 1,
                adapter_deadline_exceeded: 1,
                interrupted_before_settlement: 1,
                never_dispatched: 1,
                provider_refused_before_work: 0,
                unsettled: 1,
            }
        );
        assert_eq!((d.calls, d.priced_calls, d.faulted_calls), (9, 3, 6));
        assert_eq!(d.unlogged_calls, 8);
        assert_eq!(d.truncated_answers, 1);
        assert_eq!(
            (d.attempts, d.decisions, d.decisions_with_requests),
            (1, 2, 1)
        );
        assert_eq!(
            d.per_decision[0],
            DecisionStruggle {
                window_index: 1,
                seed: 2,
                attempt: 3,
                decision: 0,
                requests: 1,
                served: 1,
                refused_by_limit: 0,
                calls: 1,
                priced_calls: 1,
                faulted_calls: 0,
                truncated_answers: 0,
                retries_at_least: 0,
                retries_at_most: 0,
                input_tokens: Some(10),
                output_tokens: Some(5),
            }
        );
        // A decision that sent nothing is in the breakdown at zero, and has no
        // tokens to be measured or unmeasured.
        assert_eq!(d.requests_per_decision, BTreeMap::from([(0, 1), (1, 1)]));
        assert_eq!(d.calls_per_decision, BTreeMap::from([(0, 1), (1, 1)]));
        assert_eq!(
            d.tokens_per_decision,
            TokensPerDecision {
                measured_decisions: 1,
                unmeasured_decisions: 0,
                input_total: 10,
                output_total: 5,
                input_max: 10,
                output_max: 5,
                input_mean: Some(10.0),
                output_mean: Some(5.0),
            }
        );
        assert_eq!(
            d.by_alias,
            [
                AliasUse {
                    alias: "a".into(),
                    calls: 6,
                    priced_calls: 1,
                    faulted_calls: 5,
                    truncated_answers: 0,
                    input_tokens: None,
                    output_tokens: None,
                },
                AliasUse {
                    alias: "b".into(),
                    calls: 3,
                    priced_calls: 2,
                    faulted_calls: 1,
                    truncated_answers: 1,
                    input_tokens: None,
                    output_tokens: None,
                },
            ]
        );
        assert!(d.rank_neutral);
        assert_eq!(d.sweep_sha256, None);
    }

    #[test]
    fn an_empty_journal_reports_zeros_and_no_means() {
        let d = gateway_diagnostics(&journal());
        assert_eq!((d.decisions, d.calls, d.unlogged_calls), (0, 0, 0));
        assert_eq!(d.tokens_per_decision, TokensPerDecision::default());
        assert!(d.by_alias.is_empty() && d.per_decision.is_empty());
        assert_eq!(
            (d.max_requests_in_a_decision, d.max_calls_in_a_decision),
            (0, 0)
        );
    }
}
