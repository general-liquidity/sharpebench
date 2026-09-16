//! Opt-in Sharpe diagnostics for a scored board, computed beside it and never
//! folded into it.
//!
//! The three estimators are in [`sharpebench_stats::opt_in_diagnostics`]: an
//! autocorrelation-aware PSR, the PSR with its standard error evaluated under
//! the null, and the manipulation-proof performance measure. This module runs
//! the requested ones on the same pooled track the board's PSR and DSR read,
//! so a diagnostic and the board column it shadows differ only by the
//! estimator. A fourth, `expected-shortfall`, shadows no column: it reports
//! the historical expected shortfall of the same track with the count of
//! observations in its tail, and the fraction of observations below zero
//! ([`sharpebench_stats::tail_risk`]), which the board's downside deviation
//! mixes together. A [`SharpeDiagnostics`] is a separate record: it is not a field
//! of [`CompositeScore`], and no gate, eligibility rule or rank predicate
//! reads it. A caller that does not ask for diagnostics gets exactly the board
//! it got before this module existed.

use serde::Serialize;
use sharpebench_stats::{
    first_order_autocorrelation, manipulation_proof_performance,
    probabilistic_sharpe_ratio_autocorrelated, StandardErrorAt, DEFAULT_MPPM_RISK_AVERSION,
};
use sharpebench_stats::{
    historical_expected_shortfall, loss_frequency, tail_size, DEFAULT_MIN_TAIL_OBSERVATIONS,
    DEFAULT_TAIL_LEVEL,
};

use crate::composite::{
    pooled_returns, restrict_to_shared_positions, AgentSubmission, CompositeScore, ScoreConfig,
};

/// One opt-in diagnostic, by the identifier the command line accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SharpeDiagnostic {
    /// `autocorrelated-psr`: the board's PSR and DSR recomputed with the
    /// pooled track's own first-order autocorrelation in the Sharpe variance
    /// (López de Prado, Lipton and Zoonekynd 2026, eqs. 2 and 3, p. 9), the
    /// standard error still at the observed Sharpe. Only the autocorrelation
    /// term differs from the board.
    AutocorrelatedPsr,
    /// `null-se-psr`: the board's PSR and DSR with the standard error evaluated
    /// at the benchmark rather than the observed Sharpe (the same paper, eqs. 4
    /// and 5, p. 10), serial independence kept. Only the evaluation point
    /// differs from the board.
    NullSePsr,
    /// `mppm`: the manipulation-proof performance measure of Goetzmann,
    /// Ingersoll, Spiegel and Welch (2007, eq. 18) at risk aversion 3 and a
    /// zero risk-free rate, annualized at the board's periods per year.
    Mppm,
    /// `expected-shortfall`: the historical expected shortfall of the pooled
    /// track at tail level 0.05, the number of observations in that tail, and
    /// the fraction of observations below zero. Downside deviation mixes how
    /// often a track loses with how much; this reports the two apart. The
    /// shortfall is unavailable, with the reason, when the tail holds fewer
    /// than 10 whole observations.
    ExpectedShortfall,
}

impl SharpeDiagnostic {
    /// Every diagnostic, in report order.
    pub const ALL: [SharpeDiagnostic; 4] = [
        SharpeDiagnostic::AutocorrelatedPsr,
        SharpeDiagnostic::NullSePsr,
        SharpeDiagnostic::Mppm,
        SharpeDiagnostic::ExpectedShortfall,
    ];

    /// The command-line identifier.
    pub fn id(self) -> &'static str {
        match self {
            SharpeDiagnostic::AutocorrelatedPsr => "autocorrelated-psr",
            SharpeDiagnostic::NullSePsr => "null-se-psr",
            SharpeDiagnostic::Mppm => "mppm",
            SharpeDiagnostic::ExpectedShortfall => "expected-shortfall",
        }
    }

    /// Parse a comma-separated list such as `autocorrelated-psr,mppm`. Order
    /// and repetition do not matter; the result is sorted and deduplicated. An
    /// empty list or an unknown identifier is refused by name, so a typo
    /// cannot silently request nothing.
    pub fn parse_list(list: &str) -> Result<Vec<SharpeDiagnostic>, String> {
        let mut out = Vec::new();
        for raw in list.split(',') {
            let id = raw.trim();
            let found = SharpeDiagnostic::ALL
                .into_iter()
                .find(|d| d.id() == id)
                .ok_or_else(|| {
                    let known: Vec<&str> = SharpeDiagnostic::ALL.iter().map(|d| d.id()).collect();
                    format!(
                        "unknown diagnostic `{id}`; expected a comma-separated list of {}",
                        known.join(", ")
                    )
                })?;
            out.push(found);
        }
        out.sort();
        out.dedup();
        Ok(out)
    }
}

