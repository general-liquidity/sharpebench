//! The short borrow rate is part of the cost-model identity.
//!
//! A zero rate must leave every existing cost-model digest exactly where it
//! was, so committed checkpoints, trajectory contracts and rescore bundles
//! written before the field existed still match. A set rate must change the
//! digest, so a checkpoint or trajectory captured under one rate cannot be
//! resumed or verified under another.

use sharpebench_harness::{
    cost_model_digest, run_resumable_sweep_bound, trajectory_contract, SweepContract, SweepIdentity,
};
use sharpebench_sim::{CostModel, CostProfile, Dataset, Window};

/// Digests computed on the tree before the borrow field existed.
const PINNED: [(&str, &str); 4] = [
    (
        "frictionless",
        "c430889586cbc97783661db4b1c75a2bffea3eac6cc04262942b800af05fc85b",
    ),
    (
        "typical",
        "2076df8b8a55406134d960434e051e64859bf6c54e4bffb212e353b1e111442a",
    ),
    (
        "stressed",
        "7e006b484fb67dbfb82f7bfb0d34a86ae8e5eb8ce160aace62b06ed487dac917",
    ),
    (
        "realistic",
        "a740edec2f418fc4e064b5e1e1adb8777d8ea2a34569ecbee98ed69a49c38387",
    ),
];

fn profile(name: &str) -> CostModel {
    [
        CostProfile::None,
        CostProfile::Typical,
        CostProfile::WorstCase,
        CostProfile::Realistic,
    ]
    .into_iter()
    .find(|profile| profile.name() == name)
    .expect("a pinned name is a shipped profile")
    .resolve()
    .costs
}

fn with_borrow(costs: CostModel, short_borrow_bps: f64) -> CostModel {
    CostModel {
        short_borrow_bps,
        ..costs
    }
}

#[test]
fn a_zero_borrow_rate_keeps_every_named_profile_digest() {
    for (name, digest) in PINNED {
        assert_eq!(cost_model_digest(profile(name)), digest, "{name}");
        for zero in [0.0, -0.0] {
            assert_eq!(
                cost_model_digest(with_borrow(profile(name), zero)),
                digest,
                "{name} {zero}"
            );
        }
    }
    assert_eq!(cost_model_digest(CostModel::default()), PINNED[1].1);
}

#[test]
fn a_set_borrow_rate_changes_the_digest_and_each_rate_is_distinct() {
    for (name, digest) in PINNED {
        let a = cost_model_digest(with_borrow(profile(name), 25.0));
        let b = cost_model_digest(with_borrow(profile(name), 30.0));
        let tiny = cost_model_digest(with_borrow(profile(name), f64::MIN_POSITIVE));
        assert_ne!(a, digest, "{name}");
        assert_ne!(tiny, digest, "{name}");
        assert_ne!(a, b, "{name}");
        assert_ne!(a, tiny, "{name}");
    }
}

#[test]
fn a_trajectory_contract_binds_the_borrow_rate() {
    let data = Dataset::synthetic(2, 60, 5);
    let windows = [Window { start: 20, end: 60 }];
    let base = trajectory_contract(&data, CostModel::default(), &windows, &[0]);
    let borrowed = trajectory_contract(
        &data,
        with_borrow(CostModel::default(), 25.0),
        &windows,
        &[0],
    );
    assert_ne!(base.cost_model_sha256, borrowed.cost_model_sha256);
    assert_eq!(base.dataset_sha256, borrowed.dataset_sha256);
}

fn sweep_contract(costs: CostModel, windows: &[Window], seeds: &[u64]) -> SweepContract {
    SweepContract::new(
        SweepIdentity {
            dataset_sha256: "11".repeat(32),
            cost_model_sha256: cost_model_digest(costs),
            score_config_sha256: "33".repeat(32),
            runner_artifact_sha256: "44".repeat(32),
            entrant_sha256: "55".repeat(32),
            invocation_sha256: "66".repeat(32),
        },
        windows,
        seeds,
        1,
    )
}

fn flat_run() -> sharpebench_core::Run {
    sharpebench_core::Run {
        returns: vec![0.001; 40],
        ..sharpebench_core::Run::default()
    }
}

#[test]
fn a_checkpoint_written_under_one_borrow_rate_refuses_to_resume_under_another() {
    let path = std::env::temp_dir().join(format!(
        "sharpebench-short-borrow-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let windows = [Window { start: 20, end: 60 }];
    let seeds = [0u64, 1];
    let borrowed = with_borrow(CostModel::default(), 25.0);
    let first = sweep_contract(borrowed, &windows, &seeds);
    let attempt = |_w: usize, _seed: u64| Ok(flat_run());
    run_resumable_sweep_bound(&path, "ext", &first, &windows, &seeds, 1, attempt)
        .expect("the first sweep writes its checkpoint");

    // The same rate is the same experiment and resumes.
    run_resumable_sweep_bound(&path, "ext", &first, &windows, &seeds, 1, attempt)
        .expect("the same borrow rate resumes");
    let written = std::fs::read(&path).unwrap();

    for other in [
        CostModel::default(),
        with_borrow(CostModel::default(), 30.0),
    ] {
        let changed = sweep_contract(other, &windows, &seeds);
        let error =
            match run_resumable_sweep_bound(&path, "ext", &changed, &windows, &seeds, 1, attempt) {
                Ok(_) => panic!("a different borrow rate must not resume"),
                Err(error) => error,
            };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("contract differs"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), written);
    }
    let _ = std::fs::remove_file(&path);
}
