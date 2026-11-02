//! `contamination-probe`, `presentation-stability`, `identifiability-probe`
//! and `run --declared-training-cutoff` end to end, against planted `--cmd`
//! entrants. No model is called.
//!
//! This target is built with `harness = false`, so the test binary is also
//! its own subprocess fixture: run as `<binary> --planted-agent <name>`, it
//! speaks the `--cmd` wire protocol (one observation line in, one decision
//! line out, or one identity question in and one typed answer out) as the
//! named planted agent. Otherwise it runs the tests below.
//! It answers `--list` the way nextest expects, so it runs under nextest and
//! under `cargo test` alike.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{iso_year, Dataset, IdentityQuestion, IDENTITY_ANSWER_SCHEMA};

// ---------------------------------------------------------------- planted agents

fn decide(obs: &MarketObservation, weight: impl Fn(usize, &str) -> f64) -> Decision {
    Decision {
        orders: obs
            .symbols
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = weight(i, &s.symbol);
                Order {
                    symbol: s.symbol.clone(),
                    action: if w > 0.0 { Action::Buy } else { Action::Close },
                    target_weight: w,
                    confidence: None,
                    rationale: String::new(),
                }
            })
            .collect(),
        reasoning: String::new(),
        cost: None,
    }
}

fn relatives(series: &[f64]) -> Vec<f64> {
    series.windows(2).map(|w| w[1] / w[0]).collect()
}

/// The planted agent behind `--planted-agent <name> [--data-file <csv>]`.
fn planted(name: &str, public: Option<Dataset>) -> Box<dyn FnMut(&MarketObservation) -> Decision> {
    match name {
        // Honest: holds an equal-weight book whatever it is shown.
        "hold-even" => Box::new(|obs| {
            let n = obs.symbols.len() as f64;
            decide(obs, |_, _| 1.0 / n)
        }),
        // Recalls a ticker: all in SPX when it can see the name.
        "ticker-recall" => Box::new(|obs| {
            let named = obs.symbols.iter().any(|s| s.symbol == "SPX");
            let n = obs.symbols.len() as f64;
            decide(obs, |_, sym| match (named, sym == "SPX") {
                (true, true) => 1.0,
                (true, false) => 0.0,
                (false, _) => 1.0 / n,
            })
        }),
        // Recalls calendar dates: buys the listed position that rose most by
        // the next close on a remembered date; holds evenly otherwise.
        "date-recall" => {
            let data = public.expect("date-recall needs --data-file");
            let series: Vec<Vec<f64>> = data.closes.values().cloned().collect();
            let memory: BTreeMap<String, usize> = (0..data.len() - 1)
                .map(|t| {
                    let best = (0..series.len())
                        .max_by(|&a, &b| {
                            (series[a][t + 1] / series[a][t])
                                .total_cmp(&(series[b][t + 1] / series[b][t]))
                        })
                        .unwrap();
                    (data.dates[t].clone(), best)
                })
                .collect();
            Box::new(move |obs| {
                let n = obs.symbols.len() as f64;
                match memory.get(&obs.date).copied() {
                    Some(best) => decide(obs, |i, _| if i == best { 1.0 } else { 0.0 }),
                    None => decide(obs, |_, _| 1.0 / n),
                }
            })
        }
        // Remembers whole paths: on its first observation it looks the first
        // symbol's return path up in the public data. Once found it knows every
        // later bar and holds whichever symbol rises most next, when any does.
        // Unrecognized, it holds an equal-weight book.
        "memorizer" => {
            let data = public.expect("memorizer needs --data-file");
            let series: Vec<Vec<f64>> = data.closes.values().cloned().collect();
            let mut now: Option<usize> = None;
            let mut first = true;
            Box::new(move |obs| {
                if first {
                    first = false;
                    let seen = relatives(&obs.symbols[0].close_history);
                    now = relatives(&series[0])
                        .windows(seen.len())
                        .position(|w| w == seen)
                        .map(|offset| offset + seen.len());
                } else if let Some(t) = now.as_mut() {
                    *t += 1;
                }
                match now {
                    Some(t) if t + 1 < data.len() => {
                        let (best, gain) = (0..series.len())
                            .map(|s| (s, series[s][t + 1] / series[s][t]))
                            .max_by(|a, b| a.1.total_cmp(&b.1))
                            .unwrap();
                        decide(obs, |i, _| if i == best && gain > 1.0 { 1.0 } else { 0.0 })
                    }
                    _ => {
                        let n = obs.symbols.len() as f64;
                        decide(obs, |_, _| 1.0 / n)
                    }
                }
            })
        }
        other => panic!("no planted agent `{other}`"),
    }
}

