//! `sharpebench regrade`, driven through the built binary.
//!
//! The function-level cases in `regrade_cmd.rs` hand `regrade` stand-in
//! identities and an already parsed bundle. What they cannot reach is the part
//! an operator actually runs: argument parsing, reading the evaluator files,
//! hashing the running executable and the score configuration, loading the
//! bundle declaration off disk, and emitting the receipt. These cases run all
//! of it and read the emitted document back.
//!
//! Two properties are by design and asserted as such, not as defects: a
//! regrade emits a receipt and never a figure, and the original evaluator's
//! identity is the operator's declaration, reported under `not_established`.
//!
//! Every refusal case asserts its own named cause. Several independent causes
//! exit nonzero, so an exit code alone would pass for the wrong reason.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::{json, Value};
use sharpebench_core::ScoreConfig;
use sharpebench_harness::run_agent_capture;
use sharpebench_sim::{Agent, CostModel, Dataset, EvaluatorIdentity, Momentum, Window};

const REASON: &str = "the evaluator ran out of wall clock";
const USAGE_PREFIX: &str = "usage: sharpebench regrade <bundle.json>";

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn digest(bytes: &[u8]) -> String {
    sharpebench_attest::content_digest(bytes)
}

/// The digest the binary under test must report as its own verifier artifact,
/// derived here from the file Cargo built rather than from anything the binary
/// prints.
fn binary_sha256() -> String {
    digest(&std::fs::read(env!("CARGO_BIN_EXE_sharpebench")).expect("the built binary reads"))
}

/// The digest of the configuration the binary scores with, derived here from
/// the kernel's own default configuration.
fn score_config_sha256() -> String {
    digest(&serde_json::to_vec(&ScoreConfig::default()).expect("the score config serializes"))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The evaluator that graded the artifact first: another binary, a shorter
/// wall clock. It is taken on the operator's word.
fn original_evaluator() -> EvaluatorIdentity {
    EvaluatorIdentity {
        score_config_sha256: score_config_sha256(),
        verifier_artifact_sha256: "ef".repeat(32),
        wall_clock_limit_secs: 300,
        memory_limit_bytes: 2 * 1024 * 1024 * 1024,
    }
}

/// The evaluator the superseding grade is attributed to: this binary.
fn replacement_evaluator() -> EvaluatorIdentity {
    EvaluatorIdentity {
        score_config_sha256: score_config_sha256(),
        verifier_artifact_sha256: binary_sha256(),
        wall_clock_limit_secs: 900,
        memory_limit_bytes: 2 * 1024 * 1024 * 1024,
    }
}

/// A frozen submission bundle on disk, with its two evaluator files beside it.
///
/// The frozen inputs come from the real producers: the dataset from
/// `Dataset::synthetic`, the trajectory from `run_agent_capture` on the
/// reference momentum agent, the cost model from `CostModel::default`. The
/// declaration itself is the `sharpebench.submission-bundle.v1` document, which
/// the binary parses with `deny_unknown_fields`, so a field it does not know is
/// a parse failure and not a silent acceptance.
struct Bundle {
    dir: tempfile::TempDir,
}

impl Bundle {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a bundle directory opens");

        let synthetic = Dataset::synthetic(3, 80, 20_260_621);
        let mut csv = String::from("date,symbol,close\n");
        for (symbol, closes) in &synthetic.closes {
            for (index, close) in closes.iter().enumerate() {
                csv.push_str(&format!("{},{symbol},{close}\n", synthetic.dates[index]));
            }
        }
        let data = Dataset::from_csv(&csv).expect("the fixture CSV parses");
        let costs = CostModel::default();
        let (_, trajectory) = run_agent_capture(
            "momentum",
            &data,
            &[Window { start: 20, end: 80 }],
            &[0],
            costs,
            || Box::new(Momentum::default()) as Box<dyn Agent>,
        );
        let trajectory_json =
            serde_json::to_vec_pretty(&trajectory).expect("trajectories serialize");
        let costs_json = serde_json::to_vec_pretty(&costs).expect("cost models serialize");

        let bundle = json!({
            "schema_version": "sharpebench.submission-bundle.v1",
            "agent_id": "momentum",
            "trajectory": "trajectory.json",
            "dataset": "prices.csv",
            "costs": "costs.json",
            "runner_artifact_sha256": "ba".repeat(32),
            "frozen_files": [
                { "path": "prices.csv", "sha256": digest(csv.as_bytes()) },
                { "path": "costs.json", "sha256": digest(&costs_json) },
                { "path": "trajectory.json", "sha256": digest(&trajectory_json) },
            ],
            "resources": {
                "cpu_millis": 2000,
                "memory_bytes": 2u64 * 1024 * 1024 * 1024,
                "wall_clock_seconds": 900,
            },
        });

        let fixture = Self { dir };
        fixture.write("prices.csv", csv.as_bytes());
        fixture.write("costs.json", &costs_json);
        fixture.write("trajectory.json", &trajectory_json);
        fixture.write(
            "bundle.json",
            &serde_json::to_vec_pretty(&bundle).expect("the bundle serializes"),
        );
        fixture.write_evaluator("original.json", &original_evaluator());
        fixture.write_evaluator("replacement.json", &replacement_evaluator());
        fixture
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn arg(&self, name: &str) -> String {
        self.path(name)
            .to_str()
            .expect("a UTF-8 temporary path")
            .to_string()
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.path(name), bytes).expect("fixture writes");
    }

    fn write_evaluator(&self, name: &str, identity: &EvaluatorIdentity) {
        self.write(
            name,
            &serde_json::to_vec_pretty(identity).expect("evaluator identities serialize"),
        );
    }

    fn read(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.path(name)).expect("the fixture wrote it")
    }

    /// Every file in the bundle directory, by name, with its bytes.
    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        for entry in std::fs::read_dir(self.dir.path()).expect("the bundle directory lists") {
            let entry = entry.expect("a directory entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            files.insert(name, std::fs::read(entry.path()).expect("the file reads"));
        }
        files
    }

    /// The complete valid invocation, in JSON mode.
    fn full_args(&self) -> Vec<String> {
        vec![
            "regrade".to_string(),
            self.arg("bundle.json"),
            "--original-evaluator".to_string(),
            self.arg("original.json"),
            "--replacement-evaluator".to_string(),
            self.arg("replacement.json"),
            "--reason".to_string(),
            REASON.to_string(),
            "--json".to_string(),
        ]
    }

    fn run(&self, args: &[String]) -> Output {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        cli(&args)
    }
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not a JSON document ({error}); stdout: {} stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn field<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("`{pointer}` is not a string in {value}"))
}

