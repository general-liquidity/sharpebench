//! `sharpebench regrade` — emit, or refuse, the receipt that links a frozen
//! submission artifact to the evaluator that supersedes the one which graded it.
//!
//! [`sharpebench_sim::regrade_submission`] answers a question no other path in
//! this repository answers: when an evaluator failed and its grade has to be
//! replaced, what links the replacement to the artifact and to the evaluator it
//! replaces? This command is its producer.
//!
//! # What it does not do
//!
//! It emits no figure. The repository has exactly one path from which a score
//! is published — `sharpebench rescore`, which recomputes through
//! [`sharpebench_harness::verify_trajectory_strict`] — and a regrade does not
//! add a second. The submission `regrade_submission` returns is therefore not
//! scored here: what this command takes from it is the receipt and the refusals
//! computed on the way to it. A regrade's job is the link; the figure is the
//! rescore's.
//!
//! # The source digest is read, not declared
//!
//! [`sharpebench_sim::RegradeRequest`] takes the source artifact's digest as a
//! value. Called by hand that digest is a trusted input. Called here it is not:
//! the bundle's frozen manifest is validated and read by
//! [`crate::rescore_cmd::verify_frozen`], which holds every declared path to its
//! declared digest, and the digest the receipt names is then hashed from the
//! trajectory bytes this command is holding. A bundle whose trajectory has
//! changed under its declaration refuses before a receipt exists.
//!
//! # Which evaluator identity is checked and which is taken on trust
//!
//! The replacement evaluator is the one that would do the grading, so its two
//! derivable fields are checked rather than believed: `verifier_artifact_sha256`
//! must be this running binary's own digest and `score_config_sha256` must be
//! the digest of the configuration this binary scores with. A declaration
//! naming any other binary or any other configuration refuses, for the reason
//! [`crate::rescore_cmd::check_runner`] refuses one: a grade attributed to an
//! evaluator that is not the one present is not a grade anybody can check.
//!
//! The original evaluator is historical. A bundle records the capture binary
//! and the entrant's compute, never the identity of whatever graded it, so the
//! original identity can only come from the operator. That is stated in the
//! document's `not_established` rather than papered over.

use std::path::Path;

use serde::Serialize;

use sharpebench_core::ScoreConfig;
use sharpebench_sim::{
    regrade_submission, Dataset, EvaluatorIdentity, RegradeDisposition, RegradeReceipt,
    RegradeRefusal, RegradeRequest,
};

use crate::rescore_cmd::{
    frozen_cost_model, load_bundle, verify_frozen, LoadedBundle, RescoreRefusal, SubmissionBundle,
};

/// Schema of the document a completed regrade emits.
pub const DOCUMENT_VERSION: &str = "sharpebench.regrade-document.v1";

/// Why a regrade refused. The bundle's own refusals and the sim's are carried
/// as they were raised; the two identity checks are this command's.
#[derive(Debug)]
pub enum RegradeCommandRefusal {
    /// The bundle is not a declared read set. Raised by the rescore command's
    /// validation, which this command reuses rather than re-deriving.
    Bundle(RescoreRefusal),
    /// The regrade itself refused: a missing reason, a malformed digest, or an
    /// artifact short of a decision for every step of its window.
    Regrade(RegradeRefusal),
    /// The declared replacement names a verifier that is not this binary.
    ReplacementVerifierNotThisBinary { declared: String, actual: String },
    /// The declared replacement names a scoring configuration this binary does
    /// not score with.
    ReplacementScoreConfigNotThisScorer { declared: String, actual: String },
    /// The frozen inputs the manifest bound would not parse.
    FrozenInput { stage: &'static str, reason: String },
}

impl std::fmt::Display for RegradeCommandRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bundle(refusal) => write!(f, "{refusal}"),
            Self::Regrade(refusal) => write!(f, "{refusal}"),
            Self::ReplacementVerifierNotThisBinary { declared, actual } => write!(
                f,
                "the replacement evaluator declares verifier artifact {declared} and this binary is {actual}; \
                 a grade attributed to an evaluator that is not the one present is not a grade anybody can check"
            ),
            Self::ReplacementScoreConfigNotThisScorer { declared, actual } => write!(
                f,
                "the replacement evaluator declares score config {declared} and this binary scores with {actual}; \
                 the superseding grade would be attributed to a configuration that is not the one that would produce it"
            ),
            Self::FrozenInput { stage, reason } => write!(f, "{stage}: {reason}"),
        }
    }
}

