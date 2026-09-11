//! `sharpebench rescore <bundle.json>` — the trusted evaluator's recompute.
//!
//! The boundaries this command needs already exist and are composed here
//! rather than rebuilt: [`sharpebench_harness::verify_trajectory_strict`] and
//! [`sharpebench_harness::verify_trajectory_reexecuted`] refuse mismatched
//! artifacts and re-run fresh agents on frozen windows and seeds, the arena's
//! hardened launch supplies a network-disabled container, and
//! [`crate::artifact_preflight`] binds image and policy identity and refuses an
//! incomplete scan, a failed probe or an unverified cleanup. What was missing
//! is the operator-facing command that puts them around one declared artifact:
//! a **submission bundle**.
//!
//! # What a bundle is
//!
//! A bundle is a JSON document naming, by content digest, every file the
//! evaluator is allowed to read: the trajectory, the frozen dataset, the cost
//! model, and whatever else the entrant declares. The manifest is the whole
//! read set. Nothing outside it is opened, so agent state living next to the
//! bundle (a workspace, a cache, a self-reported result file) is not an input
//! and cannot move the recomputed score. A file the manifest names whose bytes
//! do not hash to the declared digest is a refusal that says which path
//! changed and what it changed from.
//!
//! # An absent manifest refuses
//!
//! [`SubmissionBundle::frozen_files`] is an `Option` so that its absence is
//! this module's own refusal rather than a parser message: a bundle with no
//! frozen-file manifest declares no read set, and a rescore with no declared
//! read set has recomputed nothing in particular. A verifier that only warns
//! about a missing manifest is a design we decline, not a defect found
//! elsewhere: an evaluator whose scoring code is imported from a separate
//! protected copy can afford to warn, because its own scorer is out of the
//! entrant's reach either way. This command has no second copy. The manifest
//! is the only thing standing between the recompute and the entrant's disk, so
//! it is required.
//!
//! # What the recompute offers, and what it cannot
//!
//! `--reexecute` offers **deterministic policy re-execution**: a fresh agent is
//! launched per captured run, driven through the same window and seed on the
//! same frozen data, and every score-bearing decision must repeat. It is not
//! recorded provider-response replay. For an entrant whose decisions depend on
//! a sampled model response, a divergence is therefore not evidence of
//! tampering and agreement is not evidence that the recorded responses were the
//! ones the entrant actually received. Every rescore report says so in
//! `not_established`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use sharpebench_core::ScoreConfig;
use sharpebench_sim::{CostModel, Dataset};

use crate::external_capture::{sandbox_factory, FaultCell, SandboxLauncher};

/// Schema of a declared submission bundle.
pub const BUNDLE_VERSION: &str = "sharpebench.submission-bundle.v1";
/// Schema of the report a completed rescore emits.
pub const REPORT_VERSION: &str = "sharpebench.rescore-report.v1";
/// Bundles are declarations, not payloads. A manifest larger than this is a
/// refusal to read rather than an allocation.
const MAX_BUNDLE_BYTES: u64 = 1024 * 1024;
/// Files a manifest may name. A bundle is a submission, not a filesystem.
const MAX_FROZEN_FILES: usize = 1024;

/// One file the bundle declares, bound to its content digest.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BoundFile {
    /// Relative to the bundle document's own directory. Never absolute, never
    /// upward: a bundle declares its own contents, not the host's.
    pub path: String,
    pub sha256: String,
}

/// The compute the entrant was given. The budget fields bound what the agent
/// could have computed and so decide whether two submissions are comparable;
/// `disclosed` carries everything else about the environment, which is
/// published beside the score and moves nothing.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceDeclaration {
    /// Thousandths of a CPU, so the comparison is exact.
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub wall_clock_seconds: u64,
    #[serde(default)]
    pub disclosed: BTreeMap<String, String>,
}

/// The field's declared compute envelope, supplied by the operator with
/// `--envelope`. Only the budget fields appear: an envelope that constrained
/// the host's kernel build would be a claim the benchmark does not make.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceEnvelope {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub wall_clock_seconds: u64,
}

/// What the entrant says its submission scored. Read so it can be reported as
/// a claim beside the recomputation, never as the result.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClaimedScore {
    pub deflated_sharpe: f64,
}

/// A declared submission bundle, as the entrant writes it.
///
/// `Deserialize` only: a report is produced here and never accepted from an
/// entrant.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionBundle {
    pub schema_version: String,
    pub agent_id: String,
    /// Each names a path that must also appear in `frozen_files`; the manifest
    /// entry, not this field, carries the digest.
    pub trajectory: String,
    pub dataset: String,
    pub costs: String,
    /// The capture binary's identity, as the entrant declares it.
    pub runner_artifact_sha256: String,
    /// Digest-pinned reference for `--reexecute`. Absent means the bundle
    /// cannot be re-executed, only replayed.
    #[serde(default)]
    pub image: Option<String>,
    /// The complete read set. `None` is a refusal; see the module docs.
    pub frozen_files: Option<Vec<BoundFile>>,
    pub resources: ResourceDeclaration,
    #[serde(default)]
    pub claimed: Option<ClaimedScore>,
}

