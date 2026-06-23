//! Optional self-update + "a newer version exists" nudge (`--features self-update`).
//!
//! Off by default: the published CLI stays dependency-free and the musl static
//! binary stays pure. When enabled this reuses the repo's sanctioned, `ring`-free
//! TLS stack (`ureq` + `native-tls`, exactly as `xtask`), so it adds no new
//! supply-chain surface beyond what is already in the workspace graph.
//!
//! Two surfaces:
//! - [`notify_if_outdated`] — a throttled, fail-soft startup nudge to stderr.
//! - [`run_self_update`] — the `self-update` subcommand: download the latest signed
//!   release binary, verify its SHA-256, and atomically replace the running one.

use std::io::Read as _;
use std::process::ExitCode;
use std::time::Duration;

const REPO: &str = "general-liquidity/sharpebench";
/// crates.io sparse index path for a 10+-char crate name: `na/me/name`.
const SPARSE_INDEX_URL: &str = "https://index.crates.io/sh/ar/sharpebench";
/// Re-check at most once a day so normal invocations stay offline-fast.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A ureq agent on the OS TLS backend (native-tls) with tight timeouts — a version
/// check must never make the CLI hang. Mirrors `xtask`'s `build_agent`.
fn agent() -> Result<ureq::Agent, String> {
    let tls = native_tls::TlsConnector::new().map_err(|e| format!("native-tls init: {e}"))?;
    Ok(ureq::builder()
        .tls_connector(std::sync::Arc::new(tls))
        .timeout_connect(Duration::from_millis(1500))
        .timeout(Duration::from_secs(8))
        .build())
}

/// Parse `"a.b.c"` (optionally `v`-prefixed, ignoring any `-pre`/`+build` suffix)
/// into a comparable triple. Returns `None` if it isn't three integers.
fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next().unwrap_or(core);
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    Some((a, b, c))
}

fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_semver(candidate), parse_semver(current)) {
        (Some(cand), Some(cur)) => cand > cur,
        _ => false,
    }
}

/// Newest non-yanked version from the crates.io sparse index (one JSON object per
/// line, oldest→newest). Returns `None` on any network/parse failure (fail-soft).
fn latest_crate_version(agent: &ureq::Agent) -> Option<String> {
    let body = agent
        .get(SPARSE_INDEX_URL)
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let mut newest: Option<String> = None;
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("yanked").and_then(serde_json::Value::as_bool) == Some(true) {
            continue;
        }
        if let Some(vers) = v.get("vers").and_then(serde_json::Value::as_str) {
            if newest.as_deref().is_none_or(|n| is_newer(vers, n)) {
                newest = Some(vers.to_string());
            }
        }
    }
    newest
}

fn cache_path() -> std::path::PathBuf {
    std::env::temp_dir().join("sharpebench-update-check")
}

/// True at most once per [`CHECK_INTERVAL_SECS`]; records the check time. Any I/O or
/// clock error errs toward checking (returns true) — the check itself is harmless.
fn due_for_check() -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = cache_path();
    let last = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    if now.saturating_sub(last) < CHECK_INTERVAL_SECS {
        return false;
    }
    let _ = std::fs::write(&path, now.to_string());
    true
}

/// Throttled, fail-soft startup nudge. Silent unless a newer version exists; never
/// runs for machine-readable (`--json`) output or the update command itself, and
/// honours `SHARPEBENCH_NO_UPDATE_CHECK` for CI / air-gapped use.
pub fn notify_if_outdated(json: bool, subcommand: Option<&str>) {
    let suppressed = json
        || std::env::var_os("SHARPEBENCH_NO_UPDATE_CHECK").is_some()
        || matches!(subcommand, Some("self-update" | "update"));
    if suppressed || !due_for_check() {
        return;
    }
    let Ok(agent) = agent() else { return };
    if let Some(latest) = latest_crate_version(&agent) {
        if is_newer(&latest, current_version()) {
            eprintln!(
                "note: sharpebench {latest} is available (you have {}). \
                 Update with `cargo install sharpebench` or `sharpebench self-update`.",
                current_version()
            );
        }
    }
}