/// A planted agent's answer to a closed question: ranked tickers and a year,
/// or `None` to decline.
type Answerer = Box<dyn FnMut(&IdentityQuestion) -> Option<(Vec<String>, Option<i64>)>>;

fn volatility(series: &[f64]) -> f64 {
    let r: Vec<f64> = relatives(series).iter().map(|x| x - 1.0).collect();
    let m = r.iter().sum::<f64>() / r.len() as f64;
    (r.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (r.len() - 1) as f64).sqrt()
}

/// The planted agents that answer identity questions. Any other planted agent
/// does not speak the question and exits when it is asked one.
fn answerer(name: &str, public: Option<Dataset>) -> Option<Answerer> {
    match name {
        // Recalls whole paths: looks the masked return path up in the public
        // data and names the ticker and year it finds there. Unrecognized, it
        // names nothing.
        "path-recall" => {
            let data = public.expect("path-recall needs --data-file");
            Some(Box::new(move |q| {
                let wanted = relatives(&q.closes);
                for (symbol, series) in &data.closes {
                    let have = relatives(series);
                    if let Some(offset) = have.windows(wanted.len()).position(|w| w == wanted) {
                        return Some((
                            vec![symbol.clone()],
                            iso_year(&data.dates[offset + q.scored_from]),
                        ));
                    }
                }
                Some((Vec::new(), None))
            }))
        }
        // Reads texture only: ranks the tickers by how close their whole-history
        // volatility is to the series', and never names a year.
        "texture" => {
            let data = public.expect("texture needs --data-file");
            let profile: Vec<(String, f64)> = data
                .closes
                .iter()
                .map(|(s, v)| (s.clone(), volatility(v)))
                .collect();
            Some(Box::new(move |q| {
                let vol = volatility(&q.closes);
                let mut ranked = profile.clone();
                ranked.sort_by(|a, b| (a.1 - vol).abs().total_cmp(&(b.1 - vol).abs()));
                Some((ranked.into_iter().map(|(s, _)| s).collect(), None))
            }))
        }
        "decline" => Some(Box::new(|_| None)),
        _ => None,
    }
}

fn serve(args: &[String]) {
    let name = &args[args.iter().position(|a| a == "--planted-agent").unwrap() + 1];
    if name == "exit" {
        return;
    }
    let public = args
        .iter()
        .position(|a| a == "--data-file")
        .map(|i| Dataset::from_csv_file(&args[i + 1]).unwrap());
    let mut answer = answerer(name, public.clone());
    let mut agent: Box<dyn FnMut(&MarketObservation) -> Decision> = if answer.is_some() {
        Box::new(|_: &MarketObservation| panic!("a question-only agent is never asked to decide"))
    } else {
        planted(name, public)
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { return };
        let reply = match serde_json::from_str::<IdentityQuestion>(&line) {
            Ok(question) => {
                let Some(answer) = answer.as_mut() else {
                    // Does not speak the question: exits, as a decision-only
                    // entrant would.
                    return;
                };
                let mut reply = serde_json::json!({
                    "schema_version": IDENTITY_ANSWER_SCHEMA,
                    "question_id": question.question_id,
                });
                match answer(&question) {
                    Some((tickers, year)) => {
                        reply["tickers"] = tickers.into();
                        reply["year"] = year.into();
                    }
                    None => reply["declined"] = true.into(),
                }
                reply.to_string()
            }
            Err(_) => {
                let obs: MarketObservation = serde_json::from_str(&line).unwrap();
                serde_json::to_string(&agent(&obs)).unwrap()
            }
        };
        if writeln!(out, "{reply}").is_err() || out.flush().is_err() {
            return;
        }
    }
}

// ---------------------------------------------------------------- fixtures

/// The first 300 dates of the frozen daily index file, so every arm is short.
fn dataset(dir: &Path) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/us-indices-1d.csv");
    let text = std::fs::read_to_string(source).unwrap();
    let mut dates = std::collections::BTreeSet::new();
    let mut kept = vec!["date,symbol,close".to_string()];
    for line in text.lines().skip(1) {
        let date = line.split(',').next().unwrap().to_string();
        if dates.len() == 300 && !dates.contains(&date) {
            break;
        }
        dates.insert(date);
        kept.push(line.to_string());
    }
    let path = dir.join("indices-300.csv");
    std::fs::write(&path, kept.join("\n") + "\n").unwrap();
    path
}

