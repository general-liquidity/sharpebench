//! Current-engine regression characterizations over the paper's frozen input data.
//!
//! Reconstructing a field with today's engine is not replay of a historical
//! artifact. Changes here require an explicit impact assessment; they never
//! authorize rewriting the paper's frozen results. In particular, the corrected
//! inventory path changes the commodities support (see the paired historical
//! assertion below), whose raw WTI prices include a documented negative quote.
//!
//! `rank` collapses near-clone streams to one vote each before it measures
//! `trials_sr_std` (see `sharpebench_core::CLONE_COLLAPSE_COSINE`). On a field
//! with no clusters the collapse is the identity for that input to the collapse,
//! not a proof that a revised engine reproduces a historical artifact. These
//! tests reconstruct the examples' fields (same agents, seeds, windows and cost
//! model) and pin the observed clustering, separating raw and seed-averaged inputs. They
//! also record the maximum honest pair, which is the number the threshold was
//! chosen above: long-only agents on tiny universes are collinear with
//! buy-and-hold at 0.97 to 0.99, which the rediscovery screen's 0.97 would
//! have merged and the collapse's 0.995 must not.

use std::f64::consts::PI;
use std::path::Path;

use sharpebench_core::{
    clone_clusters, cosine_similarity, AgentSubmission, Run, CLONE_COLLAPSE_COSINE,
    DEFAULT_REDISCOVERY_THRESHOLD,
};
use sharpebench_harness::luck_floor;
use sharpebench_sim::agent::RiskManaged;
use sharpebench_sim::{
    run_backtest, walk_forward, Agent, BuyAndHold, CostModel, Dataset, HoldAgent, Momentum, Window,
};

/// The nine frozen datasets of `evidence_sweep` and `risk_managed_eval`.
const DATASETS: &[&str] = &[
    "us-indices-1d",
    "us-indices-1w",
    "crypto-majors-1h",
    "crypto-majors-4h",
    "crypto-majors-1d",
    "crypto-majors-1w",
    "fx-majors-1d",
    "commodities-1d",
    "rates-1d",
];
const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const LUCK_FLOOR_AGENTS: usize = 5;

/// The examples' window rule: warmup n/10 clamped to 20..60, six test windows.
fn windows_for(n: usize) -> Vec<Window> {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    walk_forward(n, warmup, test, test)
}

fn run_agent(
    id: &str,
    data: &Dataset,
    windows: &[Window],
    make: impl Fn() -> Box<dyn Agent>,
) -> AgentSubmission {
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
}

fn load(name: &str) -> Dataset {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../data/{name}.csv"));
    Dataset::from_csv_file(path.to_str().expect("utf-8 path"))
        .unwrap_or_else(|e| panic!("frozen dataset {name}: {e}"))
}

/// Largest `|cosine|` over the field's pooled streams, with the pair's ids.
fn max_pair(subs: &[AgentSubmission]) -> (f64, String, String) {
    let pooled: Vec<Vec<f64>> = subs
        .iter()
        .map(|s| {
            s.runs
                .iter()
                .flat_map(|r| r.returns.iter().copied())
                .collect()
        })
        .collect();
    let mut best = (0.0_f64, String::new(), String::new());
    for i in 0..pooled.len() {
        for j in (i + 1)..pooled.len() {
            if let Some(c) = cosine_similarity(&pooled[i], &pooled[j], false) {
                if c.abs() > best.0 {
                    best = (c.abs(), subs[i].agent_id.clone(), subs[j].agent_id.clone());
                }
            }
        }
    }
    best
}

fn merges_at(subs: &[AgentSubmission], threshold: f64) -> usize {
    let pooled: Vec<Vec<f64>> = subs
        .iter()
        .map(|s| {
            s.runs
                .iter()
                .flat_map(|r| r.returns.iter().copied())
                .collect()
        })
        .collect();
    clone_clusters(&pooled, threshold, false)
        .iter()
        .map(|c| c.len() - 1)
        .sum()
}