/// A PSR diagnostic against the board's two benchmarks.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PsrDiagnostic {
    /// The first-order autocorrelation used in the Sharpe variance: the pooled
    /// track's own estimate for `autocorrelated-psr`, 0 for `null-se-psr`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rho: Option<f64>,
    /// Against a zero Sharpe: the counterpart of the board's `psr`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psr: Option<f64>,
    /// Against the row's `deflation_bar_per_period`: the counterpart of the
    /// board's `deflated_sharpe`. Absent when the row has a `deflation_error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psr_at_deflation_bar: Option<f64>,
    /// Why the diagnostic could not be computed. When present, the values it
    /// explains are absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The manipulation-proof performance measure for one agent.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MppmDiagnostic {
    /// Relative risk aversion, the measure's `rho`.
    pub risk_aversion: f64,
    /// The periods per year the measure is annualized at (`dt = 1 / this`).
    pub periods_per_year: f64,
    /// The annualized certainty-equivalent excess return. Absent on error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annualized: Option<f64>,
    /// Why the measure could not be computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The historical expected shortfall and loss frequency of one agent's pooled
/// track, computed by [`sharpebench_stats::tail_risk`].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TailRiskDiagnostic {
    /// The tail fraction, 0.05: the worst 5% of the pooled observations.
    pub level: f64,
    /// The fewest whole tail observations a shortfall is reported from, 10.
    pub min_tail_observations: usize,
    /// `T = pooled_observations * level`, the tail size the tail sum is
    /// divided by, fractional when the product is.
    pub tail_size: f64,
    /// The observations that enter the tail with a positive weight,
    /// `ceil(T)`, reported whether or not the shortfall is.
    pub tail_observations: usize,
    /// The expected shortfall: the mean return over the tail, with the
    /// observation that straddles a fractional boundary weighted by the part
    /// of it inside. A return, not a positive loss: -0.04 means the tail lost
    /// 4% a period on average. Absent when `error` is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_mean_return: Option<f64>,
    /// Pooled observations strictly below zero. Absent for a track with no
    /// observations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub losses: Option<usize>,
    /// `losses / pooled_observations`, in [0, 1]. Absent with `losses`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss_frequency: Option<f64>,
    /// Why `tail_mean_return` is absent, such as a tail with fewer than
    /// `min_tail_observations` whole observations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The requested diagnostics for one board row. Diagnostics that were not
/// requested are absent, not null.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SharpeDiagnostics {
    pub agent_id: String,
    /// Always `false`: recorded so a reader of the JSON alone cannot mistake a
    /// diagnostic for a board column.
    pub used_by_gate: bool,
    /// Observations in the pooled track the diagnostics were computed on, the
    /// same track as the row's `pooled_observations`.
    pub pooled_observations: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autocorrelated_psr: Option<PsrDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub null_se_psr: Option<PsrDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mppm: Option<MppmDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_shortfall: Option<TailRiskDiagnostic>,
}

