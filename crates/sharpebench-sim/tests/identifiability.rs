//! Offline validation of the identifiability probe with planted attackers. No
//! model is called.
//!
//! The path-matching attacker holds the public frozen dataset and looks the
//! masked return path up in it. Masking keeps returns bit-identical, so it
//! re-identifies every real window, ticker and year: that is the negative
//! control, and it shows the mask defeats name, date and level lookups only.
//! On the twins there is no path to find. The texture attacker reads only the
//! volatility of the series, which the twin keeps, so its real and twin rates
//! match and the probe does not call it a path identification.

use sharpebench_sim::windows::walk_forward;
use sharpebench_sim::{
    identifiability_probe, iso_year, Dataset, IdentifiabilityConfig, IdentifiabilityReport,
    IdentifiabilityUnavailable, IdentityAnswer, IdentityQuestion, Window,
};

type Ask<'a> = dyn FnMut(&IdentityQuestion) -> Result<IdentityAnswer, ()> + 'a;

fn frozen() -> Dataset {
    let path = format!(
        "{}/../../data/us-indices-1d.csv",
        env!("CARGO_MANIFEST_DIR")
    );
    Dataset::from_csv_file(&path).unwrap()
}

fn windows(data: &Dataset) -> Vec<Window> {
    let w = walk_forward(data.len(), 20, 40, 150);
    assert!(w.len() >= 12, "{}", w.len());
    w.into_iter().take(12).collect()
}

fn relatives(series: &[f64]) -> Vec<f64> {
    series.windows(2).map(|w| w[1] / w[0]).collect()
}

/// Finds the exact return path in the public data.
fn path_matcher(public: &Dataset) -> Box<Ask<'_>> {
    Box::new(move |q| {
        let wanted = relatives(&q.closes);
        for (symbol, series) in &public.closes {
            let have = relatives(series);
            if let Some(offset) = have.windows(wanted.len()).position(|w| w == wanted) {
                return Ok(IdentityAnswer {
                    tickers: vec![symbol.clone()],
                    year: iso_year(&public.dates[offset + q.scored_from]),
                });
            }
        }
        Ok(IdentityAnswer::default())
    })
}

fn volatility(series: &[f64]) -> f64 {
    let r: Vec<f64> = relatives(series).iter().map(|x| x - 1.0).collect();
    let m = r.iter().sum::<f64>() / r.len() as f64;
    (r.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (r.len() - 1) as f64).sqrt()
}

/// Names the tickers ranked by how close their whole-history volatility is to
/// the series' volatility, and never a year.
fn texture_reader(public: &Dataset) -> Box<Ask<'static>> {
    let profile: Vec<(String, f64)> = public
        .closes
        .iter()
        .map(|(s, v)| (s.clone(), volatility(v)))
        .collect();
    Box::new(move |q| {
        let vol = volatility(&q.closes);
        let mut ranked = profile.clone();
        ranked.sort_by(|a, b| (a.1 - vol).abs().total_cmp(&(b.1 - vol).abs()));
        Ok(IdentityAnswer {
            tickers: ranked.into_iter().map(|(s, _)| s).collect(),
            year: None,
        })
    })
}

fn probe(ask: &mut Ask<'_>) -> IdentifiabilityReport {
    let data = frozen();
    identifiability_probe(
        &data,
        &windows(&data),
        IdentifiabilityConfig::default(),
        ask,
    )
    .unwrap()
}

#[test]
fn a_path_matcher_re_identifies_every_masked_window_and_no_twin() {
    let public = frozen();
    let report = probe(&mut *path_matcher(&public));
    eprintln!("{report:#?}");
    assert_eq!(report.windows, 12);
    assert_eq!(report.symbols_per_window, 3);
    assert_eq!(report.real.questions, 36);
    // The negative control: the mask hides names, dates and level, not paths.
    assert_eq!(report.real.ticker_top1, 1.0);
    assert_eq!(report.real.year, 1.0);
    assert_eq!(report.real.joint, 1.0);
    assert_eq!(report.twin.joint, 0.0);
    assert!(report.joint_edge.edges.iter().all(|e| *e == 1.0));
    assert!(report.path_identified, "{report:?}");
    assert_eq!(report.chance.ticker_top1, 1.0 / 3.0);
    assert_eq!(report.chance.ticker_top_k, 2.0 / 3.0);
    assert!(report.chance.year > 0.0 && report.chance.year < 0.2);
}

