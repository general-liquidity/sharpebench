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

/// Parse a partial `HonestyConfig` blob: `n_trials` is required; the rest default
/// (`trials_sr_std` → null, `confidence` → 0.95, `borderline` → 0.90,
/// `sr_benchmark` → 0.0). Built field-by-field so callers can pass just
/// `{"n_trials": N}`.
fn parse_honesty_config(json: &str) -> Result<sharpebench_edge::HonestyConfig, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let n_trials = v
        .get("n_trials")
        .and_then(serde_json::Value::as_u64)
        .ok_or("missing or non-integer field: n_trials")? as u32;
    let trials_sr_std = match v.get("trials_sr_std") {
        None | Some(serde_json::Value::Null) => None,
        Some(x) => Some(x.as_f64().ok_or("non-numeric field: trials_sr_std")?),
    };
    let confidence = v
        .get("confidence")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.95);
    let borderline = v
        .get("borderline")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.90);
    let sr_benchmark = v
        .get("sr_benchmark")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    Ok(sharpebench_edge::HonestyConfig {
        n_trials,
        trials_sr_std,
        confidence,
        borderline,
        sr_benchmark,
    })
}

/// LITE backtest-honesty verdict: "is my Sharpe real, or an artifact of luck and
/// multiple testing?" Input: a JSON array of per-period returns + a (partial)
/// `HonestyConfig`. Output: `HonestyVerdict` JSON.
pub fn is_my_sharpe_real_json(returns_json: &str, config_json: &str) -> Result<String, String> {
    let returns: Vec<f64> = serde_json::from_str(returns_json).map_err(|e| e.to_string())?;
    let cfg = parse_honesty_config(config_json)?;
    serde_json::to_string(&sharpebench_edge::is_my_sharpe_real(&returns, &cfg))
        .map_err(|e| e.to_string())
}

/// FULL backtest-honesty verdict: the winner's LITE verdict plus the multiple-
/// testing family (Reality Check / SPA / step-down) and PBO over the whole field.
/// Input: a JSON N×T field (rows = candidate strategies), the winner's row index,
/// and a (partial) `HonestyConfig`. Output: `FullVerdict` JSON.
pub fn is_my_sharpe_real_full_json(
    field_json: &str,
    winner_idx: usize,
    config_json: &str,
) -> Result<String, String> {
    let field: Vec<Vec<f64>> = serde_json::from_str(field_json).map_err(|e| e.to_string())?;
    if winner_idx >= field.len() {
        return Err(format!(
            "winner_idx {winner_idx} out of bounds for field of {} strategies",
            field.len()
        ));
    }
    let cfg = parse_honesty_config(config_json)?;
    serde_json::to_string(&sharpebench_edge::is_my_sharpe_real_full(
        &field, winner_idx, &cfg,
    ))
    .map_err(|e| e.to_string())
}

