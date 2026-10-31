//! `sharpebench grader-search`: a seeded search against the eligibility gates.
//!
//! `sharpebench audit` fires a fixed list of named attacks. This command looks
//! for the attacks nobody named: it generates synthetic submissions from moves
//! that carry no skill by construction, scores them with the real kernel, climbs
//! toward the gates, and reports every submission the kernel admits with its
//! exact return series. No model is called and no market data is read.
//!
//! **What "without skill" means here.** The kernel scores returns, and a return
//! series with a real positive mean is indistinguishable from skill on returns
//! alone: the self-audit's own `skilled_run` fixture is such a series. So the
//! oracle is the generator, never the scorer. A move counts only if it falls in
//! one of three classes, each checkable without reading a score:
//!
//! - [`Basis::Degenerate`]: inside every window the track never moves by more
//!   than [`DEGENERATE_MOTION`]. The kernel's own doctrine is that a track that
//!   never moved has no Sharpe ratio.
//! - [`Basis::FlatExceptSparse`]: at most one observation per window differs
//!   from the window's median, which is the single-observation-differs evasion
//!   of an exact-equality predicate.
//! - [`Basis::ZeroEdge`]: the expected return of every bar, conditional on the
//!   past, is zero by construction. Any in-sample edge is the seed.
//!
//! A positive-drift track with ordinary noise is outside the search on purpose.
//! Admitting one is not a defect the search can establish.
//!
//! **One admission is not a defect.** A search over seeds is a selection
//! procedure and it finds lucky draws. Every parameter set that admits a
//! submission is therefore re-drawn on held-out seeds, and the admission rate is
//! compared with an iid Gaussian null on the same layout. `structural` means the
//! lower Wilson bound clears both [`STRUCTURAL_RATE_FLOOR`] and the control's
//! upper bound; a deterministic move is structural by definition. Everything
//! else is `selection_luck`, and is still reported.
//!
//! The report is never a rank input. Nothing in the scorer, the board, the
//! self-audit or any gate reads it. Series are drawn with additions and square
//! roots only, so they do not depend on a platform's `libm`.

use std::collections::BTreeSet;

use serde::Serialize;
use sharpebench_core::composite::DISPERSION_VOTE_FENCE_Z;
use sharpebench_core::deflated_sharpe::observed_sharpe_ratio;
use sharpebench_core::rediscovery::CLONE_COLLAPSE_COSINE;
use sharpebench_core::stats::{mean, std_dev};
use sharpebench_core::{
    cosine_similarity, per_run_passes, rank, AgentSubmission, CompositeScore, PassMode, Run,
    ScoreConfig,
};

pub const SCHEMA_VERSION: &str = "sharpebench.grader-search.v1";

/// The seed the committed regression pins.
pub const DEFAULT_SEED: u64 = 20_261_030;
const DEFAULT_BUDGET: usize = 4_000;
const DEFAULT_REPLICATES: usize = 64;
const DEFAULT_MAX_FINDINGS: usize = 4;

/// Within-window motion at or below this is no motion: 0.01 basis points a bar,
/// below any tick or cost the simulator models.
pub const DEGENERATE_MOTION: f64 = 1e-6;
/// The held-out admission rate, as a Wilson lower bound, at which a parameter
/// set stops being luck. Fixed before the first run and never tuned to a result.
pub const STRUCTURAL_RATE_FLOOR: f64 = 0.01;
const WILSON_Z: f64 = 1.96;

/// Screening bootstrap draws. Screening only steers the search: every reported
/// finding is confirmed under the default configuration.
const SCREEN_N_BOOT: usize = 100;
const ROUNDS: usize = 4;
const ELITES: usize = 8;

const WINDOWS: [usize; 5] = [1, 2, 4, 5, 8];
const BARS: [usize; 4] = [30, 60, 120, 252];

const USAGE: &str = "usage: sharpebench grader-search [--seed N] [--budget N] [--replicates N] \
                     [--max-findings N] [--threads N] [--out <report.json>] [--json]";

/// SplitMix64. The draws use addition and shifts only, so a seed gives the same
/// bits on every platform.
#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        ((self.unit() * n as f64) as usize).min(n - 1)
    }

    /// Unit-variance, zero-mean draw: twelve uniforms less six. Bounded at six
    /// standard deviations, which matters to no move here, and free of `libm`.
    fn gauss(&mut self) -> f64 {
        (0..12).map(|_| self.unit()).sum::<f64>() - 6.0
    }

    fn coin(&mut self) -> f64 {
        if self.next_u64() >> 63 == 0 {
            1.0
        } else {
            -1.0
        }
    }
}

/// Why a track carries no skill, stated by the generator and checked on the
/// series where the class can be checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Degenerate,
    FlatExceptSparse,
    ZeroEdge,
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        0.5 * (sorted[n / 2 - 1] + sorted[n / 2])
    }
}

/// The strongest class the series itself establishes, if any.
fn observed_basis(windows: &[Vec<f64>]) -> Option<Basis> {
    let mut degenerate = true;
    let mut sparse = true;
    for window in windows {
        let centre = median(window);
        degenerate &= window
            .iter()
            .all(|x| (x - centre).abs() <= DEGENERATE_MOTION);
        sparse &= window.iter().filter(|x| **x != centre).count() <= 1;
    }
    if degenerate {
        Some(Basis::Degenerate)
    } else if sparse {
        Some(Basis::FlatExceptSparse)
    } else {
        None
    }
}

/// One generated submission.
#[derive(Clone)]
struct Track {
    variant: &'static str,
    params: String,
    basis: Basis,
    pass_mode: PassMode,
    windows: Vec<Vec<f64>>,
}

impl Track {
    fn new(variant: &'static str, params: String, windows: Vec<Vec<f64>>) -> Self {
        Self {
            variant,
            params,
            basis: Basis::ZeroEdge,
            pass_mode: PassMode::All,
            windows,
        }
    }

    /// A track whose claim rests on its shape: refuse it unless the series
    /// really has that shape.
    fn shaped(mut self) -> Option<Self> {
        self.basis = observed_basis(&self.windows)?;
        Some(self)
    }

    fn windows_count(&self) -> usize {
        self.windows.len()
    }

    fn bars(&self) -> usize {
        self.windows.first().map_or(0, Vec::len)
    }
}

/// A move: a set of parameter grids and a generator over them. `seeded` says
/// whether the generator reads the noise seed; a move that does not is
/// deterministic, and its held-out replication is vacuous.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Move {
    ConstantTrack,
    NearConstant,
    SingleObservation,
    Autocorrelated,
    TailSeller,
    LeverageTiming,
    WindowBoundary,
}

const LEVELS: [f64; 3] = [1e-4, 1e-3, 1e-2];
const CONSTANT_LEVELS: [f64; 5] = [0.0, 1e-4, 1e-3, 1e-2, -1e-3];
const JITTERS: [f64; 7] = [1e-15, 1e-13, 1e-12, 1e-10, 1e-8, 1e-7, 1e-6];
const SPIKES: [f64; 8] = [1e-12, 1e-9, 1e-6, 1e-4, 1e-3, 1e-2, 5e-2, 2e-1];
const PHIS: [f64; 8] = [0.0, 0.3, 0.6, 0.8, 0.9, 0.95, 0.98, 0.99];
const SIGMAS: [f64; 2] = [0.001, 0.01];
const PREMIUMS: [f64; 4] = [2e-4, 5e-4, 1e-3, 2e-3];
const CRASH_RATES: [f64; 5] = [0.001, 0.002, 0.005, 0.01, 0.02];
const NOISE_RATIOS: [f64; 3] = [0.0, 0.25, 1.0];
const CAPS: [f64; 4] = [4.0, 16.0, 64.0, 256.0];

