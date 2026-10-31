//! Size of the paired forecast test, measured through the shipped analyzer.
//!
//! `MEASURED_SIZE` in `sharpebench-core` and the rejection rates the paper prints
//! had no committed producer. This one drives the public
//! `analyze_forecast_quality` over synthetic forecast documents, so the rejection
//! decision is the report's own `familywise_significant` and not a second copy of
//! the bootstrap. Nothing here reads market data, calls a model or touches a clock.
//!
//! Each replication builds two agents on five contracts a block, with the
//! per-contract loss difference drawn from the case's law. The losses are interval
//! scores: a contract resolves inside `[0, w]`, so the score is exactly `w`, and the
//! agent with the larger loss holds `w = |difference|` while the other holds a
//! degenerate `[0, 0]`. The differences therefore reach the analyzer exact to the
//! bit, which a squared error would not give.
//!
//! The primary arm runs the shipped `ForecastAnalysisConfig` unchanged, which means
//! one fixed bootstrap seed and so one fixed resampling pattern across replications.
//! The fresh-seed arm redraws that seed each replication, on the same data, to show
//! whether the fixed pattern matters. Two alternatives the constants' documentation
//! compares against have no shipped code path, so they are computed here and labelled
//! `reconstructed`: their definitions are this file's, not the original study's, which
//! left none behind.
//!
//! Randomness is a seeded stream per (case, replication), so the output does not
//! depend on the thread count.
//!
//!   cargo run --release -p sharpebench-harness --example forecast_size_study -- \
//!     paper/evidence/forecast-size-study.json

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::thread;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sharpebench_core::forecast::{
    ForecastContract, ForecastEvidence, ForecastProducer, ForecastResolution, ForecastRevision,
    ForecastRunIdentity, InformationExposure, FORECAST_EVIDENCE_SCHEMA,
};
use sharpebench_core::{analyze_forecast_quality, ForecastAnalysisConfig};
use sharpebench_protocol::canonical::versioned_preimage;

const MASTER_SEED: u64 = 0x51ED_2026_F0EC_A57A;
const REPLICATIONS: usize = 6_000;
const CONTRACTS_PER_BLOCK: usize = 5;
const NOMINAL: f64 = 0.05;
/// Upper 2.5% point of Student-t on three degrees of freedom.
const T_975_DF3: f64 = 3.182_446_305_284_263;

/// The hard-coded values under measurement, restated so the output sits beside
/// them. `MEASURED_SIZE` is private to `sharpebench-core`, so a test below holds this
/// list to the source text and to `FAMILYWISE_SIZE_RULE`.
const STATED: &[(&str, Law, usize, f64)] = &[
    ("normal", Law::Normal, 4, 0.188),
    ("normal", Law::Normal, 5, 0.162),
    ("normal", Law::Normal, 6, 0.138),
    ("normal", Law::Normal, 10, 0.096),
    ("normal", Law::Normal, 20, 0.072),
    ("normal", Law::Normal, 40, 0.060),
    ("lognormal", Law::Lognormal, 4, 0.237),
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Law {
    /// Standard normal, shifted by the case's effect.
    Normal,
    /// `exp(Z) - exp(1/2)`: mean zero, unit log-scale, right-skewed.
    Lognormal,
}

#[derive(Clone, Copy)]
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, upper: usize) -> usize {
        ((self.next() as u128 * upper as u128) >> 64) as usize
    }

    /// Marsaglia polar: rejection, `sqrt` and `ln`, with no trigonometry whose
    /// bits would vary between platforms' libm.
    fn normal(&mut self) -> f64 {
        loop {
            let x = 2.0 * self.uniform() - 1.0;
            let y = 2.0 * self.uniform() - 1.0;
            let s = x * x + y * y;
            if s > 0.0 && s < 1.0 {
                return x * (-2.0 * s.ln() / s).sqrt();
            }
        }
    }
}

fn stream(case: usize, replication: usize) -> SplitMix {
    let mut mixer = SplitMix(MASTER_SEED ^ ((case as u64) << 40) ^ replication as u64);
    SplitMix(mixer.next())
}

fn draw(law: Law, effect: f64, rng: &mut SplitMix) -> f64 {
    match law {
        Law::Normal => rng.normal() + effect,
        Law::Lognormal => rng.normal().exp() - 0.5_f64.exp() + effect,
    }
}

