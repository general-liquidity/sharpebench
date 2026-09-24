//! A near-constant track no longer sets everybody else's deflation bar.
//!
//! The attack is a track that never moved: one value repeated, with a 1e-12
//! perturbation in each window so that no exact-value predicate recognizes it.
//! Every window varies inside itself, so the windowed refusal passes it; the
//! pooled Sharpe is then the level divided by the perturbation, near 1e10. That
//! one vote became the whole field's measured Sharpe dispersion, the deflation
//! bar followed it, and every honest entrant dropped from a deflated Sharpe of
//! 1.0 and rank-eligible to 0.0 and not.
//!
//! The fence excludes the vote from the dispersion by its distance from the
//! field, not by any property of its values, and leaves the attacker on the bar
//! the field measures with its own vote in it, so being fenced is never a
//! discount.

use sharpebench_core::{rank, AgentSubmission, CompositeScore, Run, ScoreConfig};
use sharpebench_stats::stats::std_dev;

/// The kernel's `DISPERSION_VOTE_FENCE_Z`, restated here rather than imported
/// so this file compiles against the tree that has no fence and fails there on
/// its assertions instead of on a missing name.
const FENCE_Z: f64 = 10.75;

/// Per-period Sharpes of an honest field that clears the bar: six agents whose
/// pooled tracks are ordinary and whose dispersion is well inside the fence.
const HONEST: [(&str, f64); 6] = [
    ("honest-0", 0.40),
    ("honest-1", 0.45),
    ("honest-2", 0.50),
    ("honest-3", 0.55),
    ("honest-4", 0.60),
    ("honest-5", 0.65),
];

/// A two-window track whose pooled per-period Sharpe is `sharpe` to rounding.
/// Each agent's carrier has its own frequency and phase, so no two agents are
/// near-clones and every window varies inside itself.
fn track(seed: usize, sharpe: f64) -> Vec<Vec<f64>> {
    let k = seed as f64;
    let raw: Vec<f64> = (0..120)
        .map(|t| {
            let t = t as f64;
            (t * (0.37 + 0.113 * k) + 0.9 * k).sin() + 0.5 * (t * (1.61 + 0.07 * k) + k).cos()
        })
        .collect();
    let mean = raw.iter().sum::<f64>() / raw.len() as f64;
    let centred: Vec<f64> = raw.iter().map(|x| x - mean).collect();
    let sd = std_dev(&centred);
    let b = 0.01;
    let z: Vec<f64> = centred.iter().map(|x| sharpe * b + b * x / sd).collect();
    vec![z[..60].to_vec(), z[60..].to_vec()]
}

/// The attack: 0.001 every bar, with one bar of each window moved by 1e-12.
/// `offset` shifts which bar moves, which changes nothing a predicate on values
/// can see and keeps the copies near-clones of each other.
fn near_constant_track(offset: usize) -> Vec<Vec<f64>> {
    (0..2)
        .map(|w| {
            let mut window = vec![0.001_f64; 60];
            window[(20 + offset + 7 * w) % 60] += 1e-12;
            window
        })
        .collect()
}

