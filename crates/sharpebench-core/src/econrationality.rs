//! Economic-rationality litmus tests (after EconEvals).
//!
//! A high return can come from a model that is economically *irrational* — one
//! that pays more for a strictly worse deal, or holds intransitive preferences a
//! counterparty can money-pump. These checks are orthogonal to P&L: they ask
//! whether the agent's choices are internally **coherent**, which is what you
//! actually need before trusting an agent to act with capital under novel prices.
//!
//! Pure analyzer plus a deterministic **elicitor over recorded submissions**
//! ([`elicit_revealed_selection`]). A frozen submission records exactly one
//! economic choice: the agent declared a set of candidate strategies
//! (`AgentSubmission::candidates`, each a pooled return stream) and chose to
//! submit one track out of that set. That is a revealed preference, and the
//! elicitor turns it into a [`DominanceChoice`] over per-period Sharpe values
//! (a risk-adjusted scalar, so a deliberately lower-return / lower-risk pick is
//! not mislabeled irrational). Submitting a track when a declared candidate had
//! a strictly higher Sharpe is a first-order-dominance flag, coarse and
//! reported-only: mandate constraints (e.g. a drawdown cap) can make that pick
//! rational, which is one reason this axis must not gate eligibility.
//!
//! **What cannot be derived from a recorded trace, and what would need to be
//! recorded first.** Per-decision dominance choices need each decision's
//! *considered alternatives with their values* (e.g. a
//! `ChoiceConsidered { options: Vec<f64>, chosen: usize }` trace event); the
//! trace records neither rejected orders nor their values. The money-pump test
//! ([`has_money_pump`]) needs revealed *pairwise* preferences, i.e. per-decision
//! records of which alternative was rejected in favor of which; inferring them
//! from order sequencing would invent preferences that changing market
//! information can fully explain. Until such events are recorded, those two
//! remain live-elicitation-only (`sharpebench-harness` runs them against a live
//! agent) and a frozen score reports only the selection-dominance axis.
//! Deterministic, no I/O.

use serde::Serialize;

use crate::deflated_sharpe::sharpe_ratio;
use crate::stats::std_dev;

/// A single choice among options with known scalar value (e.g. expected return,
/// already net of stated risk). `chosen` indexes into `options`.
#[derive(Clone, Debug)]
pub struct DominanceChoice {
    pub options: Vec<f64>,
    pub chosen: usize,
}

impl DominanceChoice {
    /// True if a strictly better option than the chosen one was available — the
    /// agent left value on the table (a first-order-dominance violation).
    pub fn is_dominated(&self) -> bool {
        match self.options.get(self.chosen) {
            Some(&picked) => self.options.iter().any(|&v| v > picked),
            None => false,
        }
    }
}

/// Fraction of choices that respected dominance (1.0 = perfectly rational). Empty
/// input scores 1.0 — nothing irrational happened.
pub fn rationality_score(choices: &[DominanceChoice]) -> f64 {
    if choices.is_empty() {
        return 1.0;
    }
    let bad = choices.iter().filter(|c| c.is_dominated()).count();
    1.0 - bad as f64 / choices.len() as f64
}

/// Detect a money-pump: a cycle in revealed strict preferences where `(a, b)`
/// means "a is preferred to b". An intransitive cycle (A≻B≻C≻A) lets a
/// counterparty extract value by walking the agent around the loop. `n_items`
/// bounds the node set; out-of-range edges are ignored.
pub fn has_money_pump(prefs: &[(usize, usize)], n_items: usize) -> bool {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n_items];
    for &(a, b) in prefs {
        if a < n_items && b < n_items {
            adj[a].push(b);
        }
    }
    // Three-colour DFS cycle detection (0 = unseen, 1 = on stack, 2 = done).
    fn visit(u: usize, adj: &[Vec<usize>], color: &mut [u8]) -> bool {
        color[u] = 1;
        for &v in &adj[u] {
            if color[v] == 1 || (color[v] == 0 && visit(v, adj, color)) {
                return true;
            }
        }
        color[u] = 2;
        false
    }
    let mut color = vec![0u8; n_items];
    (0..n_items).any(|u| color[u] == 0 && visit(u, &adj, &mut color))
}

/// Elicit the one economic choice a frozen submission records: the agent chose
/// to submit `submitted` (its pooled return stream) out of the candidate set it
/// declared. Options are valued by **per-period Sharpe** (the risk-adjusted
/// scalar the benchmark ranks on), so a lower-return but higher-Sharpe pick is
/// respected as rational.
///
/// Returns `None` when nothing is elicitable: no declared candidates, no
/// candidate with a finite Sharpe (a constant stream has none), or a submitted
/// stream too short/degenerate for a finite Sharpe. Deterministic: candidate
/// order is preserved and the submitted track is always the final (chosen)
/// option.
pub fn elicit_revealed_selection(
    candidates: &[Vec<f64>],
    submitted: &[f64],
) -> Option<DominanceChoice> {
    // A Sharpe is comparable only on a stream with real dispersion:
    // `sharpe_ratio` returns a sentinel 0.0 on zero variance, which would
    // misvalue a riskless drift, so degenerate streams are excluded outright.
    fn comparable_sharpe(returns: &[f64]) -> Option<f64> {
        if returns.len() < 2 || std_dev(returns) == 0.0 {
            return None;
        }
        let s = sharpe_ratio(returns);
        s.is_finite().then_some(s)
    }
    let submitted_sharpe = comparable_sharpe(submitted)?;
    let mut options: Vec<f64> = candidates
        .iter()
        .filter_map(|c| comparable_sharpe(c))
        .collect();
    if options.is_empty() {
        return None;
    }
    options.push(submitted_sharpe);
    let chosen = options.len() - 1;
    Some(DominanceChoice { options, chosen })
}

