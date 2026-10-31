//! The producer's contract: deterministic across workers, resumable, unable to
//! touch frozen evidence, unable to call itself frozen without authority, and
//! reading its configuration back from the kernel.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use sharpebench_core::composite::rank;
use sharpebench_joint_gate::campaign::*;
use sharpebench_joint_gate::controls::run_control;
use sharpebench_joint_gate::fields::{
    agent_id, draw_field, read_back, score_cell, submissions, Cell, FOCAL,
};
use sharpebench_joint_gate::gate::{authorize_frozen, run_identity};
use sharpebench_joint_gate::report::{build, decide, CellEstimate, Outcome};
use sharpebench_joint_gate::shards::{
    cell_table, check_output_dir, engine_fingerprint, load_rows, run_all, RunError, Tier,
};
use sharpebench_study::{Interval, StudyProtocol};

const SEALED_CAMPAIGN: &str = include_str!("../../../docs/studies/joint-gate/campaign.json");
const SEALED_STATISTICAL: &str =
    include_str!("../../../docs/studies/joint-gate/statistical.protocol.json");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory unique to this test, outside the repository.
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jgs-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn rule(id: &str, windows: usize, host_n: u32, prior: f64, bar: f64) -> Rule {
    Rule {
        id: id.to_string(),
        host_n,
        dispersion_prior: prior,
        windows,
        per_run_psr_bar: bar,
        dsr_bar: 0.95,
        reliability: Reliability::AllRuns,
    }
}

fn iid() -> Dgp {
    Dgp {
        id: "iid".to_string(),
        rho: 0.0,
        tail: Tail::Normal,
        garch: None,
        regime: None,
        shared_variance: 0.0,
    }
}

fn block(id: &str, role: BlockRole, effects: Vec<f64>) -> StatBlock {
    StatBlock {
        id: id.to_string(),
        role,
        replicates: 4,
        dgps: vec!["iid".to_string()],
        history_bars: vec![120],
        effects_annual: effects,
        rules: vec!["small".to_string()],
        field_size: 3,
        execution_seeds: 2,
        seed_noise_eta: 0.1,
        block_probs: vec![0.1],
        seed_paths: vec![SeedPath::Average],
    }
}

