//! WASM façade over [`sb_core`] — the bridge that lets Gordon (TypeScript/Bun)
//! consume the **identical** scoring kernel as the public harness, so the
//! internal eval and the published benchmark can never drift.
//!
//! The real work is [`score_json`]: a pure JSON-in / JSON-out function that is
//! testable on the host. Under the `wasm32` target it is additionally exported
//! through `wasm-bindgen` as `score(...)`, which Bun calls directly. There is
//! exactly one implementation of the scoring math — this just wraps it.
#![forbid(unsafe_code)]

use sb_core::{rank, AgentSubmission, ScoreConfig};

/// Score and rank a JSON array of submissions, returning the leaderboard as a
/// JSON array of `CompositeScore`. An empty/blank `config_json` uses the
/// defaults.
pub fn score_json(submissions_json: &str, config_json: &str) -> Result<String, String> {
    let subs: Vec<AgentSubmission> =
        serde_json::from_str(submissions_json).map_err(|e| e.to_string())?;
    let cfg: ScoreConfig = if config_json.trim().is_empty() {
        ScoreConfig::default()
    } else {
        serde_json::from_str(config_json).map_err(|e| e.to_string())?
    };
    let board = rank(&subs, &cfg);
    serde_json::to_string(&board).map_err(|e| e.to_string())
}

/// The wasm-bindgen export Bun invokes. Returns the leaderboard JSON, or a
/// `{"error": "..."}` JSON object on failure.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn score(submissions_json: &str, config_json: &str) -> String {
    match score_json(submissions_json, config_json) {
        Ok(s) => s,
        Err(e) => format!(
            "{{\"error\":{}}}",
            serde_json::to_string(&e).unwrap_or_default()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_json_roundtrips_and_ranks() {
        // A skilled agent (steady, multi-run) vs a flat one.
        let subs = r#"[
            {"agent_id":"skilled","runs":[
                {"returns":[0.002,0.0021,0.0019,0.002,0.0022,0.0018,0.002,0.0021,0.0019,0.002]},
                {"returns":[0.002,0.0019,0.0021,0.002,0.0018,0.0022,0.002,0.0019,0.0021,0.002]}
            ]},
            {"agent_id":"flat","runs":[{"returns":[0.0,0.0,0.0,0.0,0.0]}]}
        ]"#;
        let out = score_json(subs, "").expect("scores");
        assert!(out.contains("\"agent_id\":\"skilled\""));
        assert!(out.contains("\"agent_id\":\"flat\""));
        // The skilled agent appears before the flat one in the ranked output.
        let si = out.find("skilled").unwrap();
        let fi = out.find("flat").unwrap();
        assert!(si < fi, "skilled should rank ahead of flat");
    }

    #[test]
    fn bad_json_is_an_error_not_a_panic() {
        assert!(score_json("not json", "").is_err());
    }
}
