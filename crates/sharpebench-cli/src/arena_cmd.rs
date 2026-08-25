//! `sharpebench arena` - forward-league driver subcommands.
//!
//! The production CLI dispatches here from its `arena` command arm. Keeping the
//! lifecycle implementation in one module lets the CLI and the arena crate's
//! integration test exercise the same code.
//!
//! Contract for [`run`]: `args` is the process argv with any `--json` flags
//! already stripped (the same slice `main` dispatches on, so `args[1] == "arena"`
//! and `args[2]` is the arena subcommand); `json` selects machine-readable
//! output. Returns the process exit code: 0 success, 1 failure, 2 usage error.
//!
//! Epochs are explicit integers everywhere, as in `sharpebench-attest`: the
//! `arena advance <epoch>` subcommand is the only bridge from wall time, and the
//! caller (an operator, cron, or CI) decides what epoch "now" is.

use std::path::Path;

use sharpebench_arena::{verify_arena, Arena, RevealedEntry, SigningKey, VerifyingKey};

/// Entry point (see the module docs for the argv contract).
pub fn run(args: &[String], json: bool) -> i32 {
    match args.get(2).map(String::as_str) {
        Some("init") => cmd_init(args, json),
        Some("open") => cmd_open(args, json),
        Some("supersede-empty") => cmd_supersede_empty(args, json),
        Some("link-supersession") => cmd_link_supersession(args, json),
        Some("commit") => cmd_commit(args, json),
        Some("advance") => cmd_advance(args, json),
        Some("score") => cmd_score(args, json),
        Some("publish") => cmd_publish(args, json),
        Some("verify") => cmd_verify(args, json),
        _ => {
            usage();
            2
        }
    }
}

fn usage() {
    eprintln!("usage: sharpebench arena <subcommand> [--json]");
    eprintln!("  arena init <dir>                                       create an arena directory");
    eprintln!("  arena open <dir> <window> <commit_deadline> <reveal_epoch> --scorer-artifact-sha256 <hex> [--config <score_config.json>] [--sealed-eval-salt-sha256 <hex>]");
    eprintln!("                                                         open a window; scorer/config provenance is fixed now");
    eprintln!("  arena supersede-empty <dir> <window> <reason>          archive an empty obsolete window before reopening");
    eprintln!("  arena link-supersession <dir> <old> <new>               record the audited replacement config link");
    eprintln!("  arena commit <dir> <window> <commitment.json>          register a pre-deadline commitment (from `sharpebench commit`)");
    eprintln!("  arena advance <dir> <epoch>                            advance the epoch (operator/cron/CI supplies \"now\")");
    eprintln!("  arena score <dir> <window> <dataset> <entries.json>    verify reveals, refuse mismatches, rank the rest");
    eprintln!("  arena publish <dir> <window> <key>                     sign + write the window's Ed25519 board");
    eprintln!("  arena verify <dir> [--pubkey <hex>]                    verify every published board + the cross-window chain");
    eprintln!("\n<key> and --pubkey accept a literal, or env:NAME / file:PATH to keep secrets out of process listings.");
}

fn cmd_init(args: &[String], json: bool) -> i32 {
    let Some(dir) = args.get(3) else {
        eprintln!("usage: sharpebench arena init <dir>");
        return 2;
    };
    match Arena::init(Path::new(dir)) {
        Ok(_) => {
            if json {
                emit_json(&serde_json::json!({ "ok": true, "dir": dir }));
            } else {
                println!("initialized arena at {dir}");
            }
            0
        }
        Err(e) => fail(&e, json),
    }
}

fn cmd_open(args: &[String], json: bool) -> i32 {
    let (Some(dir), Some(window), Some(deadline), Some(reveal)) =
        (args.get(3), args.get(4), args.get(5), args.get(6))
    else {
        eprintln!(
            "usage: sharpebench arena open <dir> <window> <commit_deadline> <reveal_epoch> --scorer-artifact-sha256 <hex> [--config <score_config.json>] [--sealed-eval-salt-sha256 <hex>]"
        );
        return 2;
    };
    let (Ok(deadline), Ok(reveal)) = (deadline.parse::<u64>(), reveal.parse::<u64>()) else {
        eprintln!("error: commit_deadline and reveal_epoch must be non-negative integers");
        return 2;
    };
    let config = match flag_value(args, "--config") {
        Some(path) => match std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {path}: {e}"))
            .and_then(|t| {
                serde_json::from_str::<sharpebench_core::ScoreConfig>(&t)
                    .map_err(|e| format!("invalid ScoreConfig JSON in {path}: {e}"))
            }) {
            Ok(c) => c,
            Err(e) => return fail(&e, json),
        },
        None => sharpebench_core::ScoreConfig::default(),
    };
    let mut arena = match Arena::load(Path::new(dir)) {
        Ok(a) => a,
        Err(e) => return fail(&e, json),
    };
    let sealed_eval_salt_sha256 = flag_value(args, "--sealed-eval-salt-sha256").map(str::to_owned);
    let Some(scorer_artifact_sha256) = flag_value(args, "--scorer-artifact-sha256") else {
        return fail("--scorer-artifact-sha256 is required: freeze the exact release binary or immutable image before entries commit", json);
    };
    match arena.open_window_with_provenance(
        window,
        deadline,
        reveal,
        config,
        sealed_eval_salt_sha256,
        scorer_artifact_sha256.to_string(),
    ) {
        Ok(()) => {
            if json {
                emit_json(&serde_json::json!({
                    "ok": true,
                    "window": window,
                    "commit_deadline": deadline,
                    "data_reveal_epoch": reveal,
                    "sealed_eval_salt_sha256": arena.window(window).and_then(|w| w.sealed_eval_salt_sha256.as_deref()),
                    "scorer_artifact_sha256": arena.window(window).map(|w| w.scorer_artifact_sha256.as_str()),
                }));
            } else {
                println!(
                    "opened window `{window}` (commit deadline: epoch {deadline}, data reveal: epoch {reveal}); scoring rules recorded"
                );
            }
            0
        }
        Err(e) => fail(&e, json),
    }
}