/// A combined economic-rationality verdict over an agent's elicited choices.
#[derive(Clone, Debug, Serialize)]
pub struct EconRationalityReport {
    /// Share of choices that respected first-order dominance, in [0, 1].
    pub rationality_score: f64,
    /// Count of choices where a strictly better option was passed over.
    pub dominance_violations: usize,
    /// Whether the revealed preferences contain an exploitable intransitive cycle.
    pub has_money_pump: bool,
}

/// Assess an agent's economic rationality from its dominance choices and revealed
/// pairwise preferences.
pub fn assess_rationality(
    choices: &[DominanceChoice],
    prefs: &[(usize, usize)],
    n_items: usize,
) -> EconRationalityReport {
    EconRationalityReport {
        rationality_score: rationality_score(choices),
        dominance_violations: choices.iter().filter(|c| c.is_dominated()).count(),
        has_money_pump: has_money_pump(prefs, n_items),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(options: &[f64], chosen: usize) -> DominanceChoice {
        DominanceChoice {
            options: options.to_vec(),
            chosen,
        }
    }

    #[test]
    fn always_picking_the_best_is_rational() {
        let choices = vec![choice(&[0.1, 0.3, 0.2], 1), choice(&[0.5, 0.4], 0)];
        assert_eq!(rationality_score(&choices), 1.0);
        assert!(!choices[0].is_dominated());
    }

    #[test]
    fn leaving_value_on_the_table_is_a_violation() {
        let choices = vec![
            choice(&[0.1, 0.3, 0.2], 0), // 0.3 was available
            choice(&[0.5, 0.4], 0),      // fine
        ];
        assert_eq!(rationality_score(&choices), 0.5);
        assert!(choices[0].is_dominated());
    }

    #[test]
    fn intransitive_preferences_are_a_money_pump() {
        assert!(has_money_pump(&[(0, 1), (1, 2), (2, 0)], 3));
    }

    #[test]
    fn transitive_preferences_have_no_pump() {
        assert!(!has_money_pump(&[(0, 1), (1, 2), (0, 2)], 3));
    }

    /// A steady drift with a small deterministic wiggle so the Sharpe is finite.
    fn stream(mean_ret: f64, amp: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| mean_ret + amp * (i as f64 * 0.7).sin())
            .collect()
    }

    #[test]
    fn elicitor_is_deterministic() {
        let candidates = vec![stream(0.001, 0.002, 60), stream(0.004, 0.002, 60)];
        let submitted = stream(0.002, 0.002, 60);
        let a = elicit_revealed_selection(&candidates, &submitted).expect("elicitable");
        let b = elicit_revealed_selection(&candidates, &submitted).expect("elicitable");
        assert_eq!(a.options.len(), b.options.len());
        for (x, y) in a.options.iter().zip(&b.options) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
        assert_eq!(a.chosen, b.chosen);
    }

    #[test]
    fn submitting_below_a_declared_candidate_is_a_dominance_violation() {
        // A declared candidate with a strictly higher Sharpe than the submitted
        // track: the recorded selection left risk-adjusted value on the table.
        let candidates = vec![stream(0.004, 0.002, 60)];
        let submitted = stream(0.001, 0.002, 60);
        let choice = elicit_revealed_selection(&candidates, &submitted).expect("elicitable");
        assert_eq!(choice.chosen, choice.options.len() - 1);
        assert!(choice.is_dominated());
        assert_eq!(rationality_score(&[choice]), 0.0);

        // Submitting the best of one's own candidate set is rational.
        let best =
            elicit_revealed_selection(&candidates, &stream(0.006, 0.002, 60)).expect("elicitable");
        assert!(!best.is_dominated());
    }

    #[test]
    fn elicitor_declines_when_nothing_is_recorded() {
        let submitted = stream(0.002, 0.002, 60);
        assert!(elicit_revealed_selection(&[], &submitted).is_none());
        // Constant candidate streams carry no comparable Sharpe (0.5 is binary
        // exact, so the sample variance is exactly zero).
        assert!(elicit_revealed_selection(&[vec![0.5; 60]], &submitted).is_none());
        // A degenerate submitted stream is likewise not elicitable.
        assert!(elicit_revealed_selection(std::slice::from_ref(&submitted), &[0.5; 60]).is_none());
    }

    #[test]
    fn combined_report() {
        let choices = vec![choice(&[0.2, 0.1], 1)]; // dominated
        let r = assess_rationality(&choices, &[(0, 1), (1, 0)], 2);
        assert_eq!(r.dominance_violations, 1);
        assert!(r.has_money_pump);
        assert_eq!(r.rationality_score, 0.0);
    }
}
