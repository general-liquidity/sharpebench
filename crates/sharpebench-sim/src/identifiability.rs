//! Identifiability probe: can an attacker name the ticker and the year behind
//! a masked series?
//!
//! The probe asks a closed question per (window, symbol): here is one masked
//! close series, here are the candidate tickers and the candidate years, name
//! your top-k tickers and one year. It asks the same question about the
//! window's synthetic twin ([`crate::contamination::build_twin`]), whose
//! correct answer is taken to be the real window's ticker and year, because the
//! twin keeps the real window's texture and opportunity set and breaks only the
//! order of its bars. The real-minus-twin difference in joint hit rate (top-1
//! ticker and year both right) is then the part of identification that comes
//! from the path itself rather than from its texture, and the window is the
//! inferential unit of the sign-flip test.
//!
//! After KTD-Fin's de-anonymization probe (arXiv:2605.28359, section 4.3),
//! which scores top-1 and top-5 ticker recovery, a date tolerance and a joint
//! rate against a random baseline. That probe certifies a mask with model
//! attackers. This one is built and validated offline with planted attackers;
//! pointing it at a model field needs model calls and is a separate study.
//!
//! After Entity Neutering (Engelberg, Manela, Mullins and Vulicevic, SSRN
//! 5182756), a mask is accepted only when an independent attacker identifies
//! the subject at the rate of random chance; the chance rates reported here are
//! that bar, for a uniform guesser over the same candidates.
//!
//! The known negative control: a path-matching attacker that holds the public
//! frozen dataset re-identifies every masked window, because
//! [`Dataset::masked_view`] keeps every simple return bit-identical. The mask
//! removes name, date and level lookups, not path lookups. The probe reports
//! that rather than hiding it. After Lopez-Lira, Tang and Zhu
//! (arXiv:2504.14765), who find that masking fails because models reconstruct
//! entities and dates from minimal context, a mask is treated as a lookup
//! defense and never as a certification.
//!
//! Pure: no I/O, no clock. The real and twin questions of a window are asked in
//! a seeded order, and a question never says which arm it comes from.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::blinding::iso_year;
use crate::contamination::{
    build_twin, draw_seed, shape_probe, PairedScore, ProbeConfig, ProbeError, ShapeProbeReport,
    TwinArm, TwinSpec,
};
use crate::data::Dataset;
use crate::engine::Window;

/// Schema of an [`IdentityQuestion`] on the wire.
pub const IDENTITY_QUESTION_SCHEMA: &str = "sharpebench.identity-question.v1";

/// One closed question about one masked series.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityQuestion {
    pub schema_version: String,
    /// Position in the order asked. Carries no arm.
    pub question_id: usize,
    /// The masked id of the series.
    pub symbol: String,
    /// The masked dates of the span (`t0`, `t1`, ...).
    pub dates: Vec<String>,
    /// The masked closes of the span: the twin's warmup and scored bars.
    pub closes: Vec<f64>,
    /// Index in `closes` of the first scored bar. The year asked for is that
    /// bar's year.
    pub scored_from: usize,
    /// The tickers an answer may name: the source's symbols, sorted.
    pub candidate_tickers: Vec<String>,
    /// The years an answer may name: every year the source's dates cover.
    pub candidate_years: Vec<i64>,
    /// How many ranked tickers are scored.
    pub top_k: usize,
}

/// An attacker's answer. Tickers are ranked best first; entries after the
/// first `top_k` are ignored, and a name that is not a candidate is a miss.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IdentityAnswer {
    pub tickers: Vec<String>,
    pub year: Option<i64>,
}

/// The declared scoring rule, fixed before any answer is seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IdentifiabilityConfig {
    pub top_k: usize,
    /// A year within this many years of the truth is a hit.
    pub year_tolerance: i64,
    pub twin: TwinSpec,
    /// Window `i` is twinned with seed `twin_seed + i`.
    pub twin_seed: u64,
    /// The sign-flip test on the per-window joint-rate edge.
    pub probe: ProbeConfig,
}

