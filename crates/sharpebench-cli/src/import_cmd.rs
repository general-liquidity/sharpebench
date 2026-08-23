//! `sharpebench import`: adapt a rival benchmark's published per-period return
//! series into a SharpeBench submissions file, so its field can be re-scored
//! under SharpeBench's gates (`sharpebench score <out.json>`).
//!
//! What an import can and cannot claim is the point of this module, not a
//! footnote. An imported field carries returns only: no audit trace (so the
//! process gate trivially passes), no per-decision confidences (so calibration
//! is absent), and an unknowable search footprint (`--trials` is the importer's
//! declaration, not a measurement; the default 0 understates deflation). The
//! re-score therefore compares on deflation, per-run reliability and the
//! bootstrap only, and any demotion it shows is a lower bound on how much the
//! rival ranking overstates. See `docs/book/src/importing.md`.

use std::path::Path;

use sharpebench_core::{AgentSubmission, Run};

/// Embedded in every imported submission under `_import_note`. The scorer's
/// serde deserialization ignores unknown fields (verified by the round-trip
/// test below), so the caveat travels with the file without breaking `score`.
const IMPORT_NOTE: &str =
    "Imported from a foreign benchmark: returns only. No audit trace (process \
     gate trivially passes), no per-decision confidences (calibration absent), \
     in_sample_trials declared by the importer rather than measured (0 \
     understates deflation). Comparison is on deflation, reliability and the \
     bootstrap only; any demotion is a lower bound.";

/// The loud human-facing version of the same caveat, printed to stderr on
/// every import so it cannot be missed even when stdout is piped.
fn print_notice(trials: u32) {
    eprintln!(
        "\nNOTICE: this is an IMPORTED field, not a SharpeBench-harness field.\n\
         - no audit traces: the process gate trivially passes for every agent\n\
         - no per-decision confidences: calibration is absent from the score\n\
         - in_sample_trials={trials} is your declaration, not a measurement;\n\
           0 understates deflation, so any demotion the re-score shows is a\n\
           LOWER BOUND on how much the source ranking overstates skill.\n\
         The comparison is on deflation, per-run reliability (pass^k) and the\n\
         bootstrap only. An `_import_note` field restating this is embedded in\n\
         every submission; the scorer ignores it on read.\n"
    );
}

/// Entry point, called from `main` as `sharpebench import <format> ...` with
/// the full (already `--json`-stripped) argv.
pub fn run(args: &[String], json: bool) -> i32 {
    match args.get(2).map(String::as_str) {
        Some("csv") => run_csv(args, json),
        Some("stockbench") => run_stockbench(),
        _ => {
            usage();
            2
        }
    }
}

fn usage() {
    eprintln!(
        "usage: sharpebench import csv <dir-or-file> --out <subs.json> [--trials N] [--json]\n\
         \n\
         directory mode: every <agent_id>.csv inside holds that agent's per-period\n\
         returns, one run per column (header row optional).\n\
         single-file mode with an `agent` column: long format, one row per period,\n\
         columns `agent[,run],return`; rows group into runs per agent.\n\
         single-file mode without an `agent` column: one agent named after the\n\
         file, one run per column.\n\
         \n\
         sharpebench import stockbench <path> is documented but not importable\n\
         from public artifacts; run it for the explanation."
    );
}

/// `import stockbench` is deliberately not a parser. StockBench
/// (arXiv:2510.02209, github.com/ChenYXxxx/stockbench) publishes environment
/// inputs and code, but no per-agent per-period return series: its leaderboard
/// carries only summary statistics (final return, max drawdown, Sortino,
/// averaged over 3 runs). Synthesizing a series from those would fabricate the
/// exact data the gates exist to interrogate, so this command explains instead
/// of pretending.
fn run_stockbench() -> i32 {
    eprintln!(
        "cannot import: StockBench publishes no per-agent per-period return series.\n\
         \n\
         Its repository (github.com/ChenYXxxx/stockbench, Apache-2.0) ships the\n\
         environment (daily per-symbol parquet prices, financials, news) and the\n\
         harness code; per-agent results exist only as summary tables (final\n\
         return, max drawdown, Sortino). Re-scoring under SharpeBench's gates\n\
         needs the underlying daily series, which only the authors hold.\n\
         \n\
         If you run their harness yourself, it writes daily_nav.parquet per run\n\
         under storage/reports/backtest/<run_id>/. Export that NAV column's\n\
         percentage changes to one CSV per model and import with:\n\
         \n\
         sharpebench import csv <dir> --out subs.json --trials N\n\
         sharpebench score subs.json\n\
         \n\
         See docs/book/src/importing.md for the full account of what such a\n\
         re-score can and cannot claim."
    );
    2
}

