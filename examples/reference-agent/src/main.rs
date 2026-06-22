//! Reference SharpeBench agent (Rust) — the simplest thing that honors the protocol.
//!
//! Transport: **stdio**. Reads one [`MarketObservation`] (JSON) per line on stdin
//! and writes one [`Decision`] (JSON) per line on stdout. Strategy: equal-weight
//! buy-and-hold — the baseline every real agent must beat. Fork it, replace
//! [`decide`]. Rust agents can depend on `sharpebench-protocol` for the typed
//! contract; any other language just matches the JSON shapes in `README.md`.
//!
//!   cargo run -p reference-agent          # then feed it MarketObservation JSON lines

use std::io::{self, BufRead, Write};

use sharpebench_protocol::{Action, Decision, MarketObservation, Order};

/// `MarketObservation` -> `Decision`. Replace this body with your strategy.
fn decide(obs: &MarketObservation) -> Decision {
    let weight = 1.0 / obs.symbols.len().max(1) as f64;
    let orders = obs
        .symbols
        .iter()
        .map(|s| Order {
            symbol: s.symbol.clone(),
            action: Action::Buy,
            target_weight: weight,
            confidence: 0.5,
        })
        .collect();
    Decision {
        orders,
        reasoning: "equal-weight buy-and-hold".to_string(),
    }
}

fn hold(reason: &str) -> Decision {
    Decision {
        orders: Vec::new(),
        reasoning: reason.to_string(),
    }
}

fn main() {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Any bad input degrades to an empty-orders hold — never crashes the harness.
        let decision = serde_json::from_str::<MarketObservation>(line)
            .map(|obs| decide(&obs))
            .unwrap_or_else(|_| hold("parse error -> hold"));
        if let Ok(json) = serde_json::to_string(&decision) {
            let _ = writeln!(out, "{json}");
            let _ = out.flush(); // flush each line — the loop is line-synchronous
        }
    }
}
