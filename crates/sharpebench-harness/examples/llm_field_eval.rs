//! First LLM field for the SharpeBench paper.
//!
//! Drives external LLM agents (examples/llm-agent/llm_agent.py speaking the
//! stdio ExternalAgent protocol; one submission per model in LLM_MODELS:
//! Claude Fable 5, Claude Opus 5, Claude Haiku 4.5) through the same
//! walk-forward windows,
//! seeds, reference field, luck floor, and scoring kernel as the paper's
//! evidence sweep, on two datasets only (cost scoping): us-indices-1d and
//! crypto-majors-1d. One scoring config uses a host floor of one and a DSR bar
//! of 0.80; ranking still deflates for at least the observed field size under
//! the same measured-dispersion safeguards as every other ranked field.
//!
//! Run from the repo root with ANTHROPIC_API_KEY (and optionally LLM_CACHE_DIR /
//! LLM_STATS_DIR / LLM_STRIDE / LLM_MAX_CALLS) exported. Those four controls
//! are passed through the hermetic spawn and their effective values are written
//! into every record: the documented invocation previously allowlisted only the
//! credential, so an exported spending cap, decision cadence or evidence-cache
//! location was dropped and the shim silently used its own defaults. The
//! credential stays separately allowlisted and never reaches the evidence.
//! The run fails closed:
//! a provider/transport error or an exhausted call budget aborts the field, and
//! only a completely evaluated field is renamed to the requested output. The
//! optional dataset selector is resolved against the declared set before the
//! output is opened, and every planned dataset must have contributed records
//! before the rename: a selector that names nothing is a refusal, not an empty
//! field published under the requested name.
//!
//!   cargo run --release -p sharpebench-harness --example llm_field_eval -- <out.jsonl> [dataset]

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Duration;

use serde::Serialize;
use sharpebench_core::composite::{rank, ScoreConfig, TrialsSrStdSource};
use sharpebench_core::AgentSubmission;
use sharpebench_core::PassMode;
use sharpebench_harness::luck_floor;
use sharpebench_sim::{
    is_credential_name, run_backtest, tag_regime, walk_forward, Agent, BuyAndHold, CostModel,
    Dataset, ExternalAgent, HoldAgent, Momentum, Window,
};

#[path = "support/declared_support.rs"]
mod declared_support;
use declared_support::{or_refuse, require_evaluated, resolve_selector};

const DATASETS: &[(&str, &str, &str, f64)] = &[
    ("us-indices-1d", "equity-index", "1d", 252.0),
    ("crypto-majors-1d", "crypto", "1d", 365.0),
];