impl Default for IdentifiabilityConfig {
    fn default() -> Self {
        IdentifiabilityConfig {
            top_k: 2,
            year_tolerance: 0,
            twin: TwinSpec::default(),
            twin_seed: 1000,
            probe: ProbeConfig {
                min_edge: 0.25,
                ..ProbeConfig::default()
            },
        }
    }
}

/// Hit rates over every question of one arm.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ArmRates {
    pub questions: usize,
    pub ticker_top1: f64,
    pub ticker_top_k: f64,
    pub year: f64,
    /// Top-1 ticker and year both right.
    pub joint: f64,
}

/// What a guesser that picks uniformly among the candidates scores.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ChanceRates {
    pub ticker_top1: f64,
    pub ticker_top_k: f64,
    pub year: f64,
    pub joint: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IdentifiabilityReport {
    pub windows: usize,
    pub symbols_per_window: usize,
    pub top_k: usize,
    pub year_tolerance: i64,
    pub real: ArmRates,
    pub twin: ArmRates,
    pub chance: ChanceRates,
    /// Per-window real-minus-twin joint rate, tested by whole-window sign
    /// flips. `memorization` there is this probe's `path_identified`.
    pub joint_edge: ShapeProbeReport,
    /// The real windows are identified beyond what their texture explains.
    pub path_identified: bool,
}

/// Why the probe has no verdict.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum IdentifiabilityUnavailable<E> {
    /// `top_k` is zero or `year_tolerance` is negative.
    InvalidConfig,
    /// No windows were given.
    NoWindows,
    /// The window's first scored date has no calendar year to score against.
    UndatedWindow {
        window: usize,
    },
    Twin {
        window: usize,
        error: String,
    },
    /// The attacker failed to answer.
    Answer {
        question_id: usize,
        error: E,
    },
    Probe {
        error: String,
    },
}

#[derive(Default)]
struct Tally {
    questions: usize,
    top1: usize,
    top_k: usize,
    year: usize,
    joint: usize,
}

impl Tally {
    fn rates(&self) -> ArmRates {
        let n = self.questions.max(1) as f64;
        ArmRates {
            questions: self.questions,
            ticker_top1: self.top1 as f64 / n,
            ticker_top_k: self.top_k as f64 / n,
            year: self.year as f64 / n,
            joint: self.joint as f64 / n,
        }
    }
}