/// `<asset>` and its `.sha256` for the current platform's signed release binary.
/// `None` on platforms with no published static binary (everyone else updates via
/// `cargo install`).
fn release_asset_name() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("sharpebench-x86_64-linux-musl")
    } else {
        None
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn download_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let resp = agent
        .get(url)
        .set("User-Agent", "sharpebench-self-update")
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut buf)
        .map_err(|e| format!("read {url}: {e}"))?;
    Ok(buf)
}

/// Atomically replace the running executable with `bytes`: write a sibling temp
/// file (same dir → same filesystem, so the rename is atomic), mark it executable,
/// then rename over the current binary.
fn replace_current_exe(bytes: &[u8]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("locating current exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "current exe has no parent dir".to_string())?;
    let tmp = dir.join(".sharpebench-update.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod {}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &exe).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("replacing {}: {e}", exe.display())
    })
}

fn self_update() -> Result<String, String> {
    let Some(asset) = release_asset_name() else {
        return Err(
            "self-update only updates the published Linux static binary; on this platform \
             run `cargo install sharpebench` to upgrade."
                .to_string(),
        );
    };
    let agent = agent()?;

    // Latest release tag via the GitHub API (User-Agent is mandatory).
    let meta = agent
        .get(&format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .set("User-Agent", "sharpebench-self-update")
        .call()
        .map_err(|e| format!("querying latest release: {e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let meta: serde_json::Value = serde_json::from_str(&meta).map_err(|e| e.to_string())?;
    let tag = meta
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or("release has no tag_name")?;
    if !is_newer(tag, current_version()) {
        return Ok(format!(
            "already up to date (have {}, latest is {tag})",
            current_version()
        ));
    }

    // Resolve the binary + checksum asset download URLs from the release.
    let assets = meta
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or("release has no assets")?;
    let url_of = |name: &str| -> Option<String> {
        assets.iter().find_map(|a| {
            (a.get("name").and_then(serde_json::Value::as_str) == Some(name))
                .then(|| {
                    a.get("browser_download_url")
                        .and_then(serde_json::Value::as_str)
                })
                .flatten()
                .map(str::to_string)
        })
    };
    let bin_url = url_of(asset).ok_or_else(|| format!("release {tag} has no asset {asset}"))?;
    let sum_url = url_of(&format!("{asset}.sha256"))
        .ok_or_else(|| format!("release {tag} has no checksum for {asset}"))?;

    // Download, verify the published SHA-256, then swap the binary in place.
    let bin = download_bytes(&agent, &bin_url)?;
    let sums = String::from_utf8(download_bytes(&agent, &sum_url)?)
        .map_err(|_| "checksum file is not UTF-8".to_string())?;
    let expected = sums
        .split_whitespace()
        .next()
        .ok_or("empty checksum file")?
        .to_lowercase();
    let got = sha256_hex(&bin);
    if got != expected {
        return Err(format!(
            "checksum mismatch for {asset}: expected {expected}, got {got} — refusing to install"
        ));
    }
    replace_current_exe(&bin)?;
    Ok(format!("updated {} -> {tag}", current_version()))
}

/// The `self-update` subcommand entry point.
pub fn run_self_update() -> ExitCode {
    match self_update() {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("self-update failed: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_ordering() {
        assert!(is_newer("0.0.2", "0.0.1"));
        assert!(is_newer("v0.1.0", "0.0.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.0.1", "0.0.1"));
        assert!(!is_newer("0.0.1", "0.0.2"));
        // A pre-release/build suffix on the core triple is ignored.
        assert!(is_newer("0.0.2-rc1", "0.0.1"));
        // Unparseable versions never claim "newer" (fail-soft).
        assert!(!is_newer("garbage", "0.0.1"));
        assert!(!is_newer("0.0.2", "garbage"));
    }

    #[test]
    fn sha256_is_lowercase_hex_of_known_vector() {
        // SHA-256("") — the standard empty-input digest.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn this_platform_asset_resolves_or_is_none() {
        // Just exercise the mapping; on Linux/x86_64 it names the published asset.
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(release_asset_name(), Some("sharpebench-x86_64-linux-musl"));
        } else {
            assert!(release_asset_name().is_none());
        }
    }
}