/// Why a rescore refused. Every variant names the thing that made it refuse.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum RescoreRefusal {
    /// The declined design choice: an absent manifest is not a warning.
    FrozenManifestAbsent,
    EmptyFrozenManifest,
    TooManyFrozenFiles {
        files: usize,
    },
    UnsupportedSchema {
        found: String,
    },
    DuplicateFrozenPath {
        path: String,
    },
    /// A role file the bundle names is not in the frozen manifest, so nothing
    /// binds its bytes.
    RoleNotFrozen {
        role: &'static str,
        path: String,
    },
    NonPortablePath {
        path: String,
    },
    MalformedDigest {
        path: String,
        sha256: String,
    },
    UnpinnedImage {
        image: String,
    },
    ImageNotDeclared,
    BoundFileUnreadable {
        path: String,
        reason: String,
    },
    /// The bytes under a declared path are not the bytes that were declared.
    BoundFileChanged {
        role: String,
        path: String,
        declared: String,
        actual: String,
    },
    RunnerIdentityChanged {
        declared: String,
        actual: String,
    },
    ResourceBudgetDiffers {
        field: &'static str,
        declared: u64,
        envelope: u64,
    },
    /// The frozen inputs would not parse, or the harness refused the artifact.
    Recompute {
        stage: &'static str,
        reason: String,
    },
    Diverged {
        run: usize,
        step: usize,
        observation_id: String,
    },
    /// The entrant's transport failed, so no verdict on determinism exists.
    ReexecutionTransportFailure {
        failure: String,
    },
}

impl std::fmt::Display for RescoreRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FrozenManifestAbsent => write!(
                f,
                "the bundle declares no `frozen_files` manifest, so it declares no read set; \
                 a rescore with no declared read set has recomputed nothing in particular. \
                 This is a refusal and not a warning by choice"
            ),
            Self::EmptyFrozenManifest => write!(
                f,
                "the bundle's `frozen_files` manifest is empty; a submission that binds no bytes is not a submission"
            ),
            Self::TooManyFrozenFiles { files } => write!(
                f,
                "the bundle's manifest names {files} files, more than the {MAX_FROZEN_FILES} a submission may declare"
            ),
            Self::UnsupportedSchema { found } => write!(
                f,
                "bundle schema `{found}` is unsupported (expected `{BUNDLE_VERSION}`)"
            ),
            Self::DuplicateFrozenPath { path } => write!(
                f,
                "the bundle's manifest names `{path}` more than once; one path cannot carry two declared digests"
            ),
            Self::RoleNotFrozen { role, path } => write!(
                f,
                "the bundle names `{path}` as its {role} but does not bind it in `frozen_files`; an unbound input is not frozen"
            ),
            Self::NonPortablePath { path } => write!(
                f,
                "the bundle declares `{path}`, which is absolute or reaches outside the bundle's own directory; a bundle declares its own contents"
            ),
            Self::MalformedDigest { path, sha256 } => write!(
                f,
                "the bundle binds `{path}` to `{sha256}`, which is not 64 lowercase hex characters"
            ),
            Self::UnpinnedImage { image } => write!(
                f,
                "the bundle declares image `{image}`; a re-executable bundle must pin it as <repository>@sha256:<64 lowercase hex>"
            ),
            Self::ImageNotDeclared => write!(
                f,
                "--reexecute needs the entrant to launch, and the bundle declares no image"
            ),
            Self::BoundFileUnreadable { path, reason } => write!(
                f,
                "the bundle binds `{path}`, which cannot be read: {reason}"
            ),
            Self::BoundFileChanged {
                role,
                path,
                declared,
                actual,
            } => write!(
                f,
                "the bundle's {role} `{path}` changed: it is bound to {declared} and the bytes on disk hash to {actual}"
            ),
            Self::RunnerIdentityChanged { declared, actual } => write!(
                f,
                "the bundle declares runner artifact {declared} and this verifier is {actual}; the recompute would not be the capture binary's"
            ),
            Self::ResourceBudgetDiffers {
                field,
                declared,
                envelope,
            } => write!(
                f,
                "the bundle declares {field} = {declared} against the field envelope's {envelope}; a different compute budget is a different experiment, not a footnote"
            ),
            Self::Recompute { stage, reason } => write!(f, "{stage}: {reason}"),
            Self::Diverged {
                run,
                step,
                observation_id,
            } => write!(
                f,
                "re-execution diverged at run {run} step {step} (observation `{observation_id}`)"
            ),
            Self::ReexecutionTransportFailure { failure } => write!(
                f,
                "re-execution hit a {failure} transport failure, so no verdict on determinism was reached"
            ),
        }
    }
}

/// One verified file, as the report publishes it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct VerifiedFile {
    pub path: String,
    pub sha256: String,
    /// `trajectory`, `dataset`, `costs`, or `declared` for the rest.
    pub role: String,
}

/// The recomputed quality figures. Nothing here is read from the bundle.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RecomputedScore {
    pub deflated_sharpe: f64,
    pub raw_mean_return: f64,
    pub rank_eligible: bool,
    pub runs_replayed: usize,
    pub decisions_replayed: usize,
}