/// Compute `requested` for every row of `board`, in board order.
///
/// `subs` and `cfg` must be the field and configuration the board was ranked
/// from. The pooled track is rebuilt as [`crate::rank_declared`] builds it: the
/// shared-cell restriction when `cfg.shared_run_set` is on, then
/// [`pooled_returns`] at `cfg.execution_seeds_per_window`. Each row is matched
/// to its submission by `agent_id`, first match, as
/// [`crate::rank_certified`] does. The board is read, never written.
pub fn sharpe_diagnostics(
    subs: &[AgentSubmission],
    board: &[CompositeScore],
    cfg: &ScoreConfig,
    requested: &[SharpeDiagnostic],
) -> Vec<SharpeDiagnostics> {
    let restricted;
    let field: &[AgentSubmission] = if cfg.shared_run_set {
        restricted = restrict_to_shared_positions(subs);
        &restricted
    } else {
        subs
    };
    board
        .iter()
        .map(|row| {
            let pooled = field
                .iter()
                .find(|s| s.agent_id == row.agent_id)
                .map(|s| pooled_returns(s, cfg.execution_seeds_per_window))
                .unwrap_or_default();
            let bar = row
                .deflation_error
                .is_none()
                .then_some(row.deflation_bar_per_period);
            let wants = |d| requested.contains(&d);
            SharpeDiagnostics {
                agent_id: row.agent_id.clone(),
                used_by_gate: false,
                pooled_observations: pooled.len(),
                autocorrelated_psr: wants(SharpeDiagnostic::AutocorrelatedPsr).then(|| {
                    match first_order_autocorrelation(&pooled) {
                        Ok(rho) => psr_pair(&pooled, rho, StandardErrorAt::Observed, bar),
                        Err(e) => PsrDiagnostic {
                            rho: None,
                            psr: None,
                            psr_at_deflation_bar: None,
                            error: Some(e.to_string()),
                        },
                    }
                }),
                null_se_psr: wants(SharpeDiagnostic::NullSePsr)
                    .then(|| psr_pair(&pooled, 0.0, StandardErrorAt::Benchmark, bar)),
                mppm: wants(SharpeDiagnostic::Mppm).then(|| {
                    let measured = manipulation_proof_performance(
                        &pooled,
                        DEFAULT_MPPM_RISK_AVERSION,
                        cfg.periods_per_year,
                    );
                    MppmDiagnostic {
                        risk_aversion: DEFAULT_MPPM_RISK_AVERSION,
                        periods_per_year: cfg.periods_per_year,
                        annualized: measured.as_ref().ok().copied(),
                        error: measured.err().map(|e| e.to_string()),
                    }
                }),
                expected_shortfall: wants(SharpeDiagnostic::ExpectedShortfall)
                    .then(|| tail_risk(&pooled)),
            }
        })
        .collect()
}

fn tail_risk(pooled: &[f64]) -> TailRiskDiagnostic {
    let size = tail_size(pooled.len(), DEFAULT_TAIL_LEVEL).expect("the default level is in (0, 1]");
    let shortfall =
        historical_expected_shortfall(pooled, DEFAULT_TAIL_LEVEL, DEFAULT_MIN_TAIL_OBSERVATIONS);
    let losses = loss_frequency(pooled).ok();
    TailRiskDiagnostic {
        level: DEFAULT_TAIL_LEVEL,
        min_tail_observations: DEFAULT_MIN_TAIL_OBSERVATIONS,
        tail_size: size,
        tail_observations: size.ceil() as usize,
        tail_mean_return: shortfall.as_ref().ok().map(|es| es.tail_mean_return),
        losses: losses.map(|l| l.losses),
        loss_frequency: losses.map(|l| l.frequency),
        error: shortfall
            .err()
            .map(|e| format!("tail at level {DEFAULT_TAIL_LEVEL}: {e}")),
    }
}