/// Rank candidate return streams on a percentile of their bootstrapped utility
/// instead of the point-estimate argmax. Input: a JSON array of per-period
/// return arrays (one per candidate) + an optional params blob
/// `{utility: "mean_return"|"sharpe", alpha, seed, n_boot, block_prob}` (blank
/// or partial → utility mean_return, alpha 0.5, seed 0, n_boot 2000,
/// block_prob 0.1). Output mirrors `selection::PercentileSelection`, including
/// `alpha_warning` when alpha sits below the recommended floor of 0.3 (the
/// result is still computed: the warning flags a choice, it does not veto one).
pub fn percentile_selection_json(
    candidates_json: &str,
    params_json: &str,
) -> Result<String, String> {
    use sharpebench_core::selection::{percentile_selection, Utility, DEFAULT_SELECTION_ALPHA};

    let candidates: Vec<Vec<f64>> =
        serde_json::from_str(candidates_json).map_err(|e| e.to_string())?;
    let v: serde_json::Value = if params_json.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(params_json).map_err(|e| e.to_string())?
    };
    let utility = match v.get("utility").and_then(serde_json::Value::as_str) {
        None | Some("mean_return") => Utility::MeanReturn,
        Some("sharpe") => Utility::Sharpe,
        Some(other) => {
            return Err(format!(
                "utility must be mean_return or sharpe, got `{other}`"
            ))
        }
    };
    let num = |k: &str, default: f64| -> Result<f64, String> {
        match v.get(k) {
            None | Some(serde_json::Value::Null) => Ok(default),
            Some(x) => x.as_f64().ok_or_else(|| format!("non-numeric field: {k}")),
        }
    };
    let alpha = num("alpha", DEFAULT_SELECTION_ALPHA)?;
    let block_prob = num("block_prob", 0.1)?;
    let seed = match v.get("seed") {
        None | Some(serde_json::Value::Null) => 0,
        Some(x) => x.as_u64().ok_or("non-integer field: seed")?,
    };
    let n_boot = match v.get("n_boot") {
        None | Some(serde_json::Value::Null) => 2000,
        Some(x) => x.as_u64().ok_or("non-integer field: n_boot")? as usize,
    };
    let s = percentile_selection(&candidates, utility, alpha, seed, n_boot, block_prob);
    serde_json::to_string(&serde_json::json!({
        "alpha": s.alpha,
        "alpha_warning": s.alpha_warning,
        "candidates": s.candidates.iter().map(|c| serde_json::json!({
            "index": c.index,
            "point_utility": c.point_utility,
            "percentile_utility": c.percentile_utility,
            "optimism_gap": c.optimism_gap,
        })).collect::<Vec<_>>(),
        "selected": s.selected,
        "point_argmax": s.point_argmax,
        "agrees_with_point_argmax": s.agrees_with_point_argmax,
        "point_winner_optimism": s.point_winner_optimism,
    }))
    .map_err(|e| e.to_string())
}

/// Decompose the uncertainty behind one scored case into its three legs. Input
/// JSON: `{outcomes: bool[] (or 0/1 numbers), signals: number[][], case_returns:
/// number[], reference_returns: number[]}`; every field is optional and a
/// missing leg's input reads as empty (the leg then reports what it honestly can
/// on no evidence). Output: `{aleatoric, epistemic, distributional,
/// epistemic_caveat}`: the legs are reported side by side, never summed, and
/// the caveat is load-bearing: the epistemic leg is a lower bound, never an
/// upper one, because unanimous or correlated signals understate it.
pub fn decompose_uncertainty_json(input_json: &str) -> Result<String, String> {
    use sharpebench_core::calibration::decompose_uncertainty;

    let v: serde_json::Value = serde_json::from_str(input_json).map_err(|e| e.to_string())?;
    let floats = |k: &str| -> Result<Vec<f64>, String> {
        match v.get(k) {
            None | Some(serde_json::Value::Null) => Ok(Vec::new()),
            Some(x) => serde_json::from_value(x.clone()).map_err(|e| format!("{k}: {e}")),
        }
    };
    let outcomes: Vec<bool> = match v.get("outcomes") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(x) => x
            .as_array()
            .ok_or("outcomes must be an array")?
            .iter()
            .map(|o| match o {
                serde_json::Value::Bool(b) => Ok(*b),
                serde_json::Value::Number(n) => Ok(n.as_f64().unwrap_or(0.0) != 0.0),
                _ => Err("outcomes entries must be booleans or 0/1 numbers".to_string()),
            })
            .collect::<Result<_, _>>()?,
    };
    let signal_series: Vec<Vec<f64>> = match v.get("signals") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(x) => serde_json::from_value(x.clone()).map_err(|e| format!("signals: {e}"))?,
    };
    let signals: Vec<&[f64]> = signal_series.iter().map(Vec::as_slice).collect();
    let case_returns = floats("case_returns")?;
    let reference_returns = floats("reference_returns")?;
    let split = decompose_uncertainty(&outcomes, &signals, &case_returns, &reference_returns);
    serde_json::to_string(&serde_json::json!({
        "aleatoric": split.aleatoric,
        "epistemic": split.epistemic,
        "distributional": split.distributional,
        "epistemic_caveat": "the epistemic leg is a lower bound, never an upper one: \
    unanimous or correlated signals understate it, so treat only high readings as informative",
    }))
    .map_err(|e| e.to_string())
}

