use std::fmt::Write as _;
use std::fs;

use sharpebench_core::{
    analyze_forecast_quality, parse_forecast_evidence, ContractDigestVersion,
    ForecastAnalysisConfig, ForecastQualityReport,
};

pub(crate) fn run(args: &[String], json: bool) -> i32 {
    let paths = positional_paths(args);
    if paths.is_empty() {
        eprintln!(
            "usage: sharpebench forecast-quality <evidence.json>... \
             [--bootstrap-samples N] [--seed N] [--confidence C] [--alpha A] [--bins N] \
             [--output report.json] [--json]"
        );
        return 2;
    }
    let config = match parse_config(args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    let output = match output_path(args) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    let mut documents = Vec::with_capacity(paths.len());
    for path in paths {
        let payload = match fs::read_to_string(path) {
            Ok(payload) => payload,
            Err(error) => {
                eprintln!("error: cannot read {path}: {error}");
                return 1;
            }
        };
        match parse_forecast_evidence(&payload) {
            Ok(document) => documents.push(document),
            Err(error) => {
                eprintln!("error: {path}: {error}");
                return 1;
            }
        }
    }
    match analyze_forecast_quality(&documents, config) {
        Ok(report) => {
            let serialized = serde_json::to_string_pretty(&report)
                .expect("forecast-quality report is JSON serializable");
            if let Some(path) = output {
                if let Err(error) = fs::write(path, format!("{serialized}\n")) {
                    eprintln!("error: cannot write {path}: {error}");
                    return 1;
                }
            }
            if json {
                println!("{serialized}");
            } else {
                print!("{}", render_report(&report));
            }
            0
        }
        Err(error) => {
            eprintln!("forecast-quality analysis failed: {error}");
            1
        }
    }
}

fn positional_paths(args: &[String]) -> Vec<&str> {
    let value_flags = [
        "--bootstrap-samples",
        "--seed",
        "--confidence",
        "--alpha",
        "--bins",
        "--output",
    ];
    let mut paths = Vec::new();
    let mut index = 2;
    while index < args.len() {
        if value_flags.contains(&args[index].as_str()) {
            index += 2;
        } else if args[index].starts_with('-') {
            index += 1;
        } else {
            paths.push(args[index].as_str());
            index += 1;
        }
    }
    paths
}

fn output_path(args: &[String]) -> Result<Option<&str>, &'static str> {
    let Some(index) = args.iter().position(|value| value == "--output") else {
        return Ok(None);
    };
    match args.get(index + 1).map(String::as_str) {
        Some(path) if !path.starts_with('-') => Ok(Some(path)),
        _ => Err("--output requires a file path"),
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|value| value == flag)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

fn parse_config(args: &[String]) -> Result<ForecastAnalysisConfig, String> {
    let mut config = ForecastAnalysisConfig::default();
    if let Some(value) = flag_value(args, "--bootstrap-samples") {
        config.bootstrap_samples = value
            .parse()
            .map_err(|_| "--bootstrap-samples must be a positive integer")?;
    }
    if let Some(value) = flag_value(args, "--seed") {
        config.bootstrap_seed = value.parse().map_err(|_| "--seed must be an integer")?;
    }
    if let Some(value) = flag_value(args, "--confidence") {
        config.confidence = value
            .parse()
            .map_err(|_| "--confidence must be a number inside (0, 1)")?;
    }
    if let Some(value) = flag_value(args, "--alpha") {
        config.familywise_alpha = value
            .parse()
            .map_err(|_| "--alpha must be a number inside (0, 1)")?;
    }
    if let Some(value) = flag_value(args, "--bins") {
        config.calibration_bins = value
            .parse()
            .map_err(|_| "--bins must be a positive integer")?;
    }
    if config.bootstrap_samples == 0
        || config.calibration_bins == 0
        || !(0.0..1.0).contains(&config.confidence)
        || !(0.0..1.0).contains(&config.familywise_alpha)
    {
        return Err("forecast-quality options are outside their valid ranges".to_string());
    }
    Ok(config)
}

fn render_report(report: &ForecastQualityReport) -> String {
    let mut out = String::new();
    write_report(&mut out, report).expect("writing to a String cannot fail");
    out
}

fn write_report(out: &mut String, report: &ForecastQualityReport) -> std::fmt::Result {
    let support = &report.common_support;
    writeln!(
        out,
        "FORECAST QUALITY (reported only; never changes trading rank)"
    )?;
    writeln!(
        out,
        "field support: {} contract digest(s) resolved by at least one agent; dependence unit: {}",
        support.n_contracts, report.dependence_unit
    )?;
    writeln!(out, "pairing: {}", support.rule)?;
    let legacy_digests = report
        .contract_digest_versions
        .values()
        .filter(|version| **version == ContractDigestVersion::Legacy)
        .count();
    writeln!(
        out,
        "contract digests: {} under {}, {} legacy",
        report.contract_digest_versions.len() - legacy_digests,
        ContractDigestVersion::CanonicalJsonV1.as_str(),
        legacy_digests
    )?;
    for agent in &report.agents {
        writeln!(
            out,
            "
{}: {}/{} resolved ({:.1}%), {} blind, {} consensus-exposed",
            agent.agent_id,
            agent.n_resolved,
            agent.n_claims,
            100.0 * agent.resolution_rate,
            agent.blind_resolved,
            agent.consensus_exposed_resolved
        )?;
        if let Some(gap) = support
            .unresolved_by_agent
            .get(&agent.agent_id)
            .filter(|gap| gap.n_unresolved > 0)
        {
            writeln!(
                out,
                "  unresolved field support: {} ({} pending, {} cancelled, {} rejected, {} not claimed)",
                gap.n_unresolved,
                gap.pending.len(),
                gap.cancelled.len(),
                gap.rejected.len(),
                gap.not_claimed.len()
            )?;
        }
        for metric in &agent.metrics {
            writeln!(
                out,
                "  {:<20} mean loss {:>10.6}  n={}",
                metric.scoring_rule, metric.mean_loss, metric.n
            )?;
        }
        if let Some(calibration) = &agent.binary_calibration {
            writeln!(
                out,
                "  binary calibration   Brier {:>10.6}  skill {}",
                calibration.brier,
                calibration
                    .brier_skill
                    .map(|value| format!("{value:.6}"))
                    .unwrap_or_else(|| "undefined (constant outcomes)".to_string())
            )?;
        }
    }
    if !support.settlement_status_disagreements.is_empty() {
        writeln!(
            out,
            "
settlement status disagreements (resolved by one agent, not by another):"
        )?;
        for record in &support.settlement_status_disagreements {
            writeln!(
                out,
                "  {}  resolved by [{}]  pending for [{}]  cancelled for [{}]",
                record.contract_sha256,
                record.resolved_by.join(", "),
                record.pending_by.join(", "),
                record.cancelled_by.join(", ")
            )?;
        }
    }
    if !report.comparisons.is_empty() {
        writeln!(
            out,
            "
exact-pair-support comparisons (loss A minus loss B):"
        )?;
        for comparison in &report.comparisons {
            // A withheld comparison prints why, not a blank where a number
            // should be. The interval and the p-value are unavailable together
            // whenever the two agents resolved different contracts or the block
            // resampling law cannot resolve the level being claimed, so the
            // operator sees the reason rather than inferring a failure from
            // missing output.
            match (
                comparison.confidence_lower,
                comparison.confidence_upper,
                comparison.holm_adjusted_p_value,
            ) {
                (Some(low), Some(high), Some(p)) => writeln!(
                    out,
                    "  {} vs {}  diff={:.6}  CI=[{low:.6}, {high:.6}]  Holm p={p:.6}{}",
                    comparison.agent_a,
                    comparison.agent_b,
                    comparison.mean_loss_difference,
                    if comparison.familywise_significant {
                        "  significant"
                    } else {
                        ""
                    }
                )?,
                _ => writeln!(
                    out,
                    "  {} vs {}  diff={:.6} over {} contract(s)  inference withheld: {}",
                    comparison.agent_a,
                    comparison.agent_b,
                    comparison.mean_loss_difference,
                    comparison.n_contracts,
                    comparison
                        .inference_error
                        .as_deref()
                        .unwrap_or("insufficient settlement support")
                )?,
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn paths_skip_every_option_value() {
        let args = args(&[
            "sharpebench",
            "forecast-quality",
            "a.json",
            "--seed",
            "7",
            "b.json",
            "--bins",
            "5",
            "--output",
            "report.json",
        ]);
        assert_eq!(positional_paths(&args), ["a.json", "b.json"]);
    }

    #[test]
    fn output_path_is_explicit_and_requires_a_value() {
        let with_output = args(&[
            "sharpebench",
            "forecast-quality",
            "a.json",
            "--output",
            "report.json",
        ]);
        assert_eq!(output_path(&with_output), Ok(Some("report.json")));

        let missing = args(&["sharpebench", "forecast-quality", "a.json", "--output"]);
        assert_eq!(output_path(&missing), Err("--output requires a file path"));
    }

    #[test]
    fn output_file_contains_the_complete_machine_report() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/forecast-quality/fixtures");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos();
        let output = std::env::temp_dir().join(format!(
            "sharpebench-forecast-report-{}-{nonce}.json",
            std::process::id()
        ));
        let args = vec![
            "sharpebench".to_string(),
            "forecast-quality".to_string(),
            fixtures
                .join("agent-alpha.json")
                .to_string_lossy()
                .into_owned(),
            fixtures
                .join("agent-beta.json")
                .to_string_lossy()
                .into_owned(),
            "--bootstrap-samples".to_string(),
            "20".to_string(),
            "--output".to_string(),
            output.to_string_lossy().into_owned(),
        ];

        assert_eq!(run(&args, false), 0);
        let report: serde_json::Value = serde_json::from_slice(
            &fs::read(&output).expect("forecast report was written to --output"),
        )
        .expect("--output contains JSON");
        fs::remove_file(&output).expect("remove temporary forecast report");

        assert_eq!(report["schema_version"], "sharpebench.forecast-quality.v2");
        assert_eq!(report["common_support"]["n_contracts"], 12);
        assert_eq!(
            report["common_support"]["unresolved_by_agent"]["agent-beta"]["n_unresolved"],
            0
        );
        assert_eq!(report["agents"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn human_report_charges_a_pending_settlement_to_the_agent_that_left_it() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/forecast-quality/fixtures");
        let load = |name: &str| {
            parse_forecast_evidence(
                &fs::read_to_string(fixtures.join(name)).expect("tutorial fixture is readable"),
            )
            .expect("tutorial fixture is valid evidence")
        };
        let alpha = load("agent-alpha.json");
        let mut beta = load("agent-beta.json");
        let resolution = beta
            .resolutions
            .iter_mut()
            .find(|resolution| resolution.status == "resolved")
            .expect("the tutorial resolves every claim");
        resolution.status = "pending".to_string();
        resolution.outcome = None;
        resolution.available_at = None;

        let report = analyze_forecast_quality(&[alpha, beta], ForecastAnalysisConfig::default())
            .expect("a pending settlement is disclosed, not refused");
        let rendered = render_report(&report);

        assert!(rendered.contains("field support: 12 contract digest(s)"));
        assert!(rendered.contains(
            "unresolved field support: 1 (1 pending, 0 cancelled, 0 rejected, 0 not claimed)"
        ));
        assert_eq!(rendered.matches("unresolved field support").count(), 1);
        assert!(rendered.contains("resolved by [agent-alpha]  pending for [agent-beta]"));
        assert!(rendered.contains(
            "agent-alpha vs agent-beta  diff=-0.141809 over 11 contract(s)  inference withheld: \
             unequal resolved support: agent_a did not resolve 0 contract(s) that agent_b \
             resolved and agent_b did not resolve 1 that agent_a resolved"
        ));
    }

    #[test]
    fn invalid_resampling_configuration_is_refused() {
        let args = args(&[
            "sharpebench",
            "forecast-quality",
            "a.json",
            "--bootstrap-samples",
            "0",
        ]);
        assert!(parse_config(&args).is_err());
    }
}