fn psr_pair(pooled: &[f64], rho: f64, at: StandardErrorAt, bar: Option<f64>) -> PsrDiagnostic {
    let against_zero = probabilistic_sharpe_ratio_autocorrelated(pooled, 0.0, rho, at);
    let against_bar =
        bar.map(|bar| probabilistic_sharpe_ratio_autocorrelated(pooled, bar, rho, at));
    let error = against_zero
        .as_ref()
        .err()
        .or_else(|| against_bar.as_ref().and_then(|r| r.as_ref().err()))
        .map(ToString::to_string);
    PsrDiagnostic {
        rho: Some(rho),
        psr: against_zero.ok(),
        psr_at_deflation_bar: against_bar.and_then(Result::ok),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{rank, Run};
    use crate::process::Trace;

    fn sub(id: &str, returns: Vec<f64>) -> AgentSubmission {
        AgentSubmission {
            agent_id: id.into(),
            runs: vec![Run {
                returns,
                trace: Trace::default(),
                confidences: vec![],
                outcomes: vec![],
                cost: 0.0,
            }],
            in_sample_trials: 0,
            candidates: vec![],
        }
    }

    fn field() -> Vec<AgentSubmission> {
        let wave: Vec<f64> = (0..200)
            .map(|t| {
                let t = t as f64;
                0.001 + 0.01 * (1.3 * t).sin() + 0.004 * (0.21 * t).cos()
            })
            .collect();
        let steady: Vec<f64> = (0..200)
            .map(|i| 0.001 + 0.0002 * ((i % 7) as f64 - 3.0))
            .collect();
        vec![sub("wave", wave), sub("steady", steady)]
    }

    #[test]
    fn parse_list_accepts_known_ids_in_any_order_and_refuses_the_rest() {
        assert_eq!(
            SharpeDiagnostic::parse_list("mppm,autocorrelated-psr,mppm"),
            Ok(vec![
                SharpeDiagnostic::AutocorrelatedPsr,
                SharpeDiagnostic::Mppm
            ])
        );
        assert_eq!(
            SharpeDiagnostic::parse_list(" null-se-psr "),
            Ok(vec![SharpeDiagnostic::NullSePsr])
        );
        assert_eq!(
            SharpeDiagnostic::parse_list("expected-shortfall,mppm"),
            Ok(vec![
                SharpeDiagnostic::Mppm,
                SharpeDiagnostic::ExpectedShortfall
            ])
        );
        for bad in [
            "",
            "psr",
            "mppm,",
            "MPPM",
            "autocorrelated_psr",
            "expected_shortfall",
            "es",
            "cvar",
        ] {
            assert!(SharpeDiagnostic::parse_list(bad).is_err(), "{bad}");
        }
    }

    /// The diagnostics read the board and change nothing on it: ranking the
    /// same field again gives a byte-identical board. Each diagnostic is its
    /// estimator on the row's own pooled track and deflation bar: at rho = 0
    /// with the standard error at the observed Sharpe the same function
    /// reproduces the row's `psr` and `deflated_sharpe` bit for bit, and the
    /// autocorrelated stream's diagnostic departs from its board PSR.
    #[test]
    fn diagnostics_shadow_the_board_columns_without_touching_them() {
        let subs = field();
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let before = serde_json::to_string(&board).unwrap();
        let diags = sharpe_diagnostics(&subs, &board, &cfg, &SharpeDiagnostic::ALL);
        assert_eq!(serde_json::to_string(&rank(&subs, &cfg)).unwrap(), before);
        assert_eq!(diags.len(), board.len());

        for (row, d) in board.iter().zip(&diags) {
            assert_eq!(d.agent_id, row.agent_id);
            assert!(!d.used_by_gate);
            assert_eq!(d.pooled_observations, row.pooled_observations);
            let pooled =
                pooled_returns(subs.iter().find(|s| s.agent_id == row.agent_id).unwrap(), 1);
            // rho = 0 at the observed Sharpe is the board, bit for bit.
            let iid = |b| {
                probabilistic_sharpe_ratio_autocorrelated(
                    &pooled,
                    b,
                    0.0,
                    StandardErrorAt::Observed,
                )
                .unwrap()
            };
            assert_eq!(iid(0.0).to_bits(), row.psr.to_bits());
            assert_eq!(
                iid(row.deflation_bar_per_period).to_bits(),
                row.deflated_sharpe.to_bits()
            );
            let ac = d.autocorrelated_psr.as_ref().unwrap();
            assert_eq!(ac.error, None);
            let rho = ac.rho.unwrap();
            assert_eq!(rho, first_order_autocorrelation(&pooled).unwrap());
            assert_eq!(
                ac.psr,
                Some(
                    probabilistic_sharpe_ratio_autocorrelated(
                        &pooled,
                        0.0,
                        rho,
                        StandardErrorAt::Observed
                    )
                    .unwrap()
                )
            );
            let null = d.null_se_psr.as_ref().unwrap();
            assert_eq!(null.rho, Some(0.0));
            assert_eq!(
                null.psr_at_deflation_bar,
                Some(
                    probabilistic_sharpe_ratio_autocorrelated(
                        &pooled,
                        row.deflation_bar_per_period,
                        0.0,
                        StandardErrorAt::Benchmark
                    )
                    .unwrap()
                )
            );
            let mppm = d.mppm.as_ref().unwrap();
            assert_eq!(mppm.risk_aversion, 3.0);
            assert_eq!(mppm.periods_per_year, cfg.periods_per_year);
            assert_eq!(
                mppm.annualized,
                Some(manipulation_proof_performance(&pooled, 3.0, cfg.periods_per_year).unwrap())
            );
            // 200 pooled observations at 5% is a tail of exactly ten.
            let es = d.expected_shortfall.as_ref().unwrap();
            let expected = historical_expected_shortfall(&pooled, 0.05, 10).unwrap();
            assert_eq!(es.level, 0.05);
            assert_eq!(es.min_tail_observations, 10);
            assert_eq!(es.tail_size, 10.0);
            assert_eq!(es.tail_observations, 10);
            assert_eq!(es.tail_mean_return, Some(expected.tail_mean_return));
            let losses = loss_frequency(&pooled).unwrap();
            assert_eq!(es.losses, Some(losses.losses));
            assert_eq!(es.loss_frequency, Some(losses.frequency));
            assert_eq!(es.error, None);
        }
        // The autocorrelated stream's diagnostic departs from its board PSR.
        let wave = board.iter().position(|r| r.agent_id == "wave").unwrap();
        let ac = diags[wave].autocorrelated_psr.as_ref().unwrap();
        assert!(ac.rho.unwrap() > 0.3);
        assert!(ac.psr.unwrap() < board[wave].psr);
    }

    #[test]
    fn only_requested_diagnostics_are_present_and_errors_are_typed() {
        let mut subs = field();
        subs.push(sub("flat", vec![0.0; 200]));
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let diags = sharpe_diagnostics(&subs, &board, &cfg, &[SharpeDiagnostic::Mppm]);
        for d in &diags {
            assert!(d.autocorrelated_psr.is_none());
            assert!(d.null_se_psr.is_none());
            assert!(d.mppm.is_some());
            assert!(d.expected_shortfall.is_none());
            let json = serde_json::to_value(d).unwrap();
            assert!(json.get("autocorrelated_psr").is_none());
            assert!(json.get("null_se_psr").is_none());
            assert!(json.get("expected_shortfall").is_none());
        }
        let all = sharpe_diagnostics(&subs, &board, &cfg, &SharpeDiagnostic::ALL);
        let flat = all.iter().find(|d| d.agent_id == "flat").unwrap();
        let ac = flat.autocorrelated_psr.as_ref().unwrap();
        assert_eq!(ac.rho, None);
        assert_eq!(ac.psr, None);
        assert!(ac.error.as_deref().unwrap().contains("constant"));
        // A flat track has an MPPM of exactly zero and a defined null-SE PSR.
        assert_eq!(flat.mppm.as_ref().unwrap().annualized, Some(0.0));
        assert!(flat.null_se_psr.as_ref().unwrap().psr.is_some());
        // A flat track's tail is its zeros, and none of them is a loss.
        let tail = flat.expected_shortfall.as_ref().unwrap();
        assert_eq!(tail.tail_mean_return, Some(0.0));
        assert_eq!(tail.losses, Some(0));
        assert_eq!(tail.loss_frequency, Some(0.0));
        let only_tail =
            sharpe_diagnostics(&subs, &board, &cfg, &[SharpeDiagnostic::ExpectedShortfall]);
        for d in &only_tail {
            assert!(d.expected_shortfall.is_some());
            assert!(d.mppm.is_none() && d.autocorrelated_psr.is_none() && d.null_se_psr.is_none());
        }
        assert!(sharpe_diagnostics(&subs, &board, &cfg, &[])
            .iter()
            .all(|d| d.autocorrelated_psr.is_none()
                && d.null_se_psr.is_none()
                && d.mppm.is_none()
                && d.expected_shortfall.is_none()));
    }

    fn repeat(pattern: &[f64], times: usize) -> Vec<f64> {
        pattern
            .iter()
            .copied()
            .cycle()
            .take(pattern.len() * times)
            .collect()
    }

    /// Two agents with the same mean and the same board downside deviation,
    /// one losing 0.02 on half its bars, one losing 0.04 on one bar in eight.
    /// The board cannot tell them apart on the downside; the diagnostic
    /// reports twice the shortfall and a quarter of the loss frequency, and
    /// the board bytes do not move.
    #[test]
    fn expected_shortfall_separates_what_the_board_downside_deviation_cannot() {
        let subs = vec![
            sub("often", repeat(&[-0.02, 0.02], 120)),
            sub(
                "rarely",
                repeat(&[-0.04, 0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 30),
            ),
        ];
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let before = serde_json::to_string(&board).unwrap();
        let row = |id: &str| board.iter().find(|r| r.agent_id == id).unwrap();
        let (often, rarely) = (row("often"), row("rarely"));
        assert_eq!(often.raw_mean_return, 0.0);
        assert_eq!(rarely.raw_mean_return, 0.0);
        assert!((often.downside_deviation - rarely.downside_deviation).abs() <= 1e-15);

        let diags = sharpe_diagnostics(&subs, &board, &cfg, &[SharpeDiagnostic::ExpectedShortfall]);
        assert_eq!(serde_json::to_string(&board).unwrap(), before);
        let tail = |id: &str| {
            diags
                .iter()
                .find(|d| d.agent_id == id)
                .unwrap()
                .expected_shortfall
                .clone()
                .unwrap()
        };
        let (often, rarely) = (tail("often"), tail("rarely"));
        for t in [&often, &rarely] {
            assert_eq!(t.tail_size, 12.0);
            assert_eq!(t.tail_observations, 12);
            assert_eq!(t.error, None);
        }
        assert!((often.tail_mean_return.unwrap() - -0.02).abs() <= 1e-15);
        assert!((rarely.tail_mean_return.unwrap() - -0.04).abs() <= 1e-15);
        assert_eq!((often.losses, often.loss_frequency), (Some(120), Some(0.5)));
        assert_eq!(
            (rarely.losses, rarely.loss_frequency),
            (Some(30), Some(0.125))
        );
    }

    /// Below ten whole tail observations the shortfall is absent with the
    /// reason, the tail count and the loss frequency are still reported, and
    /// the JSON carries no number in its place. 199 observations make a tail
    /// of 9.95: ten observations carry weight, only nine whole ones.
    #[test]
    fn a_short_tail_is_unavailable_and_says_why() {
        for (n, reported) in [(100, false), (199, false), (200, true)] {
            let returns = repeat(&[-0.03, 0.01, 0.02, -0.01], 50)[..n].to_vec();
            let subs = vec![sub("short", returns)];
            let cfg = ScoreConfig::default();
            let board = rank(&subs, &cfg);
            let d = sharpe_diagnostics(&subs, &board, &cfg, &[SharpeDiagnostic::ExpectedShortfall])
                .remove(0);
            let tail = d.expected_shortfall.unwrap();
            assert_eq!(tail.tail_mean_return.is_some(), reported, "{n}");
            assert_eq!(tail.error.is_none(), reported, "{n}");
            assert_eq!(tail.tail_observations, (n as f64 * 0.05).ceil() as usize);
            let losses = (0..n).filter(|i| i % 4 == 0 || i % 4 == 3).count();
            assert_eq!(tail.losses, Some(losses), "{n}");
            let json = serde_json::to_value(&tail).unwrap();
            assert_eq!(json.get("tail_mean_return").is_some(), reported, "{n}");
            if !reported {
                let error = tail.error.unwrap();
                assert!(error.starts_with("tail at level 0.05: "), "{error}");
                assert!(error.contains("at least 10 observations"), "{error}");
                assert!(error.contains(&format!("got {}", n / 20)), "{error}");
            }
        }
        // An agent with no pooled observations has no loss frequency either.
        let subs = vec![sub("empty", vec![])];
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let tail = sharpe_diagnostics(&subs, &board, &cfg, &[SharpeDiagnostic::ExpectedShortfall])
            .remove(0)
            .expected_shortfall
            .unwrap();
        assert_eq!((tail.tail_size, tail.tail_observations), (0.0, 0));
        assert_eq!((tail.losses, tail.loss_frequency), (None, None));
        assert!(tail.error.is_some());
    }

    /// The track is the board's: with the shared-cell restriction on, an agent
    /// that completed an extra run is scored on the shared cells only, and so
    /// are its diagnostics; with it off, both see every run.
    #[test]
    fn diagnostics_follow_the_shared_cell_restriction() {
        let mut subs = field();
        let extra: Vec<f64> = (0..50).map(|i| 0.002 * ((i % 3) as f64 - 1.0)).collect();
        subs[0].runs.push(Run {
            returns: extra,
            ..Run::default()
        });
        for shared in [true, false] {
            let cfg = ScoreConfig {
                shared_run_set: shared,
                ..ScoreConfig::default()
            };
            let board = rank(&subs, &cfg);
            let diags = sharpe_diagnostics(&subs, &board, &cfg, &SharpeDiagnostic::ALL);
            for (row, d) in board.iter().zip(&diags) {
                assert_eq!(d.pooled_observations, row.pooled_observations, "{shared}");
            }
            let wave = diags.iter().find(|d| d.agent_id == "wave").unwrap();
            assert_eq!(wave.pooled_observations, if shared { 200 } else { 250 });
        }
    }
}