/// Expected edge half-life under the crowding decay model. Input JSON:
/// `{adoption, theta, delta_max, curvature?}`; all rates are per period of the
/// caller's IC series and there is deliberately no default calibration (a stock
/// calibration would smuggle a modelled number in as if it were measured).
/// Output: the `CrowdingDecayPrior` fields plus a `note` naming what this is:
/// a model prior, reported never gating.
pub fn crowding_half_life_json(params_json: &str) -> Result<String, String> {
    use sharpebench_core::decay::{crowding_half_life, CrowdingParams};

    let v: serde_json::Value = serde_json::from_str(params_json).map_err(|e| e.to_string())?;
    let num = |k: &str| -> Result<f64, String> {
        v.get(k)
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| format!("missing or non-numeric field: {k}"))
    };
    let adoption = num("adoption")?;
    let params = CrowdingParams {
        theta: num("theta")?,
        delta_max: num("delta_max")?,
        curvature: match v.get("curvature") {
            None | Some(serde_json::Value::Null) => 1.0,
            Some(x) => x.as_f64().ok_or("non-numeric field: curvature")?,
        },
    };
    let prior = crowding_half_life(adoption, params);
    let mut out = serde_json::to_value(prior).map_err(|e| e.to_string())?;
    out["note"] = serde_json::Value::String(
        "model prior, reported never gating: this comes out of a crowding model, not out of a \
dataset, and nothing should rank on it"
            .to_string(),
    );
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