fn run_csv(args: &[String], json: bool) -> i32 {
    let Some(path) = args.get(3).filter(|p| !p.starts_with('-')) else {
        usage();
        return 2;
    };
    let Some(out) = flag_value(args, "--out") else {
        eprintln!("error: --out <subs.json> is required");
        return 2;
    };
    let trials = match flag_value(args, "--trials") {
        Some(raw) => match raw.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("error: --trials must be a non-negative integer, got `{raw}`");
                return 2;
            }
        },
        None => 0,
    };

    let subs = match import_path(Path::new(path), trials) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    if subs.is_empty() {
        eprintln!("error: no agents found under {path}");
        return 1;
    }

    // Serialize through the real core types, then attach the caveat as an
    // extra field the scorer ignores on read.
    let mut docs = Vec::with_capacity(subs.len());
    for s in &subs {
        let mut v = match serde_json::to_value(s) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("error: serializing submission: {e}");
                return 1;
            }
        };
        v["_import_note"] = serde_json::Value::String(IMPORT_NOTE.to_string());
        docs.push(v);
    }
    let payload = match serde_json::to_string_pretty(&docs) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: serializing submissions: {e}");
            return 1;
        }
    };
    if let Err(e) = std::fs::write(out, payload) {
        eprintln!("error: cannot write {out}: {e}");
        return 1;
    }

    print_notice(trials);
    if json {
        let summary = serde_json::json!({
            "imported": true,
            "agents": subs.len(),
            "runs": subs.iter().map(|s| s.runs.len()).sum::<usize>(),
            "in_sample_trials": trials,
            "path": out,
            "note": IMPORT_NOTE,
        });
        match serde_json::to_string_pretty(&summary) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("error: serializing output: {e}"),
        }
    } else {
        println!(
            "imported {} agent(s), {} run(s) (in_sample_trials={trials}) -> {out}",
            subs.len(),
            subs.iter().map(|s| s.runs.len()).sum::<usize>(),
        );
        println!("re-score with: sharpebench score {out}");
    }
    0
}

/// Import a directory of `<agent_id>.csv` files, or a single CSV (long format
/// when it has an `agent` column, else wide format under the file's stem).
fn import_path(path: &Path, trials: u32) -> Result<Vec<AgentSubmission>, String> {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| format!("cannot read directory {}: {e}", path.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .map(|x| x.eq_ignore_ascii_case("csv"))
                    .unwrap_or(false)
            })
            .collect();
        // Sort for a deterministic field regardless of filesystem order.
        entries.sort();
        let mut subs = Vec::new();
        for file in entries {
            let text = std::fs::read_to_string(&file)
                .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            let agent_id = file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let runs = parse_wide(&text)
                .map_err(|e| format!("{}: {e}", file.display()))?
                .into_iter()
                .map(|returns| Run {
                    returns,
                    ..Run::default()
                })
                .collect();
            subs.push(AgentSubmission {
                agent_id,
                runs,
                in_sample_trials: trials,
                candidates: Vec::new(),
            });
        }
        Ok(subs)
    } else {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if let Some(agents) = parse_long(&text)? {
            Ok(agents
                .into_iter()
                .map(|(agent_id, runs)| AgentSubmission {
                    agent_id,
                    runs: runs
                        .into_iter()
                        .map(|returns| Run {
                            returns,
                            ..Run::default()
                        })
                        .collect(),
                    in_sample_trials: trials,
                    candidates: Vec::new(),
                })
                .collect())
        } else {
            let agent_id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let runs = parse_wide(&text)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .into_iter()
                .map(|returns| Run {
                    returns,
                    ..Run::default()
                })
                .collect();
            Ok(vec![AgentSubmission {
                agent_id,
                runs,
                in_sample_trials: trials,
                candidates: Vec::new(),
            }])
        }
    }
}