impl Move {
    const ALL: [Self; 7] = [
        Self::ConstantTrack,
        Self::NearConstant,
        Self::SingleObservation,
        Self::Autocorrelated,
        Self::TailSeller,
        Self::LeverageTiming,
        Self::WindowBoundary,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::ConstantTrack => "constant_track",
            Self::NearConstant => "near_constant",
            Self::SingleObservation => "single_observation",
            Self::Autocorrelated => "autocorrelated",
            Self::TailSeller => "tail_seller",
            Self::LeverageTiming => "leverage_timing",
            Self::WindowBoundary => "window_boundary",
        }
    }

    fn seeded(self) -> bool {
        !matches!(self, Self::ConstantTrack)
    }

    /// Grid sizes after the two layout dimensions.
    fn grids(self) -> Vec<usize> {
        let tail: &[usize] = match self {
            Self::ConstantTrack => &[CONSTANT_LEVELS.len(), 2],
            Self::NearConstant => &[LEVELS.len(), JITTERS.len(), 3],
            Self::SingleObservation => &[LEVELS.len(), SPIKES.len(), 2, 2],
            Self::Autocorrelated => &[PHIS.len(), SIGMAS.len(), 3],
            Self::TailSeller => &[PREMIUMS.len(), CRASH_RATES.len(), NOISE_RATIOS.len()],
            Self::LeverageTiming => &[SIGMAS.len(), 3, CAPS.len()],
            Self::WindowBoundary => &[SIGMAS.len(), 3, 2],
        };
        let mut grids = vec![WINDOWS.len(), BARS.len()];
        grids.extend_from_slice(tail);
        grids
    }

    fn build(self, theta: &[usize], noise_seed: u64) -> Option<Track> {
        let (w, n) = (WINDOWS[theta[0]], BARS[theta[1]]);
        let p = &theta[2..];
        let mut rng = Rng::new(noise_seed);
        match self {
            Self::ConstantTrack => constant_track(w, n, CONSTANT_LEVELS[p[0]], p[1] == 1),
            Self::NearConstant => {
                near_constant(w, n, LEVELS[p[0]], JITTERS[p[1]], p[2], &mut rng).shaped()
            }
            Self::SingleObservation => single_observation(
                w,
                n,
                LEVELS[p[0]],
                SPIKES[p[1]],
                if p[2] == 0 { 1.0 } else { -1.0 },
                p[3] == 1,
                &mut rng,
            )
            .shaped(),
            Self::Autocorrelated => Some(autocorrelated(
                w,
                n,
                PHIS[p[0]],
                SIGMAS[p[1]],
                p[2],
                &mut rng,
            )),
            Self::TailSeller => Some(tail_seller(
                w,
                n,
                PREMIUMS[p[0]],
                CRASH_RATES[p[1]],
                NOISE_RATIOS[p[2]],
                &mut rng,
            )),
            Self::LeverageTiming => Some(leverage_timing(
                w,
                n,
                SIGMAS[p[0]],
                p[1],
                CAPS[p[2]],
                &mut rng,
            )),
            Self::WindowBoundary => window_boundary(w, n, SIGMAS[p[0]], p[1], p[2] == 1, &mut rng),
        }
    }
}

fn constant_track(w: usize, n: usize, level: f64, per_window: bool) -> Option<Track> {
    let windows = (0..w)
        .map(|i| {
            let value = if per_window {
                level * (1.0 + i as f64)
            } else {
                level
            };
            vec![value; n]
        })
        .collect();
    let variant = if per_window {
        "per_window_level"
    } else {
        "one_level"
    };
    Track::new(variant, format!("level={level}"), windows).shaped()
}

fn near_constant(w: usize, n: usize, level: f64, eps: f64, shape: usize, rng: &mut Rng) -> Track {
    let windows = (0..w)
        .map(|_| {
            (0..n)
                .map(|t| {
                    level
                        + match shape {
                            0 => eps * (2.0 * rng.unit() - 1.0),
                            1 => {
                                if t.is_multiple_of(2) {
                                    eps
                                } else {
                                    -eps
                                }
                            }
                            _ => {
                                if t == 0 {
                                    eps
                                } else {
                                    0.0
                                }
                            }
                        }
                })
                .collect()
        })
        .collect();
    let variant = ["uniform_jitter", "alternating_jitter", "one_bar_jitter"][shape];
    Track::new(variant, format!("level={level} eps={eps}"), windows)
}

fn single_observation(
    w: usize,
    n: usize,
    level: f64,
    spike: f64,
    sign: f64,
    every_window: bool,
    rng: &mut Rng,
) -> Track {
    let windows = (0..w)
        .map(|i| {
            let mut window = vec![level; n];
            if every_window || i == 0 {
                window[rng.below(n)] = level + sign * spike;
            }
            window
        })
        .collect();
    let variant = if every_window {
        "every_window"
    } else {
        "first_window_only"
    };
    Track::new(
        variant,
        format!("level={level} spike={}", sign * spike),
        windows,
    )
}

fn split(pooled: &[f64], n: usize) -> Vec<Vec<f64>> {
    pooled.chunks(n).map(<[f64]>::to_vec).collect()
}

fn autocorrelated(w: usize, n: usize, phi: f64, sigma: f64, kind: usize, rng: &mut Rng) -> Track {
    let total = w * n;
    let innovation = sigma * (1.0 - phi * phi).sqrt();
    let (variant, pooled): (&'static str, Vec<f64>) = match kind {
        0 | 1 => {
            let mut x = sigma * rng.gauss();
            let mut out = Vec::with_capacity(total);
            for t in 0..total {
                if kind == 1 && t % n == 0 {
                    x = sigma * rng.gauss();
                }
                x = phi * x + innovation * rng.gauss();
                out.push(x);
            }
            (
                if kind == 0 {
                    "ar1_continuous"
                } else {
                    "ar1_restarted"
                },
                out,
            )
        }
        _ => {
            // Return smoothing: a moving average of an iid zero-mean stream, the
            // shape of an appraised or marked-to-model book.
            let span = 2 + (phi * 40.0) as usize;
            let raw: Vec<f64> = (0..total + span).map(|_| sigma * rng.gauss()).collect();
            let out = (0..total)
                .map(|t| raw[t..t + span].iter().sum::<f64>() / span as f64)
                .collect();
            ("smoothed", out)
        }
    };
    Track::new(
        variant,
        format!("phi={phi} sigma={sigma}"),
        split(&pooled, n),
    )
}

fn tail_seller(
    w: usize,
    n: usize,
    premium: f64,
    crash_rate: f64,
    noise_ratio: f64,
    rng: &mut Rng,
) -> Track {
    // The crash is sized so that the expected return of every bar is exactly zero.
    let crash = -premium * (1.0 - crash_rate) / crash_rate;
    let sigma = premium * noise_ratio;
    let pooled: Vec<f64> = (0..w * n)
        .map(|_| {
            let carry = if rng.unit() < crash_rate {
                crash
            } else {
                premium
            };
            carry + sigma * rng.gauss()
        })
        .collect();
    // A sample that never drew the crash is a positive-drift track on its face,
    // and no scorer of that sample can see otherwise. The variant keeps that case
    // apart from an admission over a crash the track actually contains.
    let crashes = pooled.iter().filter(|x| **x < crash / 2.0).count();
    let variant = if crashes == 0 {
        "crash_unseen_in_sample"
    } else {
        "crash_in_sample"
    };
    Track::new(
        variant,
        format!(
            "premium={premium} crash_rate={crash_rate} noise_ratio={noise_ratio} crashes={crashes}"
        ),
        split(&pooled, n),
    )
}