fn campaign() -> Campaign {
    let c = Campaign {
        contract: CAMPAIGN_CONTRACT.to_string(),
        campaign_id: "test".to_string(),
        master_seed: 42,
        periods_per_year: 252.0,
        vol_per_period: 0.01,
        shard_replicates: 2,
        rules: vec![rule("small", 2, 3, 0.5, 0.9)],
        dgps: vec![iid()],
        statistical: vec![
            block("nulls", BlockRole::PrimaryNull, vec![0.0]),
            block("alts", BlockRole::PrimaryPower, vec![3.0]),
        ],
        integrity: Vec::new(),
        decision: DecisionPlan {
            whole_field_limit: 0.05,
            power_bound: 0.8,
            power_targets: vec![PowerTarget {
                effect_annual: 3.0,
                history_bars: 120,
            }],
            detection_bound: 0.99,
        },
    };
    c.validate().unwrap();
    c
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(dir.join("shards"))
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().to_string(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

fn smoke_identity(c: &Campaign) -> sharpebench_joint_gate::shards::RunIdentity {
    run_identity(Tier::Smoke, c, "fingerprint".to_string(), Some(4)).unwrap()
}

#[test]
fn bytes_do_not_depend_on_the_worker_count() {
    let c = campaign();
    let identity = smoke_identity(&c);
    let (one, many) = (scratch(), scratch());
    run_all(&c, &identity, &one, 1, None).unwrap();
    run_all(&c, &identity, &many, 4, None).unwrap();
    assert_eq!(snapshot(&one), snapshot(&many));
}

#[test]
fn an_interrupted_run_resumes_to_the_same_bytes_and_does_not_redo_work() {
    let c = campaign();
    let identity = smoke_identity(&c);
    let (whole, split) = (scratch(), scratch());
    let whole_timing = run_all(&c, &identity, &whole, 2, None).unwrap();
    let first = run_all(&c, &identity, &split, 2, Some(1)).unwrap();
    assert_eq!(first.shards_run, 1);
    // A kill leaves a stray temporary behind; it must not be taken for a shard.
    fs::write(split.join("shards/stray.tmp"), b"half a shard").unwrap();
    let second = run_all(&c, &identity, &split, 2, None).unwrap();
    assert_eq!(second.shards_skipped, 1);
    assert_eq!(second.shards_run, whole_timing.shards_run - 1);
    fs::remove_file(split.join("shards/stray.tmp")).unwrap();
    assert_eq!(snapshot(&whole), snapshot(&split));
}

#[test]
fn a_shard_from_another_run_is_refused_and_not_overwritten() {
    let c = campaign();
    let dir = scratch();
    run_all(&c, &smoke_identity(&c), &dir, 2, None).unwrap();
    let before = snapshot(&dir);
    let other = run_identity(Tier::Smoke, &c, "another-engine".to_string(), Some(4)).unwrap();
    fs::remove_file(dir.join("run-identity.json")).unwrap();
    let error = run_all(&c, &other, &dir, 2, None).unwrap_err();
    assert!(
        matches!(&error, RunError::Refused(d) if d.contains("different run")),
        "{error}"
    );
    assert_eq!(before, snapshot(&dir));
}

#[test]
fn a_short_shard_is_refused() {
    let c = campaign();
    let identity = smoke_identity(&c);
    let dir = scratch();
    run_all(&c, &identity, &dir, 2, None).unwrap();
    let shard = fs::read_dir(dir.join("shards"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = fs::read_to_string(&shard).unwrap();
    let cut: Vec<&str> = text.lines().collect();
    fs::write(&shard, cut[..cut.len() - 1].join("\n")).unwrap();
    assert!(load_rows(&c, &identity, &dir).is_err());
}

#[test]
fn a_report_needs_every_shard() {
    let c = campaign();
    let identity = smoke_identity(&c);
    let dir = scratch();
    run_all(&c, &identity, &dir, 2, Some(1)).unwrap();
    let error = load_rows(&c, &identity, &dir).unwrap_err();
    assert!(matches!(&error, RunError::Refused(d) if d.contains("missing")));
}

#[test]
fn frozen_evidence_cannot_be_an_output_however_it_is_spelled() {
    let repo = scratch();
    for out in [
        "paper/evidence/final",
        "paper/evidence/final/run-1",
        "paper/evidence/scratch/../final/run-2",
        "PAPER/evidence/Development/x",
    ] {
        let target = repo.join(out);
        assert!(
            matches!(
                check_output_dir(&target, &repo),
                Err(RunError::Refused(ref d)) if d.contains("frozen evidence")
            ),
            "{out}"
        );
    }
    check_output_dir(&repo.join("paper/evidence/joint-gate-study"), &repo).unwrap();
    check_output_dir(&repo.join("paper/evidence/final-rerun"), &repo).unwrap();
}

#[test]
fn a_smoke_run_never_touches_the_frozen_draws() {
    // Six entrants reach the measurement floor. Three would report the
    // configured prior on both tiers, and equal bars would prove nothing.
    let mut c = campaign();
    for block in &mut c.statistical {
        block.field_size = 6;
    }
    let (smoke, frozen) = (scratch(), scratch());
    let frozen_identity = run_identity(Tier::Frozen, &c, "f".to_string(), None).unwrap();
    run_all(&c, &smoke_identity(&c), &smoke, 2, None).unwrap();
    run_all(&c, &frozen_identity, &frozen, 2, None).unwrap();
    let rows = |dir, identity| load_rows(&c, identity, dir).unwrap().0;
    let (a, b) = (
        rows(&smoke, &smoke_identity(&c)),
        rows(&frozen, &frozen_identity),
    );
    let bars = |r: &[sharpebench_joint_gate::fields::Row]| {
        r.iter().map(|x| x.bar_annual.to_bits()).collect::<Vec<_>>()
    };
    assert_eq!(a.len(), b.len());
    assert_ne!(
        bars(&a),
        bars(&b),
        "a smoke run must draw from a disjoint stream"
    );
}

#[test]
fn run_tiers_refuse_their_wrong_shapes() {
    let c = campaign();
    assert!(run_identity(Tier::Frozen, &c, "f".to_string(), Some(2)).is_err());
    assert!(run_identity(Tier::Smoke, &c, "f".to_string(), None).is_err());
}

fn approved(mut p: StudyProtocol) -> StudyProtocol {
    p.budget.approved = true;
    p.budget.approver = "test-owner".to_string();
    p
}

#[test]
fn a_frozen_run_needs_approval_and_the_bound_campaign_and_engine() {
    let sealed_campaign = Campaign::from_json(SEALED_CAMPAIGN).unwrap();
    let protocol = StudyProtocol::from_json(SEALED_STATISTICAL).unwrap();
    let engine = protocol.identity.scoring_spec_hash.clone();

    let refused = |p: &StudyProtocol, c: &Campaign, e: &str| match authorize_frozen(
        std::slice::from_ref(p),
        c,
        e,
    ) {
        Err(RunError::Refused(detail)) => detail,
        other => panic!("expected a refusal, got {other:?}"),
    };

    assert!(refused(&protocol, &sealed_campaign, &engine).contains("not approved"));
    assert!(authorize_frozen(&[], &sealed_campaign, &engine).is_err());

    let ready = approved(protocol);
    authorize_frozen(std::slice::from_ref(&ready), &sealed_campaign, &engine).unwrap();

    let mut moved = sealed_campaign.clone();
    moved.master_seed += 1;
    assert!(refused(&ready, &moved, &engine).contains("campaign moved"));
    assert!(refused(&ready, &sealed_campaign, "0000000000000000").contains("engine moved"));
}

#[test]
fn the_kernel_is_read_back_and_a_wrong_declaration_stops_the_cell() {
    let c = campaign();
    let dgp = iid();
    let field = draw_field(&dgp, [1, 2, 3, 4, 5], 3, 120, 2, 0.1);
    let subs = submissions(&field, 0.0, 2, &[0, 1], 0.1, 0.01);
    let r = &c.rules[0];
    let cfg = r.score_config(252.0, 2, 0.1);
    let board = rank(&subs, &cfg);
    read_back(&board, r, &cfg, 3, 120, 4).unwrap();
    assert!(read_back(&board, r, &cfg, 3, 121, 4).is_err(), "bars");
    assert!(read_back(&board, r, &cfg, 3, 120, 5).is_err(), "runs");
    assert!(read_back(&board, r, &cfg, 4, 120, 4).is_err(), "field");
    let lied = Rule {
        host_n: 9,
        ..r.clone()
    };
    assert!(read_back(&board, &lied, &cfg, 3, 120, 4).is_err(), "n");
    let lied = Rule {
        dispersion_prior: 0.1,
        ..r.clone()
    };
    assert!(read_back(&board, &lied, &cfg, 3, 120, 4).is_err(), "prior");
}

/// The field is in the measurement: a skilled focal entrant raises its own bar
/// through measured dispersion, and the host N rises to the field size. A
/// fixed-bar calculation has neither.
#[test]
fn field_effects_reach_the_verdict() {
    let mut c = campaign();
    c.rules = vec![rule("small", 2, 3, 0.05, 0.9)];
    c.statistical[0].field_size = 9;
    let b = c.statistical[0].clone();
    let dgp = iid();
    let field = draw_field(&dgp, [9, 9, 9, 9, 9], 9, 120, 2, 0.1);
    let mut bars = Vec::new();
    for effect in [0.0, 30.0] {
        let cell = Cell {
            block: "nulls".to_string(),
            dgp: "iid".to_string(),
            history_bars: 120,
            effect_annual: effect,
            rule: "small".to_string(),
            block_prob: 0.1,
            seed_path: SeedPath::Average,
        };
        bars.push(score_cell(&c, &b, &cell, &field).unwrap().bar_annual);
    }
    assert!(
        bars[1] > bars[0] * 1.5,
        "planted skill must raise the measured bar: {bars:?}"
    );
    let cfg = c.rules[0].score_config(252.0, 2, 0.1);
    let subs = submissions(&field, 0.0, 2, &[0, 1], 0.1, 0.01);
    assert_eq!(rank(&subs, &cfg)[0].effective_n_trials, 9);
    assert_eq!(agent_id(0), FOCAL);
}

#[test]
fn per_seed_paths_can_only_be_stricter_than_each_seed() {
    let mut c = campaign();
    let mut b = c.statistical[1].clone();
    b.seed_paths = vec![SeedPath::PerSeed];
    b.seed_noise_eta = 0.5;
    b.field_size = 2;
    c.statistical = vec![b.clone()];
    let field = draw_field(&iid(), [3, 1, 4, 1, 5], 2, 120, 2, 0.5);
    let cell = Cell {
        block: b.id.clone(),
        dgp: "iid".to_string(),
        history_bars: 120,
        effect_annual: 200.0,
        rule: "small".to_string(),
        block_prob: 0.1,
        seed_path: SeedPath::PerSeed,
    };
    let row = score_cell(&c, &b, &cell, &field).unwrap();
    assert!(row.focal_eligible, "a planted edge this size clears both");
    assert!(!row.any_false);
}

fn estimate(
    block: &str,
    dgp: &str,
    rule: &str,
    effect: f64,
    upper: f64,
    lower: f64,
) -> CellEstimate {
    let i = |lower, upper| Interval { lower, upper };
    CellEstimate {
        cell: 0,
        block: block.to_string(),
        dgp: dgp.to_string(),
        history_bars: 120,
        effect_annual: effect,
        rule: rule.to_string(),
        block_prob: 0.1,
        fields: 100,
        focal_eligible: 0,
        any_false: 0,
        unavailable_entrants: 0,
        focal_interval: i(lower, 1.0),
        any_false_interval: i(0.0, upper),
        legs_dsr: 0,
        legs_reliability: 0,
        legs_bootstrap: 0,
        mean_bar_annual: 0.0,
    }
}

#[test]
fn each_of_the_three_stated_outcomes_is_reachable() {
    let c = campaign();
    let run = |null_upper, power_lower| {
        decide(
            &c,
            &[
                estimate("nulls", "iid", "small", 0.0, null_upper, 0.0),
                estimate("alts", "iid", "small", 3.0, 0.0, power_lower),
            ],
        )
        .1
        .unwrap()
    };
    assert_eq!(
        run(0.04, 0.85),
        Outcome::FeasibleRule {
            rules: vec!["small".to_string()]
        }
    );
    assert_eq!(
        run(0.04, 0.70),
        Outcome::NoFeasibleGeometry {
            size_controlled_rules: vec!["small".to_string()]
        }
    );
    assert_eq!(run(0.06, 0.85), Outcome::SizeNotControlled);
    // At the boundary the rule holds: both comparisons are inclusive.
    assert!(matches!(run(0.05, 0.80), Outcome::FeasibleRule { .. }));
}

#[test]
fn one_bad_null_cell_fails_the_rule() {
    let c = campaign();
    let (findings, _) = decide(
        &c,
        &[
            estimate("nulls", "iid", "small", 0.0, 0.01, 0.0),
            estimate("nulls", "ar", "small", 0.0, 0.20, 0.0),
            estimate("alts", "iid", "small", 3.0, 0.0, 0.9),
        ],
    );
    assert!(!findings[0].size_controlled);
    assert_eq!(findings[0].worst_null_dgp, "ar");
}

#[test]
fn a_smoke_report_states_no_outcome() {
    let c = campaign();
    let identity = smoke_identity(&c);
    let dir = scratch();
    run_all(&c, &identity, &dir, 2, None).unwrap();
    let (rows, controls) = load_rows(&c, &identity, &dir).unwrap();
    assert_eq!(rows.len(), 8);
    let report = build(&c, Tier::Smoke, &rows, &controls);
    assert_eq!(report.outcome, None);
    assert_eq!(report.integrity_passed, None);
    assert_eq!(report.cells.len(), cell_table(&c).len());
    assert_eq!(report.cells[0].fields, 4);
}

fn control(kind: ControlKind) -> IntegrityBlock {
    IntegrityBlock {
        kind,
        replicates: 1,
        // Long enough for a lone Sharpe 8 to clear the dispersion it raises
        // itself. On 300 bars it does not, which is a property of the bar and
        // would leave the twin ineligible for a reason unrelated to the control.
        history_bars: 1260,
        windows: 3,
        execution_seeds: 2,
        rule: "control".to_string(),
    }
}

fn control_campaign() -> Campaign {
    let mut c = campaign();
    c.rules.push(rule("control", 3, 50, 0.5, 0.9));
    c
}

/// Each control exercises its own mechanism: the twin clears, the violating
/// copy does not, and for the two re-execution controls the scorer alone was
/// not what refused them.
#[test]
fn every_planted_violation_is_refused_by_its_own_mechanism() {
    let c = control_campaign();
    for kind in [ControlKind::ProcessViolation, ControlKind::MandateBreach] {
        let row = run_control(&c, &control(kind), 7, 0);
        assert!(row.twin_ok, "{kind:?}: the clean twin must be admitted");
        assert!(row.detected, "{kind:?}");
    }
    let row = run_control(&c, &control(ControlKind::ConstantWindowGriefer), 7, 0);
    assert!(row.twin_ok && row.detected, "{row:?}");
    for kind in [ControlKind::LookAheadOracle, ControlKind::Fabrication] {
        let row = run_control(&c, &control(kind), 7, 0);
        assert!(
            row.detected,
            "{kind:?}: re-execution must refuse it: {row:?}"
        );
    }
}

#[test]
fn the_look_ahead_oracle_earns_what_a_look_ahead_oracle_earns() {
    // Without this a broken oracle would pass re-execution-control trivially.
    let c = control_campaign();
    let row = run_control(&c, &control(ControlKind::LookAheadOracle), 7, 0);
    assert!(row.measure > 0.9, "oracle DSR {}", row.measure);
}

#[test]
fn a_campaign_with_an_unknown_key_or_name_is_refused() {
    assert!(Campaign::from_json(&SEALED_CAMPAIGN.replacen(
        "\"campaign_id\"",
        "\"campaign_idd\"",
        1
    ))
    .is_err());
    let mut c = campaign();
    c.statistical[0].rules = vec!["nobody".to_string()];
    assert!(c.validate().is_err());
    let mut c = campaign();
    c.statistical[0].effects_annual = vec![1.0];
    assert!(
        c.validate().is_err(),
        "a null block may not plant an effect"
    );
    let mut c = campaign();
    c.statistical[0].history_bars = vec![121];
    assert!(c.validate().is_err(), "windows must divide the history");
}

#[test]
fn the_fingerprint_is_stable_within_a_build() {
    assert_eq!(engine_fingerprint(), engine_fingerprint());
    assert_eq!(engine_fingerprint().len(), 16);
}

/// The drift gate over the sealed files. It names which side moved: a failure
/// on the engine line means the scorer changed and the protocol needs an
/// amendment at a raised version, never an edit of this expectation.
#[test]
fn the_seal_binds_the_committed_files_and_the_linked_engine() {
    let seal: serde_json::Value =
        serde_json::from_str(include_str!("../../../docs/studies/joint-gate/seal.json")).unwrap();
    let campaign = Campaign::from_json(SEALED_CAMPAIGN).unwrap();
    assert_eq!(seal["campaign_digest"], campaign.digest(), "campaign moved");
    for text in [
        SEALED_STATISTICAL,
        include_str!("../../../docs/studies/joint-gate/integrity.protocol.json"),
    ] {
        let protocol = StudyProtocol::from_json(text).unwrap();
        assert_eq!(protocol.identity.configuration_digest, campaign.digest());
        assert_eq!(
            seal["protocol_digests"][&protocol.protocol_id],
            sharpebench_joint_gate::gate::protocol_digest(&protocol),
            "protocol {} moved",
            protocol.protocol_id
        );
        assert_eq!(
            protocol.identity.scoring_spec_hash,
            engine_fingerprint(),
            "engine moved: the scorer no longer matches the sealed protocol; amend at a raised version"
        );
    }
}

#[test]
fn every_rule_and_generator_in_the_campaign_is_used() {
    let c = Campaign::from_json(SEALED_CAMPAIGN).unwrap();
    assert!(c.rules.len() <= 10, "Bonferroni at 0.005 covers ten rules");
    for rule in &c.rules {
        assert!(c
            .statistical
            .iter()
            .any(|b| b.role == BlockRole::PrimaryNull && b.rules.contains(&rule.id)));
    }
    for dgp in &c.dgps {
        assert!(c.statistical[0].dgps.contains(&dgp.id));
    }
}

#[test]
fn a_two_thirds_share_is_two_thirds_of_the_runs() {
    let r = Rule {
        reliability: Reliability::AtLeastShare { share: 2.0 / 3.0 },
        ..rule("m", 6, 50, 0.5, 0.9)
    };
    assert_eq!(
        r.score_config(252.0, 8, 0.1).pass_mode,
        sharpebench_core::PassMode::AtLeast(32)
    );
}