const DSR_BAR: f64 = 0.80;
const N_TRIALS: u32 = 1;
const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const LUCK_FLOOR_AGENTS: usize = 5;
const NEVER_CATASTROPHIC_RUN_DD: f64 = 0.20;
/// The three-model field, in budget priority order: if spend forces a cut,
/// the frontier model completes first. The model id is handed to the agent
/// script as argv[1]; the script keys its response cache per model.
const LLM_MODELS: &[&str] = &[
    "claude-fable-5",
    "claude-opus-5",
    "claude-haiku-4-5-20251001",
];
const LLM_SCRIPT: &str = "examples/llm-agent/llm_agent.py";
/// The credential the shim needs, allowlisted on its own so no cost control can
/// widen what carries secret material.
const LLM_CREDENTIAL: &str = "ANTHROPIC_API_KEY";
/// The non-secret controls the module docs tell operators to export. They must
/// reach the shim, or the documented invocation does not do what it says.
const LLM_CONTROLS: &[&str] = &[
    "LLM_CACHE_DIR",
    "LLM_STATS_DIR",
    "LLM_STRIDE",
    "LLM_MAX_CALLS",
];
const EXTERNAL_MAX_RETRIES: u32 = 2;
/// Generous per-decision budget: an API round trip (frontier-tier thinking
/// included) plus SDK retries.
const LLM_DECIDE_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Serialize)]
struct Record<'a> {
    dataset: &'a str,
    asset_class: &'a str,
    timeframe: &'a str,
    periods_per_year: f64,
    n_bars: usize,
    n_symbols: usize,
    n_windows: usize,
    window_len: usize,
    n_seeds: usize,
    regimes: Vec<String>,
    dsr_bar: f64,
    n_trials: u32,
    sr_std_pinned: Option<f64>,
    agent_id: String,
    /// The LLM behind the agent, for LLM rows; None for reference agents and
    /// the luck floor.
    ///
    /// This is the *requested* model id, and it is also the effective one: the
    /// shim pins policy identity to the requested model, refuses any
    /// substitution the provider would serve under that name, and fails the
    /// subprocess instead. A run that reaches this record therefore has no
    /// requested/effective gap to report. Before that, an unknown-model error
    /// silently rebound the shim to an unversioned alias while the producer
    /// went on writing the requested id here.
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    /// `NAME=value` for every non-secret control the hermetic spawn passed to
    /// the shim, resolved at spawn time, with credential-shaped names bound as
    /// `NAME=<secret>`.
    ///
    /// Written on every row, LLM or reference, because it describes the run and
    /// not the agent. Without it the record could not distinguish a field run
    /// under a lowered call cap or a widened decision stride from one under the
    /// shim's own defaults.
    agent_env_controls: Vec<String>,
    /// Model-output protocol failures are agent evidence and become sentinel
    /// runs. Host/provider/runtime failures still abort the whole field.
    ///
    /// Emitted unconditionally, with no `skip_serializing_if`: a zero here is an
    /// affirmative record that the field saw no protocol faults, which is not
    /// the same statement as the key being absent.
    ///
    /// A local `paper/evidence/final/llm-field-records-*.jsonl` from an earlier
    /// run will lack the key. That is not a divergence from committed evidence:
    /// those files are gitignored paid-run scratch (`.gitignore`, "LLM-field
    /// runs are paid, resumable experiments"), and only a complete assembled
    /// `llm-field.jsonl` is ever admitted. Adding `skip_serializing_if` to make
    /// an old scratch file look shape-compatible would be worse than useless:
    /// such a file also predates `examples/llm-agent/llm_agent.py` no longer
    /// converting malformed model output into a hold, so its return series
    /// reflects masked faults that are now scored, and it does not reproduce
    /// whatever this key does.
    agent_protocol_failures: usize,
    deflated_sharpe: f64,
    psr: f64,
    passed_k: bool,
    process_ok: bool,
    bootstrap_p: f64,
    raw_mean_return: f64,
    rank_eligible: bool,
    eligible_never_catastrophic: bool,
    worst_run_drawdown: f64,
    field_reality_check_p: f64,
    step_down_significant: bool,
    trials_sr_std_used: f64,
    trials_sr_std_source: String,
}

type AgentFactory = Box<dyn Fn() -> Box<dyn Agent>>;

/// Every variable the hermetic spawn passes to the shim: the credential plus the
/// documented controls, and nothing else.
fn agent_passthrough() -> Vec<&'static str> {
    let mut names = vec![LLM_CREDENTIAL];
    names.extend_from_slice(LLM_CONTROLS);
    names
}

/// The effective value of each control, for the evidence record.
///
/// Same shape as the sweep checkpoint identity in `agent_env_identity`: an
/// unset name binds `<unset>`, which is distinct from any value it could hold,
/// and a credential-shaped name binds `<secret>` so no key material can reach
/// a published record through a control that was later renamed. Pure in its
/// inputs so the binding can be asserted without an ambient environment.
fn effective_controls(names: &[&str], lookup: impl Fn(&str) -> Option<String>) -> Vec<String> {
    names
        .iter()
        .map(|name| {
            if is_credential_name(name) {
                format!("{name}=<secret>")
            } else {
                match lookup(name) {
                    Some(value) => format!("{name}={value}"),
                    None => format!("{name}=<unset>"),
                }
            }
        })
        .collect()
}

fn windows_for(n: usize) -> (Vec<Window>, usize) {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    (walk_forward(n, warmup, test, test), test)
}

fn reference_field(data: &Dataset, windows: &[Window]) -> Vec<AgentSubmission> {
    let agents: Vec<(&str, AgentFactory)> = vec![
        ("buy-and-hold", Box::new(|| Box::new(BuyAndHold))),
        ("momentum", Box::new(|| Box::new(Momentum::default()))),
        ("hold", Box::new(|| Box::new(HoldAgent))),
    ];
    let mut subs: Vec<AgentSubmission> = agents
        .into_iter()
        .map(|(id, make)| {
            let mut runs = Vec::new();
            for w in windows {
                for seed in EXEC_SEEDS {
                    let mut agent = make();
                    runs.push(run_backtest(
                        data,
                        agent.as_mut(),
                        *w,
                        seed,
                        CostModel::default(),
                    ));
                }
            }
            AgentSubmission {
                agent_id: id.to_string(),
                runs,
                in_sample_trials: 0,
                candidates: Vec::new(),
            }
        })
        .collect();
    subs.extend(luck_floor(
        data,
        windows,
        &EXEC_SEEDS,
        CostModel::default(),
        LUCK_FLOOR_AGENTS,
    ));
    subs
}