struct Fixture {
    dir: PathBuf,
    data: PathBuf,
    exe: String,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sharpebench-contamination-cli-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let data = dataset(&dir);
        let exe = std::env::current_exe().unwrap().display().to_string();
        Fixture { dir, data, exe }
    }

    /// The `--cmd` value that runs this binary as the named planted agent.
    fn agent(&self, name: &str) -> String {
        format!(
            "{} --planted-agent {name} --data-file {}",
            self.exe,
            self.data.display()
        )
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .args(args)
            .current_dir(&self.dir)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> (Option<i32>, serde_json::Value) {
        let mut all = args.to_vec();
        all.push("--json");
        let out = self.cli(&all);
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let value = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!(
                "{args:?}: {e}\nstdout: {text}\nstderr: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        });
        (out.status.code(), value)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Disagreeing steps per pair of every window, in suite order: tickers-only,
/// dates-only, levels-only, full, reordered, then the date shift.
fn disagreements(doc: &serde_json::Value) -> Vec<Vec<u64>> {
    doc["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            w["suite"]["pairs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["comparison"]["disagreeing_steps"].as_u64().unwrap())
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------- tests

fn presentation_suite_separates_ticker_and_date_recall_through_cmd() {
    let f = Fixture::new("presentation");
    let data = f.data.display().to_string();
    let run = |agent: &str| {
        let cmd = f.agent(agent);
        let (code, doc) = f.json(&[
            "presentation-stability",
            "--cmd",
            &cmd,
            "--data",
            &data,
            "--date-shift-days",
            "364",
        ]);
        assert_eq!(code, Some(0), "{agent}: {doc}");
        assert_eq!(doc["status"], "measured");
        assert_eq!(doc["rank_input"], false);
        assert_eq!(
            doc["schema_version"],
            "sharpebench.presentation-stability.v1"
        );
        disagreements(&doc)
    };
    for windows in run("hold-even") {
        assert_eq!(windows, [0, 0, 0, 0, 0, 0]);
    }
    for w in run("ticker-recall") {
        assert!(
            w[0] > 0 && w[3] > 0,
            "tickers-only and full catch it: {w:?}"
        );
        assert_eq!([w[1], w[2], w[4], w[5]], [0, 0, 0, 0], "{w:?}");
    }
    for w in run("date-recall") {
        assert!(
            w[1] > 0 && w[3] > 0 && w[5] > 0,
            "dates-only, full and the shift: {w:?}"
        );
        assert_eq!(
            [w[0], w[2]],
            [0, 0],
            "levels and tickers keep the calendar: {w:?}"
        );
    }
}

fn shape_probe_convicts_a_cmd_memorizer_and_clears_an_honest_entrant() {
    let f = Fixture::new("shape");
    let data = f.data.display().to_string();
    let run = |agent: &str| {
        let cmd = f.agent(agent);
        let (code, doc) = f.json(&["contamination-probe", "--cmd", &cmd, "--data", &data]);
        assert_eq!(code, Some(0), "{agent}: {doc}");
        // Frictionless is the default: no flag asks for it.
        assert_eq!(doc["context"]["frictionless"], true);
        assert_eq!(doc["schema_version"], "sharpebench.contamination-probe.v1");
        assert_eq!(doc["rank_input"], false);
        doc
    };
    let memorizer = run("memorizer");
    assert_eq!(
        memorizer["result"]["report"]["memorization"], true,
        "{memorizer}"
    );
    assert_eq!(memorizer["result"]["windows"].as_array().unwrap().len(), 7);
    let honest = run("hold-even");
    assert_eq!(
        honest["result"]["report"]["memorization"], false,
        "{honest}"
    );
}

fn with_costs_opts_both_arms_into_runs_cost_model() {
    let f = Fixture::new("with-costs");
    let data = f.data.display().to_string();
    let cmd = f.agent("memorizer");
    let probe = |extra: &[&str]| {
        let mut args = vec!["contamination-probe", "--cmd", &cmd, "--data", &data];
        args.extend_from_slice(extra);
        let (code, doc) = f.json(&args);
        assert_eq!(code, Some(0), "{extra:?}: {doc}");
        doc
    };
    let free = probe(&[]);
    let costed = probe(&["--with-costs", "--short-borrow-bps", "25"]);
    assert_eq!(costed["context"]["frictionless"], false, "{costed}");
    assert_ne!(
        costed["result"]["report"]["mean_edge"], free["result"]["report"]["mean_edge"],
        "the costed arms must actually pay costs"
    );
}

fn a_failed_cmd_arm_is_a_typed_refusal_naming_the_arm() {
    let f = Fixture::new("refusal");
    let data = f.data.display().to_string();
    let cmd = f.agent("exit");
    let (code, doc) = f.json(&["presentation-stability", "--cmd", &cmd, "--data", &data]);
    assert_eq!(code, Some(1), "{doc}");
    assert_eq!(doc["status"], "unavailable");
    assert_eq!(doc["rank_input"], false);
    assert_eq!(doc["unavailable"]["window"], 0);
    assert_eq!(doc["unavailable"]["reason"], "arm");
    assert_eq!(doc["unavailable"]["presentation"]["kind"], "unmasked");
    assert!(doc["unavailable"]["error"].is_string(), "{doc}");
    assert!(doc.get("result").is_none());

    let (code, doc) = f.json(&["contamination-probe", "--cmd", &cmd, "--data", &data]);
    assert_eq!(code, Some(1), "{doc}");
    assert_eq!(doc["unavailable"]["reason"], "arm");
    assert_eq!(doc["unavailable"]["window"], 0);
    assert_eq!(doc["unavailable"]["arm"], "real");
}

fn reference_agents_and_the_declared_cutoff_reach_the_document() {
    let f = Fixture::new("reference");
    let data = f.data.display().to_string();
    let (code, doc) = f.json(&[
        "presentation-stability",
        "--agent",
        "momentum",
        "--data",
        &data,
        "--declared-training-cutoff",
        "2017-01-01",
    ]);
    assert_eq!(code, Some(0), "{doc}");
    assert_eq!(doc["context"]["entrant"], "momentum");
    for w in disagreements(&doc) {
        assert_eq!(w, [0, 0, 0, 0, 0]);
    }
    let cutoff = &doc["context"]["declared_training_cutoff"];
    assert_eq!(cutoff["declared"], "2017-01-01");
    assert_eq!(cutoff["trusted"], false);
    assert_eq!(cutoff["verified"], false);
    let w = &cutoff["windows"];
    let classified = [
        "ending_before_cutoff",
        "straddling_cutoff",
        "starting_on_or_after_cutoff",
    ]
    .iter()
    .map(|k| w[*k].as_u64().unwrap())
    .sum::<u64>();
    assert_eq!(classified, 2, "{cutoff}");
}

fn run_publishes_the_declared_cutoff_without_touching_evidence_mode() {
    let f = Fixture::new("run");
    let data = f.data.display().to_string();
    let cmd = f.agent("hold-even");
    let row = |extra: &[&str]| {
        let mut args = vec!["run", "--cmd", &cmd, "--data", &data];
        args.extend_from_slice(extra);
        let (code, board) = f.json(&args);
        assert_eq!(code, Some(0), "{board}");
        board
            .as_array()
            .unwrap()
            .iter()
            .find(|r| {
                r["agent_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("cmd:"))
            })
            .cloned()
            .unwrap()
    };
    let plain = row(&[]);
    let declared = row(&["--declared-training-cutoff", "2017-01-01"]);
    assert!(plain.get("declared_training_cutoff").is_none());
    assert_eq!(declared["evidence_mode"], plain["evidence_mode"]);
    assert_eq!(
        declared["declared_training_cutoff"]["declared"],
        "2017-01-01"
    );
    assert_eq!(
        declared["declared_training_cutoff"]["changes_evidence_mode"],
        false
    );
    // Everything but the cutoff and the host-clock durations is the same row.
    let strip = |row: &serde_json::Value| {
        let mut row = row.clone();
        row.as_object_mut()
            .unwrap()
            .remove("declared_training_cutoff");
        row["attempt_accounting"]["attempts"]["duration_ns_total"] = serde_json::Value::Null;
        row
    };
    assert_eq!(
        strip(&declared),
        strip(&plain),
        "nothing else on the row moves"
    );
}

fn identifiability_probe_catches_a_recalling_cmd_entrant_and_not_a_texture_reader() {
    let f = Fixture::new("identify");
    let data = f.data.display().to_string();
    let run = |agent: &str| {
        let cmd = f.agent(agent);
        let (code, doc) = f.json(&["identifiability-probe", "--cmd", &cmd, "--data", &data]);
        assert_eq!(code, Some(0), "{agent}: {doc}");
        assert_eq!(doc["status"], "measured");
        assert_eq!(
            doc["schema_version"],
            "sharpebench.identifiability-probe.v1"
        );
        assert_eq!(doc["rank_input"], false);
        assert_eq!(doc["rank_neutral"], true);
        doc["result"].clone()
    };
    // 300 bars give seven 40-bar windows after the warmup, of three symbols.
    let recall = run("path-recall");
    assert_eq!(recall["windows"], 7, "{recall}");
    assert_eq!(recall["real"]["questions"], 21);
    assert_eq!(recall["twin"]["questions"], 21);
    assert_eq!(recall["real"]["joint"], 1.0, "{recall}");
    assert_eq!(recall["twin"]["joint"], 0.0, "{recall}");
    assert_eq!(recall["path_identified"], true, "{recall}");
    // The chance bar a mask must be held to: one ticker in three.
    let chance = recall["chance"]["ticker_top1"].as_f64().unwrap();
    assert!((chance - 1.0 / 3.0).abs() < 1e-12, "{recall}");

    let texture = run("texture");
    assert_eq!(texture["real"]["year"], 0.0, "{texture}");
    assert_eq!(texture["path_identified"], false, "{texture}");
    let gap = texture["real"]["ticker_top1"].as_f64().unwrap()
        - texture["twin"]["ticker_top1"].as_f64().unwrap();
    assert!(gap.abs() <= 0.2, "texture survives the twin: {texture}");
}

fn a_refusing_or_mute_entrant_reads_unavailable() {
    let f = Fixture::new("identify-refusal");
    let data = f.data.display().to_string();
    let refused = |agent: &str| {
        let cmd = f.agent(agent);
        let (code, doc) = f.json(&["identifiability-probe", "--cmd", &cmd, "--data", &data]);
        assert_eq!(code, Some(1), "{agent}: {doc}");
        assert_eq!(doc["status"], "unavailable");
        assert_eq!(doc["rank_input"], false);
        assert!(doc.get("result").is_none(), "{doc}");
        assert_eq!(doc["unavailable"]["reason"], "answer", "{doc}");
        assert_eq!(doc["unavailable"]["question_id"], 0, "{doc}");
        doc["unavailable"]["error"].clone()
    };
    assert_eq!(
        refused("decline"),
        serde_json::json!({ "kind": "declined" })
    );
    // A decision-only entrant does not speak the question and exits.
    let mute = refused("hold-even");
    assert_eq!(mute["kind"], "failed", "{mute}");
    assert_eq!(mute["failure"], "transport_error", "{mute}");
    // The source must carry calendar years to score against.
    let cmd = f.agent("path-recall");
    let (code, doc) = f.json(&["identifiability-probe", "--cmd", &cmd]);
    assert_eq!(code, Some(1), "{doc}");
    assert_eq!(doc["unavailable"]["reason"], "undated_window", "{doc}");
}

/// `--http` asks over `POST /identify`: an endpoint that declines the first
/// question it is sent leaves the probe unavailable.
fn an_http_entrant_is_asked_over_identify() {
    let f = Fixture::new("identify-http");
    let data = f.data.display().to_string();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let mut length = 0;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header.trim().is_empty() {
                break;
            }
            if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().unwrap();
            }
        }
        let mut body = vec![0; length];
        std::io::Read::read_exact(&mut reader, &mut body).unwrap();
        let question: IdentityQuestion = serde_json::from_slice(&body).unwrap();
        let reply = serde_json::json!({
            "schema_version": IDENTITY_ANSWER_SCHEMA,
            "question_id": question.question_id,
            "declined": true,
        })
        .to_string();
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        )
        .unwrap();
        let _ = socket.shutdown(std::net::Shutdown::Write);
        let _ = std::io::Read::read_to_end(&mut reader, &mut Vec::new());
        request_line
    });
    let (code, doc) = f.json(&["identifiability-probe", "--http", &addr, "--data", &data]);
    assert_eq!(code, Some(1), "{doc}");
    assert_eq!(doc["context"]["entrant"], format!("http:{addr}"));
    assert_eq!(doc["unavailable"]["error"]["kind"], "declined", "{doc}");
    assert_eq!(server.join().unwrap().trim_end(), "POST /identify HTTP/1.1");
}