/// Whether the submission is comparable with the rest of the field.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "assessment", rename_all = "snake_case")]
pub enum Comparability {
    /// No `--envelope` was supplied, so the budget was read but not judged.
    NotAssessed { disclosed: BTreeMap<String, String> },
    /// Every budget field matched the field's envelope exactly.
    Comparable { disclosed: BTreeMap<String, String> },
}

/// What a passed re-execution adds.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReexecutionReport {
    pub image: String,
    pub runs_reexecuted: usize,
    pub decisions_compared: usize,
}

/// The operator-facing result: what was verified, what was recomputed, and
/// what the recomputation does not establish.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RescoreReport {
    pub schema_version: &'static str,
    pub bundle_version: String,
    pub agent_id: String,
    pub bundle_sha256: String,
    /// Digest over the canonical `path\0sha256\n` listing, so a changed read
    /// set is a changed report even when every file still verifies.
    pub frozen_manifest_sha256: String,
    pub files_verified: Vec<VerifiedFile>,
    pub runner_artifact_sha256: String,
    pub dataset_sha256: String,
    pub cost_model_sha256: String,
    pub recomputed: RecomputedScore,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed: Option<ClaimedScore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_matches_recomputation: Option<bool>,
    pub comparability: Comparability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reexecution: Option<ReexecutionReport>,
    pub verified: Vec<String>,
    pub not_established: Vec<String>,
}

/// How the score is recomputed.
pub enum Recompute<'a> {
    /// Replay the recorded decisions through the frozen engine.
    Replay,
    /// Replay, then re-execute every run with a fresh agent from `image`.
    Reexecute {
        image: &'a str,
        launcher: &'a dyn SandboxLauncher,
    },
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A relative path made only of ordinary components. `..`, a root and a
/// Windows drive prefix are all refused: the manifest is the read set, and a
/// read set that can name `/etc` or `../` is not one.
fn portable_relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// The validated read set, in canonical path order.
#[derive(Debug)]
pub struct FrozenIndex {
    files: Vec<BoundFile>,
    roles: BTreeMap<String, &'static str>,
}

impl FrozenIndex {
    pub fn manifest_sha256(&self) -> String {
        let mut preimage = String::new();
        for file in &self.files {
            preimage.push_str(&file.path);
            preimage.push('\0');
            preimage.push_str(&file.sha256);
            preimage.push('\n');
        }
        sharpebench_attest::content_digest(preimage.as_bytes())
    }

    fn role_of(&self, path: &str) -> &'static str {
        self.roles.get(path).copied().unwrap_or("declared")
    }
}

/// Parse a bundle document. A parse error is the serde message; every
/// bundle-level rule is [`validate`]'s, so its refusals are this module's own.
pub fn parse_bundle(text: &str) -> Result<SubmissionBundle, String> {
    serde_json::from_str(text).map_err(|error| format!("invalid bundle JSON: {error}"))
}

/// Every rule a bundle must satisfy before a single declared byte is read.
pub fn validate(bundle: &SubmissionBundle) -> Result<FrozenIndex, RescoreRefusal> {
    if bundle.schema_version != BUNDLE_VERSION {
        return Err(RescoreRefusal::UnsupportedSchema {
            found: bundle.schema_version.clone(),
        });
    }
    let declared = match bundle.frozen_files.as_deref() {
        None => return Err(RescoreRefusal::FrozenManifestAbsent),
        Some([]) => return Err(RescoreRefusal::EmptyFrozenManifest),
        Some(files) if files.len() > MAX_FROZEN_FILES => {
            return Err(RescoreRefusal::TooManyFrozenFiles { files: files.len() })
        }
        Some(files) => files,
    };
    let mut seen = BTreeSet::new();
    for file in declared {
        if !portable_relative(&file.path) {
            return Err(RescoreRefusal::NonPortablePath {
                path: file.path.clone(),
            });
        }
        if !is_sha256(&file.sha256) {
            return Err(RescoreRefusal::MalformedDigest {
                path: file.path.clone(),
                sha256: file.sha256.clone(),
            });
        }
        if !seen.insert(file.path.clone()) {
            return Err(RescoreRefusal::DuplicateFrozenPath {
                path: file.path.clone(),
            });
        }
    }
    let mut roles = BTreeMap::new();
    for (role, path) in [
        ("trajectory", &bundle.trajectory),
        ("dataset", &bundle.dataset),
        ("costs", &bundle.costs),
    ] {
        if !seen.contains(path) {
            return Err(RescoreRefusal::RoleNotFrozen {
                role,
                path: path.clone(),
            });
        }
        roles.insert(path.clone(), role);
    }
    if let Some(image) = &bundle.image {
        if crate::artifact_preflight::validate_pinned_reference(image).is_err() {
            return Err(RescoreRefusal::UnpinnedImage {
                image: image.clone(),
            });
        }
    }
    let mut files = declared.to_vec();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(FrozenIndex { files, roles })
}

