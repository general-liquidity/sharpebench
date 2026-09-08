//! Synthetic pass witness for the SharpeBench paper.
//!
//! Across the paper's full evidence grid zero agents are rank-eligible, which
//! leaves two hypotheses observationally equivalent: the gates discriminate, or
//! the eligibility conjunction is jointly unsatisfiable at the shipped defaults.
//! This example separates them by construction. A family of witness agents with
//! a controlled injected edge — per-period returns drawn as
//! `sigma * (s + z_t)` with `z_t` standard normal, so the true per-period
//! Sharpe is `s` by construction — is scored through the shipped default
//! `ScoreConfig` against a **separate, frozen** five-agent zero-edge calibration
//! field. The calibration field fixes the measured dispersion before any witness
//! is generated; sweeping `s` therefore cannot let the proposed witness lower or
//! raise its own deflation bar. Sweeping `s` then locates the eligibility
//! boundary and proves the acceptance region is nonempty.
//!
//! Two track shapes mirror the real datasets' window rule: a weekly-shaped
//! track (six 77-bar windows, 52 periods per year, the us-indices-1w geometry)
//! and a daily-shaped track (six 409-bar windows, 252 periods per year, the
//! us-indices-1d geometry). Eight seeds per window, as everywhere else.
//!
//! Deterministic: no clock, no ambient RNG. Run with
//!
//!   cargo run --release -p sharpebench-harness --example pass_witness -- <out.jsonl>

use std::env;
use std::f64::consts::PI;
use std::fs::File;
use std::io::{BufWriter, Write};

use serde::Serialize;
use sharpebench_core::composite::{rank, AgentSubmission, Run, ScoreConfig};

/// (name, periods per year, window length in bars) — the us-indices window
/// geometries from the evidence sweep's window rule.
const SHAPES: &[(&str, f64, usize)] = &[("weekly-shaped", 52.0, 77), ("daily-shaped", 252.0, 409)];

const N_WINDOWS: usize = 6;
const N_SEEDS: usize = 8;
const N_ZERO_EDGE: usize = 5;
/// Per-period return volatility of every synthetic track.
const SIGMA: f64 = 0.02;
/// Injected per-period Sharpe levels swept.
const EDGES: &[f64] = &[
    0.00, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60,
];

#[derive(Serialize)]
struct WitnessRecord<'a> {
    kind: &'static str,
    shape: &'a str,
    periods_per_year: f64,
    window_len: usize,
    n_windows: usize,
    n_seeds: usize,
    injected_sharpe_per_period: f64,
    injected_sharpe_annualized: f64,
    agent_id: String,
    deflated_sharpe: f64,
    psr: f64,
    passed_k: bool,
    bootstrap_p: f64,
    worst_run_drawdown: f64,
    rank_eligible: bool,
    trials_sr_std_used: f64,
    trials_sr_std_source: &'static str,
    calibration_agents: usize,
    calibration_trials_sr_std_per_period: f64,
    calibration_trials_sr_std_annualized_equivalent: f64,
    deflation_bar_per_period: f64,
    pooled_observations: usize,
}

/// Deterministic splitmix-based RNG matching the style used across the
/// workspace; standard normal via Box–Muller.
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

/// Domain tags. One per synthetic population, so a stream is identified by
/// which population it belongs to and never only by an arithmetic offset.
const DOMAIN_CALIBRATION: u64 = 1;
const DOMAIN_ZERO_EDGE: u64 = 2;
const DOMAIN_WITNESS: u64 = 3;

