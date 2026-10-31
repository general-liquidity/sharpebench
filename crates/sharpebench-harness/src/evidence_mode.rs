//! Rank-neutral disclosure of what kind of evidence a row's verdict is, derived
//! mechanically from facts the host holds rather than from anything an entrant
//! declares.
//!
//! Two facts decide it. The first is whether the scored window is historical or
//! forward: a forward window's bars did not exist when the entrant was
//! committed, a historical window's did. The second is what the host knows
//! about the entrant's access to information beyond the observations it was
//! handed:
//!
//! - **Model calls.** The paper's protocol rule is that a backtest verdict for
//!   a pretrained model is advisory only, because the model's weights may
//!   already hold the window's prices. Declared training cutoffs are not
//!   trusted, so the rule is applied from what the host observed: a historical
//!   window plus model calls the gateway saw is `backtest_advisory`. When the
//!   host could not observe model use at all (a host process or an operator
//!   endpoint with no gateway), the mode is `backtest_unobserved`, never the
//!   plain `backtest` a reader would take as "no model was used".
//! - **Network reach.** A runner that leaves the network open lets the entrant
//!   fetch post-window prices on a historical window. Such a row does not
//!   certify. A forward window is unaffected: its future bars are not published
//!   anywhere yet.
//!
//! Nothing here reaches a score, a rank or the pass^k pool. Pure: no I/O, no
//! clock.

use serde::Serialize;
use sharpebench_sim::external::CellIsolation;

/// Schema of the object [`disclosure`] publishes on a row.
pub const EVIDENCE_MODE_VERSION: &str = "sharpebench.evidence-mode.v1";

/// Whether the scored window's bars existed when the entrant was fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowTiming {
    /// The bars were published before the run: a backtest.
    Historical,
    /// The bars were revealed after the entrant was committed.
    Forward,
}

/// What the host knows about the entrant's model calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelCalls {
    /// The host's gateway served the entrant and saw this many calls reach, or
    /// possibly reach, a provider.
    Observed(u32),
    /// The runner gave the entrant no route to any model: a container with no
    /// network and no gateway attached.
    Unreachable,
    /// The host has no record of model use either way.
    Unobserved,
}

/// The evidence mode of one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceMode {
    /// A forward window: the verdict is evidence in its own right.
    Forward,
    /// A historical window with no model calls observed, on a runner where the
    /// host would have seen them.
    Backtest,
    /// A historical window on which the gateway observed model calls: advisory
    /// only, by the paper's protocol rule for pretrained models.
    BacktestAdvisory,
    /// A historical window on which the host could not observe whether a model
    /// was used. Not read as `backtest`.
    BacktestUnobserved,
}

/// The mode, from the window timing and the model-call evidence.
pub fn evidence_mode(timing: WindowTiming, calls: ModelCalls) -> EvidenceMode {
    match (timing, calls) {
        (WindowTiming::Forward, _) => EvidenceMode::Forward,
        (WindowTiming::Historical, ModelCalls::Observed(0) | ModelCalls::Unreachable) => {
            EvidenceMode::Backtest
        }
        (WindowTiming::Historical, ModelCalls::Observed(_)) => EvidenceMode::BacktestAdvisory,
        (WindowTiming::Historical, ModelCalls::Unobserved) => EvidenceMode::BacktestUnobserved,
    }
}

/// Whether a row on this window and runner may certify. A historical window
/// with a network-reachable entrant does not: the entrant could have fetched
/// the prices it was scored on. `None` is a runner whose network reach is
/// unknown, which fails closed on a historical window. A forward window is
/// unaffected by network reach.
pub fn network_permits_certification(
    timing: WindowTiming,
    isolation: Option<CellIsolation>,
) -> bool {
    match timing {
        WindowTiming::Forward => true,
        WindowTiming::Historical => isolation.is_some_and(|class| !class.network_reachable()),
    }
}

/// The object published on the entrant's row, beside `cell_isolation`.
pub fn disclosure(
    timing: WindowTiming,
    isolation: Option<CellIsolation>,
    calls: ModelCalls,
) -> serde_json::Value {
    let mode = evidence_mode(timing, calls);
    let certifying = network_permits_certification(timing, isolation);
    let mut reasons = Vec::new();
    if !certifying {
        reasons.push("historical window with a network-reachable entrant");
    }
    serde_json::json!({
        "schema_version": EVIDENCE_MODE_VERSION,
        "window": timing,
        "mode": mode,
        "model_calls_observed": match calls {
            ModelCalls::Observed(n) => serde_json::json!(n),
            ModelCalls::Unreachable | ModelCalls::Unobserved => serde_json::Value::Null,
        },
        "model_reachable": !matches!(calls, ModelCalls::Unreachable),
        "network_reachable": isolation.map(CellIsolation::network_reachable),
        "certifying": certifying,
        "noncertifying_reasons": reasons,
        "rank_neutral": true,
    })
}

