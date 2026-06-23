//! WASM façade over [`sharpebench_core`] (+ the canary primitive from
//! [`sharpebench_attest`]) — the bridge that lets Gordon (TypeScript/Bun) and the
//! published `@general-liquidity/sharpebench` npm package consume the **identical**
//! scoring kernel as the harness, so the internal eval and the public benchmark
//! can never drift.
//!
//! Every entry point is a pure JSON-in / JSON-out function with a host-testable
//! `*_json` core and, under `wasm32`, a `wasm-bindgen` export of the same name.
//! There is exactly one implementation of the scoring math; this only marshals.
#![forbid(unsafe_code)]

use sharpebench_core::{
    audit_briefing, bs_greeks, bs_price, classify_greeks_risk, rank, score_agent, AgentSubmission,
    AllocationPolicy, AllocationTrajectory, Briefing, BriefingPolicy, GreeksPolicy, ScoreConfig,
};

/// Parse an optional config blob: blank → `T::default()`.
fn parse_or_default<T: serde::de::DeserializeOwned + Default>(json: &str) -> Result<T, String> {
    if json.trim().is_empty() {
        Ok(T::default())
    } else {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }
}

/// Score and rank a JSON array of submissions → JSON array of `CompositeScore`.
/// Blank `config_json` uses the defaults.
pub fn score_json(submissions_json: &str, config_json: &str) -> Result<String, String> {
    let subs: Vec<AgentSubmission> =
        serde_json::from_str(submissions_json).map_err(|e| e.to_string())?;
    let cfg: ScoreConfig = parse_or_default(config_json)?;
    serde_json::to_string(&rank(&subs, &cfg)).map_err(|e| e.to_string())
}

/// Score a single submission → one `CompositeScore` (carries the deflated Sharpe,
/// pass^k verdict, process score, rolling worst-case Sharpe, etc.).
pub fn score_agent_json(submission_json: &str, config_json: &str) -> Result<String, String> {
    let sub: AgentSubmission = serde_json::from_str(submission_json).map_err(|e| e.to_string())?;
    let cfg: ScoreConfig = parse_or_default(config_json)?;
    serde_json::to_string(&score_agent(&sub, &cfg)).map_err(|e| e.to_string())
}

/// Run the benchmark self-audit (fires the known gaming attacks at the scorer) →
/// `SelfAuditReport` JSON. Takes no input.
pub fn self_audit_json() -> Result<String, String> {
    serde_json::to_string(&sharpebench_core::run_self_audit()).map_err(|e| e.to_string())
}

/// Audit a shared briefing for input-side salience bias → `BriefingAudit` JSON.
/// Blank `policy_json` uses the default policy.
pub fn audit_briefing_json(briefing_json: &str, policy_json: &str) -> Result<String, String> {
    let briefing: Briefing = serde_json::from_str(briefing_json).map_err(|e| e.to_string())?;
    let policy: BriefingPolicy = parse_or_default(policy_json)?;
    serde_json::to_string(&audit_briefing(&briefing, &policy)).map_err(|e| e.to_string())
}

/// Score a target-allocation trajectory (validity + turnover) → `AllocationReport`
/// JSON. Blank `policy_json` uses the default policy.
pub fn score_allocation_json(trajectory_json: &str, policy_json: &str) -> Result<String, String> {
    let traj: AllocationTrajectory =
        serde_json::from_str(trajectory_json).map_err(|e| e.to_string())?;
    let policy: AllocationPolicy = parse_or_default(policy_json)?;
    serde_json::to_string(&sharpebench_core::score_allocation(&traj, &policy))
        .map_err(|e| e.to_string())
}

/// Black-Scholes price + Greeks + tail-risk classification for one option. Input
/// JSON: `{spot, strike, t_years, rate, vol, is_call}`. Output JSON:
/// `{price, greeks, risk}`.
pub fn greeks_json(params_json: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(params_json).map_err(|e| e.to_string())?;
    let num = |k: &str| -> Result<f64, String> {
        v.get(k)
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| format!("missing or non-numeric field: {k}"))
    };
    let (spot, strike, t, r, vol) = (
        num("spot")?,
        num("strike")?,
        num("t_years")?,
        num("rate")?,
        num("vol")?,
    );
    let is_call = v
        .get("is_call")
        .and_then(serde_json::Value::as_bool)
        .ok_or("missing or non-boolean field: is_call")?;
    let price = bs_price(spot, strike, t, r, vol, is_call);
    let greeks = bs_greeks(spot, strike, t, r, vol, is_call);
    let risk = classify_greeks_risk(&greeks, &GreeksPolicy::default());
    serde_json::to_string(&serde_json::json!({ "price": price, "greeks": greeks, "risk": risk }))
        .map_err(|e| e.to_string())
}

