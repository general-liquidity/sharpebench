//! Multi-agent role attribution — which role in a trading team adds skill?
//!
//! A team submission (analyst, risk manager, PM, …) produces a team return plus
//! a return/signal series per role. We regress the team return on each role to
//! estimate that role's loading on the team outcome — a cheap, deterministic way
//! to see which role is load-bearing and which is dead weight. (After the
//! TradingAgents multi-agent firm structure.)

use serde::Serialize;

use crate::attribution::alpha_beta;
use crate::stats::mean;

/// One role's contribution to a team.
#[derive(Clone, Debug, Serialize)]
pub struct RoleContribution {
    pub role: String,
    /// Regression beta of the team return on this role — how much the team moves
    /// per unit of this role's signal. Near 0 ⇒ the role isn't load-bearing.
    pub beta_to_team: f64,
    pub mean_return: f64,
}

/// Attribute a team's return to its roles.
pub fn attribute_roles(team: &[f64], roles: &[(String, Vec<f64>)]) -> Vec<RoleContribution> {
    roles
        .iter()
        .map(|(name, r)| {
            let (_, beta) = alpha_beta(team, r);
            RoleContribution {
                role: name.clone(),
                beta_to_team: beta,
                mean_return: mean(r),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_bearing_role_dominates() {
        let team: Vec<f64> = (0..40).map(|i| 0.001 * (i as f64 * 0.3).sin()).collect();
        let roles = vec![
            ("driver".to_string(), team.clone()),
            (
                "noise".to_string(),
                (0..40).map(|i| 0.001 * (i as f64 * 1.7).cos()).collect(),
            ),
        ];
        let attr = attribute_roles(&team, &roles);
        assert!(
            (attr[0].beta_to_team - 1.0).abs() < 1e-6,
            "driver={:?}",
            attr[0]
        );
        assert!(
            attr[0].beta_to_team.abs() > attr[1].beta_to_team.abs(),
            "driver should out-load noise"
        );
    }
}
