use std::process::Command;

fn greeks(spot: &str, time: &str, rate: &str, vol: &str, kind: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(["greeks", spot, "100", time, rate, vol, kind, "--json"])
        .output()
        .expect("run actual CLI")
}

#[test]
fn zero_volatility_cli_quote_retains_discounting_and_the_new_risk_shape() {
    let output = greeks("100", "1", "0.05", "0", "call");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let quote: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!((quote["price"].as_f64().unwrap() - 4.877057549928594).abs() < 1e-10);
    assert_eq!(quote["greeks"]["delta"], 1.0);
    assert_eq!(quote["risk"]["net_short_gamma"], false);
    assert!(quote["risk"].get("unbounded_tail").is_none());
    assert!(quote["risk"].get("naked_short_gamma").is_none());
}

#[test]
fn malformed_and_undefined_quotes_exit_with_a_specific_error_and_no_json() {
    for (s, t, r, v, expected) in [
        ("100", "1", "0.05", "-0.1", "invalid options parameter: vol"),
        ("NaN", "1", "0.05", "0.2", "invalid options parameter: spot"),
        ("100", "1", "inf", "0.2", "invalid options parameter: rate"),
        (
            "100",
            "-1",
            "0.05",
            "0.2",
            "invalid options parameter: t_years",
        ),
        ("100", "1", "0", "0", "Greeks are undefined"),
        ("100", "0", "0.05", "0.2", "Greeks are undefined"),
    ] {
        let output = greeks(s, t, r, v, "call");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
}