/// Wide format: one run per column, per-period returns down the rows. A first
/// row with any non-numeric cell is treated as a header and skipped. Trailing
/// empty cells mean "this column's series ended" (columns of unequal length).
fn parse_wide(text: &str) -> Result<Vec<Vec<f64>>, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(first) = lines.next() else {
        return Err("empty file".to_string());
    };
    let first_cells: Vec<&str> = first.split(',').map(str::trim).collect();
    let is_header = first_cells
        .iter()
        .any(|c| !c.is_empty() && c.parse::<f64>().is_err());
    let n_cols = first_cells.len();
    let mut runs: Vec<Vec<f64>> = vec![Vec::new(); n_cols];
    let body: Vec<&str> = if is_header {
        lines.collect()
    } else {
        std::iter::once(first).chain(lines).collect()
    };
    if body.is_empty() {
        return Err("no data rows".to_string());
    }
    for line in body {
        for (i, cell) in line.split(',').map(str::trim).enumerate() {
            if cell.is_empty() {
                continue;
            }
            let run = runs
                .get_mut(i)
                .ok_or_else(|| format!("row has more cells than the first row ({n_cols})"))?;
            let v = cell
                .parse::<f64>()
                .map_err(|_| format!("non-numeric return `{cell}`"))?;
            run.push(v);
        }
    }
    runs.retain(|r| !r.is_empty());
    if runs.is_empty() {
        return Err("no numeric returns found".to_string());
    }
    Ok(runs)
}

/// Long format: header row with an `agent` (or `agent_id`) column, an optional
/// `run` column, and a returns column (`return`, `returns` or `ret`; else the
/// first column that is neither agent nor run). One row per period; rows group
/// into runs per agent, in first-appearance order for a deterministic field.
///
/// Returns `Ok(None)` when the file has no agent column (caller falls back to
/// wide format).
type LongAgents = Vec<(String, Vec<Vec<f64>>)>;

/// Intermediate grouping: per agent, its runs still keyed by run label.
type LabeledAgents = Vec<(String, Vec<(String, Vec<f64>)>)>;

fn parse_long(text: &str) -> Result<Option<LongAgents>, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(first) = lines.next() else {
        return Err("empty file".to_string());
    };
    let header: Vec<String> = first
        .split(',')
        .map(|c| c.trim().to_ascii_lowercase())
        .collect();
    let Some(agent_col) = header.iter().position(|h| h == "agent" || h == "agent_id") else {
        return Ok(None);
    };
    let run_col = header.iter().position(|h| h == "run");
    let ret_col = header
        .iter()
        .position(|h| h == "return" || h == "returns" || h == "ret")
        .or_else(|| (0..header.len()).find(|&i| i != agent_col && Some(i) != run_col))
        .ok_or("no returns column beside the agent column")?;

    // (agent, runs as (run_label, returns)) in first-appearance order.
    let mut agents: LabeledAgents = Vec::new();
    for (row_idx, line) in lines.enumerate() {
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        let get = |i: usize| cells.get(i).copied().unwrap_or_default();
        let agent = get(agent_col);
        if agent.is_empty() {
            return Err(format!("row {}: empty agent cell", row_idx + 2));
        }
        let run_label = run_col.map(get).unwrap_or_default().to_string();
        let cell = get(ret_col);
        let v = cell
            .parse::<f64>()
            .map_err(|_| format!("row {}: non-numeric return `{cell}`", row_idx + 2))?;
        let entry = match agents.iter_mut().find(|(a, _)| a == agent) {
            Some(e) => e,
            None => {
                agents.push((agent.to_string(), Vec::new()));
                agents.last_mut().expect("just pushed")
            }
        };
        let run = match entry.1.iter_mut().find(|(l, _)| *l == run_label) {
            Some(r) => r,
            None => {
                entry.1.push((run_label.clone(), Vec::new()));
                entry.1.last_mut().expect("just pushed")
            }
        };
        run.1.push(v);
    }
    if agents.is_empty() {
        return Err("no data rows".to_string());
    }
    Ok(Some(
        agents
            .into_iter()
            .map(|(a, runs)| (a, runs.into_iter().map(|(_, r)| r).collect()))
            .collect(),
    ))
}