fn assert_no_merges(label: &str, subs: &[AgentSubmission]) -> f64 {
    let (max, a, b) = max_pair(subs);
    let merges = merges_at(subs, CLONE_COLLAPSE_COSINE);
    eprintln!(
        "{label}: {} agents, max |cos| {max:.4} ({a} vs {b}), merges at {CLONE_COLLAPSE_COSINE}: {merges}",
        subs.len()
    );
    assert_eq!(
        merges, 0,
        "{label}: the collapse merged an honest pair ({a} vs {b} at {max:.4}); the committed evidence would change"
    );
    assert!(max < CLONE_COLLAPSE_COSINE);
    max
}

/// Every `rank`-scored field behind `paper/evidence/final/*.jsonl` and the
/// risk-managed run: zero merges, so the collapse is the identity there.
#[test]
fn committed_evidence_fields_have_no_clone_merges() {
    let mut honest_max = 0.0_f64;
    let mut merges_at_rediscovery = 0usize;
    for name in DATASETS {
        let data = load(name);
        let windows = windows_for(data.len());
        let floor = luck_floor(
            &data,
            &windows,
            &EXEC_SEEDS,
            CostModel::default(),
            LUCK_FLOOR_AGENTS,
        );

        // evidence_sweep: three reference agents + the five-agent luck floor.
        let mut sweep = vec![
            run_agent("buy-and-hold", &data, &windows, || Box::new(BuyAndHold)),
            run_agent(
                "momentum",
                &data,
                &windows,
                || Box::new(Momentum::default()),
            ),
            run_agent("hold", &data, &windows, || Box::new(HoldAgent)),
        ];
        sweep.extend(floor.iter().cloned());
        honest_max = honest_max.max(assert_no_merges(&format!("{name} evidence-sweep"), &sweep));
        merges_at_rediscovery += merges_at(&sweep, DEFAULT_REDISCOVERY_THRESHOLD);

        // risk_managed_eval: the risk-managed agent + buy-and-hold + the floor.
        let mut managed = vec![
            run_agent("risk-managed", &data, &windows, || {
                Box::new(RiskManaged::new())
            }),
            run_agent("buy-and-hold", &data, &windows, || Box::new(BuyAndHold)),
        ];
        managed.extend(floor);
        honest_max = honest_max.max(assert_no_merges(&format!("{name} risk-managed"), &managed));
    }
    eprintln!("maximum honest pair across all evidence fields: {honest_max:.4}");
    // The reason the collapse has its own constant: at the rediscovery
    // screen's threshold these honest fields would lose votes.
    assert!(
        merges_at_rediscovery > 0,
        "expected honest collinear pairs above {DEFAULT_REDISCOVERY_THRESHOLD}; if the fields changed, revisit CLONE_COLLAPSE_COSINE"
    );
    assert!(honest_max < CLONE_COLLAPSE_COSINE);
}

// --- seed-averaged streams: the streams `rank` actually measures -------------