fn main() {
    // Required positional; see evidence_sweep for why there is no default.
    let out = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: llm_field_eval <out.jsonl> [dataset]");
        std::process::exit(2);
    });
    // Resolve the optional dataset selector before anything is opened: an
    // unrecognized name used to skip every dataset and publish an empty field.
    let declared: Vec<&str> = DATASETS.iter().map(|(name, ..)| *name).collect();
    let planned = or_refuse(resolve_selector(
        "dataset",
        &declared,
        env::args().nth(2).as_deref(),
    ));
    // Resolved once, before anything is spawned, so every record in the file
    // reports the same controls the first spawn actually received.
    let passthrough = agent_passthrough();
    let controls = effective_controls(&passthrough, |name| env::var(name).ok());
    eprintln!("agent controls: {}", controls.join(" "));
    let partial = format!("{out}.partial");
    let mut w = BufWriter::new(File::create(&partial).expect("create partial output"));
    let mut n_records = 0usize;
    let mut evaluated: Vec<String> = Vec::new();

    for (name, class, tf, ppy) in DATASETS {
        if !planned.iter().any(|d| d == name) {
            continue;
        }
        let before = n_records;
        let path = format!("data/{name}.csv");
        let data = Dataset::from_csv_file(&path).expect("load dataset");
        let n = data.len();
        let (windows, window_len) = windows_for(n);
        let regimes: Vec<String> = windows
            .iter()
            .map(|w| format!("{:?}", tag_regime(&data, *w)))
            .collect();
        eprintln!(
            "{name}: {n} bars, {} symbols, {} windows of {window_len}",
            data.symbols().len(),
            windows.len()
        );

        let mut subs = reference_field(&data, &windows);
        let mut model_by_agent: Vec<(String, String, usize)> = Vec::new();

        for model in LLM_MODELS {
            let agent_id = format!("llm-{model}");
            eprintln!("{name}: running {agent_id}");
            let res = sharpebench_harness::run_external_agent(
                &agent_id,
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                EXTERNAL_MAX_RETRIES,
                || {
                    // Hermetic spawn + exactly the credential and the four
                    // documented controls; the rest of the harness environment
                    // stays out.
                    ExternalAgent::spawn_with_env("python", &[LLM_SCRIPT, model], &passthrough)
                        .ok()
                        .map(|a| a.with_decide_timeout(LLM_DECIDE_TIMEOUT))
                },
            );
            if res.failures.runtime_failures() > 0 {
                panic!(
                    "{name}: incomplete field: {} transport failure(s) for {agent_id} ({} runtime, {} agent-fault); refusing to publish partial evidence",
                    res.failures.records.len(),
                    res.failures.runtime_failures(),
                    res.failures.agent_faults(),
                );
            }
            model_by_agent.push((agent_id, model.to_string(), res.failures.agent_faults()));
            subs.insert(0, res.submission);
        }

        let cfg = ScoreConfig {
            dsr_bar: DSR_BAR,
            n_trials: N_TRIALS,
            execution_seeds_per_window: EXEC_SEEDS.len(),
            ..ScoreConfig::for_periods_per_year(*ppy)
        };
        let mut cfg_nc = cfg.clone();
        cfg_nc.pass_mode = PassMode::Any;
        cfg_nc.mandate.max_run_drawdown = NEVER_CATASTROPHIC_RUN_DD;
        let scored_nc = rank(&subs, &cfg_nc);
        for s in rank(&subs, &cfg) {
            let nc = scored_nc
                .iter()
                .find(|x| x.agent_id == s.agent_id)
                .expect("same field under both gates");
            let rec = Record {
                dataset: name,
                asset_class: class,
                timeframe: tf,
                periods_per_year: *ppy,
                n_bars: n,
                n_symbols: data.symbols().len(),
                n_windows: windows.len(),
                window_len,
                n_seeds: EXEC_SEEDS.len(),
                regimes: regimes.clone(),
                dsr_bar: DSR_BAR,
                n_trials: N_TRIALS,
                sr_std_pinned: None,
                agent_id: s.agent_id.clone(),
                model: model_by_agent
                    .iter()
                    .find(|(a, _, _)| *a == s.agent_id)
                    .map(|(_, m, _)| m.clone()),
                agent_env_controls: controls.clone(),
                agent_protocol_failures: model_by_agent
                    .iter()
                    .find(|(a, _, _)| *a == s.agent_id)
                    .map_or(0, |(_, _, failures)| *failures),
                deflated_sharpe: s.deflated_sharpe,
                psr: s.psr,
                passed_k: s.passed_k,
                process_ok: s.process_ok,
                bootstrap_p: s.bootstrap_p,
                raw_mean_return: s.raw_mean_return,
                rank_eligible: s.rank_eligible,
                eligible_never_catastrophic: nc.rank_eligible,
                worst_run_drawdown: s.worst_run_drawdown,
                field_reality_check_p: s.field_reality_check_p,
                step_down_significant: s.step_down_significant,
                trials_sr_std_used: s.trials_sr_std,
                trials_sr_std_source: match s.trials_sr_std_source {
                    TrialsSrStdSource::Measured => "measured".into(),
                    TrialsSrStdSource::MeasuredFloored => "measured_floored".into(),
                    TrialsSrStdSource::Configured => "configured".into(),
                },
            };
            serde_json::to_writer(&mut w, &rec).expect("write record");
            w.write_all(b"\n").expect("newline");
            n_records += 1;
        }
        if n_records > before {
            evaluated.push((*name).to_string());
        }
        w.flush().expect("flush");
    }
    w.flush().expect("flush completed field");
    drop(w);
    // The rename is the publication. Everything planned must be in the file.
    or_refuse(require_evaluated("dataset", &planned, &evaluated));
    std::fs::rename(&partial, &out).expect("publish completed field atomically");
    eprintln!("wrote {n_records} complete records to {out}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented invocation must actually reach the shim. Before this,
    /// only the credential was allowlisted and an exported cap or stride was
    /// dropped by `env_clear`.
    #[test]
    fn every_documented_control_is_passed_through() {
        let names = agent_passthrough();
        for control in LLM_CONTROLS {
            assert!(
                names.contains(control),
                "{control} is documented but not passed to the shim"
            );
        }
        assert!(names.contains(&LLM_CREDENTIAL));
    }

    /// Passthrough is an allowlist, not a hole: nothing beyond the credential
    /// and the four documented controls may cross the hermetic boundary.
    #[test]
    fn passthrough_is_exactly_the_credential_and_the_controls() {
        let names = agent_passthrough();
        assert_eq!(names.len(), 1 + LLM_CONTROLS.len());
        for name in &names {
            assert!(
                *name == LLM_CREDENTIAL || LLM_CONTROLS.contains(name),
                "{name} is neither the credential nor a documented control"
            );
        }
    }

    /// An unset control binds a value distinct from anything it could hold, so
    /// "the operator exported nothing" is readable off the record.
    #[test]
    fn unset_control_is_distinguishable_from_any_value() {
        let bound = effective_controls(&["LLM_MAX_CALLS"], |_| None);
        assert_eq!(bound, vec!["LLM_MAX_CALLS=<unset>".to_string()]);
        let set = effective_controls(&["LLM_MAX_CALLS"], |_| Some("40".into()));
        assert_eq!(set, vec!["LLM_MAX_CALLS=40".to_string()]);
        assert_ne!(bound, set);
    }

    /// The credential's presence is recorded; its value never is. A control
    /// renamed into credential shape must not leak either.
    #[test]
    fn credential_values_never_reach_the_record() {
        let bound = effective_controls(&agent_passthrough(), |name| {
            if name == LLM_CREDENTIAL {
                Some("sk-live-do-not-log".into())
            } else {
                Some(format!("value-of-{name}"))
            }
        });
        assert!(
            bound.contains(&format!("{LLM_CREDENTIAL}=<secret>")),
            "the credential must be bound by presence, not by value"
        );
        assert!(
            !bound
                .iter()
                .any(|entry| entry.contains("sk-live-do-not-log")),
            "a secret value reached the evidence: {bound:?}"
        );
    }
}
