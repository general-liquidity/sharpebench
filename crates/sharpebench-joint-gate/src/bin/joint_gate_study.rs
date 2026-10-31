//! The joint-gate study producer.
//!
//! ```text
//! joint-gate-study fingerprint
//! joint-gate-study plan   --campaign FILE
//! joint-gate-study seal   --campaign FILE --protocol FILE --protocol FILE --out FILE
//! joint-gate-study smoke  --campaign FILE --out DIR --replicates N [--workers W]
//! joint-gate-study frozen --campaign FILE --out DIR --protocol FILE [--protocol FILE] [--workers W]
//! joint-gate-study report --campaign FILE --out DIR
//! ```
//!
//! `smoke` runs reduced counts on seeds disjoint from the frozen ones. `frozen`
//! refuses unless every named protocol validates, its budget is approved, and it
//! binds this campaign and the linked engine. Both resume: rerun the same
//! command after an interruption. Exit status is nonzero on any refusal.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sharpebench_joint_gate::campaign::Campaign;
use sharpebench_joint_gate::gate::{authorize_frozen, protocol_digest, run_identity};
use sharpebench_joint_gate::report::build;
use sharpebench_joint_gate::shards::{
    check_output_dir, engine_fingerprint, load_rows, rank_calls_per_replicate, run_all, RunError,
    RunIdentity, Tier,
};
use sharpebench_study::StudyProtocol;

fn refuse(detail: impl Into<String>) -> RunError {
    RunError::Refused(detail.into())
}

#[derive(Default)]
struct Args {
    campaign: Option<PathBuf>,
    out: Option<PathBuf>,
    protocols: Vec<PathBuf>,
    replicates: Option<u64>,
    workers: Option<usize>,
    stop_after: Option<usize>,
}

fn parse(mut tokens: std::vec::IntoIter<String>) -> Result<Args, RunError> {
    let mut args = Args::default();
    while let Some(flag) = tokens.next() {
        let mut value = || {
            tokens
                .next()
                .ok_or_else(|| refuse(format!("{flag} needs a value")))
        };
        match flag.as_str() {
            "--campaign" => args.campaign = Some(value()?.into()),
            "--out" => args.out = Some(value()?.into()),
            "--protocol" => args.protocols.push(value()?.into()),
            "--replicates" => {
                let v = value()?;
                args.replicates = Some(v.parse().map_err(|_| refuse(format!("bad count {v}")))?);
            }
            "--workers" => {
                let v = value()?;
                args.workers = Some(v.parse().map_err(|_| refuse(format!("bad count {v}")))?);
            }
            "--stop-after" => {
                let v = value()?;
                args.stop_after = Some(v.parse().map_err(|_| refuse(format!("bad count {v}")))?);
            }
            other => return Err(refuse(format!("unknown argument {other}"))),
        }
    }
    Ok(args)
}

fn load_campaign(args: &Args) -> Result<Campaign, RunError> {
    let path = args
        .campaign
        .as_ref()
        .ok_or_else(|| refuse("--campaign is required"))?;
    let text = fs::read_to_string(path).map_err(|e| refuse(format!("{}: {e}", path.display())))?;
    Campaign::from_json(&text).map_err(|e| refuse(e.to_string()))
}

fn output_dir(args: &Args) -> Result<PathBuf, RunError> {
    let out = args
        .out
        .clone()
        .ok_or_else(|| refuse("--out is required"))?;
    // Checked against this checkout as built and against the working directory,
    // so running from elsewhere cannot make the frozen directories invisible.
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    check_output_dir(&out, &built)?;
    check_output_dir(&out, Path::new("."))?;
    Ok(out)
}

fn workers(args: &Args) -> usize {
    args.workers.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    })
}

fn execute(args: &Args, campaign: &Campaign, identity: &RunIdentity) -> Result<(), RunError> {
    let out = output_dir(args)?;
    let timing = run_all(campaign, identity, &out, workers(args), args.stop_after)?;
    let runtime = serde_json::to_string_pretty(&timing).expect("timing serializes") + "\n";
    fs::write(out.join("runtime.json"), runtime).map_err(|e| refuse(e.to_string()))?;
    eprintln!(
        "ran {} shards, skipped {} already complete, {:.1}s on {} workers",
        timing.shards_run, timing.shards_skipped, timing.wall_seconds, timing.workers
    );
    Ok(())
}