/// The passing control. A frozen bundle with two well-formed evaluator files
/// produces a receipt, and every identity in it is one the test derived
/// independently: the source digest from the trajectory bytes on disk, the
/// verifier from the built executable, the score configuration from the
/// kernel's default. Without this case a binary that refused every invocation
/// would satisfy every refusal case below.
#[test]
fn a_frozen_bundle_emits_a_receipt_derived_from_the_trajectory_bytes_and_this_binary() {
    let bundle = Bundle::new();
    let before = bundle.snapshot();

    let output = bundle.run(&bundle.full_args());
    assert_eq!(
        output.status.code(),
        Some(0),
        "a frozen bundle regrades; stderr: {}",
        stderr(&output)
    );
    let document = stdout_json(&output);

    assert_eq!(
        field(&document, "/schema_version"),
        "sharpebench.regrade-document.v1"
    );
    assert_eq!(document["used_by_gate"], json!(false));
    assert_eq!(field(&document, "/agent_id"), "momentum");
    assert_eq!(field(&document, "/trajectory_path"), "trajectory.json");
    assert_eq!(
        field(&document, "/bundle_sha256"),
        digest(&bundle.read("bundle.json")),
        "the bundle digest is hashed from the declaration the binary loaded"
    );
    assert!(is_sha256(field(&document, "/frozen_manifest_sha256")));

    // The source digest is the trajectory's, recomputed here from the bytes on
    // disk, and is neither the bundle declaration's digest nor another bound
    // file's.
    let source = field(&document, "/receipt/source_artifact_sha256");
    assert_eq!(
        source,
        digest(&bundle.read("trajectory.json")),
        "the receipt names the trajectory it regraded"
    );
    assert_ne!(source, field(&document, "/bundle_sha256"));
    for other in ["prices.csv", "costs.json"] {
        assert_ne!(
            source,
            digest(&bundle.read(other)),
            "`{other}` is not the artifact"
        );
    }

    // The replacement identity is this binary and this scorer, and the document
    // says it checked both rather than echoing them.
    let binary = binary_sha256();
    let config = score_config_sha256();
    assert!(is_sha256(&binary) && is_sha256(&config));
    let replacement: EvaluatorIdentity =
        serde_json::from_value(document["receipt"]["replacement_evaluator"].clone())
            .expect("the receipt carries an evaluator identity");
    assert_eq!(replacement.verifier_artifact_sha256, binary);
    assert_eq!(replacement.score_config_sha256, config);
    let verified: Vec<&str> = document["verified"]
        .as_array()
        .expect("a verified list")
        .iter()
        .map(|line| line.as_str().expect("verified lines are strings"))
        .collect();
    assert!(
        verified.iter().any(|line| line.contains(&format!(
            "the replacement evaluator is this binary: verifier artifact {binary} scoring under configuration {config}"
        ))),
        "the document must record the derived identity it checked: {verified:?}"
    );
    assert!(
        verified.iter().any(|line| line.contains(&format!(
            "the source artifact digest {source} was hashed from the trajectory bytes"
        ))),
        "the document must record where the source digest came from: {verified:?}"
    );

    // The original identity is recorded as declared, and reported as a
    // declaration.
    let original: EvaluatorIdentity =
        serde_json::from_value(document["receipt"]["original_evaluator"].clone())
            .expect("the receipt carries an evaluator identity");
    assert_eq!(original, original_evaluator());
    let not_established: Vec<&str> = document["not_established"]
        .as_array()
        .expect("a not_established list")
        .iter()
        .map(|line| line.as_str().expect("not_established lines are strings"))
        .collect();
    assert!(
        not_established.iter().any(|line| line
            .starts_with("that the original evaluator is the one the operator named")
            && line.contains("the original identity is the operator's declaration")),
        "the original evaluator must be reported as not established: {not_established:?}"
    );
    assert!(
        not_established
            .iter()
            .any(|line| line.contains("this command emits the link, not a figure")),
        "a regrade must say it emitted no figure: {not_established:?}"
    );

    assert_eq!(
        document["receipt"]["changed_evaluator_inputs"],
        json!(["verifier_artifact_sha256", "wall_clock_limit_secs"])
    );
    assert_eq!(field(&document, "/receipt/reason"), REASON);
    assert_eq!(
        document["receipt"]["disposition"],
        json!({ "disposition": "replaces_source" })
    );
    assert_eq!(document["may_replace_published"], json!(true));

    // No agent was invoked: every graded step was read out of the artifact.
    assert_eq!(document["receipt"]["agent_invocations"], json!(0));
    assert_eq!(document["receipt"]["decisions_replayed"], json!(60));

    // A regrade reads its inputs and writes nothing beside them.
    assert_eq!(
        bundle.snapshot(),
        before,
        "the regrade must leave every input byte, and the directory listing, unchanged"
    );
}

