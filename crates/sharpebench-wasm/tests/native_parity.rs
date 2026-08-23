//! WASM-façade ↔ native-kernel parity. The wasm crate's `*_json` entry points are
//! plain Rust (wasm-bindgen is only pulled in under `wasm32`), so they run in a
//! host test. Each test scores the same field through the façade and through
//! `sharpebench_core` directly and asserts the serialised bytes are identical —
//! "one kernel, zero drift" as a check rather than an argument. The third test
//! ties the façade to the committed golden fixtures, so the wasm path is pinned
//! to the same numbers the native CI matrix pins.

use std::path::{Path, PathBuf};

use sharpebench_core::{rank, score_agent, AgentSubmission, ScoreConfig};
use sharpebench_wasm::{score_agent_json, score_json};

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read(rel: &str) -> String {
    let path = repo_path(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The two deterministic fields the golden tests pin: the public example suite and
/// the committed simulator-generated field.
fn fields() -> Vec<(&'static str, String)> {
    vec![
        (
            "suites/example_submissions.json",
            read("../../suites/example_submissions.json"),
        ),
        (
            "golden/synthetic_field.input.json",
            read("../sharpebench-core/golden/synthetic_field.input.json"),
        ),
    ]
}

/// Strip insignificant whitespace from pretty JSON without parsing floats (a
/// parse → re-serialise round trip is exactly the kind of step that could hide a
/// 1-ULP difference). String contents are preserved verbatim.
fn minify(pretty: &str) -> String {
    let mut out = String::with_capacity(pretty.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in pretty.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

#[test]
fn wasm_score_json_is_byte_identical_to_native_rank() {
    for (name, raw) in fields() {
        let subs: Vec<AgentSubmission> = serde_json::from_str(&raw).expect("field parses");
        let native = serde_json::to_string(&rank(&subs, &ScoreConfig::default())).unwrap();
        let wasm = score_json(&raw, "").expect("score_json");
        assert!(native == wasm, "{name}: wasm façade output differs from native rank\n--- native ---\n{native}\n--- wasm ---\n{wasm}");
    }
}

#[test]
fn wasm_score_agent_json_is_byte_identical_to_native_score_agent() {
    for (name, raw) in fields() {
        let subs: Vec<AgentSubmission> = serde_json::from_str(&raw).expect("field parses");
        for sub in &subs {
            let sub_json = serde_json::to_string(sub).unwrap();
            let native = serde_json::to_string(&score_agent(sub, &ScoreConfig::default())).unwrap();
            let wasm = score_agent_json(&sub_json, "").expect("score_agent_json");
            assert!(
                native == wasm,
                "{name}/{}: wasm façade output differs from native score_agent",
                sub.agent_id
            );
        }
    }
}

#[test]
fn wasm_score_json_matches_committed_golden_fixtures() {
    for (input, golden) in [
        (
            "../../suites/example_submissions.json",
            "../sharpebench-core/golden/example_submissions.scores.json",
        ),
        (
            "../sharpebench-core/golden/synthetic_field.input.json",
            "../sharpebench-core/golden/synthetic_field.scores.json",
        ),
    ] {
        let wasm = score_json(&read(input), "").expect("score_json");
        let expected = minify(&read(golden));
        assert!(
            wasm == expected,
            "{golden}: wasm façade output differs from the committed golden (regenerate with SHARPEBENCH_UPDATE_GOLDEN=1 cargo test -p sharpebench-core --test golden_scores if the kernel changed deliberately)"
        );
    }
}