/// Value following a `--flag` in argv, if present.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_core::{rank, ScoreConfig};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("sharpebench-import-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("create temp dir");
        d
    }

    /// A mildly positive, non-constant 60-period series (2 runs per agent),
    /// enough for the scorer to compute every gate without degenerate stats.
    fn synthetic_csv(seed: f64) -> String {
        let mut s = String::from("run_a,run_b\n");
        for i in 0..60 {
            let a = 0.001 + 0.0005 * ((i as f64) * 0.7 + seed).sin();
            let b = 0.0008 + 0.0006 * ((i as f64) * 1.3 + seed).cos();
            s.push_str(&format!("{a},{b}\n"));
        }
        s
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn round_trip_csv_dir_to_rankable_submissions() {
        let dir = temp_dir("roundtrip");
        std::fs::write(dir.join("alpha.csv"), synthetic_csv(0.0)).unwrap();
        std::fs::write(dir.join("beta.csv"), synthetic_csv(2.0)).unwrap();
        let out = dir.join("subs.json");
        let code = run(
            &argv(&[
                "sharpebench",
                "import",
                "csv",
                dir.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--trials",
                "7",
            ]),
            false,
        );
        assert_eq!(code, 0);

        let text = std::fs::read_to_string(&out).unwrap();
        // The caveat is embedded, and the scorer's serde path ignores it: the
        // same bytes deserialize into the real submission type.
        assert!(text.contains("_import_note"));
        let subs: Vec<AgentSubmission> = serde_json::from_str(&text).unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].agent_id, "alpha");
        assert_eq!(subs[1].agent_id, "beta");
        assert_eq!(subs[0].runs.len(), 2);
        assert_eq!(subs[0].runs[0].returns.len(), 60);
        assert_eq!(subs[0].in_sample_trials, 7);
        assert!(subs[0].runs[0].trace.events.is_empty());

        // `rank` accepts the imported field end to end.
        let board = rank(&subs, &ScoreConfig::default());
        assert_eq!(board.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_format_agent_column_groups_runs() {
        let dir = temp_dir("long");
        let mut csv = String::from("agent,run,return\n");
        for run_label in ["r0", "r1"] {
            for i in 0..40 {
                csv.push_str(&format!("mo,{run_label},{}\n", 0.001 + 0.0001 * i as f64));
            }
        }
        for i in 0..40 {
            csv.push_str(&format!("bh,r0,{}\n", 0.0005 + 0.0002 * i as f64));
        }
        let file = dir.join("field.csv");
        std::fs::write(&file, csv).unwrap();
        let out = dir.join("subs.json");
        let code = run(
            &argv(&[
                "sharpebench",
                "import",
                "csv",
                file.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
            ]),
            true,
        );
        assert_eq!(code, 0);
        let subs: Vec<AgentSubmission> =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].agent_id, "mo");
        assert_eq!(subs[0].runs.len(), 2);
        assert_eq!(subs[1].agent_id, "bh");
        assert_eq!(subs[1].runs.len(), 1);
        assert_eq!(subs[1].runs[0].returns.len(), 40);
        // Default trials is 0 (undeclared), the documented lower-bound stance.
        assert_eq!(subs[0].in_sample_trials, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_csv_errors_cleanly() {
        let dir = temp_dir("malformed");
        std::fs::write(dir.join("bad.csv"), "0.001,0.002\n0.001,oops\n").unwrap();
        let out = dir.join("subs.json");
        let code = run(
            &argv(&[
                "sharpebench",
                "import",
                "csv",
                dir.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
            ]),
            false,
        );
        assert_eq!(code, 1);
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_note_text_carries_the_caveats() {
        for needle in ["process", "calibration", "lower bound", "deflation"] {
            assert!(IMPORT_NOTE.contains(needle), "note lost caveat: {needle}");
        }
    }

    #[test]
    fn usage_and_stockbench_exit_codes() {
        assert_eq!(run(&argv(&["sharpebench", "import"]), false), 2);
        assert_eq!(run(&argv(&["sharpebench", "import", "nonsense"]), false), 2);
        // No --out is a usage error, not a silent default.
        assert_eq!(
            run(&argv(&["sharpebench", "import", "csv", "somewhere"]), false),
            2
        );
        // StockBench publishes no per-period series; the command explains, never parses.
        assert_eq!(
            run(&argv(&["sharpebench", "import", "stockbench", "x"]), false),
            2
        );
    }
}
