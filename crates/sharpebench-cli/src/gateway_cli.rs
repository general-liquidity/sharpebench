//! `sharpebench gateway`: report and preflight the host-observed model gateway.
//!
//! This surface never calls a provider. It resolves the host's route manifest,
//! binds credentials from the environment by name, prints the frozen identity
//! the sweep would run under, the bound table it would enforce, and what the
//! money journal has already committed. It refuses, with an exit code, when the
//! configuration would not support a run.
//!
//! Credential values are read but never printed: the report says which variable
//! backs each alias and whether it is set.
//!
//! Serving is not this command's job, because serving needs a
//! `ProviderTransport` and none ships in this build. The serving loop is
//! `sharpebench_harness::gateway::serve::run_gateway_sweep`, which an operator
//! calls from a binary that supplies the transport; this report reads the
//! journal such a sweep writes, whichever sweep it is bound to.

use std::path::PathBuf;

use serde::Deserialize;
use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway::{GatewayLimits, ModelRoute, RouteTable, Secret};
use sharpebench_harness::gateway_journal::{GatewayBudget, GatewayJournal};

const ROUTES_SCHEMA_VERSION: &str = "sharpebench.gateway-routes.v1";
const MAX_ROUTES_BYTES: u64 = 256 * 1024;

const USAGE: &str = "usage: sharpebench gateway --routes <routes.json> --budget-usd-nanos <n> --max-calls <n> [--journal <journal.json>] [--json]";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutesManifest {
    schema_version: String,
    routes: Vec<RouteEntry>,
}

/// One declared route. The credential is named, never written here: a manifest
/// that could carry key material would put it in every diff and backup.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteEntry {
    alias: String,
    destination: String,
    credential_env: String,
    max_output_tokens: u32,
    /// Input tokens the provider bills for framing this route's requests, on
    /// top of the entrant's content. Required: the reservation is a ceiling on
    /// the whole billed request, and a manifest that omits this would reserve
    /// against content alone.
    input_token_overhead: u64,
    rate_card: serde_json::Value,
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let index = args.iter().position(|arg| arg == name)?;
    args.get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
}

/// `lookup` is how a credential name becomes a value. It is a parameter, not
/// an ambient `env::var`, so the refusal paths can be tested without a process
/// environment and without a real key anywhere near the test.
fn load_routes(
    path: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<(RouteTable, Vec<(String, String)>), String> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| format!("cannot open route manifest: {error}"))?
        .take(MAX_ROUTES_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read route manifest: {error}"))?;
    if bytes.len() as u64 > MAX_ROUTES_BYTES {
        return Err("route manifest exceeds the accepted size".into());
    }
    let manifest: RoutesManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid route manifest: {error}"))?;
    if manifest.schema_version != ROUTES_SCHEMA_VERSION {
        return Err(format!(
            "route manifest schema_version must be {ROUTES_SCHEMA_VERSION}"
        ));
    }
    if manifest.routes.is_empty() {
        return Err("route manifest declares no routes".into());
    }
    let mut routes = Vec::new();
    let mut bindings = Vec::new();
    for entry in manifest.routes {
        let card_bytes = serde_json::to_vec(&entry.rate_card)
            .map_err(|error| format!("invalid rate card for {}: {error}", entry.alias))?;
        let card = RateCard::from_json(&card_bytes)
            .map_err(|error| format!("invalid rate card for {}: {error}", entry.alias))?;
        // The credential is bound by name from the host environment. An unset
        // variable is a refusal, not an empty key that would fail at the wire.
        let value = lookup(&entry.credential_env).ok_or_else(|| {
            format!(
                "credential variable {} for alias {} is not set",
                entry.credential_env, entry.alias
            )
        })?;
        bindings.push((entry.alias.clone(), entry.credential_env.clone()));
        routes.push(ModelRoute::new(
            entry.alias,
            entry.destination,
            Secret::new(value),
            card,
            entry.max_output_tokens,
            entry.input_token_overhead,
        )?);
    }
    Ok((RouteTable::new(routes)?, bindings))
}

