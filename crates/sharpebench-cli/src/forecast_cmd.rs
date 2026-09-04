use std::fs;

use sharpebench_core::{
    analyze_forecast_quality, parse_forecast_evidence, ForecastAnalysisConfig,
    ForecastQualityReport,
};

pub(crate) fn run(args: &[String], json: bool) -> i32 {
    let paths = positional_paths(args);
    if paths.is_empty() {
        eprintln!(
            "usage: sharpebench forecast-quality <evidence.json>... \
             [--bootstrap-samples N] [--seed N] [--confidence C] [--alpha A] [--bins N] [--json]"
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
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .expect("forecast-quality report is JSON serializable")
                );
            } else {
                print_report(&report);
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

fn print_report(report: &ForecastQualityReport) {
    println!("FORECAST QUALITY (reported only; never changes trading rank)");
    println!(
        "common support: {} exact contract(s); dependence unit: {}",
        report.common_support.n_contracts, report.dependence_unit
    );
    for agent in &report.agents {
        println!(
            "\n{}: {}/{} resolved ({:.1}%), {} blind, {} consensus-exposed",
            agent.agent_id,
            agent.n_resolved,
            agent.n_claims,
            100.0 * agent.resolution_rate,
            agent.blind_resolved,
            agent.consensus_exposed_resolved
        );
        for metric in &agent.metrics {
            println!(
                "  {:<20} mean loss {:>10.6}  n={}",
                metric.scoring_rule, metric.mean_loss, metric.n
            );
        }
        if let Some(calibration) = &agent.binary_calibration {
            println!(
                "  binary calibration   Brier {:>10.6}  skill {}",
                calibration.brier,
                calibration
                    .brier_skill
                    .map(|value| format!("{value:.6}"))
                    .unwrap_or_else(|| "undefined (constant outcomes)".to_string())
            );
        }
    }
    if !report.comparisons.is_empty() {
        println!("\nexact-common-support comparisons (loss A minus loss B):");
        for comparison in &report.comparisons {
            println!(
                "  {} vs {}  diff={:.6}  CI=[{:.6}, {:.6}]  Holm p={:.6}{}",
                comparison.agent_a,
                comparison.agent_b,
                comparison.mean_loss_difference,
                comparison.confidence_lower,
                comparison.confidence_upper,
                comparison.holm_adjusted_p_value,
                if comparison.familywise_significant {
                    "  significant"
                } else {
                    ""
                }
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ]);
        assert_eq!(positional_paths(&args), ["a.json", "b.json"]);
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