fn contract(index: usize, block: usize) -> ForecastContract {
    ForecastContract {
        schema_version: "sharpearena.forecast-contract.v1".to_string(),
        contract_id: format!("c{index}"),
        question: format!("size study contract {index}"),
        instrument: "SYNTH".to_string(),
        target: "synthetic_loss_difference".to_string(),
        kind: "interval".to_string(),
        opens_at: 0,
        deadline: 1,
        resolves_at: (block as u64 + 1) * 10,
        observation_source: "synthetic:size-study".to_string(),
        open_definition: "synthetic".to_string(),
        close_definition: "synthetic".to_string(),
        unit: "synthetic".to_string(),
        scoring_rule: "interval_score".to_string(),
        neutral_threshold: 0.0,
        boundary_ownership: "not applicable".to_string(),
        missing_data_policy: "cancel".to_string(),
        fallback_policy: "cancel".to_string(),
        categories: vec![],
        interval_alpha: Some(0.5),
    }
}

/// One agent's document for `blocks` settlement blocks, widths zero.
fn document(agent: &str, blocks: usize) -> ForecastEvidence {
    let contracts: Vec<_> = (0..blocks * CONTRACTS_PER_BLOCK)
        .map(|index| contract(index, index / CONTRACTS_PER_BLOCK))
        .collect();
    let hashes: Vec<String> = contracts
        .iter()
        .map(|contract| {
            let value = serde_json::to_value(contract).expect("contract serializes");
            let framed = versioned_preimage(&value).expect("contract frames");
            Sha256::digest(&framed)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        })
        .collect();
    ForecastEvidence {
        schema_version: FORECAST_EVIDENCE_SCHEMA.to_string(),
        producer: ForecastProducer {
            name: "sharpearena".to_string(),
            contract: "native".to_string(),
        },
        generated_at: 0,
        identity: ForecastRunIdentity {
            agent_id: agent.to_string(),
            model_id: "synthetic".to_string(),
            model_sha256: "a".repeat(64),
            scaffold_id: "synthetic".to_string(),
            scaffold_sha256: "b".repeat(64),
            prompt_sha256: "c".repeat(64),
            operator_id: "synthetic".to_string(),
            config_sha256: "d".repeat(64),
        },
        revisions: hashes
            .iter()
            .enumerate()
            .map(|(index, hash)| ForecastRevision {
                revision_id: format!("claim-{index}:r0"),
                claim_id: format!("claim-{index}"),
                ordinal: 0,
                supersedes: None,
                contract_sha256: hash.clone(),
                contract_digest_encoding: None,
                prediction: vec![0.0, 0.0],
                confidence: 0.5,
                rationale: "synthetic".to_string(),
                submitted_at: 0,
                status: "eligible".to_string(),
                reason: None,
                trigger_event_id: None,
                revision_reason: None,
                exposure: InformationExposure {
                    observed_at: 0,
                    market_snapshot_sha256: None,
                    consensus_visible: false,
                    consensus_snapshot_sha256: None,
                    source_ids: vec![],
                },
                idempotency_key: format!("request-{index}"),
            })
            .collect(),
        resolutions: contracts
            .iter()
            .enumerate()
            .map(|(index, contract)| ForecastResolution {
                claim_id: format!("claim-{index}"),
                status: "resolved".to_string(),
                outcome: Some(Value::from(0.0)),
                available_at: Some(contract.resolves_at),
                recorded_at: contract.resolves_at,
                reason: None,
            })
            .collect(),
        contracts,
    }
}

/// Install `differences` (loss of `a` minus loss of `b`) as interval widths.
fn install(a: &mut ForecastEvidence, b: &mut ForecastEvidence, differences: &[f64]) {
    for (index, difference) in differences.iter().enumerate() {
        a.revisions[index].prediction[1] = difference.max(0.0);
        b.revisions[index].prediction[1] = (-difference).max(0.0);
    }
}

#[derive(Clone, Copy, Default)]
struct Tally {
    rejected: u64,
    rejected_fresh_seed: u64,
    covered: u64,
    rejected_student_t: u64,
    rejected_studentised: u64,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.rejected += other.rejected;
        self.rejected_fresh_seed += other.rejected_fresh_seed;
        self.covered += other.covered;
        self.rejected_student_t += other.rejected_student_t;
        self.rejected_studentised += other.rejected_studentised;
    }
}

fn analyze(
    a: &ForecastEvidence,
    b: &ForecastEvidence,
    config: ForecastAnalysisConfig,
) -> sharpebench_core::forecast::PairwiseForecastComparison {
    let report = analyze_forecast_quality(&[a.clone(), b.clone()], config)
        .expect("synthetic documents analyze");
    let mut comparisons = report.comparisons;
    assert_eq!(comparisons.len(), 1, "one pair on one stratum");
    let comparison = comparisons.remove(0);
    assert!(
        comparison.inference_error.is_none(),
        "{:?}",
        comparison.inference_error
    );
    comparison
}

