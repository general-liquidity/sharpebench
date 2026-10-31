//! `--masked` and declared mark times through the real binary, on frozen
//! data. No model and no network.

use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn data(name: &str) -> String {
    format!("{}/../../data/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn ok(args: &[&str]) {
    let out = cli(args);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn refused(args: &[&str]) -> String {
    let out = cli(args);
    assert_ne!(out.status.code(), Some(0), "{args:?} was accepted");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_masked_capture_verifies_only_against_the_masked_data() {
    let dir = std::env::temp_dir().join(format!("sb-masked-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let traj = dir.join("traj.json");
    let traj = traj.to_str().unwrap();
    let csv = data("us-indices-1d.csv");
    ok(&["capture", "momentum", traj, "--data", &csv, "--masked"]);
    let text = std::fs::read_to_string(traj).unwrap();
    assert!(text.contains("ASSET_000") && !text.contains("\"SPX\""));
    ok(&["verify-trajectory", traj, "--data", &csv, "--masked"]);
    let err = refused(&["verify-trajectory", traj, "--data", &csv]);
    assert!(err.contains("dataset"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn commodities_need_a_declared_fill_delay() {
    let dir = std::env::temp_dir().join(format!("sb-marks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let traj = dir.join("traj.json");
    let traj = traj.to_str().unwrap();
    let csv = data("commodities-1d.csv");
    let err = refused(&["capture", "momentum", traj, "--data", &csv]);
    assert!(err.contains("BRENT is marked before WTI"), "{err}");
    let err = refused(&["run", "--data", &csv]);
    assert!(err.contains("BRENT is marked before WTI"), "{err}");
    ok(&[
        "capture",
        "momentum",
        traj,
        "--data",
        &csv,
        "--fill-delay-bars",
        "1",
    ]);
    ok(&[
        "verify-trajectory",
        traj,
        "--data",
        &csv,
        "--fill-delay-bars",
        "1",
    ]);
    let err = refused(&[
        "capture",
        "momentum",
        traj,
        "--data",
        &csv,
        "--fill-delay-bars",
        "0",
    ]);
    assert!(err.contains("at least 1"), "{err}");
    // Datasets without a sheet are unaffected.
    ok(&[
        "capture",
        "momentum",
        traj,
        "--data",
        &data("fx-majors-1d.csv"),
    ]);
    std::fs::remove_dir_all(&dir).unwrap();
}