fn parse_budget(args: &[String]) -> Result<GatewayBudget, String> {
    let money = flag(args, "--budget-usd-nanos").ok_or(
        "--budget-usd-nanos <n> is required; a paid gateway run needs an explicit ceiling",
    )?;
    let calls = flag(args, "--max-calls")
        .ok_or("--max-calls <n> is required; a paid gateway run needs an explicit call ceiling")?;
    let max_usd_nanos: u128 = money
        .parse()
        .map_err(|_| format!("--budget-usd-nanos must be a non-negative integer, got `{money}`"))?;
    let max_calls: u32 = calls
        .parse()
        .map_err(|_| format!("--max-calls must be a non-negative integer, got `{calls}`"))?;
    if max_usd_nanos == 0 || max_calls == 0 {
        return Err("a gateway budget of zero would refuse every call".into());
    }
    Ok(GatewayBudget {
        max_usd_nanos,
        max_calls,
    })
}

fn limits_json(limits: &GatewayLimits) -> serde_json::Value {
    serde_json::json!({
        "max_request_line_bytes": limits.max_request_line_bytes,
        "max_response_line_bytes": limits.max_response_line_bytes,
        "max_messages": limits.max_messages,
        "max_message_bytes": limits.max_message_bytes,
        "max_tools": limits.max_tools,
        "max_tool_payload_bytes": limits.max_tool_payload_bytes,
        "max_output_tokens": limits.max_output_tokens,
        "max_response_body_bytes": limits.max_response_body_bytes,
        "max_response_text_bytes": limits.max_response_text_bytes,
        "max_retries_per_request": limits.max_retries_per_request,
        "max_concurrent_calls": limits.max_concurrent_calls,
        "provider_read_timeout_ms": limits.provider_read_timeout.as_millis() as u64,
        "provider_call_timeout_ms": limits.provider_call_timeout.as_millis() as u64,
        "max_requests_per_decision": limits.max_requests_per_decision,
    })
}