fn leverage_timing(w: usize, n: usize, sigma: f64, kind: usize, cap: f64, rng: &mut Rng) -> Track {
    let variant = ["martingale_doubling", "stop_when_ahead", "pyramiding"][kind];
    let windows = (0..w)
        .map(|_| {
            let mut stake = 1.0_f64;
            let mut ahead = 0.0_f64;
            let mut stopped = false;
            (0..n)
                .map(|_| {
                    if stopped {
                        return 0.0;
                    }
                    // A fair coin, and a stake chosen from the past only, so the
                    // conditional expected return of the bar is zero.
                    let coin = rng.coin();
                    let r = sigma * stake * coin;
                    ahead += r;
                    match kind {
                        0 => {
                            stake = if coin > 0.0 {
                                1.0
                            } else {
                                (2.0 * stake).min(cap)
                            }
                        }
                        1 => stopped = ahead > 0.0,
                        _ => {
                            stake = if coin > 0.0 {
                                (2.0 * stake).min(cap)
                            } else {
                                1.0
                            }
                        }
                    }
                    r
                })
                .collect()
        })
        .collect();
    Track::new(variant, format!("sigma={sigma} cap={cap}"), windows)
}

fn window_boundary(
    w: usize,
    n: usize,
    size: f64,
    kind: usize,
    any_mode: bool,
    rng: &mut Rng,
) -> Option<Track> {
    let windows: Vec<Vec<f64>> = (0..w)
        .map(|i| match kind {
            0 => {
                let level = size * (1.0 + (i % 3) as f64);
                (0..n)
                    .map(|_| level + 1e-12 * (2.0 * rng.unit() - 1.0))
                    .collect()
            }
            1 => {
                let mut window = vec![0.0; n];
                window[n - 1] = size * rng.coin();
                window
            }
            _ => {
                let mut window = vec![0.0; n];
                for x in &mut window[n.saturating_sub(3)..] {
                    *x = size * rng.gauss();
                }
                window
            }
        })
        .collect();
    let variant = ["level_step_jitter", "last_bar_coin", "last_bars_noise"][kind];
    let mut track = Track::new(variant, format!("size={size}"), windows);
    if any_mode {
        track.pass_mode = PassMode::Any;
    }
    if kind == 0 {
        track.shaped()
    } else {
        Some(track)
    }
}

fn agent(id: &str, windows: &[Vec<f64>]) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.to_string(),
        runs: windows
            .iter()
            .map(|returns| Run {
                returns: returns.clone(),
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn config(pass_mode: PassMode, n_boot: usize) -> ScoreConfig {
    ScoreConfig {
        pass_mode,
        n_boot,
        ..ScoreConfig::default()
    }
}

fn row<'a>(board: &'a [CompositeScore], id: &str) -> &'a CompositeScore {
    board
        .iter()
        .find(|r| r.agent_id == id)
        .expect("every agent is on the board")
}

/// Distance to the gates, in [0, 1]; an eligible row scores 1. It steers the
/// search and decides nothing.
fn fitness(score: &CompositeScore, sub: &AgentSubmission, cfg: &ScoreConfig) -> f64 {
    if score.rank_eligible {
        return 1.0;
    }
    let passes = per_run_passes(sub, None, cfg);
    let passed = passes.iter().filter(|p| **p).count() as f64 / passes.len().max(1) as f64;
    let deflated = (score.deflated_sharpe / cfg.dsr_bar).clamp(0.0, 1.0);
    let bootstrap = if score.bootstrap_p > 0.0 {
        (cfg.alpha / score.bootstrap_p).min(1.0)
    } else {
        1.0
    };
    0.98 * (passed + deflated + bootstrap) / 3.0
}

/// One entrant through `rank`, exactly as `sharpebench score` scores a field of
/// one, under the track's verdict.
fn score_alone(windows: &[Vec<f64>], pass_mode: PassMode, n_boot: usize) -> (CompositeScore, f64) {
    let cfg = config(pass_mode, n_boot);
    let sub = agent("candidate", windows);
    let board = rank(std::slice::from_ref(&sub), &cfg);
    let score = row(&board, "candidate").clone();
    let fit = fitness(&score, &sub, &cfg);
    (score, fit)
}

/// Zero-mean, unit-variance noise of length `n`.
fn standardized(rng: &mut Rng, n: usize) -> Vec<f64> {
    let raw: Vec<f64> = (0..n).map(|_| rng.gauss()).collect();
    let centre = mean(&raw);
    let centred: Vec<f64> = raw.iter().map(|x| x - centre).collect();
    let sd = std_dev(&centred);
    centred.iter().map(|x| x / sd).collect()
}

/// A pooled track whose per-period Sharpe is `sharpe` to rounding.
fn sharpe_track(rng: &mut Rng, n: usize, sharpe: f64, scale: f64) -> Vec<f64> {
    standardized(rng, n)
        .iter()
        .map(|z| scale * (sharpe + z))
        .collect()
}

/// Reference entrants of no particular quality, the same for every candidate on
/// a layout: the field a submission meets in the field-context confirmation.
fn null_entrants(windows: usize, bars: usize, count: usize) -> Vec<AgentSubmission> {
    (0..count)
        .map(|k| {
            let mut rng = Rng::new(0x5EED_0000 + k as u64);
            let pooled: Vec<f64> = (0..windows * bars).map(|_| 0.01 * rng.gauss()).collect();
            agent(&format!("reference-{k}"), &split(&pooled, bars))
        })
        .collect()
}

fn admitted_in_field(track: &Track, entrants: usize) -> bool {
    let mut field = null_entrants(track.windows_count(), track.bars(), entrants);
    field.push(agent("candidate", &track.windows));
    let board = rank(
        &field,
        &config(track.pass_mode, ScoreConfig::default().n_boot),
    );
    row(&board, "candidate").rank_eligible
}

fn parallel_map<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let threads = threads.clamp(1, items.len().max(1));
    let chunk = items.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let work = &work;
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|slice| scope.spawn(move || slice.iter().map(work).collect::<Vec<R>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a search worker does not panic"))
            .collect()
    })
}

fn hash_mix(state: u64, value: u64) -> u64 {
    value.to_le_bytes().iter().fold(state, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01B3)
    })
}

const HASH_START: u64 = 0xCBF2_9CE4_8422_2325;

fn hash_text(state: u64, text: &str) -> u64 {
    text.bytes().fold(state, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3)
    })
}

fn hash_series(state: u64, windows: &[Vec<f64>]) -> u64 {
    windows
        .iter()
        .flatten()
        .fold(state, |h, x| hash_mix(h, x.to_bits()))
}

/// Wilson score interval for `k` of `n`.
fn wilson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let (k, n) = (k as f64, n as f64);
    let p = k / n;
    let z2 = WILSON_Z * WILSON_Z;
    let denominator = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / denominator;
    let half = WILSON_Z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denominator;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

#[derive(Clone, Debug)]
pub struct SearchSpec {
    pub seed: u64,
    pub budget: usize,
    pub replicates: usize,
    pub max_findings: usize,
    pub threads: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Deterministic, or admitted on held-out seeds above the floor and the control.
    Structural,
    /// Admitted on the search's own seed and not shown to replicate.
    SelectionLuck,
}