/// Name every disqualification/quality signal that fired for each agent in a
/// field of submissions, taking the same JSON array `score_json` takes. Pure
/// legibility over the composite score: nothing here changes eligibility
/// semantics. Output: `[{agent_id, rank_eligible, reasons}]` with reasons in
/// stable enum order; the advisory reasons (high_selection_gap, is_rediscovery,
/// oos_decay) never gate.
pub fn classify_disqualification_json(
    submissions_json: &str,
    config_json: &str,
) -> Result<String, String> {
    use sharpebench_core::{classify_disqualification, DisqualThresholds};

    let subs: Vec<AgentSubmission> =
        serde_json::from_str(submissions_json).map_err(|e| e.to_string())?;
    let cfg: ScoreConfig = parse_or_default(config_json)?;
    let thresholds = DisqualThresholds::from_score_config(&cfg);
    let out: Vec<serde_json::Value> = subs
        .iter()
        .map(|sub| {
            let score = score_agent(sub, &cfg);
            let reasons = classify_disqualification(&score, &thresholds, None, None);
            serde_json::json!({
                "agent_id": score.agent_id,
                "rank_eligible": score.rank_eligible,
                "reasons": reasons,
            })
        })
        .collect();
    serde_json::to_string(&out).map_err(|e| e.to_string())
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

    #[wasm_bindgen]
    pub fn is_my_sharpe_real(returns_json: &str, config_json: &str) -> String {
        wrap(super::is_my_sharpe_real_json(returns_json, config_json))
    }

    #[wasm_bindgen]
    pub fn is_my_sharpe_real_full(
        field_json: &str,
        winner_idx: usize,
        config_json: &str,
    ) -> String {
        wrap(super::is_my_sharpe_real_full_json(
            field_json,
            winner_idx,
            config_json,
        ))
    }

    #[wasm_bindgen]
    pub fn percentile_selection(candidates_json: &str, params_json: &str) -> String {
        wrap(super::percentile_selection_json(
            candidates_json,
            params_json,
        ))
    }

    #[wasm_bindgen]
    pub fn decompose_uncertainty(input_json: &str) -> String {
        wrap(super::decompose_uncertainty_json(input_json))
    }

    #[wasm_bindgen]
    pub fn crowding_half_life(params_json: &str) -> String {
        wrap(super::crowding_half_life_json(params_json))
    }

    #[wasm_bindgen]
    pub fn classify_disqualification(submissions_json: &str, config_json: &str) -> String {
        wrap(super::classify_disqualification_json(
            submissions_json,
            config_json,
        ))
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

    #[test]
    fn is_my_sharpe_real_json_parses_and_carries_a_verdict() {
        // A long, clean, single-trial edge → a verdict is present.
        let returns: Vec<f64> = (0..400)
            .map(|i| 0.001 + 0.00005 * ((i % 4) as f64 - 1.5))
            .collect();
        let returns_json = serde_json::to_string(&returns).unwrap();
        let out = is_my_sharpe_real_json(&returns_json, r#"{"n_trials":1}"#).expect("verdict");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("verdict").is_some());
        assert!(v.get("deflated_sharpe").is_some());
        assert!(v.get("haircut_sharpe").is_some());
    }

    #[test]
    fn is_my_sharpe_real_json_defaults_optional_config() {
        // Only n_trials supplied; the rest default without error.
        let out = is_my_sharpe_real_json("[0.001,0.002,0.0015,0.0018]", r#"{"n_trials":10}"#)
            .expect("verdict");
        assert!(out.contains("\"n_trials\":10"));
    }

    #[test]
    fn is_my_sharpe_real_json_missing_n_trials_is_error() {
        assert!(is_my_sharpe_real_json("[0.001,0.002]", "{}").is_err());
        assert!(is_my_sharpe_real_json("not json", r#"{"n_trials":1}"#).is_err());
    }

    #[test]
    fn is_my_sharpe_real_full_json_runs_the_family() {
        let field: Vec<Vec<f64>> = (0..5)
            .map(|j| {
                (0..80)
                    .map(|i| {
                        let edge = if j == 2 { 0.004 } else { 0.0005 };
                        edge + 0.003 * (((i + j) % 6) as f64 - 2.5)
                    })
                    .collect()
            })
            .collect();
        let field_json = serde_json::to_string(&field).unwrap();
        let out = is_my_sharpe_real_full_json(&field_json, 2, r#"{"n_trials":5}"#).expect("full");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("honesty").and_then(|h| h.get("verdict")).is_some());
        assert!(v.get("pbo").is_some());
        assert!(v.get("reality_check_p").is_some());
    }

    #[test]
    fn is_my_sharpe_real_full_json_out_of_bounds_is_error() {
        assert!(is_my_sharpe_real_full_json("[[0.1,0.2]]", 5, r#"{"n_trials":1}"#).is_err());
        assert!(is_my_sharpe_real_full_json("not json", 0, r#"{"n_trials":1}"#).is_err());
    }

    #[test]
    fn percentile_selection_json_separates_a_spike_from_a_steady_earner() {
        // Candidate 0 earns steadily; candidate 1's entire mean is one spike.
        let steady: Vec<f64> = (0..100)
            .map(|i| 0.003 + 0.0001 * (i as f64 * 0.9).sin())
            .collect();
        let mut spiky = vec![0.0005; 100];
        spiky[50] = 0.35;
        let candidates = serde_json::to_string(&vec![steady, spiky]).unwrap();
        // Same operating point as the library's own test: the recommended floor,
        // a seed, and enough resamples for the percentile to be stable.
        let params = r#"{"alpha":0.3,"seed":11,"n_boot":4000}"#;
        let out = percentile_selection_json(&candidates, params).expect("selection");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["point_argmax"], 1, "on the observed path the spike wins");
        assert_eq!(
            v["selected"], 0,
            "across resampled histories the steady earner wins"
        );
        assert_eq!(v["agrees_with_point_argmax"], false);
        assert_eq!(v["alpha_warning"], false);
        // Deterministic given the same input.
        let again = percentile_selection_json(&candidates, params).unwrap();
        assert_eq!(out, again);
    }

    #[test]
    fn percentile_selection_json_warns_below_the_floor_and_validates_input() {
        let out = percentile_selection_json("[[0.01,0.02,0.01,0.02]]", r#"{"alpha":0.05}"#)
            .expect("selection");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["alpha_warning"], true, "below 0.3 announces itself");
        assert!(percentile_selection_json("not json", "").is_err());
        assert!(percentile_selection_json("[[0.01]]", r#"{"utility":"sortino"}"#).is_err());
    }

    #[test]
    fn decompose_uncertainty_json_reports_three_legs_and_the_caveat() {
        let input = serde_json::json!({
            "outcomes": (0..50).map(|i| i % 2 == 0).collect::<Vec<bool>>(),
            "signals": [vec![0.9; 50], vec![0.1; 50]],
            "case_returns": (0..50).map(|i| 0.004 + 0.0005 * (i as f64).sin()).collect::<Vec<f64>>(),
            "reference_returns": (0..50).map(|i| 0.004 + 0.0005 * (i as f64).sin()).collect::<Vec<f64>>(),
        });
        let out = decompose_uncertainty_json(&input.to_string()).expect("decomposition");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(
            (v["aleatoric"].as_f64().unwrap() - 1.0).abs() < 1e-12,
            "coin flip"
        );
        assert!(
            v["epistemic"].as_f64().unwrap() > 0.5,
            "the streams contradict each other"
        );
        assert!(
            v["distributional"].as_f64().unwrap() < 1e-12,
            "case vs itself"
        );
        assert!(v["epistemic_caveat"]
            .as_str()
            .unwrap()
            .contains("lower bound"));
        // Numeric 0/1 outcomes coerce; absent fields read as empty.
        assert!(decompose_uncertainty_json(r#"{"outcomes":[1,0,1]}"#).is_ok());
        assert!(decompose_uncertainty_json("{}").is_ok());
        assert!(decompose_uncertainty_json("not json").is_err());
    }

    #[test]
    fn crowding_half_life_json_is_a_prior_and_says_so() {
        let out = crowding_half_life_json(r#"{"adoption":1.0,"theta":0.05,"delta_max":0.05}"#)
            .expect("prior");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        // ln2 / 0.10 at full adoption with these per-period test rates.
        assert!((v["expected_half_life"].as_f64().unwrap() - 6.9315).abs() < 1e-3);
        assert!(v["note"]
            .as_str()
            .unwrap()
            .contains("model prior, reported never gating"));
        // The model rates are required: no default calibration is smuggled in.
        assert!(crowding_half_life_json(r#"{"adoption":0.5}"#).is_err());
    }

    #[test]
    fn classify_disqualification_json_names_reasons_over_the_score_field() {
        let steady: Vec<f64> = (0..60)
            .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
            .collect();
        let noisy: Vec<f64> = (0..60).map(|i| 0.02 * (i as f64 * 0.7).sin()).collect();
        let subs = serde_json::json!([
            { "agent_id": "steady", "runs": (0..5).map(|_| serde_json::json!({"returns": steady})).collect::<Vec<_>>() },
            { "agent_id": "noise", "runs": (0..5).map(|_| serde_json::json!({"returns": noisy})).collect::<Vec<_>>() },
        ]);
        let out = classify_disqualification_json(&subs.to_string(), "").expect("classification");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["agent_id"], "steady");
        assert_eq!(arr[0]["reasons"].as_array().unwrap().len(), 0);
        let noise_reasons: Vec<&str> = arr[1]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.as_str())
            .collect();
        assert!(
            noise_reasons.contains(&"dsr_below_bar"),
            "{noise_reasons:?}"
        );
        assert!(classify_disqualification_json("not json", "").is_err());
    }
}