fn block_means(differences: &[f64]) -> Vec<f64> {
    differences
        .chunks(CONTRACTS_PER_BLOCK)
        .map(|block| block.iter().sum::<f64>() / block.len() as f64)
        .collect()
}

fn t_statistic(means: &[f64], centre: f64) -> Option<f64> {
    let n = means.len() as f64;
    let mean = means.iter().sum::<f64>() / n;
    let variance = means.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (variance > 0.0).then(|| (mean - centre) / (variance / n).sqrt())
}

/// Reconstructed bootstrap-t on block means with the shipped resampler and
/// replication count. A resample that repeats one block has no spread and no
/// statistic, so it counts toward the denominator and never as extreme.
fn studentised_rejects(means: &[f64], samples: usize, rng: &mut SplitMix) -> bool {
    let observed_mean = means.iter().sum::<f64>() / means.len() as f64;
    let Some(observed) = t_statistic(means, 0.0) else {
        return false;
    };
    let mut extreme = 0usize;
    let mut resample = vec![0.0; means.len()];
    for _ in 0..samples {
        for slot in resample.iter_mut() {
            *slot = means[rng.below(means.len())];
        }
        if let Some(statistic) = t_statistic(&resample, observed_mean) {
            if statistic.abs() >= observed.abs() {
                extreme += 1;
            }
        }
    }
    (extreme as f64 + 1.0) / (samples as f64 + 1.0) <= NOMINAL
}

fn run_shard(
    case: usize,
    law: Law,
    effect: f64,
    blocks: usize,
    replications: std::ops::Range<usize>,
) -> Tally {
    let mut a = document("agent_a", blocks);
    let mut b = document("agent_b", blocks);
    let shipped = ForecastAnalysisConfig::default();
    let mut tally = Tally::default();
    let mut differences = vec![0.0; blocks * CONTRACTS_PER_BLOCK];
    for replication in replications {
        let mut rng = stream(case, replication);
        for difference in differences.iter_mut() {
            *difference = draw(law, effect, &mut rng);
        }
        install(&mut a, &mut b, &differences);

        let primary = analyze(&a, &b, shipped);
        tally.rejected += u64::from(primary.familywise_significant);
        let (lower, upper) = (
            primary.confidence_lower.expect("admitted comparison"),
            primary.confidence_upper.expect("admitted comparison"),
        );
        tally.covered += u64::from(lower <= effect && effect <= upper);

        let fresh = analyze(
            &a,
            &b,
            ForecastAnalysisConfig {
                bootstrap_seed: rng.next(),
                ..shipped
            },
        );
        tally.rejected_fresh_seed += u64::from(fresh.familywise_significant);

        if blocks == 4 {
            let means = block_means(&differences);
            if let Some(statistic) = t_statistic(&means, 0.0) {
                tally.rejected_student_t += u64::from(statistic.abs() >= T_975_DF3);
            }
            tally.rejected_studentised += u64::from(studentised_rejects(
                &means,
                shipped.bootstrap_samples,
                &mut rng,
            ));
        }
    }
    tally
}

fn run_case(
    case: usize,
    law: Law,
    effect: f64,
    blocks: usize,
    replications: usize,
    threads: usize,
) -> Tally {
    let threads = threads.clamp(1, replications.max(1));
    let shards: Vec<_> = (0..threads)
        .map(|shard| (replications * shard / threads)..(replications * (shard + 1) / threads))
        .collect();
    let mut total = Tally::default();
    thread::scope(|scope| {
        let handles: Vec<_> = shards
            .into_iter()
            .map(|range| scope.spawn(move || run_shard(case, law, effect, blocks, range)))
            .collect();
        for handle in handles {
            total.add(&handle.join().expect("shard finished"));
        }
    });
    total
}

/// Wilson 95% interval on a binomial rate.
fn wilson(hits: u64, n: usize) -> (f64, f64) {
    let (p, n, z) = (hits as f64 / n as f64, n as f64, 1.959_963_984_540_054);
    let denominator = 1.0 + z * z / n;
    let centre = (p + z * z / (2.0 * n)) / denominator;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denominator;
    (centre - half, centre + half)
}

#[derive(Serialize)]
struct Rate {
    hits: u64,
    rate: f64,
    wilson_lower: f64,
    wilson_upper: f64,
}