/// A trajectory whose bytes changed under its declaration is refused as a
/// changed bound file naming both digests, before any receipt exists.
#[test]
fn a_trajectory_changed_under_its_declaration_is_refused_as_a_changed_bound_file() {
    let bundle = Bundle::new();
    let declared = digest(&bundle.read("trajectory.json"));
    let mut tampered = bundle.read("trajectory.json");
    tampered.extend_from_slice(b"\n");
    bundle.write("trajectory.json", &tampered);
    let actual = digest(&tampered);
    let before = bundle.snapshot();

    let output = bundle.run(&bundle.full_args());
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    let document = stdout_json(&output);
    assert!(
        document.get("receipt").is_none(),
        "a refusal carries no receipt: {document}"
    );
    assert_eq!(document["used_by_gate"], json!(false));
    assert_eq!(
        field(&document, "/refusal"),
        format!(
            "the bundle's trajectory `trajectory.json` changed: it is bound to {declared} and the bytes on disk hash to {actual}"
        )
    );
    assert_eq!(bundle.snapshot(), before, "a refusal modifies no input");
}

/// A declared evaluator file that does not exist is refused naming the path it
/// could not read. In JSON mode the refusal is the result, so it is the same
/// refusal document a regrade refusal emits, on stdout; in text mode it is an
/// error on stderr.
#[test]
fn a_missing_evaluator_file_is_refused_naming_the_unreadable_path() {
    let bundle = Bundle::new();
    let missing = bundle.arg("no-such-original.json");
    let mut args = bundle.full_args();
    args[3] = missing.clone();

    let output = bundle.run(&args);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    let document = stdout_json(&output);
    assert!(
        document.get("receipt").is_none(),
        "a refusal carries no receipt: {document}"
    );
    assert_eq!(document["used_by_gate"], json!(false));
    assert_eq!(
        field(&document, "/schema_version"),
        "sharpebench.regrade-document.v1"
    );
    assert_eq!(field(&document, "/bundle"), bundle.arg("bundle.json"));
    assert!(
        field(&document, "/refusal").starts_with(&format!("cannot read {missing}: ")),
        "the refusal must name the unreadable evaluator file: {document}"
    );

    args.retain(|arg| arg != "--json");
    let output = bundle.run(&args);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(
        output.stdout.is_empty(),
        "no receipt is emitted: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr(&output).starts_with(&format!("error: cannot read {missing}: ")),
        "the refusal must name the unreadable evaluator file: {}",
        stderr(&output)
    );
}

