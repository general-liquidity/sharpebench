//! SharpeBench dev tasks — offline data ingestion (run via `cargo run -p xtask`).
//!
//! Fetches public market data over HTTPS (`native-tls`, no `ring`), normalizes it
//! to the frozen point-in-time CSV the benchmark loads, and writes the dataset plus
//! a SHA-256 sidecar. `publish = false`: these deps never ship in the CLI or the
//! published library crates, so the scoring tree stays dependency-minimal. The
//! benchmark only ever reads the *frozen* artifact — there is no network in the
//! scoring path, which is what keeps a score reproducible forever.
//!
//!   cargo run -p xtask -- crypto    # BTC/ETH/SOL/BNB/XRP daily closes (Binance)
//!   cargo run -p xtask -- indices   # SPX/DJI/IXIC daily closes (FRED, public domain)
//!   cargo run -p xtask -- all

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::process::ExitCode;

use sha2::{Digest, Sha256};

/// symbol -> (date -> close).
type Series = BTreeMap<String, BTreeMap<String, f64>>;

/// A ureq agent wired to the OS TLS backend (native-tls). ureq 2.x does not
/// auto-select a backend once its rustls default is disabled, so we set it here.
fn build_agent() -> Result<ureq::Agent, String> {
    let tls = native_tls::TlsConnector::new().map_err(|e| format!("native-tls init: {e}"))?;
    Ok(ureq::builder()
        .tls_connector(std::sync::Arc::new(tls))
        .build())
}

fn http_get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    agent
        .get(url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?
        .into_string()
        .map_err(|e| format!("read {url}: {e}"))
}

/// Civil date (UTC) from days since the Unix epoch — Howard Hinnant's algorithm,
/// so we don't pull a date crate for one conversion.
fn epoch_days_to_iso(days: i64) -> String {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = era * 400 + yoe + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Daily closes for crypto majors from Binance's public klines API (no key).
fn fetch_binance(agent: &ureq::Agent) -> Result<Series, String> {
    let tickers = [
        ("BTCUSDT", "BTC"),
        ("ETHUSDT", "ETH"),
        ("SOLUSDT", "SOL"),
        ("BNBUSDT", "BNB"),
        ("XRPUSDT", "XRP"),
    ];
    let mut out = Series::new();
    for (ticker, sym) in tickers {
        let url =
            format!("https://api.binance.com/api/v3/klines?symbol={ticker}&interval=1d&limit=1000");
        let v: serde_json::Value =
            serde_json::from_str(&http_get(agent, &url)?).map_err(|e| e.to_string())?;
        let rows = v.as_array().ok_or("binance: expected a JSON array")?;
        let mut m = BTreeMap::new();
        for k in rows {
            let kl = k.as_array().ok_or("binance: expected a kline array")?;
            let open_ms = kl
                .first()
                .and_then(serde_json::Value::as_i64)
                .ok_or("binance: open_time")?;
            let close: f64 = kl
                .get(4)
                .and_then(serde_json::Value::as_str)
                .ok_or("binance: close")?
                .parse()
                .map_err(|_| "binance: non-numeric close")?;
            m.insert(epoch_days_to_iso(open_ms / 86_400_000), close);
        }
        out.insert(sym.to_string(), m);
    }
    Ok(out)
}

/// Daily closes for US equity indices from FRED's public CSV endpoint (no key).
fn fetch_fred(agent: &ureq::Agent) -> Result<Series, String> {
    let series = [("SP500", "SPX"), ("DJIA", "DJI"), ("NASDAQCOM", "IXIC")];
    let mut out = Series::new();
    for (id, sym) in series {
        let url = format!("https://fred.stlouisfed.org/graph/fredgraph.csv?id={id}");
        let body = http_get(agent, &url)?;
        let mut m = BTreeMap::new();
        for line in body.lines().skip(1) {
            let mut it = line.split(',');
            let (Some(date), Some(val)) = (it.next(), it.next()) else {
                continue;
            };
            let (date, val) = (date.trim(), val.trim());
            if date.is_empty() || val.is_empty() || val == "." {
                continue; // FRED marks holidays / missing as "."
            }
            if let Ok(c) = val.parse::<f64>() {
                m.insert(date.to_string(), c);
            }
        }
        out.insert(sym.to_string(), m);
    }
    Ok(out)
}

/// Align every symbol on its common date axis, then write a long-format CSV plus a
/// SHA-256 sidecar to `data/<name>.csv`.
fn write_dataset(name: &str, series: &Series, decimals: usize) -> Result<(), String> {
    let mut axis: Option<BTreeSet<String>> = None;
    for m in series.values() {
        let set: BTreeSet<String> = m.keys().cloned().collect();
        axis = Some(match axis {
            Some(a) => a.intersection(&set).cloned().collect(),
            None => set,
        });
    }
    let dates: Vec<String> = axis.unwrap_or_default().into_iter().collect();
    if dates.len() < 2 {
        return Err(format!("{name}: fewer than 2 dates common to all symbols"));
    }

    let mut csv = String::from("date,symbol,close\n");
    for d in &dates {
        for (sym, m) in series {
            let _ = writeln!(csv, "{d},{sym},{:.*}", decimals, m[d]);
        }
    }

    let path = format!("data/{name}.csv");
    std::fs::write(&path, &csv).map_err(|e| format!("write {path}: {e}"))?;
    let mut hex = String::new();
    for b in Sha256::digest(csv.as_bytes()) {
        let _ = write!(hex, "{b:02x}");
    }
    std::fs::write(format!("{path}.sha256"), format!("{hex}  {name}.csv\n"))
        .map_err(|e| e.to_string())?;
    println!(
        "wrote {path}  ({} rows x {} symbols)  sha256={hex}",
        dates.len(),
        series.len()
    );
    Ok(())
}

fn run() -> Result<(), String> {
    let task = std::env::args().nth(1).unwrap_or_default();
    let agent = match task.as_str() {
        "crypto" | "indices" | "all" => build_agent()?,
        _ => return Err("usage: cargo run -p xtask -- <crypto|indices|all>".to_string()),
    };
    match task.as_str() {
        "crypto" => write_dataset("crypto-majors-1d", &fetch_binance(&agent)?, 8),
        "indices" => write_dataset("us-indices-1d", &fetch_fred(&agent)?, 4),
        _ => {
            write_dataset("crypto-majors-1d", &fetch_binance(&agent)?, 8)?;
            write_dataset("us-indices-1d", &fetch_fred(&agent)?, 4)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