fn usage_errors_launch_nothing() {
    let f = Fixture::new("usage");
    let cmd = f.agent("hold-even");
    for args in [
        vec!["presentation-stability"],
        vec!["contamination-probe", "--agent", "momentum", "--cmd", &cmd],
        vec!["presentation-stability", "--agent", "random"],
        vec!["presentation-stability", "--agent", "momentum", "--masked"],
        vec![
            "contamination-probe",
            "--agent",
            "momentum",
            "--window-bars",
            "1",
        ],
        vec![
            "contamination-probe",
            "--agent",
            "momentum",
            "--windows",
            "x",
        ],
        // A cost flag without --with-costs would be silently ignored.
        vec![
            "contamination-probe",
            "--agent",
            "momentum",
            "--short-borrow-bps",
            "25",
        ],
        vec![
            "contamination-probe",
            "--agent",
            "momentum",
            "--with-costs",
            "--frictionless",
        ],
        vec![
            "presentation-stability",
            "--agent",
            "momentum",
            "--date-shift-days",
            "1.5",
        ],
        vec![
            "presentation-stability",
            "--agent",
            "momentum",
            "--declared-training-cutoff",
            "2017-02-30",
        ],
        vec!["identifiability-probe", "--agent", "momentum"],
        vec!["identifiability-probe", "--cmd", &cmd, "--with-costs"],
        vec!["identifiability-probe", "--cmd", &cmd, "--top-k", "0"],
        vec!["identifiability-probe"],
        vec!["run", "--declared-training-cutoff", "2017-01-01"],
        vec!["run", "--cmd", &cmd, "--declared-training-cutoff"],
    ] {
        let out = f.cli(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("NO sandbox"),
            "{args:?} launched"
        );
    }
}