/// The emitted document. `used_by_gate` is recorded for the reason
/// `ComparisonDocument` records it: a reader holding the JSON alone must not
/// mistake a provenance link for a result.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RegradeDocument {
    pub schema_version: &'static str,
    pub used_by_gate: bool,
    pub bundle_version: String,
    pub agent_id: String,
    pub bundle_sha256: String,
    pub frozen_manifest_sha256: String,
    /// The manifest path the receipt's `source_artifact_sha256` was hashed from.
    pub trajectory_path: String,
    pub receipt: RegradeReceipt,
    pub may_replace_published: bool,
    pub verified: Vec<String>,
    pub not_established: Vec<String>,
}

/// The digest of the configuration this binary scores a rescored bundle with.
pub fn this_score_config_sha256() -> Result<String, String> {
    crate::digest_json("score configuration", &ScoreConfig::default())
}

/// Build the receipt for one declared bundle, or refuse.
#[allow(clippy::too_many_arguments)]
pub fn regrade(
    bundle: &SubmissionBundle,
    bundle_sha256: &str,
    root: &Path,
    verifier_artifact_sha256: &str,
    score_config_sha256: &str,
    original_evaluator: &EvaluatorIdentity,
    replacement_evaluator: &EvaluatorIdentity,
    reason: &str,
    frozen_published: &[String],
) -> Result<RegradeDocument, RegradeCommandRefusal> {
    let index = crate::rescore_cmd::validate(bundle).map_err(RegradeCommandRefusal::Bundle)?;
    if replacement_evaluator.verifier_artifact_sha256 != verifier_artifact_sha256 {
        return Err(RegradeCommandRefusal::ReplacementVerifierNotThisBinary {
            declared: replacement_evaluator.verifier_artifact_sha256.clone(),
            actual: verifier_artifact_sha256.to_string(),
        });
    }
    if replacement_evaluator.score_config_sha256 != score_config_sha256 {
        return Err(RegradeCommandRefusal::ReplacementScoreConfigNotThisScorer {
            declared: replacement_evaluator.score_config_sha256.clone(),
            actual: score_config_sha256.to_string(),
        });
    }

    let bytes = verify_frozen(&index, root).map_err(RegradeCommandRefusal::Bundle)?;
    // The seam: the digest the receipt names is hashed from the trajectory
    // bytes this command read, not read off the declaration that named them.
    let trajectory_bytes = &bytes[&bundle.trajectory];
    let source_artifact_sha256 = sharpebench_attest::content_digest(trajectory_bytes);

    let dataset_text = std::str::from_utf8(&bytes[&bundle.dataset]).map_err(|error| {
        RegradeCommandRefusal::FrozenInput {
            stage: "frozen dataset",
            reason: error.to_string(),
        }
    })?;
    let data =
        Dataset::from_csv(dataset_text).map_err(|reason| RegradeCommandRefusal::FrozenInput {
            stage: "frozen dataset",
            reason,
        })?;
    let costs = frozen_cost_model(&bytes[&bundle.costs]).map_err(RegradeCommandRefusal::Bundle)?;
    let traj: sharpebench_protocol::AgentTrajectory = serde_json::from_slice(trajectory_bytes)
        .map_err(|error| RegradeCommandRefusal::FrozenInput {
            stage: "frozen trajectory",
            reason: error.to_string(),
        })?;

    // The submission is deliberately dropped. Scoring it here would put a
    // second path to a published figure in the repository; what a regrade adds
    // is the link, and the figure stays the rescore's.
    let (_submission, receipt) = regrade_submission(
        &data,
        &traj,
        costs,
        RegradeRequest {
            source_artifact_sha256: &source_artifact_sha256,
            original_evaluator,
            replacement_evaluator,
            reason,
            frozen_published,
        },
    )
    .map_err(RegradeCommandRefusal::Regrade)?;

    let may_replace_published = receipt.may_replace_published();
    let mut verified = vec![
        format!(
            "every one of the {} files the bundle declares hashes to its declared digest",
            index.declared_file_count()
        ),
        format!(
            "the source artifact digest {source_artifact_sha256} was hashed from the trajectory bytes read under `{}`, not taken from the declaration that names them",
            bundle.trajectory
        ),
        format!(
            "the replacement evaluator is this binary: verifier artifact {verifier_artifact_sha256} scoring under configuration {score_config_sha256}"
        ),
        format!(
            "{} recorded decisions were read out of the artifact and {} agents were invoked to produce them",
            receipt.decisions_replayed, receipt.agent_invocations
        ),
    ];
    let mut not_established = vec![
        "the superseding grade itself: this command emits the link, not a figure. \
         The repository has one path from which a score is published, `sharpebench rescore`, \
         and a regrade does not add a second"
            .to_string(),
        "that the original evaluator is the one the operator named: a bundle records the capture \
         binary and the entrant's compute, never the identity of whatever graded it, so the \
         original identity is the operator's declaration and is reported as one"
            .to_string(),
        "nothing about files this bundle does not declare: they were not read".to_string(),
    ];
    match &receipt.disposition {
        RegradeDisposition::ReplacesSource => not_established.push(
            "that this source was never published: it appears in no `--frozen-published` list \
             this invocation was given, and an omitted list is an omission, not a finding"
                .to_string(),
        ),
        RegradeDisposition::OperationalOnly { frozen_record } => verified.push(format!(
            "{frozen_record} is frozen published evidence, so the superseding grade is reportable and never a replacement"
        )),
    }

    Ok(RegradeDocument {
        schema_version: DOCUMENT_VERSION,
        used_by_gate: false,
        bundle_version: bundle.schema_version.clone(),
        agent_id: bundle.agent_id.clone(),
        bundle_sha256: bundle_sha256.to_string(),
        frozen_manifest_sha256: index.manifest_sha256(),
        trajectory_path: bundle.trajectory.clone(),
        receipt,
        may_replace_published,
        verified,
        not_established,
    })
}