/// The raw-concatenation tests above pin the collapse on the streams as
/// submitted. The live measured path is different: `rank` clusters the
/// seed-AVERAGED pooled streams of `pooled_returns` (aligned execution
/// replicates averaged per bar) before it measures `trials_sr_std`. Averaging
/// eight independent per-seed draws shrinks seed-specific noise by roughly
/// `sqrt(8)`, so streams that are honestly dissimilar raw can merge once
/// averaged: the five luck-floor agents converge toward the same
/// market-average exposure. This test reproduces the live clustering, counts
/// post-collapse dispersion votes the way `measured_trials_sr_std` does
/// (qualifiers whose stream has a Sharpe ratio, clusters vote once, `min_field`
/// five), and asserts current-engine support. The qualification predicate is
/// `observed_sharpe_ratio`, the engine's own: `hold`, whose seed-averaged stream
/// is identically zero, no longer qualifies and no longer votes. The commodities
/// field no longer reproduces its historical measured stamp: only one current
/// stream has a Sharpe ratio. This test records the incompatibility; neither
/// successful CSV parsing nor a dispersion fallback validates percentage returns
/// across a negative raw quote.
#[test]
fn current_seed_averaged_streams_have_expected_dispersion_support() {
    use sharpebench_core::composite::pooled_returns;
    use sharpebench_core::deflated_sharpe::observed_sharpe_ratio;

    /// Current support, not a relabelling of the frozen artifact stamps.
    const CONFIGURED_FALLBACK: &[&str] = &[
        "us-indices-1w",
        "crypto-majors-1w",
        "crypto-majors-1d",
        "us-indices-1d",
        "crypto-majors-4h",
        "commodities-1d",
    ];
    const MIN_FIELD: usize = 5;

    for name in DATASETS {
        let data = load(name);
        let windows = windows_for(data.len());
        let mut sweep = vec![
            run_agent("buy-and-hold", &data, &windows, || Box::new(BuyAndHold)),
            run_agent(
                "momentum",
                &data,
                &windows,
                || Box::new(Momentum::default()),
            ),
            run_agent("hold", &data, &windows, || Box::new(HoldAgent)),
        ];
        sweep.extend(luck_floor(
            &data,
            &windows,
            &EXEC_SEEDS,
            CostModel::default(),
            LUCK_FLOOR_AGENTS,
        ));

        // Mirror `measured_trials_sr_std`: seed-averaged streams, qualifiers
        // whose stream has a Sharpe ratio, one vote per clone cluster at the
        // collapse threshold.
        let averaged: Vec<(String, Vec<f64>)> = sweep
            .iter()
            .map(|s| (s.agent_id.clone(), pooled_returns(s, EXEC_SEEDS.len())))
            .filter(|(_, p)| observed_sharpe_ratio(p).is_ok())
            .collect();
        assert!(
            !averaged.iter().any(|(id, _)| id == "hold"),
            "{name}: a track with no Sharpe ratio must not qualify to vote"
        );
        let streams: Vec<Vec<f64>> = averaged.iter().map(|(_, p)| p.clone()).collect();
        let clusters = clone_clusters(&streams, CLONE_COLLAPSE_COSINE, false);
        let votes = clusters.len();
        let merges: usize = clusters.iter().map(|c| c.len() - 1).sum();

        let mut max = (0.0_f64, String::new(), String::new());
        for i in 0..streams.len() {
            for j in (i + 1)..streams.len() {
                if let Some(c) = cosine_similarity(&streams[i], &streams[j], false) {
                    if c.abs() > max.0 {
                        max = (c.abs(), averaged[i].0.clone(), averaged[j].0.clone());
                    }
                }
            }
        }
        for cluster in clusters.iter().filter(|c| c.len() > 1) {
            let ids: Vec<&str> = cluster.iter().map(|&i| averaged[i].0.as_str()).collect();
            eprintln!("{name} seed-averaged merge cluster: {ids:?}");
        }
        eprintln!(
            "{name} seed-averaged: {} qualifiers, {votes} votes, {merges} merges, max |cos| {:.4} ({} vs {})",
            averaged.len(),
            max.0,
            max.1,
            max.2
        );

        let expect_configured = CONFIGURED_FALLBACK.contains(name);
        if *name == "commodities-1d" {
            // One, not two: the second of the two streams that used to qualify
            // here was `hold`, which has no Sharpe ratio.
            assert_eq!(
                averaged.len(),
                1,
                "review the documented nonfinite-support limitation if this changes"
            );
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../paper/evidence/final/commodities-1d.jsonl");
            let artifact = std::fs::read_to_string(path).unwrap();
            let historical: Vec<serde_json::Value> = artifact
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .filter(|row: &serde_json::Value| {
                    row["n_trials"] == 50
                        && row["dsr_bar"] == 0.95
                        && row["sr_std_pinned"].is_null()
                })
                .collect();
            assert!(
                !historical.is_empty(),
                "the historical comparison must actually exist"
            );
            assert!(historical
                .iter()
                .all(|row| row["trials_sr_std_source"] == "measured_floored"));
        }
        assert_eq!(
            votes < MIN_FIELD,
            expect_configured,
            "{name}: {votes} post-collapse votes contradicts reviewed current-engine support (expected {})",
            if expect_configured {
                "configured fallback (< 5 votes)"
            } else {
                "measured (>= 5 votes)"
            }
        );
    }
}

// --- the mandate field: one more agent, a different dispersion source --------