// ---------------------------------------------------------------- runner

type Case = (&'static str, fn());

const CASES: &[Case] = &[
    (
        "presentation_suite_separates_ticker_and_date_recall_through_cmd",
        presentation_suite_separates_ticker_and_date_recall_through_cmd,
    ),
    (
        "shape_probe_convicts_a_cmd_memorizer_and_clears_an_honest_entrant",
        shape_probe_convicts_a_cmd_memorizer_and_clears_an_honest_entrant,
    ),
    (
        "with_costs_opts_both_arms_into_runs_cost_model",
        with_costs_opts_both_arms_into_runs_cost_model,
    ),
    (
        "a_failed_cmd_arm_is_a_typed_refusal_naming_the_arm",
        a_failed_cmd_arm_is_a_typed_refusal_naming_the_arm,
    ),
    (
        "reference_agents_and_the_declared_cutoff_reach_the_document",
        reference_agents_and_the_declared_cutoff_reach_the_document,
    ),
    (
        "run_publishes_the_declared_cutoff_without_touching_evidence_mode",
        run_publishes_the_declared_cutoff_without_touching_evidence_mode,
    ),
    (
        "identifiability_probe_catches_a_recalling_cmd_entrant_and_not_a_texture_reader",
        identifiability_probe_catches_a_recalling_cmd_entrant_and_not_a_texture_reader,
    ),
    (
        "a_refusing_or_mute_entrant_reads_unavailable",
        a_refusing_or_mute_entrant_reads_unavailable,
    ),
    (
        "an_http_entrant_is_asked_over_identify",
        an_http_entrant_is_asked_over_identify,
    ),
    ("usage_errors_launch_nothing", usage_errors_launch_nothing),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--planted-agent") {
        serve(&args);
        return;
    }
    if args.iter().any(|a| a == "--list") {
        if !args.iter().any(|a| a == "--ignored") {
            for (name, _) in CASES {
                println!("{name}: test");
            }
        }
        return;
    }
    let exact = args.iter().any(|a| a == "--exact");
    let filter = args.iter().skip(1).find(|a| !a.starts_with("--"));
    let selected: Vec<&Case> = CASES
        .iter()
        .filter(|(name, _)| match filter {
            None => true,
            Some(f) if exact => name == f,
            Some(f) => name.contains(f.as_str()),
        })
        .collect();
    // `--cmd` splits its value on whitespace, so the fixture path must have none.
    if std::env::current_exe()
        .unwrap()
        .display()
        .to_string()
        .contains(char::is_whitespace)
    {
        println!("skipped: the test binary's path contains whitespace");
        return;
    }
    let mut failed = 0;
    println!("running {} tests", selected.len());
    for (name, case) in &selected {
        let ok = std::panic::catch_unwind(case).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!(
        "test result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        selected.len() - failed
    );
    if failed > 0 {
        std::process::exit(101);
    }
}