const USAGE: &str = "usage: sharpebench regrade <bundle.json> \
                     --original-evaluator <evaluator.json> \
                     --replacement-evaluator <evaluator.json> \
                     --reason <text> [--frozen-published <digests.json>] [--json]";

fn read_evaluator(flag: &str, path: &str) -> Result<EvaluatorIdentity, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{flag} {path} is not an evaluator identity: {error}"))
}

fn read_frozen_published(path: &str) -> Result<Vec<String>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("--frozen-published {path} is not a list of digests: {error}"))
}

/// A value flag's argument, with presence decided apart from the value.
/// `Ok(None)` is a flag that was never given; a flag given as the last argument
/// or followed by another `--flag` is a usage error naming it. `flag_value`
/// cannot tell those apart, and for `--frozen-published` the difference is
/// between an omitted list and permission to replace published evidence.
fn value_flag<'a>(args: &'a [String], flag: &str, what: &str) -> Result<Option<&'a str>, String> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        return Ok(None);
    };
    // Only the first occurrence would be read, so a repeated flag would silently
    // drop every later value, including a published list that names the source.
    if args.iter().filter(|arg| *arg == flag).count() > 1 {
        return Err(format!("error: {flag} given more than once"));
    }
    match args.get(index + 1) {
        Some(value) if !value.starts_with("--") => Ok(Some(value)),
        _ => Err(format!("error: {flag} requires {what}")),
    }
}

type ValueFlags<'a> = (
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

/// `--original-evaluator`, `--replacement-evaluator`, `--reason` and
/// `--frozen-published`, in that order, each checked for a value before any
/// input is read.
fn value_flags(args: &[String]) -> Result<ValueFlags<'_>, String> {
    Ok((
        value_flag(args, "--original-evaluator", "a JSON file path")?,
        value_flag(args, "--replacement-evaluator", "a JSON file path")?,
        value_flag(args, "--reason", "a text")?,
        value_flag(args, "--frozen-published", "a JSON file path")?,
    ))
}

/// The refusal document. A refusal raised while loading inputs and one raised
/// by the regrade share it, so a machine reader finds every refusal on stdout.
fn refusal_document(path: &str, refusal: &str) -> serde_json::Value {
    serde_json::json!({
        "used_by_gate": false,
        "schema_version": DOCUMENT_VERSION,
        "bundle": path,
        "refusal": refusal,
    })
}