fn agent(id: &str, windows: Vec<Vec<f64>>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.into(),
        runs: windows
            .into_iter()
            .map(|returns| Run {
                returns,
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn honest_field() -> Vec<AgentSubmission> {
    HONEST
        .iter()
        .enumerate()
        .map(|(i, &(id, sharpe))| agent(id, track(i, sharpe)))
        .collect()
}

fn cfg() -> ScoreConfig {
    ScoreConfig::for_periods_per_year(252.0)
}

fn row<'a>(board: &'a [CompositeScore], id: &str) -> &'a CompositeScore {
    board
        .iter()
        .find(|r| r.agent_id == id)
        .unwrap_or_else(|| panic!("{id} is on the board"))
}

#[test]
fn the_honest_field_is_rank_eligible_before_the_attack() {
    let board = rank(&honest_field(), &cfg());
    for (id, _) in HONEST {
        let r = row(&board, id);
        assert!(r.rank_eligible, "{id}: {r:?}");
        assert!(
            r.deflated_sharpe >= cfg().dsr_bar,
            "{id}: deflated Sharpe {}",
            r.deflated_sharpe
        );
    }
}

/// The regression. Against the tree without the fence every one of these
/// assertions fails: the honest bar moves from 0.09 to about 5e9, every honest
/// deflated Sharpe goes to 0.0 and every honest entrant loses rank eligibility.
#[test]
fn a_near_constant_entrant_does_not_move_the_honest_bar() {
    let clean = rank(&honest_field(), &cfg());
    let mut attacked_field = honest_field();
    attacked_field.push(agent("near-constant", near_constant_track(0)));
    let attacked = rank(&attacked_field, &cfg());

    for (id, _) in HONEST {
        let before = row(&clean, id);
        let after = row(&attacked, id);
        assert_eq!(
            after.trials_sr_std.to_bits(),
            before.trials_sr_std.to_bits(),
            "{id}: the entrant moved the measured dispersion"
        );
        assert_eq!(
            after.deflation_bar_per_period.to_bits(),
            before.deflation_bar_per_period.to_bits(),
            "{id}: the entrant moved the bar"
        );
        assert_eq!(
            after.deflated_sharpe.to_bits(),
            before.deflated_sharpe.to_bits(),
            "{id}: the entrant moved the deflated Sharpe"
        );
        assert!(after.rank_eligible, "{id}: the entrant cost it eligibility");
    }
}

/// Exclusion is not a discount: the fenced entrant keeps the bar the field
/// measures with its own vote in it, which is the bar it had before the fence
/// existed, and it is not rank-eligible on it.
#[test]
fn the_fenced_entrant_keeps_the_bar_measured_with_its_own_vote() {
    let mut field = honest_field();
    field.push(agent("near-constant", near_constant_track(0)));
    let board = rank(&field, &cfg());
    let attacker = row(&board, "near-constant");
    let honest = row(&board, "honest-0");

    assert!(
        attacker.trials_sr_std > 1e8,
        "the fenced row carries the dispersion its own vote makes: {}",
        attacker.trials_sr_std
    );
    assert!(
        attacker.trials_sr_std > honest.trials_sr_std,
        "a fenced entrant must never be measured against a lighter bar than the field it was \
         excluded from"
    );
    assert!(!attacker.rank_eligible, "{attacker:?}");
}

/// The clone collapse makes a flood of near-identical attackers one vote. That
/// vote is fenced, and fencing it fences every stream it spoke for: otherwise
/// one copy would carry the huge bar and the rest would inherit a bar measured
/// without them.
#[test]
fn a_flood_of_near_clone_attackers_is_fenced_whole() {
    let mut field = honest_field();
    for k in 0..3 {
        field.push(agent(&format!("near-constant-{k}"), near_constant_track(k)));
    }
    let board = rank(&field, &cfg());
    let honest_bar = row(&board, "honest-0").trials_sr_std;
    assert!(honest_bar < 1.0, "the honest field still measures itself");
    for k in 0..3 {
        let id = format!("near-constant-{k}");
        let r = row(&board, &id);
        assert!(r.trials_sr_std > 1e8, "{id}: {}", r.trials_sr_std);
        assert!(!r.rank_eligible, "{id}");
    }
}

/// Nothing is fenced on a field whose votes are ordinary, so every number is
/// the one the plain standard deviation of every vote gives, bit for bit.
#[test]
fn an_ordinary_field_is_measured_exactly_as_before() {
    let board = rank(&honest_field(), &cfg());
    let mut votes: Vec<f64> = HONEST.iter().map(|&(_, s)| s).collect();
    votes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let measured = std_dev(&votes);
    for (id, _) in HONEST {
        let got = row(&board, id).trials_sr_std;
        assert!(
            (got - measured).abs() < 1e-3,
            "{id}: {got} against the field's own dispersion {measured}"
        );
    }
}

// --- where the fence actually stands -----------------------------------------

/// The votes the kernel measures, as the dispersion sorts them.
fn votes_of(field: &[AgentSubmission]) -> Vec<f64> {
    let mut v: Vec<f64> = field
        .iter()
        .map(|s| {
            let pooled: Vec<f64> = s.runs.iter().flat_map(|r| r.returns.clone()).collect();
            sharpebench_stats::deflated_sharpe::observed_sharpe_ratio(&pooled)
                .expect("every track varies")
        })
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// How far a vote may sit from this field's median, restated from the kernel's
/// rule so a change to the kernel's scale or constant moves the boundary the
/// two probes below straddle.
fn reach_of(sorted_votes: &[f64], scale_floor: f64) -> (f64, f64) {
    let centre = median(sorted_votes);
    let scale = pairwise_median_scale(sorted_votes).max(scale_floor);
    (centre, FENCE_Z * scale)
}

/// The honest field, optionally one agent wider, plus a probe. Seven votes and
/// eight are both built, because the median of an odd sample is an element and
/// the median of an even one is the midpoint of two, and both the votes and
/// their pairwise gaps change parity between the two fields: seven votes give
/// twenty-one gaps and eight give twenty-eight.
fn field_with_probe(extra_honest: usize, sharpe: f64) -> Vec<AgentSubmission> {
    let mut field = honest_field();
    for k in 0..extra_honest {
        let seed = HONEST.len() + k;
        field.push(agent(
            &format!("honest-{seed}"),
            track(seed, 0.70 + 0.05 * k as f64),
        ));
    }
    field.push(agent("probe", track(HONEST.len() + extra_honest, sharpe)));
    field
}

/// A probe half a percent inside the fence votes, and the field measures the
/// dispersion of all seven votes. A probe half a percent outside it does not,
/// and the six honest rows measure the dispersion of six.
///
/// The two probes straddle one boundary, so any change to the distance the
/// kernel computes moves one of them across it: the scale, its consistency
/// constant, the centre, the direction of the comparison and the constant
/// itself are all pinned by this pair, not by a number copied out of a run.
#[test]
fn the_fence_stands_where_the_field_and_the_constant_put_it() {
    let floor = cfg().min_measured_trials_sr_std / cfg().periods_per_year.sqrt();
    for extra in [0, 1] {
        let boundary = {
            // The probe is in the sample that sets the centre and the scale, so
            // the boundary is the fixed point of "sit exactly at the reach".
            let mut sharpe = 1.0;
            for _ in 0..200 {
                let votes = votes_of(&field_with_probe(extra, sharpe));
                let (centre, reach) = reach_of(&votes, floor);
                let next = centre + reach;
                if (sharpe - next).abs() < 1e-9 {
                    break;
                }
                sharpe = next;
            }
            sharpe
        };
        let inside = boundary * 0.995;
        let outside = boundary * 1.005;
        let honest_count = HONEST.len() + extra;

        let field = field_with_probe(extra, inside);
        let all_votes = std_dev(&votes_of(&field));
        for r in &rank(&field, &cfg()) {
            assert_eq!(
                r.trials_sr_std.to_bits(),
                all_votes.to_bits(),
                "{extra}/{}: a vote inside the fence votes",
                r.agent_id
            );
        }

        let field = field_with_probe(extra, outside);
        let board = rank(&field, &cfg());
        let mut honest_votes = votes_of(&field);
        honest_votes.retain(|v| *v < outside / 2.0);
        assert_eq!(
            honest_votes.len(),
            honest_count,
            "{extra}: probe is the outlier"
        );
        let without_probe = std_dev(&honest_votes);
        for r in &board {
            let want = if r.agent_id == "probe" {
                std_dev(&votes_of(&field))
            } else {
                without_probe
            };
            assert_eq!(
                r.trials_sr_std.to_bits(),
                want.to_bits(),
                "{extra}/{}: a vote outside the fence must not vote",
                r.agent_id
            );
        }
    }
}

/// The robust scale is floored at the per-period dispersion the configuration
/// already declines to measure below. On a field whose votes agree closely the
/// unfloored scale is far smaller than that floor, and a vote that the floor
/// admits would be many scales out without it.
#[test]
fn the_dispersion_floor_floors_the_fence_scale_too() {
    let tight: Vec<AgentSubmission> = (0..6)
        .map(|i| agent(&format!("tight-{i}"), track(i, 0.40 + 0.0005 * i as f64)))
        .collect();
    let floor = cfg().min_measured_trials_sr_std / cfg().periods_per_year.sqrt();
    let mut field = tight.clone();
    // Inside the floored fence, far outside the unfloored one.
    field.push(agent("probe", track(6, 0.40 + 3.0 * floor)));
    let votes = votes_of(&field);
    let unfloored = pairwise_median_scale(&votes);
    assert!(
        unfloored * FENCE_Z < 3.0 * floor,
        "without the floor this probe is outside the fence: scale {unfloored}, floor {floor}"
    );
    let board = rank(&field, &cfg());
    let all_seven = std_dev(&votes);
    for r in &board {
        assert_eq!(
            r.trials_sr_std.to_bits(),
            all_seven.to_bits(),
            "{}: the floored scale admits this probe",
            r.agent_id
        );
    }
}

/// A field of exactly `min_field_for_measured_sr_std` votes measures; one vote
/// fewer takes the configured prior. The fence runs before that count, so a
/// field that only falls under the floor because a vote was fenced falls back
/// to the prior rather than measuring from the rump.
#[test]
fn the_smallest_measurable_field_still_measures() {
    let cfg = cfg();
    let exactly: Vec<AgentSubmission> = HONEST
        .iter()
        .take(cfg.min_field_for_measured_sr_std)
        .enumerate()
        .map(|(i, &(id, sharpe))| agent(id, track(i, sharpe)))
        .collect();
    assert_eq!(exactly.len(), cfg.min_field_for_measured_sr_std);
    for r in rank(&exactly, &cfg) {
        assert_eq!(
            r.trials_sr_std_source,
            sharpebench_core::TrialsSrStdSource::Measured,
            "{}",
            r.agent_id
        );
    }
    let one_fewer: Vec<AgentSubmission> = exactly[..exactly.len() - 1].to_vec();
    for r in rank(&one_fewer, &cfg) {
        assert_eq!(
            r.trials_sr_std_source,
            sharpebench_core::TrialsSrStdSource::Configured,
            "{}",
            r.agent_id
        );
    }
}

/// A field whose votes agree except for one states no robust scale at all: more
/// than half its pairwise gaps are exactly zero, so the pairwise median is
/// zero, and an operator who set the configured minimum dispersion to zero has
/// removed the floor that would otherwise stand in. A fence with no scale
/// measures no distance and excludes nobody, so the one vote that differs
/// still votes and the field measures all six.
///
/// The same field is the one case where a vote has no finite leverage: remove
/// it and the five that remain are bitwise equal, so the dispersion without it
/// is exactly zero. The disclosure reports that as an absent leverage rather
/// than an infinity.
///
/// Each of the five carries the same dyadic values in its own stride order, so
/// every sum, mean and square is exact and the five Sharpes agree bit for bit
/// while the streams are nowhere near clones of each other.
#[test]
fn a_field_with_no_robust_scale_fences_nobody() {
    let base: Vec<f64> = (1..=30)
        .flat_map(|j| [(1 + j) as f64 / 64.0, (1 - j) as f64 / 64.0])
        .collect();
    let strides = [1, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    let mut field: Vec<AgentSubmission> = (0..5)
        .map(|k| {
            let windows: Vec<Vec<f64>> = (0..2)
                .map(|w| {
                    let stride = strides[2 * k + w];
                    (0..60).map(|t| base[(t * stride + k) % 60]).collect()
                })
                .collect();
            agent(&format!("agrees-{k}"), windows)
        })
        .collect();
    field.push(agent("differs", track(9, 0.50)));

    let votes = votes_of(&field);
    assert!(
        votes[..5].iter().all(|v| v.to_bits() == votes[0].to_bits()),
        "the five agree bit for bit: {votes:?}"
    );
    assert_eq!(
        pairwise_median_scale(&votes),
        0.0,
        "more than half the gaps are exactly zero"
    );

    let cfg = ScoreConfig {
        min_measured_trials_sr_std: 0.0,
        ..cfg()
    };
    let all_six = std_dev(&votes);
    for r in &rank(&field, &cfg) {
        assert_eq!(
            r.trials_sr_std.to_bits(),
            all_six.to_bits(),
            "{}: a field with no scale fences nobody",
            r.agent_id
        );
        let vote = r
            .trials_sr_std_most_influential_vote
            .as_ref()
            .expect("a measured field with unequal votes names one");
        assert_eq!(vote.agent_id, "differs");
        assert_eq!(
            vote.measured_dispersion_without_it, 0.0,
            "the five that remain are equal"
        );
        assert!(
            vote.leverage.is_none(),
            "a vote that alone is the dispersion has no finite leverage: {:?}",
            vote.leverage
        );
    }
}

// --- the constant the fence borrows ------------------------------------------

/// `FENCE_C_7` is the 99th percentile of the largest robust z in an honest
/// normal field of seven, measured with the median absolute deviation when a
/// dispersion cap was proposed and rejected. The fence reuses the number with a
/// different scale, the median of the pairwise absolute differences, so the
/// reuse has to be shown conservative rather than assumed: the same percentile
/// under the pairwise scale must sit below it, with room.
///
/// Deterministic: a fixed-seed counter-based generator, no clock, no ambient
/// RNG, no market data.
#[test]
fn the_pairwise_scale_puts_an_honest_normal_field_well_inside_the_fence() {
    const FIELDS: usize = 20_000;
    let mut maxima: Vec<f64> = (0..FIELDS)
        .map(|f| {
            let mut sample: Vec<f64> = (0..7).map(|i| normal(f as u64, i as u64)).collect();
            sample.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let centre = median(&sample);
            let scale = pairwise_median_scale(&sample);
            sample
                .iter()
                .map(|x| (x - centre).abs() / scale)
                .fold(0.0_f64, f64::max)
        })
        .collect();
    maxima.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p99 = maxima[(0.99 * FIELDS as f64) as usize];
    assert!(
        p99 < FENCE_Z,
        "the borrowed constant must be looser than the pairwise scale needs: p99 {p99:.3} against \
         {FENCE_Z}"
    );
}

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        0.5 * (sorted[n / 2 - 1] + sorted[n / 2])
    }
}

/// The kernel's robust scale, restated: the median of the pairwise absolute
/// differences, scaled so it estimates a normal standard deviation.
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

/// SplitMix64 over (field, member), then Box-Muller. Reproducible everywhere.
fn normal(field: u64, member: u64) -> f64 {
    let bits = |mut z: u64| {
        z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let unit = |z: u64| ((z >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    let key =
        field.wrapping_mul(0x1000_0000_0000_0001) ^ member.wrapping_mul(0x2545_F491_4F6C_DD1D);
    let u1 = unit(bits(key));
    let u2 = unit(bits(key ^ 0xA076_1D64_78BD_642F));
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
}