/// Periods per year for each frozen dataset, as `examples/mandate_eval.rs` and
/// `examples/evidence_sweep.rs` configure them.
const PERIODS_PER_YEAR: &[(&str, f64)] = &[
    ("us-indices-1d", 252.0),
    ("us-indices-1w", 52.0),
    ("crypto-majors-1h", 8760.0),
    ("crypto-majors-4h", 2190.0),
    ("crypto-majors-1d", 365.0),
    ("crypto-majors-1w", 52.0),
    ("fx-majors-1d", 252.0),
    ("commodities-1d", 252.0),
    ("rates-1d", 252.0),
];

/// `tab:eligibility` and `tab:mandate` print different deflated Sharpes for
/// buy-and-hold on us-indices-1d and crypto-majors-4h. The manuscript explains
/// that by field composition, not by a scoring inconsistency: `tab:eligibility`
/// scores the eight-agent evidence-sweep field and `tab:mandate` the nine-agent
/// field that adds the risk-managed agent, three panels sat one vote short of
/// the five-vote measurement minimum after clone collapse, and the ninth agent
/// supplied the missing vote. That explanation is a property of the kernel the
/// frozen evidence was produced under, and this test now pins what the current
/// engine does instead.
///
/// `hold` never trades, so its seed-averaged pooled stream is identically zero
/// and has no Sharpe ratio. The frozen kernel let it qualify at the
/// zero-variance sentinel and vote; the current one excludes it from the
/// dispersion sample on every panel (`measured_trials_sr_std`). Each of the
/// three panels therefore loses a vote in both fields, all three sit below the
/// minimum in both, and the ninth agent changes the dispersion source on no
/// panel at all. The three panels that measure in the eight-agent field
/// (crypto-majors-1h, fx-majors-1d, rates-1d) still measure in both. No frozen
/// number is restated here: this test asserts which path the current engine
/// takes, and `paper/sections/E-repairs.tex` records the divergence.
///
/// Ignored by default: reconstructing both fields on all nine datasets takes
/// about six and a half minutes, and the workspace test binaries run crate by
/// crate, so this one test floored every OS leg of CI. It runs unchanged, with
/// `-- --ignored`, in the Ubuntu-only `slow harness (clone-merge regression)`
/// job of `.github/workflows/ci.yml`, in parallel with the OS matrix.
#[test]
#[ignore = "about 6.5 minutes; runs in the ubuntu-only slow-harness CI job"]
fn the_mandate_field_no_longer_changes_the_dispersion_source_on_any_panel() {
    use sharpebench_core::{rank, ScoreConfig, TrialsSrStdSource};

    /// The panels the manuscript names, where the ninth agent used to supply the
    /// fifth vote and now cannot, because `hold`'s vote is withdrawn from both
    /// fields. Both fields take the configured path on each.
    const WAS_LIFTED_BY_THE_NINTH_AGENT: &[&str] =
        &["us-indices-1d", "crypto-majors-4h", "crypto-majors-1w"];
    /// The panels that keep enough votes to measure in both fields.
    const MEASURES_IN_BOTH: &[&str] = &["crypto-majors-1h", "fx-majors-1d", "rates-1d"];

    // Every panel is reconstructed before anything is asserted, so one run of
    // this six-minute test reports all nine rows rather than the first failure.
    let mut observed: Vec<(&str, TrialsSrStdSource, f64, TrialsSrStdSource, f64)> = Vec::new();

    for (name, ppy) in PERIODS_PER_YEAR {
        let data = load(name);
        let windows = windows_for(data.len());
        let floor = luck_floor(
            &data,
            &windows,
            &EXEC_SEEDS,
            CostModel::default(),
            LUCK_FLOOR_AGENTS,
        );
        let reference = || {
            vec![
                run_agent("buy-and-hold", &data, &windows, || Box::new(BuyAndHold)),
                run_agent(
                    "momentum",
                    &data,
                    &windows,
                    || Box::new(Momentum::default()),
                ),
                run_agent("hold", &data, &windows, || Box::new(HoldAgent)),
            ]
        };

        let mut sweep = reference();
        sweep.extend(floor.iter().cloned());

        let mut mandate = reference();
        mandate.push(run_agent("risk-managed", &data, &windows, || {
            Box::new(RiskManaged::new())
        }));
        mandate.extend(floor);

        let cfg = ScoreConfig {
            execution_seeds_per_window: EXEC_SEEDS.len(),
            ..ScoreConfig::for_periods_per_year(*ppy)
        };
        let source_of = |subs: &[AgentSubmission]| {
            let scored = rank(subs, &cfg);
            let s = scored.first().expect("a ranked field is never empty");
            (
                s.trials_sr_std_source,
                s.trials_sr_std_annualized_equivalent,
            )
        };
        let (sweep_source, sweep_sigma) = source_of(&sweep);
        let (mandate_source, mandate_sigma) = source_of(&mandate);
        eprintln!(
            "{name}: eight-agent {sweep_source:?} (sigma_ann {sweep_sigma:.4}), \
             nine-agent {mandate_source:?} (sigma_ann {mandate_sigma:.4})"
        );
        observed.push((
            *name,
            sweep_source,
            sweep_sigma,
            mandate_source,
            mandate_sigma,
        ));
    }

    assert_eq!(observed.len(), PERIODS_PER_YEAR.len());
    for &(name, sweep_source, sweep_sigma, mandate_source, mandate_sigma) in &observed {
        let name = &name;
        assert_eq!(
            sweep_source, mandate_source,
            "{name}: the ninth agent changed the dispersion source, which the \
             current engine no longer lets it do on any panel"
        );
        if WAS_LIFTED_BY_THE_NINTH_AGENT.contains(name) {
            assert_eq!(
                sweep_source,
                TrialsSrStdSource::Configured,
                "{name}: without `hold`'s vote both fields sit below the minimum"
            );
            assert!(
                (mandate_sigma - sweep_sigma).abs() < 1e-12,
                "{name}: both fields must read the same configured prior"
            );
        } else if MEASURES_IN_BOTH.contains(name) {
            assert_eq!(sweep_source, TrialsSrStdSource::Measured, "{name}");
        }
    }
}