/// Refuse before a receipt exists.
fn refuse_input(path: &str, error: &str, json: bool) -> i32 {
    if json {
        crate::emit_json(&refusal_document(path, error));
    } else {
        eprintln!("error: {error}");
    }
    1
}

/// The operator entry point. `0` emits the receipt, `1` refuses it, `2` is a
/// usage error.
pub fn run(args: &[String], json: bool) -> i32 {
    let Some(path) = args.get(2).filter(|value| !value.starts_with("--")) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let (original_path, replacement_path, reason, frozen_published_path) = match value_flags(args) {
        Ok(flags) => flags,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            return 2;
        }
    };
    let (Some(original_path), Some(replacement_path), Some(reason)) =
        (original_path, replacement_path, reason)
    else {
        eprintln!("{USAGE}");
        return 2;
    };

    let LoadedBundle {
        bundle,
        bundle_sha256,
        root,
    } = match load_bundle(path) {
        Ok(loaded) => loaded,
        Err(error) => return refuse_input(path, &error, json),
    };
    let original = match read_evaluator("--original-evaluator", original_path) {
        Ok(identity) => identity,
        Err(error) => return refuse_input(path, &error, json),
    };
    let replacement = match read_evaluator("--replacement-evaluator", replacement_path) {
        Ok(identity) => identity,
        Err(error) => return refuse_input(path, &error, json),
    };
    let frozen_published = match frozen_published_path {
        Some(list_path) => match read_frozen_published(list_path) {
            Ok(list) => list,
            Err(error) => return refuse_input(path, &error, json),
        },
        None => Vec::new(),
    };
    let verifier = match crate::current_executable_sha256() {
        Ok(digest) => digest,
        Err(error) => return refuse_input(path, &error, json),
    };
    let score_config = match this_score_config_sha256() {
        Ok(digest) => digest,
        Err(error) => return refuse_input(path, &error, json),
    };

    match regrade(
        &bundle,
        &bundle_sha256,
        &root,
        &verifier,
        &score_config,
        &original,
        &replacement,
        reason,
        &frozen_published,
    ) {
        Ok(document) => {
            emit_document(&document, json);
            0
        }
        Err(refusal) => {
            if json {
                // The refusal is the result, so it is emitted as a document
                // rather than printed to stderr and lost by a machine reader.
                crate::emit_json(&refusal_document(path, &refusal.to_string()));
            } else {
                eprintln!("refused: {refusal}");
            }
            1
        }
    }
}