/// The RNG seed for one synthetic return stream, from the tuple that names it.
///
/// The old expression was `base_seed ^ (window << 32) ^ (exec_seed + 1)` with
/// `base_seed = 0x00CC_0000 + member`, which put the member index and the
/// execution-seed value in overlapping low bits: `0xCC0000 ^ 3` equals
/// `0xCC0001 ^ 2`, so calibration member 0 at execution value 3 and member 1 at
/// execution value 2 drew the identical series. Five members by eight seeds
/// yielded 13 distinct streams per window instead of 40, and the "independent
/// zero-edge calibrators" that fix the dispersion bar were not independent.
///
/// Each coordinate is now mixed in its own round rather than folded into
/// shared bits, so no coordinate can cancel another. Uniqueness is not argued
/// from the construction: `every_stream_tuple_has_its_own_seed` enumerates the
/// whole grid this example draws and is the guarantee.
///
/// The common random numbers this example does want are deliberately outside
/// the tuple: the injected edge `s` is not a coordinate, so the witness
/// redraws the same streams at every point of the edge sweep and the
/// monotonicity assertion still compares like with like.
fn stream_seed(domain: u64, member: usize, window: usize, exec_seed: usize) -> u64 {
    let mut mixed = domain;
    for coordinate in [member as u64, window as u64, exec_seed as u64] {
        mixed = mix(mixed ^ coordinate.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    mixed
}

/// splitmix64's finalizer: a bijection on u64, so distinct inputs to any single
/// round stay distinct.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One submission: `N_WINDOWS x N_SEEDS` runs of `window_len` returns with true
/// per-period Sharpe `s`, each run its own seeded stream.
fn submission(id: &str, domain: u64, member: usize, s: f64, window_len: usize) -> AgentSubmission {
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

fn main() {
    // Required positional; see evidence_sweep for why there is no default.
    let out = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: pass_witness <out.jsonl>");
        std::process::exit(2);
    });
    let mut w = BufWriter::new(File::create(&out).expect("create output"));

    for (shape, ppy, window_len) in SHAPES {
        println!("== {shape} (ppy {ppy}, {N_WINDOWS} windows of {window_len}, {N_SEEDS} seeds) ==");
        println!(
            "{:>6} {:>8} {:>8} {:>7} {:>9} {:>9}",
            "s", "s_ann", "dsr", "pass^k", "worst_dd", "eligible"
        );
        let mut last_witness_dsr = f64::NEG_INFINITY;
        let mut eligibility_open = false;
        // Estimate the field dispersion exactly once, from independent
        // zero-edge calibrators. `rank` applies the ordinary measured-field
        // floor, after which the value is frozen as an explicit prior for every
        // witness edge. This is exogenous to the entire edge sweep.
        let calibration_subs: Vec<AgentSubmission> = (0..N_ZERO_EDGE)
            .map(|k| {
                submission(
                    &format!("calibration-zero-edge-{k:02}"),
                    DOMAIN_CALIBRATION,
                    k,
                    0.0,
                    *window_len,
                )
            })
            .collect();
        let calibration_cfg = ScoreConfig {
            execution_seeds_per_window: N_SEEDS,
            ..ScoreConfig::for_periods_per_year(*ppy)
        };
        let calibration = rank(&calibration_subs, &calibration_cfg);
        let calibration_sr_std = calibration
            .first()
            .expect("non-empty calibration field")
            .trials_sr_std;
        assert!(
            calibration
                .iter()
                .all(|s| s.trials_sr_std.to_bits() == calibration_sr_std.to_bits()),
            "one frozen calibration bar must apply to every calibration member"
        );
        for &s in EDGES {
            // The witness may share a display field with zero-edge controls, but
            // its threshold is the frozen *external* calibration above. Disable
            // field remeasurement so neither the witness nor these controls can
            // alter the bar at any sweep point.
            let mut subs: Vec<AgentSubmission> = (0..N_ZERO_EDGE)
                .map(|k| {
                    submission(
                        &format!("zero-edge-{k:02}"),
                        DOMAIN_ZERO_EDGE,
                        k,
                        0.0,
                        *window_len,
                    )
                })
                .collect();
            subs.push(submission(
                "witness",
                // Common random numbers across the edge sweep: the seed tuple
                // does not include the injected mean, so only the mean changes
                // between sweep points. Without this, a reported first pass
                // could be a local crossing caused by a different noise draw.
                DOMAIN_WITNESS,
                0,
                s,
                *window_len,
            ));
            let cfg = ScoreConfig {
                trials_sr_std: calibration_sr_std * ppy.sqrt(),
                // A separate zero-edge synthetic calibration population fixes
                // the dispersion; the separately declared conservative
                // zero-Sharpe null fixes E[SR_null] = 0. It is
                // never the witness field, so Eq. 6's mean term is explicit.
                deflation_null_mean_per_period: 0.0,
                min_field_for_measured_sr_std: usize::MAX,
                execution_seeds_per_window: N_SEEDS,
                ..ScoreConfig::for_periods_per_year(*ppy)
            };
            let scored = rank(&subs, &cfg);
            for sc in &scored {
                if sc.agent_id == "witness" {
                    assert!(
                        sc.deflated_sharpe + 1e-12 >= last_witness_dsr,
                        "witness DSR must be monotone under common random numbers"
                    );
                    assert!(
                        !eligibility_open || sc.rank_eligible,
                        "witness eligibility closed after opening"
                    );
                    last_witness_dsr = sc.deflated_sharpe;
                    eligibility_open |= sc.rank_eligible;
                    println!(
                        "{:>6.2} {:>8.2} {:>8.4} {:>7} {:>9.4} {:>9}",
                        s,
                        s * ppy.sqrt(),
                        sc.deflated_sharpe,
                        sc.passed_k,
                        sc.worst_run_drawdown,
                        sc.rank_eligible
                    );
                }
                let rec = WitnessRecord {
                    kind: "pass_witness",
                    shape,
                    periods_per_year: *ppy,
                    window_len: *window_len,
                    n_windows: N_WINDOWS,
                    n_seeds: N_SEEDS,
                    injected_sharpe_per_period: s,
                    injected_sharpe_annualized: s * ppy.sqrt(),
                    agent_id: sc.agent_id.clone(),
                    deflated_sharpe: sc.deflated_sharpe,
                    psr: sc.psr,
                    passed_k: sc.passed_k,
                    bootstrap_p: sc.bootstrap_p,
                    worst_run_drawdown: sc.worst_run_drawdown,
                    rank_eligible: sc.rank_eligible,
                    trials_sr_std_used: sc.trials_sr_std,
                    trials_sr_std_source: "exogenous_zero_edge_calibration",
                    calibration_agents: N_ZERO_EDGE,
                    calibration_trials_sr_std_per_period: calibration_sr_std,
                    calibration_trials_sr_std_annualized_equivalent: calibration_sr_std
                        * ppy.sqrt(),
                    deflation_bar_per_period: sc.deflation_bar_per_period,
                    pooled_observations: sc.pooled_observations,
                };
                serde_json::to_writer(&mut w, &rec).expect("write record");
                w.write_all(b"\n").expect("newline");
            }
        }
    }
    w.flush().expect("flush");
    eprintln!("wrote {out}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Every (domain, member, window, execution seed) tuple in the example's
    /// full grid must own its stream. The finding was that it did not: the
    /// calibration field drew 13 distinct streams per window where 40 were
    /// claimed.
    #[test]
    fn every_stream_tuple_has_its_own_seed() {
        let mut seen: HashMap<u64, (u64, usize, usize, usize)> = HashMap::new();
        for domain in [DOMAIN_CALIBRATION, DOMAIN_ZERO_EDGE, DOMAIN_WITNESS] {
            for member in 0..N_ZERO_EDGE {
                for window in 0..N_WINDOWS {
                    for exec_seed in 0..N_SEEDS {
                        let tuple = (domain, member, window, exec_seed);
                        let seed = stream_seed(domain, member, window, exec_seed);
                        if let Some(other) = seen.insert(seed, tuple) {
                            panic!("seed {seed:#x} shared by {other:?} and {tuple:?}");
                        }
                    }
                }
            }
        }
        assert_eq!(seen.len(), 3 * N_ZERO_EDGE * N_WINDOWS * N_SEEDS);
    }

    /// The exact historical collision, kept as a named case: calibration member
    /// 0 with execution seed 3 and member 1 with execution seed 2 drew one
    /// stream, because `0xCC0000 ^ 3 == 0xCC0001 ^ 2`.
    #[test]
    fn the_reported_calibration_collision_is_gone() {
        // The old expression bound execution-seed index k to the value k + 1,
        // so this equality is member 0 at index 2 against member 1 at index 1.
        assert_eq!(0x00CC_0000u64 ^ 3, 0x00CC_0001u64 ^ 2);
        assert_ne!(
            stream_seed(DOMAIN_CALIBRATION, 0, 0, 2),
            stream_seed(DOMAIN_CALIBRATION, 1, 0, 1)
        );
    }

    /// Calibration members must be independent draws, which is the property the
    /// frozen dispersion bar is claimed to rest on.
    #[test]
    fn calibration_members_do_not_share_return_series() {
        let a = submission("a", DOMAIN_CALIBRATION, 0, 0.0, 16);
        let b = submission("b", DOMAIN_CALIBRATION, 1, 0.0, 16);
        for (i, (run_a, run_b)) in a.runs.iter().zip(b.runs.iter()).enumerate() {
            assert_ne!(run_a.returns, run_b.returns, "run {i} is a shared stream");
        }
        for run_a in &a.runs {
            for run_b in &b.runs {
                assert_ne!(run_a.returns, run_b.returns);
            }
        }
    }

    /// The distinct populations must not overlap either: a calibration stream
    /// reappearing as a zero-edge control or as the witness would make the
    /// "separate, frozen" calibration field circular.
    #[test]
    fn the_three_populations_do_not_share_streams() {
        let calibration = submission("c", DOMAIN_CALIBRATION, 0, 0.0, 16);
        let control = submission("z", DOMAIN_ZERO_EDGE, 0, 0.0, 16);
        let witness = submission("w", DOMAIN_WITNESS, 0, 0.0, 16);
        for run in &calibration.runs {
            assert!(!control.runs.iter().any(|r| r.returns == run.returns));
            assert!(!witness.runs.iter().any(|r| r.returns == run.returns));
        }
    }

    /// The common random numbers the edge sweep does want are still there: the
    /// injected mean is not a seed coordinate, so two edge levels differ by a
    /// constant shift and nothing else. Without this the monotonicity assertion
    /// in `main` would be comparing different noise draws.
    #[test]
    fn the_witness_keeps_common_random_numbers_across_edges() {
        let low = submission("w", DOMAIN_WITNESS, 0, 0.10, 16);
        let high = submission("w", DOMAIN_WITNESS, 0, 0.50, 16);
        assert_eq!(low.runs.len(), high.runs.len());
        for (run_low, run_high) in low.runs.iter().zip(high.runs.iter()) {
            for (a, b) in run_low.returns.iter().zip(run_high.returns.iter()) {
                let shift = b - a;
                assert!(
                    (shift - SIGMA * (0.50 - 0.10)).abs() < 1e-12,
                    "edge levels differ by more than the injected mean"
                );
            }
        }
    }
}