#[derive(Clone, Debug, Serialize)]
pub struct Replication {
    pub seeds: usize,
    pub admitted: usize,
    pub rate_low: f64,
    pub rate_high: f64,
    /// The iid Gaussian null on the same layout and verdict.
    pub control_admitted: usize,
    pub control_rate_high: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Gates {
    pub deflated_sharpe: f64,
    pub deflation_bar_per_period: f64,
    pub psr: f64,
    pub passed_k: bool,
    pub bootstrap_p: f64,
    pub pooled_sharpe: Option<f64>,
    pub raw_mean_return: f64,
}

impl Gates {
    fn of(score: &CompositeScore, windows: &[Vec<f64>]) -> Self {
        Self {
            deflated_sharpe: score.deflated_sharpe,
            deflation_bar_per_period: score.deflation_bar_per_period,
            psr: score.psr,
            passed_k: score.passed_k,
            bootstrap_p: score.bootstrap_p,
            pooled_sharpe: observed_sharpe_ratio(&windows.concat()).ok(),
            raw_mean_return: score.raw_mean_return,
        }
    }
}

/// An admitted submission, confirmed under the default configuration.
#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub id: String,
    #[serde(rename = "move")]
    pub search_move: String,
    pub variant: String,
    pub params: String,
    pub no_skill_basis: Basis,
    pub verdict: Verdict,
    pub windows: usize,
    pub bars: usize,
    /// Argument for `sharpebench score --pass-mode`.
    pub pass_mode: String,
    pub noise_seed: u64,
    pub gates: Gates,
    /// Admitted as a field of one, and in fields of 7 and 13 agents. Absent for
    /// a finding whose reproducer is itself a field.
    pub admitted_alone: Option<bool>,
    pub admitted_field_7: Option<bool>,
    pub admitted_field_13: Option<bool>,
    /// The trials this move spent, and whether the track still clears when it
    /// declares them. An undeclared search is what `in_sample_trials` is for.
    pub search_trials: u32,
    pub admitted_with_trials_declared: Option<bool>,
    pub replication: Option<Replication>,
    /// The submissions that reproduce it through `sharpebench score`.
    pub reproducer: Vec<AgentSubmission>,
    /// The candidate's agent id inside `reproducer`.
    pub candidate_id: String,
    /// The candidate's returns as `f64::to_bits` in hexadecimal, window by
    /// window, for a reader whose JSON parser does not round-trip.
    pub return_bits: Vec<Vec<String>>,
}