fn emit_document(document: &RegradeDocument, json: bool) {
    if json {
        crate::emit_json(document);
        return;
    }
    println!(
        "regrade receipt for `{}` from bundle {}",
        document.agent_id, document.bundle_sha256
    );
    println!("  frozen manifest : {}", document.frozen_manifest_sha256);
    println!(
        "  source artifact : {} ({})",
        document.receipt.source_artifact_sha256, document.trajectory_path
    );
    println!(
        "  changed inputs  : {}",
        if document.receipt.changed_evaluator_inputs.is_empty() {
            "none".to_string()
        } else {
            document.receipt.changed_evaluator_inputs.join(", ")
        }
    );
    println!("  reason          : {}", document.receipt.reason);
    println!(
        "  decisions read  : {} (agents invoked: {})",
        document.receipt.decisions_replayed, document.receipt.agent_invocations
    );
    println!(
        "  disposition     : {}",
        if document.may_replace_published {
            "the superseding grade may replace the record it grades"
        } else {
            "operational only; the published record keeps its own number"
        }
    );
    println!("\nVerified:");
    for line in &document.verified {
        println!("  - {line}");
    }
    println!("\nNot established:");
    for line in &document.not_established {
        println!("  - {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rescore_cmd::{BoundFile, ResourceDeclaration, BUNDLE_VERSION};
    use sharpebench_harness::run_agent_capture;
    use sharpebench_sim::{Agent, CostModel, Momentum, Window};
    use std::collections::BTreeMap;

    /// Stand-ins for this host's two derived identities, so the tests assert
    /// the checks rather than the digests of whatever binary ran them.
    const VERIFIER: &str = "ab";
    const SCORE_CONFIG: &str = "cd";

    fn verifier() -> String {
        VERIFIER.repeat(32)
    }

    fn score_config() -> String {
        SCORE_CONFIG.repeat(32)
    }

    fn evaluator(verifier_artifact: &str, score_config: &str, wall: u64) -> EvaluatorIdentity {
        EvaluatorIdentity {
            score_config_sha256: score_config.to_string(),
            verifier_artifact_sha256: verifier_artifact.to_string(),
            wall_clock_limit_secs: wall,
            memory_limit_bytes: 2 * 1024 * 1024 * 1024,
        }
    }

    /// The evaluator that graded the artifact first: a different binary, under
    /// a shorter wall clock.
    fn original() -> EvaluatorIdentity {
        evaluator(&"ef".repeat(32), &score_config(), 300)
    }

    /// The evaluator this command would attribute the superseding grade to.
    fn replacement() -> EvaluatorIdentity {
        evaluator(&verifier(), &score_config(), 900)
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
        trajectory_sha256: String,
    }

    impl Fixture {
        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, relative: &str, bytes: &[u8]) {
            std::fs::write(self.dir.path().join(relative), bytes).expect("fixture writes");
        }

        fn regrade(&self) -> Result<RegradeDocument, RegradeCommandRefusal> {
            self.regrade_with(&replacement(), "the evaluator ran out of wall clock", &[])
        }

        fn regrade_with(
            &self,
            replacement: &EvaluatorIdentity,
            reason: &str,
            frozen_published: &[String],
        ) -> Result<RegradeDocument, RegradeCommandRefusal> {
            regrade(
                &self.bundle,
                &"01".repeat(32),
                self.root(),
                &verifier(),
                &score_config(),
                &original(),
                replacement,
                reason,
                frozen_published,
            )
        }
    }

    /// A complete bundle, with one undeclared file beside it standing in for
    /// the agent state a submission leaves on disk.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let csv = dataset_csv();
        let data = Dataset::from_csv(&csv).expect("the fixture CSV parses");
        let costs = CostModel::default();
        let windows = [Window { start: 20, end: 80 }];
        let (_, traj) = run_agent_capture("momentum", &data, &windows, &[0], costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        });

        let trajectory_json = serde_json::to_vec_pretty(&traj).expect("trajectories serialize");
        let costs_json = serde_json::to_vec_pretty(&costs).expect("cost models serialize");

        std::fs::write(dir.path().join("prices.csv"), csv.as_bytes()).expect("fixture writes");
        std::fs::write(dir.path().join("costs.json"), &costs_json).expect("fixture writes");
        std::fs::write(dir.path().join("trajectory.json"), &trajectory_json)
            .expect("fixture writes");
        std::fs::write(
            dir.path().join("agent_state.bin"),
            b"self_reported_return=9999",
        )
        .expect("fixture writes");

        let trajectory_sha256 = sharpebench_attest::content_digest(&trajectory_json);
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
                sha256: trajectory_sha256.clone(),
            },
        ];
        let bundle = SubmissionBundle {
            schema_version: BUNDLE_VERSION.to_string(),
            agent_id: "momentum".to_string(),
            trajectory: "trajectory.json".to_string(),
            dataset: "prices.csv".to_string(),
            costs: "costs.json".to_string(),
            runner_artifact_sha256: "ba".repeat(32),
            image: None,
            frozen_files: Some(frozen),
            resources: ResourceDeclaration {
                cpu_millis: 2000,
                memory_bytes: 2 * 1024 * 1024 * 1024,
                wall_clock_seconds: 900,
                disclosed: BTreeMap::new(),
            },
            claimed: None,
        };
        Fixture {
            dir,
            bundle,
            trajectory_sha256,
        }
    }

    /// The passing control: a complete bundle produces the link, states that
    /// it bought no decisions, and names what changed.
    #[test]
    fn a_complete_bundle_emits_a_receipt_that_bought_no_decisions() {
        let fixture = fixture();
        let document = fixture.regrade().expect("a complete bundle regrades");
        assert_eq!(document.schema_version, DOCUMENT_VERSION);
        assert_eq!(document.agent_id, "momentum");
        assert_eq!(document.receipt.agent_invocations, 0);
        assert_eq!(document.receipt.decisions_replayed, 60);
        assert_eq!(
            document.receipt.reason,
            "the evaluator ran out of wall clock"
        );
        assert_eq!(
            document.receipt.original_evaluator,
            original(),
            "the receipt records the evaluator that graded first as the original"
        );
        assert_eq!(
            document.receipt.replacement_evaluator,
            replacement(),
            "and the one that supersedes it as the replacement"
        );
        assert_eq!(
            document.receipt.changed_evaluator_inputs,
            vec![
                "verifier_artifact_sha256".to_string(),
                "wall_clock_limit_secs".to_string()
            ],
            "the receipt names every evaluator input that differs"
        );
    }

    /// The document is provenance surface. A reader holding the JSON alone
    /// must not mistake it for a result.
    #[test]
    fn the_document_declares_itself_unused_by_the_gate() {
        let document = fixture().regrade().expect("a complete bundle regrades");
        assert!(!document.used_by_gate);
        assert!(
            document
                .not_established
                .iter()
                .any(|line| line.contains("this command emits the link, not a figure")),
            "a regrade must say that it published no figure: {:?}",
            document.not_established
        );
    }

    /// The seam this command exists to close: the digest the receipt names is
    /// hashed from the trajectory bytes, never read off a declaration and
    /// never some other file the bundle happens to bind.
    #[test]
    fn the_source_digest_is_hashed_from_the_trajectory_bytes() {
        let fixture = fixture();
        let document = fixture.regrade().expect("a complete bundle regrades");
        let trajectory_on_disk =
            std::fs::read(fixture.root().join("trajectory.json")).expect("the fixture wrote it");
        assert_eq!(
            document.receipt.source_artifact_sha256,
            sharpebench_attest::content_digest(&trajectory_on_disk),
            "the receipt must name the artifact it regraded"
        );
        assert_eq!(
            document.receipt.source_artifact_sha256,
            fixture.trajectory_sha256
        );
        for other in ["prices.csv", "costs.json"] {
            let bytes = std::fs::read(fixture.root().join(other)).expect("the fixture wrote it");
            assert_ne!(
                document.receipt.source_artifact_sha256,
                sharpebench_attest::content_digest(&bytes),
                "`{other}` is not the artifact being regraded"
            );
        }
        assert_ne!(
            document.receipt.source_artifact_sha256, document.bundle_sha256,
            "the bundle declaration is not the artifact being regraded"
        );
    }

    /// A trajectory that changed under its declaration refuses before any
    /// receipt exists, which is what makes the digest above a read value.
    #[test]
    fn a_trajectory_changed_under_its_declaration_refuses_before_a_receipt_exists() {
        let fixture = fixture();
        let mut trajectory =
            std::fs::read(fixture.root().join("trajectory.json")).expect("the fixture wrote it");
        trajectory.extend_from_slice(b"\n");
        fixture.write("trajectory.json", &trajectory);
        let refusal = fixture.regrade().expect_err("bound bytes changed");
        let RegradeCommandRefusal::Bundle(RescoreRefusal::BoundFileChanged {
            role,
            path,
            declared,
            actual,
        }) = refusal
        else {
            panic!("a changed artifact must refuse as a changed bound file: {refusal:?}");
        };
        assert_eq!(role, "trajectory");
        assert_eq!(path, "trajectory.json");
        assert_ne!(declared, actual);
    }

    /// The replacement evaluator is the one that would grade, so a declaration
    /// naming another binary is refused rather than recorded.
    #[test]
    fn a_replacement_evaluator_naming_another_binary_is_refused() {
        let fixture = fixture();
        let impostor = evaluator(&"99".repeat(32), &score_config(), 900);
        let refusal = fixture
            .regrade_with(&impostor, "the evaluator ran out of wall clock", &[])
            .expect_err("a replacement that is not this binary is visible");
        let RegradeCommandRefusal::ReplacementVerifierNotThisBinary { declared, actual } = refusal
        else {
            panic!("a foreign verifier must refuse as one: {refusal:?}");
        };
        assert_eq!(declared, "99".repeat(32));
        assert_eq!(actual, verifier());
    }

    /// Same rule for the other derivable field: a superseding grade cannot be
    /// attributed to a configuration this binary does not score with.
    #[test]
    fn a_replacement_evaluator_naming_another_score_config_is_refused() {
        let fixture = fixture();
        let impostor = evaluator(&verifier(), &"77".repeat(32), 900);
        let refusal = fixture
            .regrade_with(&impostor, "the evaluator ran out of wall clock", &[])
            .expect_err("a replacement scoring under another configuration is visible");
        let RegradeCommandRefusal::ReplacementScoreConfigNotThisScorer { declared, actual } =
            refusal
        else {
            panic!("a foreign score configuration must refuse as one: {refusal:?}");
        };
        assert_eq!(declared, "77".repeat(32));
        assert_eq!(actual, score_config());
    }

    /// A regrade with no stated reason is not auditable, and the reason the
    /// receipt carries is the one the operator gave.
    #[test]
    fn a_regrade_without_a_stated_reason_is_refused() {
        let fixture = fixture();
        let refusal = fixture
            .regrade_with(&replacement(), "   ", &[])
            .expect_err("a regrade must state its reason");
        assert!(
            matches!(
                refusal,
                RegradeCommandRefusal::Regrade(RegradeRefusal::MissingReason)
            ),
            "an unreasoned regrade must refuse as one: {refusal:?}"
        );
        let stated = fixture
            .regrade_with(
                &replacement(),
                "the verifier host lost power mid-grade",
                &[],
            )
            .expect("a stated reason regrades");
        assert_eq!(
            stated.receipt.reason, "the verifier host lost power mid-grade",
            "the receipt carries the operator's reason and not a stand-in"
        );
    }

    /// A source listed as frozen published evidence yields a superseding grade
    /// that is reportable and never a replacement.
    #[test]
    fn a_frozen_published_source_is_operational_only() {
        let fixture = fixture();
        let loose = fixture.regrade().expect("an unpublished source regrades");
        assert!(loose.may_replace_published);
        assert_eq!(
            loose.receipt.disposition,
            RegradeDisposition::ReplacesSource
        );

        let frozen = fixture
            .regrade_with(
                &replacement(),
                "the evaluator ran out of wall clock",
                std::slice::from_ref(&fixture.trajectory_sha256),
            )
            .expect("a published source still regrades");
        assert!(!frozen.may_replace_published);
        assert_eq!(
            frozen.receipt.disposition,
            RegradeDisposition::OperationalOnly {
                frozen_record: fixture.trajectory_sha256.clone(),
            }
        );
        assert!(
            frozen.receipt.clone().into_published_replacement().is_err(),
            "a frozen published record keeps its own number"
        );
    }

    /// An artifact short of a decision for a step of its window is refused,
    /// never replayed into holds the agent never made.
    #[test]
    fn an_artifact_short_of_a_decision_is_refused_rather_than_replayed() {
        let fixture = fixture();
        let mut traj: sharpebench_protocol::AgentTrajectory = serde_json::from_slice(
            &std::fs::read(fixture.root().join("trajectory.json")).expect("the fixture wrote it"),
        )
        .expect("the fixture trajectory parses");
        traj.runs[0].steps.pop();
        let shortened = serde_json::to_vec_pretty(&traj).expect("trajectories serialize");
        fixture.write("trajectory.json", &shortened);
        let mut bundle = fixture.bundle.clone();
        bundle.frozen_files.as_mut().expect("a manifest")[2].sha256 =
            sharpebench_attest::content_digest(&shortened);
        let refusal = regrade(
            &bundle,
            &"01".repeat(32),
            fixture.root(),
            &verifier(),
            &score_config(),
            &original(),
            &replacement(),
            "the evaluator ran out of wall clock",
            &[],
        )
        .expect_err("a short artifact cannot be graded");
        assert_eq!(
            format!("{refusal}"),
            "run 0 records 59 of 60 decisions: a regrade would have to invent the rest"
        );
    }

    /// The read set is the manifest. A file beside the bundle that it does not
    /// declare is never opened and cannot move a field of the document.
    #[test]
    fn state_outside_the_declared_bundle_cannot_move_the_receipt() {
        let fixture = fixture();
        let before = fixture.regrade().expect("the bundle regrades");
        fixture.write(
            "agent_state.bin",
            b"self_reported_return=0.0001 and much longer",
        );
        fixture.write("undeclared_extra.csv", b"date,symbol,close\nx,Y,1\n");
        let after = fixture.regrade().expect("the bundle still regrades");
        assert_eq!(
            before, after,
            "a receipt must be a function of the declared bundle alone"
        );
    }
}
