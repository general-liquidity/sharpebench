//! `sharpebench leaderboard export` - write a leaderboard export from signed
//! boards for command-line and agent use.
//!
//! The input is an arena directory or a board file written by `sharpebench
//! sign`. It is verified before anything is read from it, and an input that
//! does not verify is refused with nothing written. The export is a canonical
//! JSON record (`leaderboard.json`, schema `sharpebench.leaderboard-export.v1`)
//! and a Markdown table (`leaderboard.md`) derived from it. Both are plain
//! files: there is no page, script or server.
//!
//! Contract for [`run`]: `args` is the process argv with `--json` already
//! stripped (`args[1] == "leaderboard"`, `args[2]` the subcommand). Returns 0
//! on success, 1 when the input is refused or cannot be read or written, and
//! 2 on a usage error.

use std::path::Path;

use sharpebench_arena::leaderboard_export::{export_arena, export_signed_board, BoardVerifier};
use sharpebench_arena::{ExpectedHead, VerifyingKey};
use sharpebench_core::leaderboard_export::{render_markdown, to_canonical_json};

/// File name of the canonical JSON record inside `--out`.
pub const JSON_FILE: &str = "leaderboard.json";
/// File name of the Markdown table inside `--out`.
pub const MARKDOWN_FILE: &str = "leaderboard.md";

const USAGE: &str = "usage: sharpebench leaderboard export <arena-dir | board.json> [--out <dir>] [--pubkey <hex>] [--expect-head <count>:<sha256>] [--key <secret>] [--public] [--json]";

/// Entry point (see the module docs for the argv contract).
pub fn run(args: &[String], json: bool) -> i32 {
    match args.get(2).map(String::as_str) {
        Some("export") => cmd_export(args, json),
        _ => usage(),
    }
}

fn usage() -> i32 {
    eprintln!("{USAGE}");
    eprintln!("  an arena directory is verified like `arena verify` (--pubkey pins its key, --expect-head pins a head);");
    eprintln!("  a board file from `sharpebench sign` needs --key <secret> (HMAC), --pubkey <hex> or --public (Ed25519).");
    eprintln!("  --out <dir> writes {JSON_FILE} and {MARKDOWN_FILE}; without it the Markdown, or the JSON under --json, goes to stdout.");
    eprintln!("  <secret> and <hex> accept a literal, or env:NAME / file:PATH.");
    2
}

fn cmd_export(args: &[String], json: bool) -> i32 {
    let Some(input) = args.get(3).filter(|a| !a.starts_with("--")) else {
        return usage();
    };
    for flag in ["--out", "--pubkey", "--expect-head", "--key"] {
        if args.iter().any(|a| a == flag)
            && flag_value(args, flag).is_none_or(|v| v.starts_with("--"))
        {
            eprintln!("error: {flag} needs a value");
            return 2;
        }
    }
    let path = Path::new(input);
    let pinned = match flag_value(args, "--pubkey").map(parse_pubkey).transpose() {
        Ok(pinned) => pinned,
        Err(e) => return fail(&e, json),
    };
    let export = if path.is_dir() {
        if args.iter().any(|a| a == "--key" || a == "--public") {
            eprintln!("error: an arena directory is verified under Ed25519; --key and --public apply to a board file");
            return 2;
        }
        let expected = match flag_value(args, "--expect-head").map(ExpectedHead::parse) {
            Some(Err(e)) => {
                eprintln!("error: --expect-head: {e}");
                return 2;
            }
            Some(Ok(expected)) => Some(expected),
            None => None,
        };
        export_arena(path, pinned.as_ref(), expected.as_ref())
    } else {
        if args.iter().any(|a| a == "--expect-head") {
            eprintln!("error: --expect-head applies to an arena directory");
            return 2;
        }
        let public = args.iter().any(|a| a == "--public");
        let key = flag_value(args, "--key");
        let chosen =
            usize::from(public) + usize::from(key.is_some()) + usize::from(pinned.is_some());
        if chosen != 1 {
            eprintln!("error: a board file needs exactly one of --key <secret>, --pubkey <hex> or --public");
            return 2;
        }
        let document = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => return fail(&format!("cannot read {input}: {e}"), json),
        };
        let name = path
            .file_name()
            .map_or_else(|| input.clone(), |n| n.to_string_lossy().into_owned());
        match key.map(resolve_key).transpose() {
            Ok(Some(secret)) => export_signed_board(&name, &document, BoardVerifier::Hmac(&secret)),
            Ok(None) => {
                export_signed_board(&name, &document, BoardVerifier::Ed25519(pinned.as_ref()))
            }
            Err(e) => return fail(&e, json),
        }
    };
    let export = match export {
        Ok(export) => export,
        Err(e) => return fail(&e, json),
    };
    let record = to_canonical_json(&export);
    let table = render_markdown(&export);
    let Some(out) = flag_value(args, "--out") else {
        print!("{}", if json { &record } else { &table });
        return 0;
    };
    let out = Path::new(out);
    let written = std::fs::create_dir_all(out)
        .and_then(|()| std::fs::write(out.join(JSON_FILE), &record))
        .and_then(|()| std::fs::write(out.join(MARKDOWN_FILE), &table));
    if let Err(e) = written {
        return fail(
            &format!("cannot write the export to {}: {e}", out.display()),
            json,
        );
    }
    if json {
        emit_json(&serde_json::json!({
            "ok": true,
            "boards": export.boards.len(),
            "rows": export.rows.len(),
            "json": out.join(JSON_FILE),
            "markdown": out.join(MARKDOWN_FILE),
        }));
    } else {
        println!(
            "exported {} row(s) from {} verified board(s) -> {} and {}",
            export.rows.len(),
            export.boards.len(),
            out.join(JSON_FILE).display(),
            out.join(MARKDOWN_FILE).display()
        );
    }
    0
}

fn parse_pubkey(spec: &str) -> Result<VerifyingKey, String> {
    let bytes = resolve_key(spec)?;
    let hex = String::from_utf8_lossy(&bytes).trim().to_string();
    VerifyingKey::from_hex(&hex)
        .ok_or_else(|| "--pubkey is not a valid 64-hex-char Ed25519 key".to_string())
}

fn fail(message: &str, json: bool) -> i32 {
    if json {
        emit_json(&serde_json::json!({ "ok": false, "error": message }));
    } else {
        eprintln!("error: {message}");
    }
    1
}

/// Local copies of `main.rs`'s helpers, so this module stays standalone.
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
fn resolve_key(spec: &str) -> Result<Vec<u8>, String> {
    if let Some(var) = spec.strip_prefix("env:") {
        std::env::var(var)
            .map(String::into_bytes)
            .map_err(|_| format!("env var {var} is not set"))
    } else if let Some(path) = spec.strip_prefix("file:") {
        std::fs::read_to_string(path)
            .map(|s| s.trim_end().as_bytes().to_vec())
            .map_err(|e| format!("cannot read {path}: {e}"))
    } else {
        Ok(spec.as_bytes().to_vec())
    }
}