/// One vote moving the bar for entrants who did nothing. Reported apart from
/// admissions: it costs others eligibility and buys the griefer none.
#[derive(Clone, Debug, Serialize)]
pub struct Demotion {
    pub direction: String,
    /// Position against the fence: below 1 votes, above 1 is fenced.
    pub fence_fraction: f64,
    pub griefer_vote: f64,
    pub bar_before: f64,
    pub bar_after: f64,
    pub honest_eligible_before: usize,
    pub honest_eligible_after: usize,
    pub griefer_eligible: bool,
    pub griefer_fenced: bool,
    pub reproducer: Vec<AgentSubmission>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MoveReport {
    pub name: String,
    pub evaluated: usize,
    pub admitted_on_screen: usize,
    pub distinct_admitting_params: usize,
    pub structural_findings: usize,
    pub best_fitness: f64,
    pub best_params: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Control {
    pub skilled_fixture_admitted: bool,
    /// Every iid Gaussian null on five 60-bar windows was refused.
    pub iid_null_refused: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraderSearchReport {
    pub schema_version: &'static str,
    /// Always false. No gate, board or self-audit reads this report.
    pub rank_input: bool,
    pub seed: u64,
    pub budget_per_move: usize,
    pub replicates: usize,
    pub max_findings_per_move: usize,
    pub structural_rate_floor: f64,
    pub degenerate_motion: f64,
    pub control: Control,
    pub moves: Vec<MoveReport>,
    pub findings: Vec<Finding>,
    pub demotions: Vec<Demotion>,
    pub structural_findings: usize,
    /// Over every series and verdict reported, not over the thread count.
    pub digest: String,
    /// What this search cannot see.
    pub limits: Vec<&'static str>,
}

const LIMITS: [&str; 6] = [
    "A track with a real positive mean is outside the oracle: on returns alone it is indistinguishable from skill, so a fabricated series is a provenance question for intake, not for this search.",
    "Moves are generators a person wrote. No finding means none on these moves, grids and seeds under the default configuration, not that none exists.",
    "A structural verdict is a Wilson bound on the held-out seeds drawn here, at one layout. It is not an admission rate for any live field.",
    "Field contexts use reference entrants of no skill. A field of other strengths moves the bar, within the fence.",
    "Clone chains that bridge honest entrants through single linkage are not searched.",
    "Held-out rates are not corrected for the several parameter sets a move reports, and a crash_unseen_in_sample tail seller is a track no scorer of that sample could refuse.",
];

fn screen_config_mode(track: &Track) -> &'static str {
    match track.pass_mode {
        PassMode::Any => "any",
        _ => "all",
    }
}

fn bits_of(windows: &[Vec<f64>]) -> Vec<Vec<String>> {
    windows
        .iter()
        .map(|w| w.iter().map(|x| format!("{:016x}", x.to_bits())).collect())
        .collect()
}

fn random_theta(grids: &[usize], rng: &mut Rng) -> Vec<usize> {
    grids.iter().map(|&g| rng.below(g)).collect()
}

fn mutate(theta: &[usize], grids: &[usize], rng: &mut Rng) -> Vec<usize> {
    let mut next = theta.to_vec();
    let axis = rng.below(grids.len());
    if grids[axis] > 1 {
        let step = if rng.coin() > 0.0 { 1 } else { grids[axis] - 1 };
        next[axis] = (next[axis] + step) % grids[axis];
    }
    next
}

struct Evaluated {
    theta: Vec<usize>,
    noise_seed: u64,
    variant: &'static str,
    fit: f64,
    eligible: bool,
    params: String,
}

fn evaluate_batch(mv: Move, batch: &[(Vec<usize>, u64)], spec: &SearchSpec) -> Vec<Evaluated> {
    parallel_map(batch, spec.threads, |(theta, noise_seed)| {
        let track = mv.build(theta, *noise_seed)?;
        let (score, fit) = score_alone(&track.windows, track.pass_mode, SCREEN_N_BOOT);
        Some(Evaluated {
            theta: theta.clone(),
            noise_seed: *noise_seed,
            variant: track.variant,
            fit,
            eligible: score.rank_eligible,
            params: format!(
                "{} windows={} bars={} {}",
                track.variant,
                track.windows_count(),
                track.bars(),
                track.params
            ),
        })
    })
    .into_iter()
    .flatten()
    .collect()
}

/// Every evaluated point of one move, in a deterministic order.
fn explore(mv: Move, spec: &SearchSpec) -> (Vec<Evaluated>, usize) {
    let grids = mv.grids();
    let mut rng = Rng::new(hash_text(spec.seed ^ HASH_START, mv.name()));
    let mut seen: BTreeSet<(Vec<usize>, u64)> = BTreeSet::new();
    let mut all: Vec<Evaluated> = Vec::new();
    let per_round = spec.budget.div_ceil(ROUNDS).max(1);
    if !mv.seeded() {
        // A move that reads no noise has a finite grid, so it is walked whole and
        // its count is the same at every budget.
        let mut batch: Vec<(Vec<usize>, u64)> = vec![(Vec::new(), 0)];
        for &size in &grids {
            batch = batch
                .iter()
                .flat_map(|(theta, _)| {
                    (0..size).map(|v| {
                        let mut next = theta.clone();
                        next.push(v);
                        (next, 0)
                    })
                })
                .collect();
        }
        let points = evaluate_batch(mv, &batch, spec);
        let evaluated = points.len();
        return (points, evaluated);
    }
    for round in 0..ROUNDS {
        let mut elites: Vec<&Evaluated> = all.iter().collect();
        elites.sort_by(|a, b| {
            b.fit
                .total_cmp(&a.fit)
                .then_with(|| a.theta.cmp(&b.theta))
                .then_with(|| a.noise_seed.cmp(&b.noise_seed))
        });
        elites.truncate(ELITES);
        let elite_thetas: Vec<Vec<usize>> = elites.iter().map(|e| e.theta.clone()).collect();
        let mut batch: Vec<(Vec<usize>, u64)> = Vec::new();
        let mut attempts = 0usize;
        while batch.len() < per_round && attempts < per_round * 8 {
            attempts += 1;
            let theta = if round > 0 && !elite_thetas.is_empty() && attempts.is_multiple_of(2) {
                let parent = &elite_thetas[rng.below(elite_thetas.len())];
                mutate(parent, &grids, &mut rng)
            } else {
                random_theta(&grids, &mut rng)
            };
            let noise_seed = if mv.seeded() { rng.next_u64() } else { 0 };
            if seen.insert((theta.clone(), noise_seed)) {
                batch.push((theta, noise_seed));
            }
        }
        all.extend(evaluate_batch(mv, &batch, spec));
    }
    let evaluated = all.len();
    (all, evaluated)
}

fn replicate(
    mv: Move,
    theta: &[usize],
    pass_mode: PassMode,
    spec: &SearchSpec,
    layout: (usize, usize),
) -> Replication {
    let domain = hash_text(spec.seed ^ 0x4845_4C44, mv.name());
    let domain = theta.iter().fold(domain, |h, t| hash_mix(h, *t as u64));
    let seeds: Vec<u64> = (0..spec.replicates as u64)
        .map(|i| Rng::new(hash_mix(domain, i)).next_u64())
        .collect();
    let n_boot = ScoreConfig::default().n_boot;
    let admitted: usize = parallel_map(&seeds, spec.threads, |s| {
        mv.build(theta, *s)
            .is_some_and(|t| score_alone(&t.windows, pass_mode, n_boot).0.rank_eligible)
    })
    .into_iter()
    .filter(|a| *a)
    .count();
    let (windows, bars) = layout;
    let control_admitted: usize = parallel_map(&seeds, spec.threads, |s| {
        let mut rng = Rng::new(*s ^ 0xC017_201C);
        let pooled: Vec<f64> = (0..windows * bars).map(|_| 0.01 * rng.gauss()).collect();
        score_alone(&split(&pooled, bars), pass_mode, n_boot)
            .0
            .rank_eligible
    })
    .into_iter()
    .filter(|a| *a)
    .count();
    let (rate_low, rate_high) = wilson(admitted, seeds.len());
    let (_, control_rate_high) = wilson(control_admitted, seeds.len());
    Replication {
        seeds: seeds.len(),
        admitted,
        rate_low,
        rate_high,
        control_admitted,
        control_rate_high,
    }
}

fn is_structural(replication: Option<&Replication>) -> bool {
    replication
        .is_none_or(|r| r.rate_low >= STRUCTURAL_RATE_FLOOR && r.rate_low > r.control_rate_high)
}

/// Confirm one admitted point under the default configuration and turn it into
/// a finding. `None` when only the cheap screen admitted it.
fn confirm(mv: Move, point: &Evaluated, trials: usize, spec: &SearchSpec) -> Option<Finding> {
    let track = mv.build(&point.theta, point.noise_seed)?;
    let n_boot = ScoreConfig::default().n_boot;
    let (score, _) = score_alone(&track.windows, track.pass_mode, n_boot);
    if !score.rank_eligible {
        return None;
    }
    let search_trials = u32::try_from(trials).unwrap_or(u32::MAX);
    let declared = {
        let mut sub = agent("candidate", &track.windows);
        sub.in_sample_trials = search_trials;
        let board = rank(std::slice::from_ref(&sub), &config(track.pass_mode, n_boot));
        row(&board, "candidate").rank_eligible
    };
    let replication = mv.seeded().then(|| {
        replicate(
            mv,
            &point.theta,
            track.pass_mode,
            spec,
            (track.windows_count(), track.bars()),
        )
    });
    let verdict = if is_structural(replication.as_ref()) {
        Verdict::Structural
    } else {
        Verdict::SelectionLuck
    };
    let id_hash = hash_series(hash_text(HASH_START, &point.params), &track.windows);
    Some(Finding {
        id: format!("{}-{id_hash:016x}", mv.name()),
        search_move: mv.name().to_string(),
        variant: track.variant.to_string(),
        params: track.params.clone(),
        no_skill_basis: track.basis,
        verdict,
        windows: track.windows_count(),
        bars: track.bars(),
        pass_mode: screen_config_mode(&track).to_string(),
        noise_seed: point.noise_seed,
        gates: Gates::of(&score, &track.windows),
        admitted_alone: Some(true),
        admitted_field_7: Some(admitted_in_field(&track, 6)),
        admitted_field_13: Some(admitted_in_field(&track, 12)),
        search_trials,
        admitted_with_trials_declared: Some(declared),
        replication,
        reproducer: vec![agent("candidate", &track.windows)],
        candidate_id: "candidate".to_string(),
        return_bits: bits_of(&track.windows),
    })
}

fn search_move(mv: Move, spec: &SearchSpec) -> (MoveReport, Vec<Finding>) {
    let (points, evaluated) = explore(mv, spec);
    let best = points.iter().max_by(|a, b| {
        a.fit
            .total_cmp(&b.fit)
            .then_with(|| b.theta.cmp(&a.theta))
            .then_with(|| b.noise_seed.cmp(&a.noise_seed))
    });
    let mut admitting: Vec<&Evaluated> = points.iter().filter(|p| p.eligible).collect();
    let distinct: BTreeSet<&Vec<usize>> = admitting.iter().map(|p| &p.theta).collect();
    let distinct_admitting_params = distinct.len();

    // One representative of each parameter set and variant, spread evenly over the admitting
    // sets so a cap keeps the breadth of what was found rather than its first
    // corner. The count of everything admitted is on the move's row.
    admitting.sort_by(|a, b| {
        a.variant
            .cmp(b.variant)
            .then_with(|| a.theta.cmp(&b.theta))
            .then_with(|| a.noise_seed.cmp(&b.noise_seed))
    });
    let mut representatives: Vec<&Evaluated> = Vec::new();
    let mut last: Option<(&str, &Vec<usize>)> = None;
    for point in &admitting {
        if last != Some((point.variant, &point.theta)) {
            representatives.push(point);
            last = Some((point.variant, &point.theta));
        }
    }
    let cap = spec.max_findings.max(1);
    let mut findings: Vec<Finding> = Vec::new();
    for group in representatives.chunk_by(|a, b| a.variant == b.variant) {
        let stride = group.len().div_ceil(cap).max(1);
        findings.extend(
            group
                .iter()
                .step_by(stride)
                .filter_map(|point| confirm(mv, point, evaluated, spec)),
        );
    }

    let report = MoveReport {
        name: mv.name().to_string(),
        evaluated,
        admitted_on_screen: admitting.len(),
        distinct_admitting_params,
        structural_findings: findings
            .iter()
            .filter(|f| f.verdict == Verdict::Structural)
            .count(),
        best_fitness: best.map_or(0.0, |p| p.fit),
        best_params: best.map_or_else(String::new, |p| p.params.clone()),
    };
    (report, findings)
}

/// A dispersed reference field: Sharpes spread enough that the measured
/// dispersion sits above the floor, on tracks that are nobody's clone.
const FIXTURE_SHARPES: [f64; 6] = [0.40, 0.45, 0.50, 0.55, 0.60, 0.65];

fn fixture_field(windows: usize, bars: usize) -> Vec<AgentSubmission> {
    FIXTURE_SHARPES
        .iter()
        .enumerate()
        .map(|(k, &sharpe)| {
            let mut rng = Rng::new(0xF1E1_D000 + k as u64);
            let pooled = sharpe_track(&mut rng, windows * bars, sharpe, 0.01);
            agent(&format!("fixture-{k}"), &split(&pooled, bars))
        })
        .collect()
}

const CLONE_COSINES: [f64; 7] = [0.90, 0.97, 0.99, 0.994, 0.9949, 0.9951, 0.999];
/// Copies, which the collapse folds to one vote: the control for the rate.
const CLONE_CONTROL_COSINE: f64 = 0.9999;
const CLONE_TARGETS: usize = 32;
/// Draws spent finding targets, at least. A target sits in a narrow band of
/// Sharpes, so a small budget would otherwise leave this move with nothing to lift.
const CLONE_MIN_DRAWS: usize = 3_000;
const CLONE_COUNTS: [usize; 4] = [4, 8, 16, 32];
const CLONE_BARS: usize = 120;

/// `count` puppets at pairwise uncentered cosine about `pair_cosine`, around a
/// base at the field's median Sharpe, so their votes shrink the dispersion
/// instead of widening it.
fn puppets(count: usize, pair_cosine: f64, rng: &mut Rng) -> Vec<Vec<f64>> {
    let base = sharpe_track(rng, CLONE_BARS, 0.525, 0.01);
    let base_norm2: f64 = base.iter().map(|x| x * x).sum();
    let ratio = (1.0 / pair_cosine - 1.0).sqrt();
    (0..count)
        .map(|_| {
            let noise: Vec<f64> = (0..CLONE_BARS).map(|_| rng.gauss()).collect();
            let along = noise.iter().zip(&base).map(|(a, b)| a * b).sum::<f64>() / base_norm2;
            let orthogonal: Vec<f64> = noise
                .iter()
                .zip(&base)
                .map(|(a, b)| a - along * b)
                .collect();
            let norm = orthogonal.iter().map(|x| x * x).sum::<f64>().sqrt();
            let scale = ratio * base_norm2.sqrt() / norm;
            base.iter()
                .zip(&orthogonal)
                .map(|(b, o)| b + scale * o)
                .collect()
        })
        .collect()
}

fn largest_pair_cosine(tracks: &[Vec<f64>]) -> f64 {
    let mut largest = 0.0_f64;
    for (i, a) in tracks.iter().enumerate() {
        for b in &tracks[i + 1..] {
            if let Some(c) = cosine_similarity(a, b, false) {
                largest = largest.max(c.abs());
            }
        }
    }
    largest
}

/// Clones near the collapse threshold: can a target the dispersed field refuses
/// be admitted by puppets that sit just below the cosine that would fold them?
fn search_clones(spec: &SearchSpec) -> (MoveReport, Vec<Finding>) {
    let mut rng = Rng::new(hash_text(spec.seed ^ HASH_START, "clone_threshold"));
    let screen = config(PassMode::All, SCREEN_N_BOOT);
    let fixtures = fixture_field(1, CLONE_BARS);
    // Targets are zero-edge draws. Only the ones a field of one admits can be
    // lifted, so they are found first and the puppets are spent on those.
    let seeds: Vec<u64> = (0..spec.budget.max(CLONE_MIN_DRAWS))
        .map(|_| rng.next_u64())
        .collect();
    let targets: Vec<(u64, Vec<f64>)> = parallel_map(&seeds, spec.threads, |s| {
        let mut draw = Rng::new(*s);
        let pooled: Vec<f64> = (0..CLONE_BARS).map(|_| 0.01 * draw.gauss()).collect();
        let eligible = score_alone(std::slice::from_ref(&pooled), PassMode::All, SCREEN_N_BOOT)
            .0
            .rank_eligible;
        eligible.then_some((*s, pooled))
    })
    .into_iter()
    .flatten()
    .collect();
    let refused: Vec<(u64, Vec<f64>)> = parallel_map(&targets, spec.threads, |(s, pooled)| {
        let mut field = fixtures.clone();
        field.push(agent("candidate", std::slice::from_ref(pooled)));
        (!row(&rank(&field, &screen), "candidate").rank_eligible).then(|| (*s, pooled.clone()))
    })
    .into_iter()
    .flatten()
    .collect();

    let cosines: Vec<f64> = CLONE_COSINES
        .iter()
        .copied()
        .chain([CLONE_CONTROL_COSINE])
        .collect();
    let mut grid: Vec<(usize, usize, usize, u64)> = Vec::new();
    let targets_tried = refused
        .len()
        .min(CLONE_TARGETS)
        .min(spec.budget.div_ceil(10));
    for t in 0..targets_tried {
        for c in 0..cosines.len() {
            for m in 0..CLONE_COUNTS.len() {
                grid.push((t, c, m, rng.next_u64()));
            }
        }
    }
    let evaluated = grid.len() + seeds.len();
    let attacked = |target: &[f64], s: u64, c: usize, m: usize| -> (Vec<AgentSubmission>, f64) {
        let mut draw = Rng::new(s);
        let flood = puppets(CLONE_COUNTS[m], cosines[c], &mut draw);
        let largest = largest_pair_cosine(&flood);
        let mut field = fixtures.clone();
        field.push(agent("candidate", &[target.to_vec()]));
        for (k, track) in flood.iter().enumerate() {
            field.push(agent(&format!("puppet-{k}"), std::slice::from_ref(track)));
        }
        (field, largest)
    };
    let flips: Vec<(usize, usize, usize, u64)> =
        parallel_map(&grid, spec.threads, |&(t, c, m, s)| {
            let (field, _) = attacked(&refused[t].1, s, c, m);
            row(&rank(&field, &screen), "candidate")
                .rank_eligible
                .then_some((t, c, m, s))
        })
        .into_iter()
        .flatten()
        .collect();
    let flipped = |c: usize, m: usize| flips.iter().filter(|f| f.1 == c && f.2 == m).count();

    // Only puppets the collapse can miss are findings. The control column is the
    // same flood as copies, which the collapse folds to one vote.
    let control_index = cosines.len() - 1;
    let mut cells: Vec<(usize, usize, usize, u64)> = Vec::new();
    for c in 0..CLONE_COSINES.len() {
        for m in 0..CLONE_COUNTS.len() {
            if let Some(first) = flips.iter().find(|f| f.1 == c && f.2 == m) {
                cells.push(*first);
            }
        }
    }
    // The cells that lift the most targets are the ones worth keeping.
    cells.sort_by_key(|&(_, c, m, _)| (std::cmp::Reverse(flipped(c, m)), c, m));
    let cap = spec.max_findings.max(1);
    let n_boot = ScoreConfig::default().n_boot;
    let clean = config(PassMode::All, n_boot);
    let findings: Vec<Finding> = cells
        .iter()
        .take(cap)
        .filter_map(|&(t, c, m, s)| {
            let (noise_seed, target) = &refused[t];
            let windows = vec![target.clone()];
            let mut alone_field = fixtures.clone();
            alone_field.push(agent("candidate", &windows));
            let clean_board = rank(&alone_field, &clean);
            let clean_row = row(&clean_board, "candidate");
            if clean_row.rank_eligible {
                return None;
            }
            let clean_bar = clean_row.deflation_bar_per_period;
            let (field, largest) = attacked(target, s, c, m);
            let board = rank(&field, &clean);
            let score = row(&board, "candidate");
            if !score.rank_eligible {
                return None;
            }
            let params = format!(
                "pair_cosine={} puppets={} largest_pair_cosine={largest:.6} collapse_at={CLONE_COLLAPSE_COSINE} bar_without_puppets={clean_bar:.4} bar_with_puppets={:.4}",
                cosines[c], CLONE_COUNTS[m], score.deflation_bar_per_period
            );
            let (rate_low, rate_high) = wilson(flipped(c, m), targets_tried);
            let control_admitted = flipped(control_index, m);
            let replication = Replication {
                seeds: targets_tried,
                admitted: flipped(c, m),
                rate_low,
                rate_high,
                control_admitted,
                control_rate_high: wilson(control_admitted, targets_tried).1,
            };
            let verdict = if is_structural(Some(&replication)) {
                Verdict::Structural
            } else {
                Verdict::SelectionLuck
            };
            let id_hash = hash_series(hash_text(HASH_START, &params), &windows);
            Some(Finding {
                id: format!("clone_threshold-{id_hash:016x}"),
                search_move: "clone_threshold".to_string(),
                variant: "dispersion_dilution".to_string(),
                params,
                no_skill_basis: Basis::ZeroEdge,
                verdict,
                windows: 1,
                bars: CLONE_BARS,
                pass_mode: "all".to_string(),
                noise_seed: *noise_seed,
                gates: Gates::of(score, &windows),
                admitted_alone: None,
                admitted_field_7: None,
                admitted_field_13: None,
                search_trials: u32::try_from(evaluated).unwrap_or(u32::MAX),
                admitted_with_trials_declared: None,
                replication: Some(replication),
                reproducer: field,
                candidate_id: "candidate".to_string(),
                return_bits: bits_of(&windows),
            })
        })
        .collect();
    let report = MoveReport {
        name: "clone_threshold".to_string(),
        evaluated,
        admitted_on_screen: flips.len(),
        distinct_admitting_params: cells.len(),
        structural_findings: findings
            .iter()
            .filter(|f| f.verdict == Verdict::Structural)
            .count(),
        best_fitness: if flips.is_empty() { 0.0 } else { 1.0 },
        best_params: format!(
            "{} zero-edge targets admitted alone, {} of them refused by the dispersed field",
            targets.len(),
            refused.len()
        ),
    };
    (report, findings)
}

const FENCE_FRACTIONS: [f64; 6] = [0.5, 0.8, 0.95, 0.995, 1.005, 1.1];
const GRIEF_WINDOWS: usize = 2;
const GRIEF_BARS: usize = 60;

fn pooled_vote(sub: &AgentSubmission) -> f64 {
    let pooled: Vec<f64> = sub.runs.iter().flat_map(|r| r.returns.clone()).collect();
    observed_sharpe_ratio(&pooled).expect("a reference track varies")
}

/// The kernel's robust scale, restated only to aim the probe. The kernel alone
/// decides whether the vote is fenced; a drifted restatement mis-aims a probe
/// and the row's `griefer_fenced` says so.
fn fence_reach(votes: &[f64], floor: f64) -> (f64, f64) {
    const PAIRWISE_MEDIAN_TO_SIGMA: f64 = 1.048_358_013_869_129;
    let mut gaps = Vec::new();
    for (i, a) in votes.iter().enumerate() {
        for b in &votes[i + 1..] {
            gaps.push((a - b).abs());
        }
    }
    let scale = (PAIRWISE_MEDIAN_TO_SIGMA * median(&gaps)).max(floor);
    (median(votes), DISPERSION_VOTE_FENCE_Z * scale)
}

/// Dispersion griefing near the fence: one vote just inside it, on either side.
/// The griefer needs no return at all on the negative side, only a losing track.
fn search_grief(spec: &SearchSpec) -> (MoveReport, Vec<Demotion>) {
    let cfg = config(PassMode::All, ScoreConfig::default().n_boot);
    let honest = fixture_field(GRIEF_WINDOWS, GRIEF_BARS);
    let ids: Vec<String> = honest.iter().map(|s| s.agent_id.clone()).collect();
    let clean = rank(&honest, &cfg);
    let eligible =
        |board: &[CompositeScore]| ids.iter().filter(|id| row(board, id).rank_eligible).count();
    let before = eligible(&clean);
    let bar_before = row(&clean, &ids[0]).deflation_bar_per_period;
    let floor = cfg.min_measured_trials_sr_std / cfg.periods_per_year.sqrt();
    let votes: Vec<f64> = honest.iter().map(pooled_vote).collect();

    let mut grid = Vec::new();
    for sign in [1.0_f64, -1.0] {
        for &fraction in &FENCE_FRACTIONS {
            grid.push((sign, fraction));
        }
    }
    let evaluated = grid.len();
    let demotions: Vec<Demotion> = parallel_map(&grid, spec.threads, |&(sign, fraction)| {
        // The probe is in the sample that sets the centre and the scale, so the
        // aim is the fixed point of "sit exactly at this fraction of the reach".
        let mut vote = sign;
        for _ in 0..200 {
            let mut all = votes.clone();
            all.push(vote);
            let (centre, reach) = fence_reach(&all, floor);
            let next = centre + sign * fraction * reach;
            if (next - vote).abs() < 1e-12 {
                break;
            }
            vote = next;
        }
        let mut draw = Rng::new(spec.seed ^ 0x6721_4546);
        let pooled = sharpe_track(&mut draw, GRIEF_WINDOWS * GRIEF_BARS, vote, 0.01);
        let griefer = agent("griefer", &split(&pooled, GRIEF_BARS));
        let mut field = honest.clone();
        field.push(griefer);
        let board = rank(&field, &cfg);
        let griefer_row = row(&board, "griefer");
        let honest_row = row(&board, &ids[0]);
        Demotion {
            direction: if sign > 0.0 { "high" } else { "low" }.to_string(),
            fence_fraction: fraction,
            griefer_vote: vote,
            bar_before,
            bar_after: honest_row.deflation_bar_per_period,
            honest_eligible_before: before,
            honest_eligible_after: eligible(&board),
            griefer_eligible: griefer_row.rank_eligible,
            griefer_fenced: griefer_row.trials_sr_std.to_bits()
                != honest_row.trials_sr_std.to_bits(),
            reproducer: field,
        }
    })
    .into_iter()
    .filter(|d| d.honest_eligible_after < d.honest_eligible_before)
    .collect();
    let report = MoveReport {
        name: "dispersion_grief".to_string(),
        evaluated,
        admitted_on_screen: 0,
        distinct_admitting_params: 0,
        structural_findings: 0,
        best_fitness: 0.0,
        best_params: format!(
            "{} of {evaluated} positions demote an honest entrant",
            demotions.len()
        ),
    };
    (report, demotions)
}

fn run_controls(spec: &SearchSpec) -> Control {
    // The self-audit's skilled shape, without the sine: a positive drift with a
    // small wiggle must be admitted, or the search could never report anything.
    let skilled: Vec<Vec<f64>> = (0..5)
        .map(|w| {
            let mut rng = Rng::new(0x5B11_1ED0 + w);
            (0..60).map(|_| 0.002 + 0.0005 * rng.gauss()).collect()
        })
        .collect();
    let n_boot = ScoreConfig::default().n_boot;
    let skilled_fixture_admitted = score_alone(&skilled, PassMode::All, n_boot).0.rank_eligible;
    let seeds: Vec<u64> = (0..spec.replicates.max(8) as u64).collect();
    let iid_null_refused = parallel_map(&seeds, spec.threads, |s| {
        let mut rng = Rng::new(0x001D_0000 + *s);
        let pooled: Vec<f64> = (0..300).map(|_| 0.01 * rng.gauss()).collect();
        score_alone(&split(&pooled, 60), PassMode::All, n_boot)
            .0
            .rank_eligible
    })
    .iter()
    .all(|admitted| !admitted);
    Control {
        skilled_fixture_admitted,
        iid_null_refused,
    }
}

pub fn grader_search(spec: &SearchSpec) -> GraderSearchReport {
    let control = run_controls(spec);
    let mut moves = Vec::new();
    let mut findings = Vec::new();
    for mv in Move::ALL {
        let (report, found) = search_move(mv, spec);
        moves.push(report);
        findings.extend(found);
    }
    let (report, found) = search_clones(spec);
    moves.push(report);
    findings.extend(found);
    let (report, demotions) = search_grief(spec);
    moves.push(report);

    let mut digest = HASH_START;
    for m in &moves {
        digest = hash_text(digest, &m.name);
        digest = hash_mix(digest, m.evaluated as u64);
        digest = hash_mix(digest, m.admitted_on_screen as u64);
        digest = hash_mix(digest, m.best_fitness.to_bits());
    }
    for f in &findings {
        digest = hash_text(digest, &f.id);
        digest = hash_mix(digest, u64::from(f.verdict == Verdict::Structural));
        digest = hash_mix(digest, f.gates.deflated_sharpe.to_bits());
    }
    for d in &demotions {
        digest = hash_mix(digest, d.griefer_vote.to_bits());
        digest = hash_mix(digest, d.honest_eligible_after as u64);
    }
    let structural_findings = findings
        .iter()
        .filter(|f| f.verdict == Verdict::Structural)
        .count();
    GraderSearchReport {
        schema_version: SCHEMA_VERSION,
        rank_input: false,
        seed: spec.seed,
        budget_per_move: spec.budget,
        replicates: spec.replicates,
        max_findings_per_move: spec.max_findings,
        structural_rate_floor: STRUCTURAL_RATE_FLOOR,
        degenerate_motion: DEGENERATE_MOTION,
        control,
        moves,
        findings,
        demotions,
        structural_findings,
        digest: format!("{digest:016x}"),
        limits: LIMITS.to_vec(),
    }
}

fn parsed_flag<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> Result<T, String> {
    match args.iter().position(|a| a == flag) {
        None => Ok(default),
        Some(i) => args
            .get(i + 1)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("{flag} needs a non-negative integer")),
    }
}

fn spec_of(args: &[String]) -> Result<SearchSpec, String> {
    let known = [
        "--seed",
        "--budget",
        "--replicates",
        "--max-findings",
        "--threads",
        "--out",
    ];
    let mut i = 2;
    while i < args.len() {
        if !known.contains(&args[i].as_str()) {
            return Err(format!("unknown argument: {}", args[i]));
        }
        if args.get(i + 1).is_none() {
            return Err(format!("{} needs a value", args[i]));
        }
        i += 2;
    }
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(32);
    let spec = SearchSpec {
        seed: parsed_flag(args, "--seed", DEFAULT_SEED)?,
        budget: parsed_flag(args, "--budget", DEFAULT_BUDGET)?,
        replicates: parsed_flag(args, "--replicates", DEFAULT_REPLICATES)?,
        max_findings: parsed_flag(args, "--max-findings", DEFAULT_MAX_FINDINGS)?,
        threads: parsed_flag(args, "--threads", threads)?,
    };
    if spec.budget == 0 || spec.replicates == 0 || spec.threads == 0 {
        return Err("--budget, --replicates and --threads must be positive".to_string());
    }
    Ok(spec)
}

fn label(value: Option<bool>) -> String {
    value.map_or_else(|| "n/a".to_string(), |v| v.to_string())
}

fn print_report(report: &GraderSearchReport) {
    println!(
        "SharpeBench grader search (seed {}, {} points a move, {} held-out seeds). Rank-neutral: nothing reads this.\n",
        report.seed, report.budget_per_move, report.replicates
    );
    println!(
        "controls: skilled fixture admitted={} iid null refused={}\n",
        report.control.skilled_fixture_admitted, report.control.iid_null_refused
    );
    for m in &report.moves {
        println!(
            "{:<20} evaluated {:>6}  admitted {:>6}  params {:>4}  structural {:>2}  best {:.3}",
            m.name,
            m.evaluated,
            m.admitted_on_screen,
            m.distinct_admitting_params,
            m.structural_findings,
            m.best_fitness
        );
    }
    println!();
    for f in &report.findings {
        let replication = f.replication.as_ref().map_or_else(
            || "deterministic".to_string(),
            |r| {
                format!(
                    "{}/{} held-out, control {}/{}",
                    r.admitted, r.seeds, r.control_admitted, r.seeds
                )
            },
        );
        println!(
            "[{:?}] {} {} {} windows={} bars={} mode={} basis={:?} | {replication} | field7={} field13={} trials-declared={}",
            f.verdict,
            f.search_move,
            f.variant,
            f.params,
            f.windows,
            f.bars,
            f.pass_mode,
            f.no_skill_basis,
            label(f.admitted_field_7),
            label(f.admitted_field_13),
            label(f.admitted_with_trials_declared)
        );
    }
    for d in &report.demotions {
        println!(
            "[demotion] {} vote {:.3} at {:.3} of the fence: bar {:.4} -> {:.4}, honest eligible {} -> {}, griefer eligible={} fenced={}",
            d.direction,
            d.griefer_vote,
            d.fence_fraction,
            d.bar_before,
            d.bar_after,
            d.honest_eligible_before,
            d.honest_eligible_after,
            d.griefer_eligible,
            d.griefer_fenced
        );
    }
    println!(
        "\n{} admitted submission(s) reported, {} structural; {} demotion position(s); digest {}.",
        report.findings.len(),
        report.structural_findings,
        report.demotions.len(),
        report.digest
    );
}

/// Exit 0 when the search admitted nothing, 1 when it reports a finding, 2 on a
/// bad invocation. No gate reads either.
pub fn run(args: &[String], json: bool) -> i32 {
    let spec = match spec_of(args) {
        Ok(spec) => spec,
        Err(error) => {
            eprintln!("error: {error}\n{USAGE}");
            return 2;
        }
    };
    let report = grader_search(&spec);
    if let Some(i) = args.iter().position(|a| a == "--out") {
        let path = &args[i + 1];
        let text = serde_json::to_string_pretty(&report).expect("the report serializes");
        if let Err(error) = std::fs::write(path, text) {
            eprintln!("error: cannot write {path}: {error}");
            return 1;
        }
    }
    if json {
        crate::emit_json(&report);
    } else {
        print_report(&report);
    }
    i32::from(!report.findings.is_empty())
}