// --- pass witness: the synthetic field of `examples/pass_witness.rs` ---------

const SHAPES: &[(&str, usize)] = &[("weekly-shaped", 77), ("daily-shaped", 409)];
const N_WINDOWS: usize = 6;
const N_SEEDS: usize = 8;
const N_ZERO_EDGE: usize = 5;
const SIGMA: f64 = 0.02;
const EDGES: &[f64] = &[
    0.00, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60,
];

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed ^ 0x5EED_2026_CAFE_F00D)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn normal(&mut self) -> f64 {
        let u1 = self.unit();
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
    }
}

/// The example's population tags and per-stream seed, mirrored so this test
/// draws the field the committed witness evidence was generated from.
const DOMAIN_ZERO_EDGE: u64 = 2;
const DOMAIN_WITNESS: u64 = 3;

fn stream_seed(domain: u64, member: usize, window: usize, exec_seed: usize) -> u64 {
    let mut mixed = domain;
    for coordinate in [member as u64, window as u64, exec_seed as u64] {
        mixed = mix(mixed ^ coordinate.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    mixed
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn witness_submission(
    id: &str,
    domain: u64,
    member: usize,
    s: f64,
    window_len: usize,
) -> AgentSubmission {
    let mut runs = Vec::new();
    for w in 0..N_WINDOWS {
        for k in 0..N_SEEDS {
            let mut rng = Rng::new(stream_seed(domain, member, w, k));
            let returns: Vec<f64> = (0..window_len)
                .map(|_| SIGMA * (s + rng.normal()))
                .collect();
            runs.push(Run {
                returns,
                ..Run::default()
            });
        }
    }
    AgentSubmission {
        agent_id: id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

#[test]
fn pass_witness_fields_have_no_clone_merges() {
    for (shape, window_len) in SHAPES {
        for &s in EDGES {
            let mut subs: Vec<AgentSubmission> = (0..N_ZERO_EDGE)
                .map(|k| {
                    witness_submission(
                        &format!("zero-edge-{k:02}"),
                        DOMAIN_ZERO_EDGE,
                        k,
                        0.0,
                        *window_len,
                    )
                })
                .collect();
            subs.push(witness_submission(
                "witness",
                DOMAIN_WITNESS,
                0,
                s,
                *window_len,
            ));
            assert_no_merges(&format!("pass-witness {shape} s={s:.2}"), &subs);
        }
    }
}

// --- the vote that sets the measured bar, on the panels that measure ----------

/// The rejected cap's constant at seven votes: the 99th percentile of the
/// largest robust z in an honest normal field of seven.
const FENCE_C_7: f64 = 10.75;

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// The measured-and-rejected cap: winsorize the votes at
/// `median +/- c * max(1.4826 * MAD, floor)` and take the standard deviation.
/// Returns that dispersion and the largest robust z, the distance from the
/// median in units of the unfloored scale.
fn fenced_dispersion(sorted: &[f64], c: f64, floor: f64) -> (f64, f64) {
    use sharpebench_core::stats::std_dev;
    let m = median(sorted);
    let mut dev: Vec<f64> = sorted.iter().map(|x| (x - m).abs()).collect();
    dev.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let raw_scale = 1.4826 * median(&dev);
    let scale = raw_scale.max(floor);
    let clipped: Vec<f64> = sorted
        .iter()
        .map(|x| x.clamp(m - c * scale, m + c * scale))
        .collect();
    (std_dev(&clipped), dev[dev.len() - 1] / raw_scale)
}

/// On the three panels the current engine measures, the disclosure names the
/// honest reference vote that carries the dispersion, and the bar is untouched.
///
/// The five luck-floor agents sit in a tight cluster and buy-and-hold sits far
/// from it, so one honest vote multiplies the measured dispersion: 4.89 on
/// hourly crypto, 4.15 on daily FX and 1.55 on daily rates, measured on this
/// engine. A cap on the vote was measured against exactly these fields and not
/// shipped, because containing one hostile entrant would have lowered the hourly
/// crypto bar by 74.5% and the daily FX bar by 17.4%. The disclosure reports the
/// row instead, and the measured dispersion is still the plain standard
/// deviation of the sorted votes, bit for bit.
///
/// The same loop recomputes the rejected cap on these votes, so the chapter's
/// binding-table rows for the three sweep panels, and the robust distance it
/// quotes for hourly buy-and-hold, are reproduced by a test rather than
/// carried as numbers from a study.
#[test]
fn the_measured_panels_disclose_the_vote_that_sets_their_bar() {
    use sharpebench_core::composite::window_tracks;
    use sharpebench_core::deflated_sharpe::observed_sharpe_ratio_of_windows;
    use sharpebench_core::stats::std_dev;
    use sharpebench_core::{rank, ScoreConfig, TrialsSrStdSource};

    for (name, periods, leverage, bar_change, robust_z) in [
        ("crypto-majors-1h", 8760.0, 4.89, -0.745, Some(162.0)),
        ("fx-majors-1d", 252.0, 4.15, -0.174, None),
        ("rates-1d", 252.0, 1.55, 0.0, None),
    ] {
        let data = load(name);
        let windows = windows_for(data.len());
        let mut sweep = vec![
            run_agent("buy-and-hold", &data, &windows, || Box::new(BuyAndHold)),
            run_agent(
                "momentum",
                &data,
                &windows,
                || Box::new(Momentum::default()),
            ),
            run_agent("hold", &data, &windows, || Box::new(HoldAgent)),
        ];
        sweep.extend(luck_floor(
            &data,
            &windows,
            &EXEC_SEEDS,
            CostModel::default(),
            LUCK_FLOOR_AGENTS,
        ));
        // The bootstrap legs are orthogonal to the dispersion; a small count
        // keeps the hourly panel affordable in a debug test run.
        let cfg = ScoreConfig {
            execution_seeds_per_window: EXEC_SEEDS.len(),
            n_boot: 20,
            ..ScoreConfig::for_periods_per_year(periods)
        };
        let board = rank(&sweep, &cfg);

        // The kernel's qualification predicate: a vote is a track whose
        // windowed Sharpe exists.
        let mut votes: Vec<f64> = sweep
            .iter()
            .filter_map(|s| {
                let tracks = window_tracks(s, EXEC_SEEDS.len());
                let slices: Vec<&[f64]> = tracks.iter().map(Vec::as_slice).collect();
                observed_sharpe_ratio_of_windows(&slices).ok()
            })
            .collect();
        votes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let measured = std_dev(&votes);

        let floor = cfg.min_measured_trials_sr_std / periods.sqrt();
        let (capped, z) = fenced_dispersion(&votes, FENCE_C_7, floor);
        let change = capped / measured - 1.0;
        eprintln!("{name}: cap moves the dispersion {measured:.5} to {capped:.5} ({change:+.4}), top robust z {z:.1}");
        assert!(
            (change - bar_change).abs() < 0.0005,
            "{name}: the rejected cap would move the bar {change:+.4}, the chapter says {bar_change:+.3}"
        );
        if let Some(want) = robust_z {
            assert!(
                (z - want).abs() < 0.5,
                "{name}: robust z {z:.2}, the chapter says {want}"
            );
        }

        for row in &board {
            assert_eq!(
                row.trials_sr_std_source,
                TrialsSrStdSource::Measured,
                "{name}"
            );
            assert_eq!(
                row.trials_sr_std.to_bits(),
                measured.to_bits(),
                "{name}: the disclosure must not move the measured dispersion"
            );
            let vote = row
                .trials_sr_std_most_influential_vote
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: a measured row carries the disclosure"));
            assert_eq!(vote.agent_id, "buy-and-hold", "{name}: {vote:?}");
            assert_eq!(vote.agents_in_vote, 1, "{name}");
            assert_eq!(vote.votes, 7, "{name}");
            let got = vote.leverage.expect("the other votes are not all equal");
            assert!(
                (got - leverage).abs() < 0.005,
                "{name}: leverage {got}, measured {leverage}"
            );
        }
    }
}

/// The vote fence admits every vote on the three panels the engine measures,
/// with room, so the shipped bar on each is the one the plain standard
/// deviation of all seven votes gives and the fence changes no published
/// number. The test above pins that dispersion bit for bit; this one pins the
/// margin by which it is safe.
///
/// The fence's scale is the median of the pairwise absolute differences, not
/// the median absolute deviation. The MAD of these fields is the spread inside
/// the five-agent luck-floor cluster, which puts the honest hourly carrier at a
/// robust z of 162 and would fence a legitimate entrant; the pairwise median
/// reads every pair and puts the same vote at 5.2. The kernel's scale is
/// restated here rather than imported, so a change to it has to move this
/// number to pass.
#[test]
fn the_measured_panels_sit_inside_the_vote_fence() {
    use sharpebench_core::composite::window_tracks;
    use sharpebench_core::deflated_sharpe::observed_sharpe_ratio_of_windows;
    use sharpebench_core::ScoreConfig;

    for (name, periods, largest_z) in [
        ("crypto-majors-1h", 8760.0, 5.199),
        ("fx-majors-1d", 252.0, 4.541),
        ("rates-1d", 252.0, 2.698),
    ] {
        let data = load(name);
        let windows = windows_for(data.len());
        let mut sweep = vec![
            run_agent("buy-and-hold", &data, &windows, || Box::new(BuyAndHold)),
            run_agent(
                "momentum",
                &data,
                &windows,
                || Box::new(Momentum::default()),
            ),
            run_agent("hold", &data, &windows, || Box::new(HoldAgent)),
        ];
        sweep.extend(luck_floor(
            &data,
            &windows,
            &EXEC_SEEDS,
            CostModel::default(),
            LUCK_FLOOR_AGENTS,
        ));
        let cfg = ScoreConfig {
            execution_seeds_per_window: EXEC_SEEDS.len(),
            ..ScoreConfig::for_periods_per_year(periods)
        };

        let mut votes: Vec<f64> = sweep
            .iter()
            .filter_map(|s| {
                let tracks = window_tracks(s, EXEC_SEEDS.len());
                let slices: Vec<&[f64]> = tracks.iter().map(Vec::as_slice).collect();
                observed_sharpe_ratio_of_windows(&slices).ok()
            })
            .collect();
        votes.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let floor = cfg.min_measured_trials_sr_std / periods.sqrt();
        let raw_scale = pairwise_median_scale(&votes);
        assert!(
            raw_scale > floor,
            "{name}: the dispersion floor does not bind on this panel, so the pinned distance is              the measured one: scale {raw_scale}, floor {floor}"
        );
        let centre = median(&votes);
        let z = votes
            .iter()
            .map(|v| (v - centre).abs() / raw_scale)
            .fold(0.0_f64, f64::max);
        eprintln!("{name}: largest vote distance {z:.3} of the fence's {FENCE_C_7}");
        assert!(
            (z - largest_z).abs() < 0.01,
            "{name}: largest vote distance {z:.3}, measured {largest_z}"
        );
        assert!(
            z < FENCE_C_7,
            "{name}: an honest vote reaches the fence at {z:.3}, so the fence would move this              panel's published bar"
        );
    }
}

/// The kernel's robust scale for the vote fence, restated: the median of the
/// pairwise absolute differences of a sorted sample, scaled so it estimates a
/// normal standard deviation the way 1.4826 times the MAD does.
fn pairwise_median_scale(sorted: &[f64]) -> f64 {
    let mut gaps = Vec::new();
    for (i, low) in sorted.iter().enumerate() {
        for high in &sorted[i + 1..] {
            gaps.push(high - low);
        }
    }
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    1.048_358_013_869_129 * median(&gaps)
}

// --- the between-window share on honest traded tracks --------------------------

/// The chapter says an honest traded track's between-window share is near 0.
/// On the nine frozen panels, for the reference agents, the risk-managed agent
/// and the five-agent luck floor, every share is below 0.03, the largest on
/// weekly crypto where the windows are shortest. The one-way split of i.i.d.
/// returns has an expected share of about `(windows - 1) / (bars - 1)`, so the
/// short weekly panels sit highest by construction. Commodities is reported
/// with its reason, not a share: its raw WTI prices include the documented
/// negative quote, which leaves non-finite returns in the pooled track.
#[test]
fn honest_tracks_on_the_frozen_panels_split_almost_nothing_between_windows() {
    use sharpebench_core::{rank, sharpe_diagnostics, ScoreConfig, SharpeDiagnostic};
    let mut largest = (0.0_f64, String::new());
    for &name in DATASETS {
        let periods = match name {
            "crypto-majors-1h" => 8760.0,
            "crypto-majors-4h" => 2190.0,
            "crypto-majors-1d" => 365.0,
            "us-indices-1w" | "crypto-majors-1w" => 52.0,
            _ => 252.0,
        };
        let data = load(name);
        let windows = windows_for(data.len());
        let mut field = vec![
            run_agent("buy-and-hold", &data, &windows, || Box::new(BuyAndHold)),
            run_agent(
                "momentum",
                &data,
                &windows,
                || Box::new(Momentum::default()),
            ),
            run_agent("risk-managed", &data, &windows, || {
                Box::new(RiskManaged::new())
            }),
        ];
        field.extend(luck_floor(
            &data,
            &windows,
            &EXEC_SEEDS,
            CostModel::default(),
            LUCK_FLOOR_AGENTS,
        ));
        let cfg = ScoreConfig {
            execution_seeds_per_window: EXEC_SEEDS.len(),
            n_boot: 20,
            ..ScoreConfig::for_periods_per_year(periods)
        };
        let board = rank(&field, &cfg);
        let requested = [SharpeDiagnostic::BetweenWindowVariance];
        for row in sharpe_diagnostics(&field, &board, &cfg, &requested) {
            let d = row.between_window_variance.expect("requested");
            match d.between_window_share {
                Some(share) => {
                    assert!(share < 0.03, "{name} {}: share {share}", row.agent_id);
                    if share > largest.0 {
                        largest = (share, format!("{name} {}", row.agent_id));
                    }
                }
                None => {
                    assert_eq!(name, "commodities-1d", "{}: {:?}", row.agent_id, d.error);
                    assert_eq!(
                        d.error.as_deref(),
                        Some("a pooled observation is not finite")
                    );
                }
            }
        }
    }
    eprintln!(
        "largest honest between-window share: {:.4} ({})",
        largest.0, largest.1
    );
}
