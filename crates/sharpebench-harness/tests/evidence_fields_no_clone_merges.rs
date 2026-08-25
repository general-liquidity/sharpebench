//! The clone collapse must not touch the paper's committed evidence.
//!
//! `rank` collapses near-clone streams to one vote each before it measures
//! `trials_sr_std` (see `sharpebench_core::CLONE_COLLAPSE_COSINE`). On a field
//! with no clusters the collapse is the identity, so the committed evidence
//! (`paper/evidence/final/*.jsonl`, the risk-managed run and the pass witness)
//! is byte-identical to a pre-collapse kernel exactly when the collapse's
//! clustering merges nothing on those fields. This test rebuilds every one of
//! those fields the way its example does (same agents, seeds, windows and cost
//! model, all pinned) and asserts zero merges at the collapse threshold. It
//! also records the maximum honest pair, which is the number the threshold was
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
/// (finite-Sharpe qualifiers, clusters vote once, `min_field` five), and
/// asserts the vote count implies exactly the `trials_sr_std_source` stamped
/// on the committed default cells: `configured` (fewer than five votes) on
/// us-indices-1w, crypto-majors-1w, crypto-majors-1d, us-indices-1d and
/// crypto-majors-4h; measured (five or more) on crypto-majors-1h, fx-majors-1d,
/// commodities-1d and rates-1d.
#[test]
fn seed_averaged_streams_match_committed_dispersion_source() {
    use sharpebench_core::composite::pooled_returns;
    use sharpebench_core::deflated_sharpe::sharpe_ratio;

    /// Datasets whose committed default cells stamp `trials_sr_std_source:
    /// "configured"` (paper/evidence/final/<dataset>.jsonl at dsr_bar 0.95,
    /// host N 50, unpinned dispersion).
    const CONFIGURED_FALLBACK: &[&str] = &[
        "us-indices-1w",
        "crypto-majors-1w",
        "crypto-majors-1d",
        "us-indices-1d",
        "crypto-majors-4h",
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

        // Mirror `measured_trials_sr_std`: seed-averaged streams, finite-Sharpe
        // qualifiers, one vote per clone cluster at the collapse threshold.
        let averaged: Vec<(String, Vec<f64>)> = sweep
            .iter()
            .map(|s| (s.agent_id.clone(), pooled_returns(s, EXEC_SEEDS.len())))
            .filter(|(_, p)| p.len() >= 2 && sharpe_ratio(p).is_finite())
            .collect();
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
        assert_eq!(
            votes < MIN_FIELD,
            expect_configured,
            "{name}: {votes} post-collapse votes contradicts the committed \
             trials_sr_std_source (expected {})",
            if expect_configured {
                "configured fallback (< 5 votes)"
            } else {
                "measured (>= 5 votes)"
            }
        );
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

fn witness_submission(id: &str, base_seed: u64, s: f64, window_len: usize) -> AgentSubmission {
    let mut runs = Vec::new();
    for w in 0..N_WINDOWS {
        for k in 0..N_SEEDS {
            let mut rng = Rng::new(base_seed ^ ((w as u64) << 32) ^ (k as u64 + 1));
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
        for (i, &s) in EDGES.iter().enumerate() {
            let mut subs: Vec<AgentSubmission> = (0..N_ZERO_EDGE)
                .map(|k| {
                    witness_submission(
                        &format!("zero-edge-{k:02}"),
                        0x00AA_0000 + k as u64,
                        0.0,
                        *window_len,
                    )
                })
                .collect();
            subs.push(witness_submission(
                "witness",
                0x00BB_0000 + i as u64,
                s,
                *window_len,
            ));
            assert_no_merges(&format!("pass-witness {shape} s={s:.2}"), &subs);
        }
    }
}
