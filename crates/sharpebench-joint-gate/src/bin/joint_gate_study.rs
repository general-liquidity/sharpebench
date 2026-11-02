//! The joint-gate study producer.
//!
//! ```text
//! joint-gate-study fingerprint
//! joint-gate-study plan   --campaign FILE
//! joint-gate-study seal   --campaign FILE --protocol FILE --protocol FILE --out FILE
//! joint-gate-study smoke  --campaign FILE --out DIR --replicates N [--workers W]
//! joint-gate-study frozen --campaign FILE --out DIR --protocol FILE [--protocol FILE] [--workers W]
//! joint-gate-study report --campaign FILE --out DIR
//! joint-gate-study prevalence --campaign FILE --out DIR --power-cell I --null-cell J --eligible E --tracks T
//! ```
//!
//! `smoke` runs reduced counts on seeds disjoint from the frozen ones. `frozen`
//! refuses unless every named protocol validates, its budget is approved, and it
//! binds this campaign and the linked engine. Both resume: rerun the same
//! command after an interruption. `prevalence` reads an observed eligible count
//! (E of T tracks admitted) against the sensitivity of power cell I and the
//! false-eligibility rate of null cell J and prints the Rogan-Gladen corrected
//! share of skilled tracks as JSON; it writes nothing. Exit status is nonzero on
//! any refusal.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sharpebench_joint_gate::campaign::Campaign;
use sharpebench_joint_gate::gate::{authorize_frozen, protocol_digest, run_identity};
use sharpebench_joint_gate::prevalence::corrected_prevalence;
use sharpebench_joint_gate::report::{build, Report};
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
    power_cell: Option<usize>,
    null_cell: Option<usize>,
    eligible: Option<u64>,
    tracks: Option<u64>,
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
            "--power-cell" => {
                let v = value()?;
                args.power_cell = Some(v.parse().map_err(|_| refuse(format!("bad cell {v}")))?);
            }
            "--null-cell" => {
                let v = value()?;
                args.null_cell = Some(v.parse().map_err(|_| refuse(format!("bad cell {v}")))?);
            }
            "--eligible" => {
                let v = value()?;
                args.eligible = Some(v.parse().map_err(|_| refuse(format!("bad count {v}")))?);
            }
            "--tracks" => {
                let v = value()?;
                args.tracks = Some(v.parse().map_err(|_| refuse(format!("bad count {v}")))?);
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

/// The report of the run in `--out`, rebuilt from its shards.
fn load_report(args: &Args, campaign: &Campaign) -> Result<Report, RunError> {
    let out = output_dir(args)?;
    let text =
        fs::read_to_string(out.join("run-identity.json")).map_err(|e| refuse(e.to_string()))?;
    let identity: RunIdentity = serde_json::from_str(&text).map_err(|e| refuse(e.to_string()))?;
    if identity.campaign_digest != campaign.digest() {
        return Err(refuse("the run in --out used another campaign"));
    }
    let (rows, controls) = load_rows(campaign, &identity, &out)?;
    Ok(build(campaign, identity.tier, &rows, &controls))
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
            let report = load_report(args, &campaign)?;
            let out = output_dir(args)?;
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
        "prevalence" => {
            let campaign = load_campaign(args)?;
            let report = load_report(args, &campaign)?;
            let need =
                |v: Option<u64>, flag: &str| v.ok_or_else(|| refuse(format!("{flag} is required")));
            let power = args
                .power_cell
                .ok_or_else(|| refuse("--power-cell is required"))?;
            let null = args
                .null_cell
                .ok_or_else(|| refuse("--null-cell is required"))?;
            let reading = corrected_prevalence(
                &campaign,
                &report,
                power,
                null,
                need(args.eligible, "--eligible")?,
                need(args.tracks, "--tracks")?,
            )
            .map_err(refuse)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&reading).expect("a reading serializes")
            );
            Ok(())
        }
        other => Err(refuse(format!("unknown command {other}"))),
    }
}

fn main() -> ExitCode {
    let mut tokens = std::env::args().skip(1).collect::<Vec<_>>().into_iter();
    let Some(name) = tokens.next() else {
        eprintln!(
            "usage: joint-gate-study fingerprint|plan|seal|smoke|frozen|report|prevalence ..."
        );
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