fn cmd_supersede_empty(args: &[String], json: bool) -> i32 {
    let (Some(dir), Some(window), Some(reason)) = (args.get(3), args.get(4), args.get(5)) else {
        eprintln!("usage: sharpebench arena supersede-empty <dir> <window> <reason>");
        return 2;
    };
    match Arena::supersede_empty_window(Path::new(dir), window, reason) {
        Ok(record) => {
            if json {
                emit_json(&serde_json::json!({ "ok": true, "supersession": record }));
            } else {
                println!(
                    "superseded empty window `{}` at epoch {}; preserved historical SHA-256 {}",
                    record.window_id, record.superseded_at_epoch, record.historical_window_sha256
                );
            }
            0
        }
        Err(e) => fail(&e, json),
    }
}

fn cmd_link_supersession(args: &[String], json: bool) -> i32 {
    let (Some(dir), Some(old), Some(new)) = (args.get(3), args.get(4), args.get(5)) else {
        eprintln!("usage: sharpebench arena link-supersession <dir> <old> <new>");
        return 2;
    };
    match Arena::link_supersession_replacement(Path::new(dir), old, new) {
        Ok(()) => {
            if json {
                emit_json(
                    &serde_json::json!({ "ok": true, "superseded": old, "replacement": new }),
                );
            } else {
                println!("linked supersession `{old}` to replacement `{new}`");
            }
            0
        }
        Err(e) => fail(&e, json),
    }
}

fn cmd_commit(args: &[String], json: bool) -> i32 {
    let (Some(dir), Some(window), Some(commitment_path)) = (args.get(3), args.get(4), args.get(5))
    else {
        eprintln!("usage: sharpebench arena commit <dir> <window> <commitment.json>");
        return 2;
    };
    let commitment: sharpebench_attest::Commitment = match std::fs::read_to_string(commitment_path)
        .map_err(|e| format!("cannot read {commitment_path}: {e}"))
        .and_then(|t| {
            serde_json::from_str(&t)
                .map_err(|e| format!("invalid commitment JSON in {commitment_path}: {e}"))
        }) {
        Ok(c) => c,
        Err(e) => return fail(&e, json),
    };
    let mut arena = match Arena::load(Path::new(dir)) {
        Ok(a) => a,
        Err(e) => return fail(&e, json),
    };
    let agent_id = commitment.agent_id.clone();
    match arena.register_entry(window, commitment) {
        Ok(()) => {
            if json {
                emit_json(
                    &serde_json::json!({ "ok": true, "window": window, "agent_id": agent_id }),
                );
            } else {
                println!("registered commitment for `{agent_id}` in window `{window}`");
            }
            0
        }
        Err(e) => fail(&e, json),
    }
}

fn cmd_advance(args: &[String], json: bool) -> i32 {
    let (Some(dir), Some(epoch)) = (args.get(3), args.get(4)) else {
        eprintln!("usage: sharpebench arena advance <dir> <epoch>");
        return 2;
    };
    let Ok(epoch) = epoch.parse::<u64>() else {
        eprintln!("error: epoch must be a non-negative integer");
        return 2;
    };
    let mut arena = match Arena::load(Path::new(dir)) {
        Ok(a) => a,
        Err(e) => return fail(&e, json),
    };
    match arena.advance(epoch) {
        Ok(()) => {
            let committed: Vec<&String> = arena
                .window_ids()
                .iter()
                .filter(|id| {
                    arena.window(id).map(|w| w.status)
                        == Some(sharpebench_arena::WindowStatus::Committed)
                })
                .collect();
            if json {
                emit_json(
                    &serde_json::json!({ "ok": true, "epoch": epoch, "committed_windows": committed }),
                );
            } else {
                println!("advanced to epoch {epoch}");
                for id in committed {
                    println!("  window `{id}` is past its commit deadline (committed)");
                }
            }
            0
        }
        Err(e) => fail(&e, json),
    }
}