/// Read exactly the declared files and hold each to its declared digest.
///
/// The iteration source is the manifest, so a file that is not declared is
/// never opened and cannot reach the recompute.
pub fn verify_frozen(
    index: &FrozenIndex,
    root: &Path,
) -> Result<BTreeMap<String, Vec<u8>>, RescoreRefusal> {
    let mut bytes = BTreeMap::new();
    for bound in &index.files {
        let absolute: PathBuf = root.join(&bound.path);
        let content =
            std::fs::read(&absolute).map_err(|error| RescoreRefusal::BoundFileUnreadable {
                path: bound.path.clone(),
                reason: error.to_string(),
            })?;
        let actual = sharpebench_attest::content_digest(&content);
        if actual != bound.sha256 {
            return Err(RescoreRefusal::BoundFileChanged {
                role: index.role_of(&bound.path).to_string(),
                path: bound.path.clone(),
                declared: bound.sha256.clone(),
                actual,
            });
        }
        bytes.insert(bound.path.clone(), content);
    }
    Ok(bytes)
}

/// The runner identity leg: the bundle's declared capture binary against this
/// verifier's own.
pub fn check_runner(declared: &str, actual: &str) -> Result<(), RescoreRefusal> {
    if declared != actual {
        return Err(RescoreRefusal::RunnerIdentityChanged {
            declared: declared.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

/// Which resource differences invalidate comparability and which are only
/// disclosed. A budget field bounds what the agent could compute, so a
/// difference there makes two scores answers to different questions and
/// refuses; everything in `disclosed` is environment description and is
/// published beside the score.
pub fn assess_resources(
    declared: &ResourceDeclaration,
    envelope: Option<&ResourceEnvelope>,
) -> Result<Comparability, RescoreRefusal> {
    let Some(envelope) = envelope else {
        return Ok(Comparability::NotAssessed {
            disclosed: declared.disclosed.clone(),
        });
    };
    for (field, declared_value, envelope_value) in [
        ("cpu_millis", declared.cpu_millis, envelope.cpu_millis),
        ("memory_bytes", declared.memory_bytes, envelope.memory_bytes),
        (
            "wall_clock_seconds",
            declared.wall_clock_seconds,
            envelope.wall_clock_seconds,
        ),
    ] {
        if declared_value != envelope_value {
            return Err(RescoreRefusal::ResourceBudgetDiffers {
                field,
                declared: declared_value,
                envelope: envelope_value,
            });
        }
    }
    Ok(Comparability::Comparable {
        disclosed: declared.disclosed.clone(),
    })
}

fn refuse(stage: &'static str, reason: impl Into<String>) -> RescoreRefusal {
    RescoreRefusal::Recompute {
        stage,
        reason: reason.into(),
    }
}

/// Recompute a declared submission's quality from its frozen bytes.
pub fn rescore(
    bundle: &SubmissionBundle,
    bundle_sha256: &str,
    root: &Path,
    runner_artifact_sha256: &str,
    envelope: Option<&ResourceEnvelope>,
    recompute: Recompute<'_>,
) -> Result<RescoreReport, RescoreRefusal> {
    let index = validate(bundle)?;
    check_runner(&bundle.runner_artifact_sha256, runner_artifact_sha256)?;
    let comparability = assess_resources(&bundle.resources, envelope)?;
    let bytes = verify_frozen(&index, root)?;

    let dataset_text = std::str::from_utf8(&bytes[&bundle.dataset])
        .map_err(|error| refuse("frozen dataset", error.to_string()))?;
    let data = Dataset::from_csv(dataset_text).map_err(|error| refuse("frozen dataset", error))?;
    let costs: CostModel = serde_json::from_slice(&bytes[&bundle.costs])
        .map_err(|error| refuse("frozen cost model", error.to_string()))?;
    let traj: sharpebench_protocol::AgentTrajectory =
        serde_json::from_slice(&bytes[&bundle.trajectory])
            .map_err(|error| refuse("frozen trajectory", error.to_string()))?;
    let cfg = ScoreConfig::default();

    let (verified_result, reexecution) = match recompute {
        Recompute::Replay => (
            sharpebench_harness::verify_trajectory_strict(
                &data,
                &traj,
                costs,
                &cfg,
                Some(runner_artifact_sha256),
            )
            .map_err(|reason| refuse("strict verification", reason))?,
            None,
        ),
        Recompute::Reexecute { image, launcher } => {
            let fault: FaultCell = std::rc::Rc::new(std::cell::RefCell::new(None));
            let mut make = sandbox_factory(image, launcher, fault.clone());
            let outcome = sharpebench_harness::verify_trajectory_reexecuted(
                &data,
                &traj,
                costs,
                &cfg,
                Some(runner_artifact_sha256),
                &mut make,
            );
            // A transport fault is reported before the outcome: a degraded
            // entrant's holds would otherwise read as a divergence.
            if let Some(kind) = fault.borrow().clone() {
                return Err(RescoreRefusal::ReexecutionTransportFailure {
                    failure: format!("{kind:?}"),
                });
            }
            let result = outcome.map_err(|error| match error {
                sharpebench_harness::ReexecutionError::Refused(reason) => {
                    refuse("strict verification", reason)
                }
                sharpebench_harness::ReexecutionError::Diverged(divergence) => {
                    RescoreRefusal::Diverged {
                        run: divergence.run,
                        step: divergence.step,
                        observation_id: divergence.observation_id,
                    }
                }
            })?;
            let report = ReexecutionReport {
                image: image.to_string(),
                runs_reexecuted: result.runs_replayed,
                decisions_compared: result.decisions_replayed,
            };
            (result, Some(report))
        }
    };

    let recomputed = RecomputedScore {
        deflated_sharpe: verified_result.score.deflated_sharpe,
        raw_mean_return: verified_result.score.raw_mean_return,
        rank_eligible: verified_result.score.rank_eligible,
        runs_replayed: verified_result.runs_replayed,
        decisions_replayed: verified_result.decisions_replayed,
    };
    // The claim is reported, never scored. `recomputed` above is read from the
    // verifier's result and from nothing the bundle carries.
    let claim_matches_recomputation = bundle
        .claimed
        .as_ref()
        .map(|claimed| (claimed.deflated_sharpe - recomputed.deflated_sharpe).abs() <= 1e-9);

    let contract = sharpebench_harness::trajectory_contract(&data, costs, &[], &[]);
    let files_verified = index
        .files
        .iter()
        .map(|file| VerifiedFile {
            path: file.path.clone(),
            sha256: file.sha256.clone(),
            role: index.role_of(&file.path).to_string(),
        })
        .collect();

    let mut verified = vec![
        format!(
            "every one of the {} files the bundle declares hashes to its declared digest",
            index.files.len()
        ),
        "the trajectory, dataset and cost model were read from those verified bytes and from no other path on this host".to_string(),
        format!(
            "the declared runner artifact {runner_artifact_sha256} is this verifier's own binary, and the trajectory's contract names it"
        ),
        format!(
            "the score was recomputed by replaying {} recorded decisions across {} runs through the frozen engine; no figure the bundle reports was read as a result",
            recomputed.decisions_replayed, recomputed.runs_replayed
        ),
    ];
    let mut not_established = vec![
        "nothing about files this bundle does not declare, including any agent state beside it: they were not read".to_string(),
        "nothing about the bytes under a declared path at any moment other than the one they were hashed and read at".to_string(),
    ];
    match &reexecution {
        Some(report) => {
            verified.push(format!(
                "every score-bearing decision repeated when {} runs were re-executed with a fresh `{}` in a network-disabled container",
                report.runs_reexecuted, report.image
            ));
            not_established.push(
                "that the entrant received the model responses it recorded: this is deterministic policy re-execution, not recorded provider-response replay, so for a stochastic entrant agreement here is agreement of the policy and not of the provider transcript".to_string(),
            );
        }
        None => not_established.push(
            "that the recorded decisions are the ones the declared policy would produce: no agent was re-executed. Pass --reexecute against the bundle's pinned image for that leg".to_string(),
        ),
    }
    if matches!(comparability, Comparability::NotAssessed { .. }) {
        not_established.push(
            "that this submission is comparable with the rest of the field: no --envelope was supplied, so the declared compute budget was recorded and not judged".to_string(),
        );
    }

    Ok(RescoreReport {
        schema_version: REPORT_VERSION,
        bundle_version: bundle.schema_version.clone(),
        agent_id: bundle.agent_id.clone(),
        bundle_sha256: bundle_sha256.to_string(),
        frozen_manifest_sha256: index.manifest_sha256(),
        files_verified,
        runner_artifact_sha256: runner_artifact_sha256.to_string(),
        dataset_sha256: contract.dataset_sha256,
        cost_model_sha256: contract.cost_model_sha256,
        recomputed,
        claimed: bundle.claimed.clone(),
        claim_matches_recomputation,
        comparability,
        reexecution,
        verified,
        not_established,
    })
}

const USAGE: &str = "usage: sharpebench rescore <bundle.json> [--envelope <envelope.json>] \
                     [--reexecute [--scan-policy <policy.json> [--runtime-allowlist <list.json>]]] [--json]";

/// The operator entry point.
pub fn run(args: &[String], json: bool) -> i32 {
    let Some(path) = args.get(2).filter(|value| !value.starts_with("--")) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let bundle_path = Path::new(path);
    let raw = match std::fs::File::open(bundle_path).and_then(|file| {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        file.take(MAX_BUNDLE_BYTES + 1).read_to_end(&mut bytes)?;
        Ok(bytes)
    }) {
        Ok(bytes) if bytes.len() as u64 <= MAX_BUNDLE_BYTES => bytes,
        Ok(_) => {
            eprintln!("error: {path} is larger than the {MAX_BUNDLE_BYTES} bytes a bundle declaration may occupy");
            return 1;
        }
        Err(error) => {
            eprintln!("error: cannot read {path}: {error}");
            return 1;
        }
    };
    let text = match std::str::from_utf8(&raw) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("error: {path} is not UTF-8: {error}");
            return 1;
        }
    };
    let bundle = match parse_bundle(text) {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let bundle_sha256 = sharpebench_attest::content_digest(&raw);
    let root = bundle_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    let envelope = match crate::flag_value(args, "--envelope") {
        Some(envelope_path) if !envelope_path.starts_with("--") => {
            match std::fs::read_to_string(envelope_path)
                .map_err(|error| error.to_string())
                .and_then(|text| {
                    serde_json::from_str::<ResourceEnvelope>(&text).map_err(|e| e.to_string())
                }) {
                Ok(envelope) => Some(envelope),
                Err(error) => {
                    eprintln!("error: cannot read the field envelope: {error}");
                    return 1;
                }
            }
        }
        Some(_) => {
            eprintln!("error: --envelope requires a JSON file path");
            return 2;
        }
        None => None,
    };

    let reexecute = args.iter().any(|arg| arg == "--reexecute");
    if !reexecute && args.iter().any(|arg| arg == "--scan-policy") {
        eprintln!(
            "error: --scan-policy is a leg of the re-execution launch; it requires --reexecute"
        );
        return 2;
    }
    // The image is the bundle's, never argv's: an operator flag that could
    // name a different image would let the rescore verify something other than
    // what was submitted.
    let image = bundle.image.clone();
    let launcher = crate::external_capture::DockerSandbox;
    let recompute = if reexecute {
        let Some(image) = image.as_deref() else {
            report_refusal(&RescoreRefusal::ImageNotDeclared, json);
            return 1;
        };
        let mut preflight_args: Vec<String> =
            vec!["sharpebench".to_string(), "rescore".to_string()];
        preflight_args.push("--image".to_string());
        preflight_args.push(image.to_string());
        for flag in ["--scan-policy", "--runtime-allowlist"] {
            if let Some(value) = crate::flag_value(args, flag) {
                preflight_args.push(flag.to_string());
                preflight_args.push(value.to_string());
            }
        }
        match crate::artifact_preflight::preflight_from_args(
            &preflight_args,
            &crate::artifact_preflight::DockerProcess,
        ) {
            Ok(None) => {}
            Ok(Some(report)) if report.authorizes_launch() => {}
            Ok(Some(_)) => {
                eprintln!(
                    "error: the image preflight did not authorize `{image}`; the bundle is not rescored from an image whose scan, allowlist, probe or cleanup did not complete clean"
                );
                return 1;
            }
            Err(failure) => {
                eprintln!("error: {failure}");
                return 1;
            }
        }
        if let Err(error) = launcher.admit(image) {
            eprintln!("error: cannot start the sandboxed agent `{image}`: {error}");
            return 1;
        }
        Recompute::Reexecute {
            image,
            launcher: &launcher,
        }
    } else {
        Recompute::Replay
    };

    let runner = match crate::current_executable_sha256() {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    match rescore(
        &bundle,
        &bundle_sha256,
        &root,
        &runner,
        envelope.as_ref(),
        recompute,
    ) {
        Ok(report) => {
            emit_report(&report, json);
            0
        }
        Err(refusal) => {
            report_refusal(&refusal, json);
            1
        }
    }
}

fn report_refusal(refusal: &RescoreRefusal, json: bool) {
    if json {
        crate::emit_json(&serde_json::json!({
            "rescored": false,
            "schema_version": REPORT_VERSION,
            "refusal": refusal,
            "message": refusal.to_string(),
        }));
    }
    eprintln!("error: {refusal}");
}

fn emit_report(report: &RescoreReport, json: bool) {
    if json {
        crate::emit_json(report);
        return;
    }
    println!(
        "rescored `{}` from bundle {}",
        report.agent_id, report.bundle_sha256
    );
    println!("  frozen manifest : {}", report.frozen_manifest_sha256);
    println!("  dataset         : {}", report.dataset_sha256);
    println!("  cost model      : {}", report.cost_model_sha256);
    println!("  runner artifact : {}", report.runner_artifact_sha256);
    println!(
        "  deflated Sharpe : {:.4}",
        report.recomputed.deflated_sharpe
    );
    println!(
        "  raw mean return : {:.5}",
        report.recomputed.raw_mean_return
    );
    println!(
        "  rank-eligible   : {}",
        if report.recomputed.rank_eligible {
            "yes"
        } else {
            "no"
        }
    );
    if let (Some(claimed), Some(matches)) = (&report.claimed, report.claim_matches_recomputation) {
        println!(
            "  entrant claimed : {:.4} ({})",
            claimed.deflated_sharpe,
            if matches {
                "matches the recomputation"
            } else {
                "does not match the recomputation; the recomputed figure is the result"
            }
        );
    }
    println!("\nVerified:");
    for line in &report.verified {
        println!("  - {line}");
    }
    println!("\nNot established:");
    for line in &report.not_established {
        println!("  - {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_harness::run_agent_capture;
    use sharpebench_sim::{Agent, Momentum, Window};

    /// A fixed stand-in for the capture binary's identity. The real value is
    /// this executable's digest; a test needs one that is stable across hosts.
    const RUNNER: &str = "ab";

    fn runner() -> String {
        RUNNER.repeat(32)
    }

    fn dataset_csv() -> String {
        let data = Dataset::synthetic(3, 80, 20_260_621);
        let mut csv = String::from("date,symbol,close\n");
        for (symbol, closes) in &data.closes {
            for (index, close) in closes.iter().enumerate() {
                csv.push_str(&format!("{},{symbol},{close}\n", data.dates[index]));
            }
        }
        csv
    }

    struct Fixture {
        dir: tempfile::TempDir,
        bundle: SubmissionBundle,
        bundle_sha256: String,
    }

    impl Fixture {
        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, relative: &str, bytes: &[u8]) {
            std::fs::write(self.dir.path().join(relative), bytes).expect("fixture writes");
        }

        fn rescore(&self) -> Result<RescoreReport, RescoreRefusal> {
            rescore(
                &self.bundle,
                &self.bundle_sha256,
                self.root(),
                &runner(),
                None,
                Recompute::Replay,
            )
        }
    }

    /// A complete bundle: a frozen CSV, a cost model, a trajectory captured
    /// from them, and one undeclared file beside them standing in for the
    /// agent state a submission leaves on disk.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let csv = dataset_csv();
        let data = Dataset::from_csv(&csv).expect("the fixture CSV parses");
        // A frozen cost model is a file, so the fixture uses a JSON-representable
        // one: `CostModel::default` carries an infinite participation cap, which
        // serializes to `null` and cannot be read back.
        let costs = CostModel {
            max_participation: 1.0,
            ..CostModel::default()
        };
        let windows = [Window { start: 20, end: 80 }];
        let (_, mut traj) = run_agent_capture("momentum", &data, &windows, &[0], costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        });
        traj.contract
            .as_mut()
            .expect("a fresh capture is bound")
            .runner_artifact_sha256 = Some(runner());

        let trajectory_json = serde_json::to_vec_pretty(&traj).expect("trajectories serialize");
        let costs_json = serde_json::to_vec_pretty(&costs).expect("cost models serialize");
        let state = b"self_reported_return=9999".to_vec();

        std::fs::write(dir.path().join("prices.csv"), csv.as_bytes()).expect("fixture writes");
        std::fs::write(dir.path().join("costs.json"), &costs_json).expect("fixture writes");
        std::fs::write(dir.path().join("trajectory.json"), &trajectory_json)
            .expect("fixture writes");
        std::fs::write(dir.path().join("agent_state.bin"), &state).expect("fixture writes");

        let frozen = vec![
            BoundFile {
                path: "prices.csv".to_string(),
                sha256: sharpebench_attest::content_digest(csv.as_bytes()),
            },
            BoundFile {
                path: "costs.json".to_string(),
                sha256: sharpebench_attest::content_digest(&costs_json),
            },
            BoundFile {
                path: "trajectory.json".to_string(),
                sha256: sharpebench_attest::content_digest(&trajectory_json),
            },
        ];
        let bundle = SubmissionBundle {
            schema_version: BUNDLE_VERSION.to_string(),
            agent_id: "momentum".to_string(),
            trajectory: "trajectory.json".to_string(),
            dataset: "prices.csv".to_string(),
            costs: "costs.json".to_string(),
            runner_artifact_sha256: runner(),
            image: None,
            frozen_files: Some(frozen),
            resources: ResourceDeclaration {
                cpu_millis: 2000,
                memory_bytes: 2 * 1024 * 1024 * 1024,
                wall_clock_seconds: 900,
                disclosed: BTreeMap::from([(
                    "host_kernel".to_string(),
                    "6.8.0-generic".to_string(),
                )]),
            },
            claimed: None,
        };
        Fixture {
            dir,
            bundle_sha256: "cd".repeat(32),
            bundle,
        }
    }

    #[test]
    fn a_complete_bundle_rescores_and_states_what_it_verified() {
        let fixture = fixture();
        let report = fixture.rescore().expect("a complete bundle rescores");
        assert_eq!(report.schema_version, REPORT_VERSION);
        assert_eq!(report.agent_id, "momentum");
        assert_eq!(report.files_verified.len(), 3);
        assert_eq!(report.recomputed.runs_replayed, 1);
        assert!(report.recomputed.decisions_replayed > 0);
        assert!(report
            .verified
            .iter()
            .any(|line| line.contains("hashes to its declared digest")));
        assert!(report
            .verified
            .iter()
            .any(|line| line.contains("no figure the bundle reports was read as a result")));
        assert!(report
            .not_established
            .iter()
            .any(|line| line.contains("no agent was re-executed")));
    }

    /// The declined design choice, asserted as the refusal it is and not
    /// merely as some refusal: an empty manifest refuses too, for a different
    /// reason, and the roles are then unbound, so a bare `is_err` here would
    /// still pass with the absence downgraded to a warning.
    #[test]
    fn an_absent_frozen_manifest_is_refused_not_warned_about() {
        let mut fixture = fixture();
        fixture.bundle.frozen_files = None;
        assert_eq!(
            fixture.rescore().expect_err("no manifest, no read set"),
            RescoreRefusal::FrozenManifestAbsent
        );
    }

    #[test]
    fn a_changed_dataset_is_refused_and_names_the_dataset() {
        let fixture = fixture();
        let mut csv = dataset_csv();
        csv.push_str("2025-081,AAA,101.5\n");
        fixture.write("prices.csv", csv.as_bytes());
        let refusal = fixture.rescore().expect_err("bound data changed");
        let RescoreRefusal::BoundFileChanged {
            role,
            path,
            declared,
            actual,
        } = refusal
        else {
            panic!("a changed bound file must refuse as one: {refusal:?}");
        };
        assert_eq!(role, "dataset");
        assert_eq!(path, "prices.csv");
        assert_ne!(declared, actual);
    }

    #[test]
    fn changed_costs_are_refused_and_name_the_cost_model() {
        let fixture = fixture();
        let costs = CostModel {
            fee_bps: 999.0,
            max_participation: 1.0,
            ..CostModel::default()
        };
        fixture.write(
            "costs.json",
            &serde_json::to_vec_pretty(&costs).expect("cost models serialize"),
        );
        let refusal = fixture.rescore().expect_err("bound costs changed");
        let RescoreRefusal::BoundFileChanged { role, path, .. } = refusal else {
            panic!("a changed cost model must refuse as a changed bound file: {refusal:?}");
        };
        assert_eq!(role, "costs");
        assert_eq!(path, "costs.json");
    }

    #[test]
    fn a_changed_runner_identity_is_refused_and_names_both_identities() {
        let fixture = fixture();
        let verifier = "ef".repeat(32);
        let refusal = rescore(
            &fixture.bundle,
            &fixture.bundle_sha256,
            fixture.root(),
            &verifier,
            None,
            Recompute::Replay,
        )
        .expect_err("a different verifier binary is visible");
        assert_eq!(
            refusal,
            RescoreRefusal::RunnerIdentityChanged {
                declared: runner(),
                actual: verifier,
            }
        );
    }

    /// The read set is the manifest. An undeclared file beside the bundle is
    /// never opened, so editing it cannot move a single field of the report.
    #[test]
    fn state_outside_the_declared_bundle_cannot_move_the_result() {
        let fixture = fixture();
        let before = fixture.rescore().expect("the bundle rescores");
        fixture.write(
            "agent_state.bin",
            b"self_reported_return=0.0001 and a much longer body than before",
        );
        fixture.write("undeclared_extra.csv", b"date,symbol,close\nx,Y,1\n");
        let after = fixture.rescore().expect("the bundle still rescores");
        assert_eq!(
            before, after,
            "a report must be a function of the declared bundle alone"
        );
        assert!(
            !before
                .files_verified
                .iter()
                .any(|file| file.path.contains("agent_state")),
            "an undeclared file must not appear as verified evidence"
        );
    }

    /// A self-reported figure is carried as a claim and never as the result.
    #[test]
    fn a_self_reported_score_is_reported_as_a_claim_and_never_scored() {
        let mut fixture = fixture();
        let honest = fixture.rescore().expect("the bundle rescores");
        fixture.bundle.claimed = Some(ClaimedScore {
            deflated_sharpe: 42.0,
        });
        let claimed = fixture.rescore().expect("the bundle still rescores");
        assert_eq!(
            claimed.recomputed, honest.recomputed,
            "an entrant's own figure must not reach the recomputation"
        );
        assert_eq!(claimed.claim_matches_recomputation, Some(false));
    }

    /// A bundle cannot name the host's disk: a manifest that could reach
    /// outside its own directory is not a read set.
    #[test]
    fn a_manifest_path_reaching_outside_the_bundle_is_refused() {
        let mut fixture = fixture();
        fixture.bundle.dataset = "../prices.csv".to_string();
        fixture
            .bundle
            .frozen_files
            .as_mut()
            .expect("the fixture declares a manifest")[0]
            .path = "../prices.csv".to_string();
        assert_eq!(
            fixture
                .rescore()
                .expect_err("a bundle declares its own contents"),
            RescoreRefusal::NonPortablePath {
                path: "../prices.csv".to_string(),
            }
        );
    }

    /// A budget difference refuses; an environment difference is disclosed.
    #[test]
    fn a_budget_difference_refuses_and_an_environment_difference_is_disclosed() {
        let declared = ResourceDeclaration {
            cpu_millis: 2000,
            memory_bytes: 2 * 1024 * 1024 * 1024,
            wall_clock_seconds: 900,
            disclosed: BTreeMap::from([("host_kernel".to_string(), "6.8.0".to_string())]),
        };
        let envelope = ResourceEnvelope {
            cpu_millis: 2000,
            memory_bytes: 2 * 1024 * 1024 * 1024,
            wall_clock_seconds: 900,
        };
        assert_eq!(
            assess_resources(&declared, Some(&envelope)).expect("the budgets agree"),
            Comparability::Comparable {
                disclosed: declared.disclosed.clone(),
            },
            "a kernel build the envelope does not constrain is disclosed, not refused"
        );
        let doubled = ResourceEnvelope {
            cpu_millis: 4000,
            ..envelope
        };
        assert_eq!(
            assess_resources(&declared, Some(&doubled))
                .expect_err("a different compute budget is a different experiment"),
            RescoreRefusal::ResourceBudgetDiffers {
                field: "cpu_millis",
                declared: 2000,
                envelope: 4000,
            }
        );
    }

    #[test]
    fn an_unpinned_image_is_refused_before_anything_is_read() {
        let mut fixture = fixture();
        fixture.bundle.image = Some("sharpebench/agent:latest".to_string());
        assert_eq!(
            fixture
                .rescore()
                .expect_err("a mutable tag is not a submitted artifact"),
            RescoreRefusal::UnpinnedImage {
                image: "sharpebench/agent:latest".to_string(),
            }
        );
    }

    #[test]
    fn a_role_file_outside_the_manifest_is_refused() {
        let mut fixture = fixture();
        fixture.bundle.costs = "other-costs.json".to_string();
        assert_eq!(
            fixture
                .rescore()
                .expect_err("an unbound input is not frozen"),
            RescoreRefusal::RoleNotFrozen {
                role: "costs",
                path: "other-costs.json".to_string(),
            }
        );
    }
}
