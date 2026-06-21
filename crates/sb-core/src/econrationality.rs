//! Economic-rationality litmus tests (after EconEvals).
//!
//! A high return can come from a model that is economically *irrational* — one
//! that pays more for a strictly worse deal, or holds intransitive preferences a
//! counterparty can money-pump. These checks are orthogonal to P&L: they ask
//! whether the agent's choices are internally **coherent**, which is what you
//! actually need before trusting an agent to act with capital under novel prices.
//!
//! Pure analyzer: the caller supplies the agent's choices (a producer that elicits
//! them from a live agent is a separate, deferred piece — same staging as
//! [`crate::roles`] before its harness existed). Deterministic, no I/O.

use serde::Serialize;

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

    #[test]
    fn combined_report() {
        let choices = vec![choice(&[0.2, 0.1], 1)]; // dominated
        let r = assess_rationality(&choices, &[(0, 1), (1, 0)], 2);
        assert_eq!(r.dominance_violations, 1);
        assert!(r.has_money_pump);
        assert_eq!(r.rationality_score, 0.0);
    }
}