fn rate(hits: u64, replications: usize) -> Rate {
    let (wilson_lower, wilson_upper) = wilson(hits, replications);
    Rate {
        hits,
        rate: hits as f64 / replications as f64,
        wilson_lower,
        wilson_upper,
    }
}

#[derive(Serialize)]
struct SizeRow {
    law: &'static str,
    blocks: usize,
    contracts: usize,
    stated: f64,
    measured: Rate,
    measured_fresh_seed: Rate,
    difference_from_stated: f64,
    stated_inside_wilson_95: bool,
    percentile_interval_coverage: Rate,
}

#[derive(Serialize)]
struct Comparator {
    name: &'static str,
    code_path: &'static str,
    law: &'static str,
    effect: f64,
    blocks: usize,
    stated: Option<f64>,
    measured: Rate,
}

#[derive(Serialize)]
struct Study {
    schema: &'static str,
    purpose: &'static str,
    master_seed: u64,
    replications: usize,
    contracts_per_block: usize,
    bootstrap_samples: usize,
    bootstrap_seed: u64,
    nominal: f64,
    rejection_rule: &'static str,
    lognormal_law: &'static str,
    float_note: &'static str,
    size: Vec<SizeRow>,
    comparators: Vec<Comparator>,
}

fn study(replications: usize, threads: usize) -> Study {
    let mut size = Vec::new();
    let mut lognormal_four = Tally::default();
    let mut normal_four = Tally::default();
    for (case, (name, law, blocks, stated)) in STATED.iter().enumerate() {
        let tally = run_case(case, *law, 0.0, *blocks, replications, threads);
        if *blocks == 4 {
            match law {
                Law::Normal => normal_four = tally,
                Law::Lognormal => lognormal_four = tally,
            }
        }
        let measured = rate(tally.rejected, replications);
        let inside = measured.wilson_lower <= *stated && *stated <= measured.wilson_upper;
        size.push(SizeRow {
            law: name,
            blocks: *blocks,
            contracts: blocks * CONTRACTS_PER_BLOCK,
            stated: *stated,
            difference_from_stated: measured.rate - stated,
            stated_inside_wilson_95: inside,
            measured,
            measured_fresh_seed: rate(tally.rejected_fresh_seed, replications),
            percentile_interval_coverage: rate(tally.covered, replications),
        });
    }

    // The half-sigma power pair: one effect case, four blocks.
    let power = run_case(STATED.len(), Law::Normal, 0.5, 4, replications, threads);
    let comparators = vec![
        Comparator {
            name: "shipped test at a half-sigma effect",
            code_path: "shipped analyzer",
            law: "normal",
            effect: 0.5,
            blocks: 4,
            stated: Some(0.745),
            measured: rate(power.rejected, replications),
        },
        Comparator {
            name: "studentised block bootstrap, size",
            code_path: "reconstructed here; no shipped code path",
            law: "normal",
            effect: 0.0,
            blocks: 4,
            stated: Some(0.013),
            measured: rate(normal_four.rejected_studentised, replications),
        },
        Comparator {
            name: "studentised block bootstrap at a half-sigma effect",
            code_path: "reconstructed here; no shipped code path",
            law: "normal",
            effect: 0.5,
            blocks: 4,
            stated: Some(0.121),
            measured: rate(power.rejected_studentised, replications),
        },
        Comparator {
            name: "Student-t on block means, size",
            code_path: "reconstructed here; no shipped code path",
            law: "normal",
            effect: 0.0,
            blocks: 4,
            stated: Some(0.050),
            measured: rate(normal_four.rejected_student_t, replications),
        },
        Comparator {
            name: "Student-t on block means, size",
            code_path: "reconstructed here; no shipped code path",
            law: "lognormal",
            effect: 0.0,
            blocks: 4,
            stated: Some(0.094),
            measured: rate(lognormal_four.rejected_student_t, replications),
        },
    ];

    let shipped = ForecastAnalysisConfig::default();
    Study {
        schema: "sharpebench.forecast-size-study.v1",
        purpose: "seeded producer for the rejection rates MEASURED_SIZE and \
                  FAMILYWISE_SIZE_RULE state; the constants are not edited by this run",
        master_seed: MASTER_SEED,
        replications,
        contracts_per_block: CONTRACTS_PER_BLOCK,
        bootstrap_samples: shipped.bootstrap_samples,
        bootstrap_seed: shipped.bootstrap_seed,
        nominal: NOMINAL,
        rejection_rule: "familywise_significant of a one-pair analyze_forecast_quality report, \
                         which with a family of one is raw_p_value <= familywise_alpha",
        lognormal_law: "exp(Z) - exp(1/2) with Z standard normal; the original study's \
                        parameters were not recorded, so this law is this producer's choice",
        float_note: "draws use sqrt and ln only; the JSON is byte-stable on one platform \
                     and the counts are integers",
        size,
        comparators,
    }
}