fn cmd_score(args: &[String], json: bool) -> i32 {
    let (Some(dir), Some(window), Some(dataset), Some(entries_path)) =
        (args.get(3), args.get(4), args.get(5), args.get(6))
    else {
        eprintln!("usage: sharpebench arena score <dir> <window> <dataset> <entries.json>");
        return 2;
    };
    let entries: Vec<RevealedEntry> = match std::fs::read_to_string(entries_path)
        .map_err(|e| format!("cannot read {entries_path}: {e}"))
        .and_then(|t| {
            serde_json::from_str(&t)
                .map_err(|e| format!("invalid entries JSON in {entries_path}: {e}"))
        }) {
        Ok(v) => v,
        Err(e) => return fail(&e, json),
    };
    let mut arena = match Arena::load(Path::new(dir)) {
        Ok(a) => a,
        Err(e) => return fail(&e, json),
    };
    match arena.reveal_and_score(window, Path::new(dataset), &entries) {
        Ok(scores) => {
            let refusals = arena
                .window(window)
                .map(|w| w.refusals.clone())
                .unwrap_or_default();
            if json {
                emit_json(&serde_json::json!({
                    "ok": true,
                    "window": window,
                    "scored": scores.len(),
                    "refused": refusals,
                    "board": scores,
                }));
            } else {
                println!(
                    "scored window `{window}`: {} entries ranked, {} refused",
                    scores.len(),
                    refusals.len()
                );
                for r in &refusals {
                    println!("  refused `{}`: {}", r.agent_id, r.reason);
                }
                print!("{}", sharpebench_leaderboard::render(&scores));
            }
            0
        }
        Err(e) => fail(&e, json),
    }
}

fn cmd_publish(args: &[String], json: bool) -> i32 {
    let (Some(dir), Some(window), Some(key_spec)) = (args.get(3), args.get(4), args.get(5)) else {
        eprintln!("usage: sharpebench arena publish <dir> <window> <key>");
        return 2;
    };
    let key = match resolve_key(key_spec) {
        Ok(secret) => SigningKey::derive(&secret),
        Err(e) => return fail(&e.to_string(), json),
    };
    let mut arena = match Arena::load(Path::new(dir)) {
        Ok(a) => a,
        Err(e) => return fail(&e, json),
    };
    match arena.publish(window, &key) {
        Ok(path) => {
            let vk = key.verifying_key().to_hex();
            if json {
                emit_json(&serde_json::json!({
                    "ok": true,
                    "window": window,
                    "board": path.display().to_string(),
                    "verifying_key": vk,
                }));
            } else {
                println!("published window `{window}` -> {}", path.display());
                println!("verifying key (publish this): {vk}");
            }
            0
        }
        Err(e) => fail(&e, json),
    }
}

fn cmd_verify(args: &[String], json: bool) -> i32 {
    let Some(dir) = args.get(3) else {
        eprintln!("usage: sharpebench arena verify <dir> [--pubkey <hex>]");
        return 2;
    };
    let pinned = match flag_value(args, "--pubkey") {
        Some(spec) => match resolve_key(spec) {
            Ok(bytes) => {
                let hex = String::from_utf8_lossy(&bytes).trim().to_string();
                match VerifyingKey::from_hex(&hex) {
                    Some(vk) => Some(vk),
                    None => {
                        eprintln!("error: --pubkey is not a valid 64-hex-char Ed25519 key");
                        return 1;
                    }
                }
            }
            Err(e) => return fail(&e.to_string(), json),
        },
        None => None,
    };
    match verify_arena(Path::new(dir), pinned.as_ref()) {
        Ok(report) => {
            if json {
                emit_json(&report);
            } else if report.ok {
                println!(
                    "OK - {} published window(s), every board and the cross-window chain valid under {} key {}",
                    report.windows.len(),
                    if pinned.is_some() { "pinned" } else { "embedded" },
                    report.verifying_key.as_deref().unwrap_or("(none)")
                );
            } else {
                eprintln!("FAIL - the arena chain does not verify:");
                for w in &report.windows {
                    eprintln!(
                        "  {} chain={} anchor={} key={} {}",
                        w.window_id, w.chain_ok, w.anchor_ok, w.key_ok, w.detail
                    );
                }
            }
            i32::from(!report.ok)
        }
        Err(e) => fail(&e, json),
    }
}

fn fail(message: &str, json: bool) -> i32 {
    if json {
        emit_json(&serde_json::json!({ "ok": false, "error": message }));
    } else {
        eprintln!("error: {message}");
    }
    1
}

/// Local copy of `main.rs`'s helpers, so this module stays standalone (main.rs
/// is wired at integration and this file must compile and test without it).
fn emit_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(j) => println!("{j}"),
        Err(e) => eprintln!("error: serializing output: {e}"),
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// `env:NAME` / `file:PATH` / literal, same convention as the rest of the CLI.
fn resolve_key(spec: &str) -> std::io::Result<Vec<u8>> {
    if let Some(var) = spec.strip_prefix("env:") {
        std::env::var(var)
            .map(String::into_bytes)
            .map_err(|_| std::io::Error::other(format!("env var {var} is not set")))
    } else if let Some(path) = spec.strip_prefix("file:") {
        Ok(std::fs::read_to_string(path)?
            .trim_end()
            .as_bytes()
            .to_vec())
    } else {
        Ok(spec.as_bytes().to_vec())
    }
}