#[test]
fn a_texture_reader_identifies_the_twin_as_often_as_the_real_window() {
    let public = frozen();
    let report = probe(&mut *texture_reader(&public));
    eprintln!("{report:#?}");
    assert_eq!(report.real.joint, 0.0);
    assert_eq!(report.twin.joint, 0.0);
    assert!(!report.path_identified, "{report:?}");
    assert!(
        (report.real.ticker_top1 - report.twin.ticker_top1).abs() <= 0.15,
        "{report:?}"
    );
}

#[test]
fn questions_carry_no_arm_and_answers_beyond_top_k_are_ignored() {
    let data = frozen();
    let w = windows(&data);
    let mut seen = Vec::new();
    // Lists every candidate: only the first two count, so listing them all is
    // never a free hit.
    let report =
        identifiability_probe(&data, &w[..2], IdentifiabilityConfig::default(), &mut |q| {
            seen.push(q.clone());
            let mut tickers = q.candidate_tickers.clone();
            tickers.reverse();
            Ok::<_, ()>(IdentityAnswer {
                tickers,
                year: q.candidate_years.first().copied(),
            })
        })
        .unwrap();
    assert_eq!(seen.len(), 12);
    assert!(seen.iter().enumerate().all(|(i, q)| q.question_id == i));
    assert!(seen
        .iter()
        .all(|q| q.symbol.starts_with("ASSET_") && q.dates[0] == "t0"));
    // Reversed order ranks the last two of three tickers first.
    assert_eq!(report.real.ticker_top_k, 2.0 / 3.0);
    assert_eq!(report.real.ticker_top1, 1.0 / 3.0);
    // Always the first candidate year (2016): right for the first window only.
    assert_eq!(report.real.year, 0.5);
    let text = serde_json::to_string(&seen[0]).unwrap();
    assert!(!text.contains("real") && !text.contains("twin"), "{text}");
}

#[test]
fn the_probe_refuses_rather_than_scores_what_it_cannot() {
    let data = frozen();
    let w = windows(&data);
    let mut ask = |_: &IdentityQuestion| Err::<IdentityAnswer, _>("timeout");
    assert_eq!(
        identifiability_probe(&data, &w, IdentifiabilityConfig::default(), &mut ask),
        Err(IdentifiabilityUnavailable::Answer {
            question_id: 0,
            error: "timeout"
        })
    );
    let zero_k = IdentifiabilityConfig {
        top_k: 0,
        ..IdentifiabilityConfig::default()
    };
    assert_eq!(
        identifiability_probe(&data, &w, zero_k, &mut ask),
        Err(IdentifiabilityUnavailable::InvalidConfig)
    );
    let negative = IdentifiabilityConfig {
        year_tolerance: -1,
        ..IdentifiabilityConfig::default()
    };
    assert_eq!(
        identifiability_probe(&data, &w, negative, &mut ask),
        Err(IdentifiabilityUnavailable::InvalidConfig)
    );
    assert_eq!(
        identifiability_probe(&data, &[], IdentifiabilityConfig::default(), &mut ask),
        Err(IdentifiabilityUnavailable::NoWindows)
    );
    let undated = Dataset::synthetic(2, 200, 4);
    assert_eq!(
        identifiability_probe(
            &undated,
            &[Window { start: 20, end: 60 }],
            IdentifiabilityConfig::default(),
            &mut ask
        ),
        Err(IdentifiabilityUnavailable::UndatedWindow { window: 0 })
    );
}