fn command(name: &str, args: &Args) -> Result<(), RunError> {
    match name {
        "fingerprint" => {
            println!("{}", engine_fingerprint());
            Ok(())
        }
        "plan" => {
            let campaign = load_campaign(args)?;
            println!("campaign digest {}", campaign.digest());
            let mut calls = 0;
            for block in &campaign.statistical {
                let per = rank_calls_per_replicate(block);
                calls += per * block.replicates;
                println!(
                    "{:<24} {:>7} replicates x {:>5} rank calls",
                    block.id, block.replicates, per
                );
            }
            for block in &campaign.integrity {
                println!(
                    "{:<24} {:>7} replicates",
                    block.kind.tag(),
                    block.replicates
                );
            }
            println!("statistical rank calls {calls}");
            Ok(())
        }
        "seal" => {
            let campaign = load_campaign(args)?;
            let mut protocols = serde_json::Map::new();
            for path in &args.protocols {
                let text = fs::read_to_string(path)
                    .map_err(|e| refuse(format!("{}: {e}", path.display())))?;
                let protocol =
                    StudyProtocol::from_json(&text).map_err(|e| refuse(e.to_string()))?;
                protocols.insert(
                    protocol.protocol_id.clone(),
                    protocol_digest(&protocol).into(),
                );
            }
            let seal = serde_json::json!({
                "campaign_digest": campaign.digest(),
                "engine_fingerprint": engine_fingerprint(),
                "protocol_digests": protocols,
            });
            let out = args
                .out
                .as_ref()
                .ok_or_else(|| refuse("--out is required"))?;
            let text = serde_json::to_string_pretty(&seal).expect("a seal serializes")
                + "
";
            fs::write(out, text).map_err(|e| refuse(e.to_string()))
        }
        "smoke" => {
            let campaign = load_campaign(args)?;
            let identity = run_identity(
                Tier::Smoke,
                &campaign,
                engine_fingerprint(),
                args.replicates,
            )?;
            execute(args, &campaign, &identity)
        }
        "frozen" => {
            let campaign = load_campaign(args)?;
            let fingerprint = engine_fingerprint();
            let mut protocols = Vec::new();
            for path in &args.protocols {
                let text = fs::read_to_string(path)
                    .map_err(|e| refuse(format!("{}: {e}", path.display())))?;
                protocols.push(StudyProtocol::from_json(&text).map_err(|e| refuse(e.to_string()))?);
            }
            authorize_frozen(&protocols, &campaign, &fingerprint)?;
            let identity = run_identity(Tier::Frozen, &campaign, fingerprint, args.replicates)?;
            execute(args, &campaign, &identity)
        }
        "report" => {
            let campaign = load_campaign(args)?;
            let out = output_dir(args)?;
            let text = fs::read_to_string(out.join("run-identity.json"))
                .map_err(|e| refuse(e.to_string()))?;
            let identity: RunIdentity =
                serde_json::from_str(&text).map_err(|e| refuse(e.to_string()))?;
            if identity.campaign_digest != campaign.digest() {
                return Err(refuse("the run in --out used another campaign"));
            }
            let (rows, controls) = load_rows(&campaign, &identity, &out)?;
            let report = build(&campaign, identity.tier, &rows, &controls);
            let text = serde_json::to_string_pretty(&report).expect("a report serializes") + "\n";
            fs::write(out.join("report.json"), &text).map_err(|e| refuse(e.to_string()))?;
            eprintln!(
                "{} cells, {} controls, outcome {:?}",
                report.cells.len(),
                report.controls.len(),
                report.outcome
            );
            Ok(())
        }
        other => Err(refuse(format!("unknown command {other}"))),
    }
}

fn main() -> ExitCode {
    let mut tokens = std::env::args().skip(1).collect::<Vec<_>>().into_iter();
    let Some(name) = tokens.next() else {
        eprintln!("usage: joint-gate-study fingerprint|plan|seal|smoke|frozen|report ...");
        return ExitCode::from(2);
    };
    match parse(tokens).and_then(|args| command(&name, &args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("refused: {error}");
            ExitCode::FAILURE
        }
    }
}