/// Ask `ask` the closed question for every symbol of every window and of its
/// twin, and score the answers. The source should be the unmasked dataset:
/// its symbols and the year of each window's first scored date are the truth.
pub fn identifiability_probe<E>(
    source: &Dataset,
    windows: &[Window],
    config: IdentifiabilityConfig,
    ask: &mut dyn FnMut(&IdentityQuestion) -> Result<IdentityAnswer, E>,
) -> Result<IdentifiabilityReport, IdentifiabilityUnavailable<E>> {
    if config.top_k == 0 || config.year_tolerance < 0 {
        return Err(IdentifiabilityUnavailable::InvalidConfig);
    }
    if windows.is_empty() {
        return Err(IdentifiabilityUnavailable::NoWindows);
    }
    let tickers = source.symbols();
    let years: Vec<i64> = source
        .dates
        .iter()
        .filter_map(|d| iso_year(d))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut truths = Vec::with_capacity(windows.len());
    let mut pairs = Vec::with_capacity(windows.len());
    for (i, w) in windows.iter().enumerate() {
        let year = source
            .dates
            .get(w.start)
            .and_then(|d| iso_year(d))
            .ok_or(IdentifiabilityUnavailable::UndatedWindow { window: i })?;
        let pair = build_twin(
            source,
            *w,
            &config.twin,
            config.twin_seed.wrapping_add(i as u64),
        )
        .map_err(|e| IdentifiabilityUnavailable::Twin {
            window: i,
            error: e.to_string(),
        })?;
        truths.push(year);
        pairs.push(pair);
    }

    let mut next_id = 0usize;
    let (mut real, mut twin) = (Tally::default(), Tally::default());
    let mut scores = Vec::with_capacity(pairs.len());
    for (i, (pair, &truth_year)) in pairs.iter().zip(&truths).enumerate() {
        let order = if draw_seed(config.probe.seed, i as u64) & 1 == 0 {
            [TwinArm::Real, TwinArm::Twin]
        } else {
            [TwinArm::Twin, TwinArm::Real]
        };
        let mut joint = [0usize; 2];
        for arm in order {
            let data = match arm {
                TwinArm::Real => &pair.real,
                TwinArm::Twin => &pair.twin,
            };
            let tally = match arm {
                TwinArm::Real => &mut real,
                TwinArm::Twin => &mut twin,
            };
            // inv: `tally` and `joint` count every answered question of this
            // arm for the symbols before `s`.
            for (s, (masked, closes)) in data.closes.iter().enumerate() {
                let question = IdentityQuestion {
                    schema_version: IDENTITY_QUESTION_SCHEMA.to_string(),
                    question_id: next_id,
                    symbol: masked.clone(),
                    dates: data.dates.clone(),
                    closes: closes.clone(),
                    scored_from: pair.window.start,
                    candidate_tickers: tickers.clone(),
                    candidate_years: years.clone(),
                    top_k: config.top_k,
                };
                next_id += 1;
                let answer =
                    ask(&question).map_err(|error| IdentifiabilityUnavailable::Answer {
                        question_id: question.question_id,
                        error,
                    })?;
                let truth = &tickers[s];
                let ranked = &answer.tickers[..answer.tickers.len().min(config.top_k)];
                let top1 = ranked.first() == Some(truth);
                let year = answer
                    .year
                    .is_some_and(|y| (y - truth_year).abs() <= config.year_tolerance);
                tally.questions += 1;
                tally.top1 += usize::from(top1);
                tally.top_k += usize::from(ranked.contains(truth));
                tally.year += usize::from(year);
                tally.joint += usize::from(top1 && year);
                joint[usize::from(arm == TwinArm::Twin)] += usize::from(top1 && year);
            }
        }
        let n = pair.real.closes.len() as f64;
        scores.push(PairedScore {
            real: Some(joint[0] as f64 / n),
            twin: Some(joint[1] as f64 / n),
        });
    }

    let joint_edge = shape_probe(&scores, config.probe).map_err(|e| match e {
        ProbeError::ScoreUnavailable { window } => IdentifiabilityUnavailable::Probe {
            error: format!("window {window} has no joint rate"),
        },
        ProbeError::Statistics(error) => IdentifiabilityUnavailable::Probe {
            error: format!("{error:?}"),
        },
    })?;
    let n_tickers = tickers.len() as f64;
    let year_chance = if years.is_empty() {
        0.0
    } else {
        truths
            .iter()
            .map(|t| {
                years
                    .iter()
                    .filter(|y| (**y - t).abs() <= config.year_tolerance)
                    .count() as f64
                    / years.len() as f64
            })
            .sum::<f64>()
            / truths.len() as f64
    };
    let top1_chance = 1.0 / n_tickers;
    Ok(IdentifiabilityReport {
        windows: windows.len(),
        symbols_per_window: tickers.len(),
        top_k: config.top_k,
        year_tolerance: config.year_tolerance,
        real: real.rates(),
        twin: twin.rates(),
        chance: ChanceRates {
            ticker_top1: top1_chance,
            ticker_top_k: config.top_k.min(tickers.len()) as f64 / n_tickers,
            year: year_chance,
            joint: top1_chance * year_chance,
        },
        path_identified: joint_edge.memorization,
        joint_edge,
    })
}