/// Put the disclosure on the entrant's row of a JSON board. The board stays an
/// array and every other field is untouched.
pub fn attach(board: &mut serde_json::Value, agent_id: &str, disclosure: &serde_json::Value) {
    if let Some(rows) = board.as_array_mut() {
        for row in rows {
            if row["agent_id"].as_str() == Some(agent_id) {
                row["evidence_mode"] = disclosure.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNERS: [CellIsolation; 3] = [
        CellIsolation::ContainerPerCell,
        CellIsolation::ProcessPerCellHostWritable,
        CellIsolation::OperatorEndpoint,
    ];

    /// The paper rule, applied from observation: a historical window plus any
    /// gateway-observed model call is advisory, and a host that could not
    /// observe model use never reports a plain backtest.
    #[test]
    fn the_mode_follows_the_window_and_the_observed_calls() {
        use ModelCalls::{Observed, Unobserved, Unreachable};
        use WindowTiming::{Forward, Historical};
        for (timing, calls, expected) in [
            (Historical, Observed(3), EvidenceMode::BacktestAdvisory),
            (Historical, Observed(0), EvidenceMode::Backtest),
            (Historical, Unreachable, EvidenceMode::Backtest),
            (Historical, Unobserved, EvidenceMode::BacktestUnobserved),
            (Forward, Observed(3), EvidenceMode::Forward),
            (Forward, Unobserved, EvidenceMode::Forward),
        ] {
            assert_eq!(
                evidence_mode(timing, calls),
                expected,
                "{timing:?} {calls:?}"
            );
        }
    }

    /// Only the `--network none` container certifies on a historical window;
    /// on a forward window every runner does, and an unknown runner fails
    /// closed on a historical one.
    #[test]
    fn a_network_reachable_entrant_does_not_certify_a_historical_window() {
        for class in RUNNERS {
            assert_eq!(
                network_permits_certification(WindowTiming::Historical, Some(class)),
                !class.network_reachable(),
                "{class:?}"
            );
            assert!(network_permits_certification(
                WindowTiming::Forward,
                Some(class)
            ));
        }
        assert!(!CellIsolation::ContainerPerCell.network_reachable());
        assert!(CellIsolation::ProcessPerCellHostWritable.network_reachable());
        assert!(CellIsolation::OperatorEndpoint.network_reachable());
        assert!(!network_permits_certification(
            WindowTiming::Historical,
            None
        ));
        assert!(network_permits_certification(WindowTiming::Forward, None));
    }

    #[test]
    fn the_disclosure_names_why_a_row_does_not_certify() {
        let value = disclosure(
            WindowTiming::Historical,
            Some(CellIsolation::OperatorEndpoint),
            ModelCalls::Unobserved,
        );
        assert_eq!(value["schema_version"], EVIDENCE_MODE_VERSION);
        assert_eq!(value["mode"], "backtest_unobserved");
        assert_eq!(value["network_reachable"], true);
        assert_eq!(value["certifying"], false);
        assert_eq!(value["model_calls_observed"], serde_json::Value::Null);
        assert_eq!(value["rank_neutral"], true);
        assert_eq!(
            value["noncertifying_reasons"].as_array().map(Vec::len),
            Some(1)
        );

        let sealed = disclosure(
            WindowTiming::Historical,
            Some(CellIsolation::ContainerPerCell),
            ModelCalls::Observed(2),
        );
        assert_eq!(sealed["mode"], "backtest_advisory");
        assert_eq!(sealed["model_calls_observed"], 2);
        assert_eq!(sealed["certifying"], true);
        assert_eq!(sealed["noncertifying_reasons"], serde_json::json!([]));
    }

    #[test]
    fn attaching_touches_only_the_named_row() {
        let mut board = serde_json::json!([{"agent_id": "a"}, {"agent_id": "b"}]);
        let value = disclosure(WindowTiming::Forward, None, ModelCalls::Unobserved);
        attach(&mut board, "b", &value);
        assert!(board[0].get("evidence_mode").is_none());
        assert_eq!(board[1]["evidence_mode"], value);
    }
}