/// An evaluator file that is not an evaluator identity is refused naming the
/// flag and the file, for both the original and the replacement.
#[test]
fn a_malformed_evaluator_file_is_refused_as_not_an_evaluator_identity() {
    let bundle = Bundle::new();

    bundle.write("original-not-json.json", b"this is not JSON");
    let mut args = bundle.full_args();
    args[3] = bundle.arg("original-not-json.json");
    let output = bundle.run(&args);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    let document = stdout_json(&output);
    assert!(document.get("receipt").is_none(), "no receipt is emitted");
    assert!(
        field(&document, "/refusal").starts_with(&format!(
            "--original-evaluator {} is not an evaluator identity: ",
            args[3]
        )),
        "the refusal must name the malformed original evaluator: {document}"
    );

    bundle.write(
        "replacement-partial.json",
        serde_json::to_string(&json!({ "verifier_artifact_sha256": binary_sha256() }))
            .expect("serializes")
            .as_bytes(),
    );
    let mut args = bundle.full_args();
    args[5] = bundle.arg("replacement-partial.json");
    let output = bundle.run(&args);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    let document = stdout_json(&output);
    assert!(document.get("receipt").is_none(), "no receipt is emitted");
    let message = field(&document, "/refusal");
    assert!(
        message.starts_with(&format!(
            "--replacement-evaluator {} is not an evaluator identity: ",
            args[5]
        )) && message.contains("missing field `score_config_sha256`"),
        "the refusal must name the malformed replacement evaluator and what it lacks: {message}"
    );
}

/// Each required argument, dropped from an otherwise valid invocation, is a
/// usage error (exit 2) and not a refusal of the bundle (exit 1).
#[test]
fn a_missing_required_argument_is_a_usage_error() {
    let bundle = Bundle::new();
    let full = bundle.full_args();

    // Positions in `full_args`: 1 bundle, 2..=3 original, 4..=5 replacement,
    // 6..=7 reason.
    let cases: [(&str, &[usize]); 4] = [
        ("the bundle path", &[1]),
        ("--original-evaluator", &[2, 3]),
        ("--replacement-evaluator", &[4, 5]),
        ("--reason", &[6, 7]),
    ];
    for (dropped, positions) in cases {
        let args: Vec<String> = full
            .iter()
            .enumerate()
            .filter(|(index, _)| !positions.contains(index))
            .map(|(_, value)| value.clone())
            .collect();
        let output = bundle.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "dropping {dropped} is a usage error; stderr: {}",
            stderr(&output)
        );
        assert!(
            output.stdout.is_empty(),
            "dropping {dropped} emits no document"
        );
        assert!(
            stderr(&output).starts_with(USAGE_PREFIX),
            "dropping {dropped} must print the regrade usage: {}",
            stderr(&output)
        );
    }
}

/// The verifier identity is derived by the running binary, not read off the
/// declaration: a replacement evaluator naming any other artifact is refused
/// naming both the declared digest and this binary's.
#[test]
fn a_replacement_evaluator_that_is_not_this_binary_is_refused() {
    let bundle = Bundle::new();
    let impostor = EvaluatorIdentity {
        verifier_artifact_sha256: "99".repeat(32),
        ..replacement_evaluator()
    };
    bundle.write_evaluator("replacement.json", &impostor);

    let output = bundle.run(&bundle.full_args());
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    let document = stdout_json(&output);
    assert!(document.get("receipt").is_none());
    let refusal = field(&document, "/refusal");
    assert!(
        refusal.starts_with(&format!(
            "the replacement evaluator declares verifier artifact {} and this binary is {}",
            "99".repeat(32),
            binary_sha256()
        )),
        "the refusal must name the declared and derived verifier: {refusal}"
    );
}

/// Asserts a usage error naming `flag` as the one given without a value: exit
/// 2, no document on stdout, and that flag named on stderr. Several causes exit
/// 2, so the exit code alone would pass for the wrong flag.
fn assert_flag_without_value(output: &Output, flag: &str, what: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "{flag} without a value is a usage error; stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        stderr(output)
    );
    assert!(
        output.stdout.is_empty(),
        "{flag} without a value emits no receipt: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr(output).starts_with(&format!("error: {flag} requires {what}\n{USAGE_PREFIX}")),
        "the usage error must name {flag}: {}",
        stderr(output)
    );
}