/// Derive a deterministic do-not-train contamination tripwire from seed material →
/// `Canary` JSON `{id, token}`.
pub fn canary_json(seed: &str) -> Result<String, String> {
    serde_json::to_string(&sharpebench_attest::make_canary(seed.as_bytes()))
        .map_err(|e| e.to_string())
}

/// The wasm-bindgen exports. Each returns the result JSON, or a `{"error":"..."}`
/// JSON object on failure (never throws across the boundary).
#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::wasm_bindgen;

    fn wrap(r: Result<String, String>) -> String {
        match r {
            Ok(s) => s,
            Err(e) => format!(
                "{{\"error\":{}}}",
                serde_json::to_string(&e).unwrap_or_default()
            ),
        }
    }

    #[wasm_bindgen]
    pub fn score(submissions_json: &str, config_json: &str) -> String {
        wrap(super::score_json(submissions_json, config_json))
    }

    #[wasm_bindgen]
    pub fn score_agent(submission_json: &str, config_json: &str) -> String {
        wrap(super::score_agent_json(submission_json, config_json))
    }

    #[wasm_bindgen]
    pub fn self_audit() -> String {
        wrap(super::self_audit_json())
    }

    #[wasm_bindgen]
    pub fn audit_briefing(briefing_json: &str, policy_json: &str) -> String {
        wrap(super::audit_briefing_json(briefing_json, policy_json))
    }

    #[wasm_bindgen]
    pub fn score_allocation(trajectory_json: &str, policy_json: &str) -> String {
        wrap(super::score_allocation_json(trajectory_json, policy_json))
    }

    #[wasm_bindgen]
    pub fn greeks(params_json: &str) -> String {
        wrap(super::greeks_json(params_json))
    }

    #[wasm_bindgen]
    pub fn canary(seed: &str) -> String {
        wrap(super::canary_json(seed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_json_roundtrips_and_ranks() {
        let subs = r#"[
            {"agent_id":"skilled","runs":[
                {"returns":[0.002,0.0021,0.0019,0.002,0.0022,0.0018,0.002,0.0021,0.0019,0.002]},
                {"returns":[0.002,0.0019,0.0021,0.002,0.0018,0.0022,0.002,0.0019,0.0021,0.002]}
            ]},
            {"agent_id":"flat","runs":[{"returns":[0.0,0.0,0.0,0.0,0.0]}]}
        ]"#;
        let out = score_json(subs, "").expect("scores");
        let si = out.find("skilled").unwrap();
        let fi = out.find("flat").unwrap();
        assert!(si < fi, "skilled should rank ahead of flat");
    }

    #[test]
    fn score_agent_emits_a_composite() {
        let sub = r#"{"agent_id":"a","runs":[{"returns":[0.002,0.0021,0.0019,0.002,0.0022]}]}"#;
        let out = score_agent_json(sub, "").expect("score_agent");
        assert!(out.contains("\"agent_id\":\"a\""));
        assert!(out.contains("deflated_sharpe"));
    }

    #[test]
    fn self_audit_reports_all_defended() {
        let out = self_audit_json().expect("self_audit");
        assert!(out.contains("all_defended"));
    }

    #[test]
    fn greeks_prices_an_atm_call() {
        let out = greeks_json(
            r#"{"spot":100,"strike":100,"t_years":1,"rate":0.05,"vol":0.2,"is_call":true}"#,
        )
        .expect("greeks");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let price = v["price"].as_f64().unwrap();
        assert!((price - 10.4506).abs() < 1e-2, "price={price}");
    }

    #[test]
    fn audit_briefing_and_allocation_and_canary_bridge() {
        // Empty briefing audits as balanced.
        let b = audit_briefing_json(r#"{"sections":[]}"#, "").expect("briefing");
        assert!(b.contains("\"balanced\":true"));
        // A single valid step has zero turnover beyond initial deployment.
        let a = score_allocation_json(r#"{"steps":[{"weights":[1.0]}]}"#, "").expect("alloc");
        assert!(a.contains("\"valid\":true"));
        // Canary derives a stable token.
        let c = canary_json("scenario-1").expect("canary");
        assert!(c.contains("\"token\""));
    }

    #[test]
    fn bad_json_is_an_error_not_a_panic() {
        assert!(score_json("not json", "").is_err());
        assert!(greeks_json("{}").is_err());
    }
}