/// Build the report without touching stdout, so it can be asserted directly.
fn report(
    args: &[String],
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<serde_json::Value, String> {
    let path = flag(args, "--routes").ok_or(USAGE)?;
    let budget = parse_budget(args)?;
    let (routes, bindings) = load_routes(path, lookup)?;
    let limits = GatewayLimits::default();
    let journal_path = flag(args, "--journal").map(PathBuf::from);
    let journal = match &journal_path {
        Some(path) if path.exists() => Some(
            GatewayJournal::load_for_routes(path, &routes.identity_digest(), budget)
                .map_err(|error| format!("cannot resume the gateway journal: {error}"))?,
        ),
        _ => None,
    };
    let spend = journal.as_ref().map(|journal| {
        let state = journal.spend();
        let mut spend = serde_json::json!({
            "calls_started": state.calls_started,
            "priced_usd_nanos": state.priced_usd_nanos.to_string(),
            "unknown_usd_nanos": state.unknown_usd_nanos.to_string(),
            "outstanding_usd_nanos": state.outstanding_usd_nanos.to_string(),
            "available_usd_nanos": journal.available_usd_nanos().to_string(),
            "overspent_usd_nanos": state.overspent_usd_nanos.to_string(),
            "overspent_calls": state.overspent_calls,
            "ceiling_breached": journal.ceiling_breached(),
            "partial": state.is_partial(),
        });
        if let Some(sweep) = &journal.identity.sweep_sha256 {
            spend["sweep_sha256"] = serde_json::Value::from(sweep.as_str());
        }
        spend
    });
    Ok(serde_json::json!({
        "route_table_sha256": routes.identity_digest(),
        "aliases": routes.aliases(),
        "credential_bindings": bindings
            .iter()
            .map(|(alias, name)| serde_json::json!({
                "alias": alias,
                "credential_env": name,
                "value": "<redacted>",
            }))
            .collect::<Vec<_>>(),
        "budget": {
            "max_usd_nanos": budget.max_usd_nanos.to_string(),
            "max_calls": budget.max_calls,
        },
        "limits": limits_json(&limits),
        "journal": journal_path.map(|path| path.display().to_string()),
        "spend": spend,
        "usage_provenance": "host_observed_not_verified_billing",
        "provider_transport": "operator_supplied_none_ships_in_this_build",
    }))
}

pub fn run(args: &[String], json: bool) -> i32 {
    match report(args, &|name| std::env::var(name).ok()) {
        Ok(value) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("the report serializes")
                );
            } else {
                println!("route table   {}", value["route_table_sha256"]);
                println!("aliases       {}", value["aliases"]);
                println!("budget        {}", value["budget"]);
                println!("limits        {}", value["limits"]);
                println!("spend         {}", value["spend"]);
                println!("usage         {}", value["usage_provenance"]);
                println!("transport     {}", value["provider_transport"]);
            }
            0
        }
        Err(error) => {
            eprintln!("{error}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_harness::gateway_journal::JournalIdentity;

    const KEY_VAR: &str = "SHARPEBENCH_TEST_GATEWAY_KEY";
    /// Not a credential: a fixed placeholder handed to a pure lookup, so no
    /// test needs a process environment or a real key.
    const PLACEHOLDER: &str = "placeholder-not-a-credential-0123";

    fn present(name: &str) -> Option<String> {
        (name == KEY_VAR).then(|| PLACEHOLDER.to_string())
    }

    /// Every report in these tests goes through the same pure lookup.
    fn present_report(args: &[String]) -> Result<serde_json::Value, String> {
        report(args, &present)
    }

    fn manifest(alias: &str, revision: &str, credential_env: &str) -> String {
        format!(
            r#"{{"schema_version":"{ROUTES_SCHEMA_VERSION}","routes":[{{"alias":"{alias}","destination":"https://provider.invalid/v1","credential_env":"{credential_env}","max_output_tokens":4096,"input_token_overhead":8,"rate_card":{{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"{revision}","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":2}}}}]}}"#
        )
    }

    fn args(pairs: &[&str]) -> Vec<String> {
        let mut args = vec!["sharpebench".to_string(), "gateway".to_string()];
        args.extend(pairs.iter().map(|value| (*value).to_string()));
        args
    }

    fn write(dir: &std::path::Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write");
        path.display().to_string()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sb-gateway-cli-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// The report states the effective configuration: which aliases exist, what
    /// each bound is, and what the budget allows. No call is made to produce it.
    #[test]
    fn the_report_states_the_effective_configuration() {
        let dir = temp_dir("effective");
        let routes = write(
            &dir,
            "routes.json",
            &manifest("fake.v1", "2026-01-01", KEY_VAR),
        );
        let value = present_report(&args(&[
            "--routes",
            &routes,
            "--budget-usd-nanos",
            "1000000",
            "--max-calls",
            "50",
        ]))
        .expect("a complete configuration reports");
        assert_eq!(value["aliases"][0], "fake.v1");
        assert_eq!(value["budget"]["max_calls"], 50);
        assert_eq!(value["budget"]["max_usd_nanos"], "1000000");
        assert_eq!(
            value["limits"]["provider_read_timeout_ms"],
            GatewayLimits::default().provider_read_timeout.as_millis() as u64
        );
        assert_eq!(
            value["usage_provenance"],
            "host_observed_not_verified_billing"
        );
        let rendered = serde_json::to_string(&value).expect("json");
        assert!(
            !rendered.contains(PLACEHOLDER),
            "the report must not carry credential material: {rendered}"
        );
        assert_eq!(value["credential_bindings"][0]["value"], "<redacted>");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A credential the host has not set is a refusal before anything runs, not
    /// an empty key discovered at the wire.
    #[test]
    fn a_missing_credential_refuses() {
        let dir = temp_dir("nocred");
        let routes = write(
            &dir,
            "routes.json",
            &manifest("fake.v1", "2026-01-01", "SHARPEBENCH_TEST_ABSENT_KEY"),
        );
        let error = present_report(&args(&[
            "--routes",
            &routes,
            "--budget-usd-nanos",
            "1000",
            "--max-calls",
            "5",
        ]))
        .expect_err("an unset credential refuses");
        assert!(error.contains("is not set"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A paid run without an explicit ceiling is refused. Both ceilings are
    /// required, and a zero ceiling is refused rather than silently accepted.
    #[test]
    fn a_missing_or_zero_budget_refuses() {
        let dir = temp_dir("nobudget");
        let routes = write(
            &dir,
            "routes.json",
            &manifest("fake.v1", "2026-01-01", KEY_VAR),
        );
        let no_money = present_report(&args(&["--routes", &routes, "--max-calls", "5"]))
            .expect_err("no money ceiling refuses");
        assert!(no_money.contains("--budget-usd-nanos"), "{no_money}");
        let no_calls = present_report(&args(&["--routes", &routes, "--budget-usd-nanos", "1000"]))
            .expect_err("no call ceiling refuses");
        assert!(no_calls.contains("--max-calls"), "{no_calls}");
        let zero = present_report(&args(&[
            "--routes",
            &routes,
            "--budget-usd-nanos",
            "0",
            "--max-calls",
            "5",
        ]))
        .expect_err("a zero ceiling refuses");
        assert!(zero.contains("zero"), "{zero}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A manifest is refused when it is the wrong schema, empty, or carries a
    /// field the host does not recognize.
    #[test]
    fn a_malformed_route_manifest_refuses() {
        let dir = temp_dir("badmanifest");
        for (name, body) in [
            ("missing.json", String::new()),
            (
                "wrong-schema.json",
                manifest("fake.v1", "2026-01-01", KEY_VAR)
                    .replace(ROUTES_SCHEMA_VERSION, "sharpebench.gateway-routes.v0"),
            ),
            (
                "empty.json",
                format!(r#"{{"schema_version":"{ROUTES_SCHEMA_VERSION}","routes":[]}}"#),
            ),
            (
                "extra-field.json",
                manifest("fake.v1", "2026-01-01", KEY_VAR)
                    .replace(r#""alias""#, r#""api_key":"sk-inline","alias""#),
            ),
        ] {
            let path = write(&dir, name, &body);
            assert!(
                present_report(&args(&[
                    "--routes",
                    &path,
                    "--budget-usd-nanos",
                    "1000",
                    "--max-calls",
                    "5",
                ]))
                .is_err(),
                "{name} must be refused"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A journal from another route table is refused rather than resumed, so a
    /// changed model cannot inherit an earlier sweep's spend.
    #[test]
    fn a_journal_bound_to_another_route_table_refuses() {
        let dir = temp_dir("rebind");
        let first = write(
            &dir,
            "first.json",
            &manifest("fake.v1", "2026-01-01", KEY_VAR),
        );
        let second = write(
            &dir,
            "second.json",
            &manifest("fake.v1", "2026-06-01", KEY_VAR),
        );
        let journal = dir.join("journal.json");
        let (routes, _) = load_routes(&first, &present).expect("routes");
        let budget = GatewayBudget {
            max_usd_nanos: 1000,
            max_calls: 5,
        };
        GatewayJournal::new(JournalIdentity::new(routes.identity_digest(), budget))
            .save(&journal)
            .expect("save");
        let journal = journal.display().to_string();

        assert!(present_report(&args(&[
            "--routes",
            &first,
            "--budget-usd-nanos",
            "1000",
            "--max-calls",
            "5",
            "--journal",
            &journal,
        ]))
        .is_ok());
        let error = present_report(&args(&[
            "--routes",
            &second,
            "--budget-usd-nanos",
            "1000",
            "--max-calls",
            "5",
            "--journal",
            &journal,
        ]))
        .expect_err("a rebound journal refuses");
        assert!(error.contains("cannot resume"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The journal a serving sweep writes is bound to that sweep. The report
    /// reads it under the same routes and budget and names the sweep it
    /// belongs to, rather than refusing it or hiding the binding.
    #[test]
    fn a_sweep_bound_journal_is_reported_with_its_sweep() {
        let dir = temp_dir("sweepbound");
        let routes_path = write(
            &dir,
            "routes.json",
            &manifest("fake.v1", "2026-01-01", KEY_VAR),
        );
        let (routes, _) = load_routes(&routes_path, &present).expect("routes");
        let budget = GatewayBudget {
            max_usd_nanos: 1000,
            max_calls: 5,
        };
        let journal = dir.join("journal.json");
        let sweep = "9".repeat(64);
        GatewayJournal::new(
            JournalIdentity::new(routes.identity_digest(), budget).for_sweep(sweep.clone()),
        )
        .save(&journal)
        .expect("save");
        let journal = journal.display().to_string();
        let value = present_report(&args(&[
            "--routes",
            &routes_path,
            "--budget-usd-nanos",
            "1000",
            "--max-calls",
            "5",
            "--journal",
            &journal,
        ]))
        .expect("a sweep-bound journal under the same routes and budget reports");
        assert_eq!(value["spend"]["sweep_sha256"], sweep.as_str());
        assert_eq!(value["limits"]["max_requests_per_decision"], 32);
        std::fs::remove_dir_all(&dir).ok();
    }
}