/// Writes a `--frozen-published` list into the bundle directory and returns
/// its path argument.
fn frozen_published_list(bundle: &Bundle, name: &str, digests: &[String]) -> String {
    bundle.write(
        name,
        &serde_json::to_vec(digests).expect("a digest list serializes"),
    );
    bundle.arg(name)
}

/// `--frozen-published` as the last argument names no list. Read as an omitted
/// list it would let the receipt replace the published evidence the operator
/// named the flag to protect.
#[test]
fn a_frozen_published_flag_as_the_last_argument_is_a_usage_error() {
    let bundle = Bundle::new();
    let mut args = bundle.full_args();
    args.retain(|arg| arg != "--json");
    args.push("--frozen-published".to_string());

    assert_flag_without_value(&bundle.run(&args), "--frozen-published", "a JSON file path");
}

/// `--json` is stripped before parsing, so `--frozen-published --json` leaves
/// the flag last: the same usage error, and no JSON receipt.
#[test]
fn a_frozen_published_flag_followed_only_by_json_is_a_usage_error() {
    let bundle = Bundle::new();
    let mut args = bundle.full_args();
    args.retain(|arg| arg != "--json");
    args.push("--frozen-published".to_string());
    args.push("--json".to_string());

    assert_flag_without_value(&bundle.run(&args), "--frozen-published", "a JSON file path");
}

/// `--original-evaluator` followed directly by `--replacement-evaluator`, with
/// everything else in the invocation valid.
#[test]
fn an_original_evaluator_flag_followed_by_another_flag_is_a_usage_error() {
    let bundle = Bundle::new();
    let mut args = bundle.full_args();
    assert_eq!(args[2], "--original-evaluator");
    args.remove(3);

    assert_flag_without_value(
        &bundle.run(&args),
        "--original-evaluator",
        "a JSON file path",
    );
}

/// `--replacement-evaluator` followed directly by `--reason`, with everything
/// else in the invocation valid.
#[test]
fn a_replacement_evaluator_flag_followed_by_another_flag_is_a_usage_error() {
    let bundle = Bundle::new();
    let mut args = bundle.full_args();
    assert_eq!(args[4], "--replacement-evaluator");
    args.remove(5);

    assert_flag_without_value(
        &bundle.run(&args),
        "--replacement-evaluator",
        "a JSON file path",
    );
}

/// `--reason --frozen-published <list>` must not record the literal
/// `--frozen-published` as the reason and silently drop the list. The list is
/// readable and valid, so the reason is the only cause.
#[test]
fn a_reason_flag_followed_by_another_flag_is_a_usage_error() {
    let bundle = Bundle::new();
    let list = frozen_published_list(
        &bundle,
        "published.json",
        &[digest(&bundle.read("trajectory.json"))],
    );
    let mut args = bundle.full_args();
    assert_eq!(args[6], "--reason");
    args[7] = "--frozen-published".to_string();
    args.insert(8, list);

    assert_flag_without_value(&bundle.run(&args), "--reason", "a text");
}

/// The control for the usage errors above: a list that was actually read
/// decides the disposition, in both directions. A list naming the source
/// forbids replacement and one that does not name it permits it, so neither a
/// binary refusing every `--frozen-published` nor one forbidding replacement
/// whenever the flag appears passes.
#[test]
fn a_frozen_published_list_that_was_read_decides_whether_the_receipt_may_replace() {
    let bundle = Bundle::new();
    let source = digest(&bundle.read("trajectory.json"));

    let naming = frozen_published_list(&bundle, "names-source.json", std::slice::from_ref(&source));
    let mut args = bundle.full_args();
    args.push("--frozen-published".to_string());
    args.push(naming);
    let output = bundle.run(&args);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let document = stdout_json(&output);
    assert_eq!(document["may_replace_published"], json!(false));
    assert_eq!(
        document["receipt"]["disposition"],
        json!({ "disposition": "operational_only", "frozen_record": source })
    );

    let other = frozen_published_list(
        &bundle,
        "names-other.json",
        &["00".repeat(32), digest(&bundle.read("costs.json"))],
    );
    let mut args = bundle.full_args();
    args.push("--frozen-published".to_string());
    args.push(other);
    let output = bundle.run(&args);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let document = stdout_json(&output);
    assert_eq!(document["may_replace_published"], json!(true));
    assert_eq!(
        document["receipt"]["disposition"],
        json!({ "disposition": "replaces_source" })
    );
}