fn main() {
    let mut args = env::args().skip(1);
    let out = args
        .next()
        .expect("usage: forecast_size_study <out.json> [replications]");
    let replications = args
        .next()
        .map_or(REPLICATIONS, |value| value.parse().expect("replications"));
    let threads = thread::available_parallelism().map_or(1, usize::from);
    let result = study(replications, threads);
    let text = serde_json::to_string_pretty(&result).expect("study serializes") + "\n";
    fs::write(&out, text).expect("write output");
    let mut table: BTreeMap<String, String> = BTreeMap::new();
    for row in &result.size {
        table.insert(
            format!("{} {:02}", row.law, row.blocks),
            format!(
                "stated {:.3}  measured {:.4} [{:.4}, {:.4}]  fresh seed {:.4}",
                row.stated,
                row.measured.rate,
                row.measured.wilson_lower,
                row.measured.wilson_upper,
                row.measured_fresh_seed.rate
            ),
        );
    }
    for (key, value) in table {
        println!("{key}  {value}");
    }
    for comparator in &result.comparators {
        println!(
            "{} ({}): stated {:?} measured {:.4}",
            comparator.name, comparator.law, comparator.stated, comparator.measured.rate
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_core::parse_forecast_evidence;

    #[test]
    fn the_stated_values_are_the_ones_the_source_states() {
        let source = include_str!("../../sharpebench-core/src/forecast.rs");
        let (rule, table) = source
            .split_once("const MEASURED_SIZE")
            .expect("the size table is in the source");
        for (name, _, blocks, stated) in STATED {
            let text = format!("{stated:.3}");
            assert!(rule.contains(&text), "the rule must state {text}");
            if *name == "normal" {
                let entry = format!("({blocks}, {text})");
                assert!(table.contains(&entry), "MEASURED_SIZE must hold {entry}");
            }
        }
    }

    #[test]
    fn the_template_document_passes_the_strict_parser() {
        let payload = serde_json::to_string(&document("agent_a", 4)).unwrap();
        parse_forecast_evidence(&payload).unwrap();
    }

    #[test]
    fn the_analyzer_sees_the_drawn_differences() {
        let mut a = document("agent_a", 4);
        let mut b = document("agent_b", 4);
        let mut rng = stream(0, 0);
        let differences: Vec<f64> = (0..20).map(|_| draw(Law::Normal, 0.0, &mut rng)).collect();
        install(&mut a, &mut b, &differences);
        let comparison = analyze(&a, &b, ForecastAnalysisConfig::default());
        let expected = differences.iter().sum::<f64>() / differences.len() as f64;
        // The analyzer sums in digest order, so the total can differ in its last bits.
        assert!((comparison.mean_loss_difference - expected).abs() < 1e-14);
        assert_eq!(comparison.n_settlement_blocks, 4);
        assert_eq!(comparison.n_contracts, 20);
    }

    #[test]
    fn the_counts_do_not_depend_on_the_thread_count() {
        let one = run_case(0, Law::Normal, 0.0, 4, 40, 1);
        let many = run_case(0, Law::Normal, 0.0, 4, 40, 7);
        assert_eq!(one.rejected, many.rejected);
        assert_eq!(one.rejected_fresh_seed, many.rejected_fresh_seed);
        assert_eq!(one.covered, many.covered);
        assert_eq!(one.rejected_studentised, many.rejected_studentised);
        assert_eq!(one.rejected_student_t, many.rejected_student_t);
    }

    #[test]
    fn a_large_effect_is_rejected_and_its_interval_covers_the_shift() {
        let tally = run_case(99, Law::Normal, 5.0, 10, 20, 2);
        assert_eq!(tally.rejected, 20);
        assert!(tally.covered >= 15, "{}", tally.covered);
    }

    #[test]
    fn the_student_t_critical_value_gives_nominal_size_under_normal_blocks() {
        let tally = run_case(98, Law::Normal, 0.0, 4, 2_000, 8);
        let observed = tally.rejected_student_t as f64 / 2_000.0;
        assert!((observed - 0.05).abs() < 0.02, "{observed}");
    }
}
