//! Single cooperating writer, durable redo, atomic file replacement.
//!
//! Files are synced before replacement. Directory entries are synced on Unix.
//! Windows uses atomic rename and file syncing; the safe standard library does
//! not provide directory fsync, so this is process-interruption recovery, not a
//! guarantee against hardware failure or sudden power loss on that platform.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sharpebench_attest::{content_digest, PublicChain};

use crate::{
    validate_window_id, StateFile, WindowState, WindowStatus, BOARD_FILE, BOARD_MD_FILE,
    STATE_FILE, WINDOWS_DIR, WINDOW_FILE,
};

pub(crate) const REDO_FILE: &str = ".arena-redo.json";
const LOCK_FILE: &str = ".arena.lock";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
thread_local! {
    // The initializer is already const; Windows' macro expansion is nevertheless
    // diagnosed by Clippy 1.96. Keep the exemption on this test-only declaration.
    #[allow(clippy::missing_const_for_thread_local)]
    static SIMULATE_DISK_FULL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn write_staged(file: &mut File, bytes: &[u8]) -> Result<(), std::io::Error> {
    #[cfg(test)]
    if SIMULATE_DISK_FULL.with(|flag| flag.replace(false)) {
        file.write_all(&bytes[..bytes.len() / 2])?;
        return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
    }
    file.write_all(bytes)
}

pub(crate) struct Lock {
    pub(crate) root: PathBuf,
    _file: File,
}

impl Lock {
    pub(crate) fn acquire(root: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(root).map_err(|e| format!("create arena: {e}"))?;
        let root = root
            .canonicalize()
            .map_err(|e| format!("resolve arena: {e}"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(LOCK_FILE))
            .map_err(|e| format!("open arena writer lock: {e}"))?;
        file.try_lock()
            .map_err(|e| format!("arena writer busy or lock unavailable: {e}"))?;
        Ok(Self { root, _file: file })
    }

    pub(crate) fn read_only(root: &Path) -> Result<Option<Self>, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("resolve arena: {e}"))?;
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(LOCK_FILE))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("open arena read lock: {error}")),
        };
        file.try_lock()
            .map_err(|e| format!("arena writer busy or lock unavailable: {e}"))?;
        Ok(Some(Self { root, _file: file }))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Replacement {
    path: String,
    text: String,
    sha256: String,
}

impl Replacement {
    pub(crate) fn text(path: String, text: String) -> Self {
        let sha256 = content_digest(text.as_bytes());
        Self { path, text, sha256 }
    }

    pub(crate) fn json<T: Serialize>(path: String, value: &T) -> Result<Self, String> {
        let text =
            serde_json::to_string_pretty(value).map_err(|e| format!("serialize arena: {e}"))?;
        Ok(Self::text(path, text))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Redo {
    version: u32,
    entries: Vec<Replacement>,
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("sync directory {}: {e}", path.display()))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "replacement has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    let (temporary, mut file) = loop {
        let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".arena-stage-{}-{nonce}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("stage {}: {error}", path.display())),
        }
    };
    let result = (|| {
        write_staged(&mut file, bytes)
            .map_err(|e| format!("write staged {}: {e}", path.display()))?;
        file.sync_all()
            .map_err(|e| format!("sync staged {}: {e}", path.display()))?;
        drop(file);
        std::fs::rename(&temporary, path)
            .map_err(|e| format!("replace {}: {e}", path.display()))?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn validated_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let components: Vec<_> = Path::new(relative).components().collect();
    if components
        .iter()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("invalid recovery target `{relative}`"));
    }
    let accepted = relative == STATE_FILE
        || match components.as_slice() {
            [Component::Normal(group), Component::Normal(id), Component::Normal(file)] => {
                *group == WINDOWS_DIR
                    && id.to_str().is_some_and(|id| validate_window_id(id).is_ok())
                    && [WINDOW_FILE, BOARD_FILE, BOARD_MD_FILE]
                        .iter()
                        .any(|name| file == name)
            }
            _ => false,
        };
    if !accepted {
        return Err(format!("invalid recovery target `{relative}`"));
    }
    let mut target = root.to_path_buf();
    for component in components {
        target.push(component.as_os_str());
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "recovery target follows a link: {}",
                    target.display()
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "inspect recovery target {}: {error}",
                    target.display()
                ))
            }
        }
    }
    Ok(target)
}

fn validate_redo(root: &Path, redo: &Redo) -> Result<Vec<PathBuf>, String> {
    if redo.version != 1
        || redo
            .entries
            .last()
            .is_none_or(|entry| entry.path != STATE_FILE)
    {
        return Err("unsupported recovery journal or missing final state replacement".to_string());
    }
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for entry in &redo.entries {
        let target = validated_target(root, &entry.path)?;
        if !seen.insert(target.clone()) || content_digest(entry.text.as_bytes()) != entry.sha256 {
            return Err(format!(
                "duplicate recovery target or digest mismatch: {}",
                entry.path
            ));
        }
        match target.file_name().and_then(|name| name.to_str()) {
            Some(STATE_FILE) => {
                let state = serde_json::from_str::<StateFile>(&entry.text)
                    .map_err(|e| format!("invalid recovery state: {e}"))?;
                for id in state
                    .window_order
                    .iter()
                    .chain(&state.published_order)
                    .chain(state.superseded.iter().map(|record| &record.window_id))
                {
                    validate_window_id(id)?;
                }
                if root.join(STATE_FILE).exists() {
                    let old: StateFile = crate::read_json(&root.join(STATE_FILE))?;
                    if !state.published_order.starts_with(&old.published_order) {
                        return Err("recovery would rewrite published window order".to_string());
                    }
                }
            }
            Some(WINDOW_FILE) => {
                let window: WindowState = serde_json::from_str(&entry.text)
                    .map_err(|e| format!("invalid recovery window: {e}"))?;
                if target
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    != Some(window.id.as_str())
                {
                    return Err("recovery window id differs from target".to_string());
                }
                let expected = crate::expected_window_schema(
                    window.fault_plan_sha256.as_deref(),
                    window.records_score_meaning(),
                );
                let value: serde_json::Value =
                    serde_json::from_str(&entry.text).map_err(|e| e.to_string())?;
                let expected_config = serde_json::to_value(crate::ScoreConfig::default())
                    .map_err(|e| e.to_string())?;
                let config = value
                    .get("score_config")
                    .and_then(serde_json::Value::as_object)
                    .ok_or_else(|| "recovery window has no explicit config".to_string())?;
                let expected_keys: BTreeSet<_> = expected_config
                    .as_object()
                    .expect("ScoreConfig is an object")
                    .keys()
                    .collect();
                if window.schema_version != expected
                    || config.keys().collect::<BTreeSet<_>>() != expected_keys
                    || crate::score_config_digest(&window.score_config)?
                        != window.score_config_sha256
                {
                    return Err(
                        "recovery window has unsupported schema or incomplete/mismatched config"
                            .to_string(),
                    );
                }
                crate::validate_sha256("recovery scorer", &window.scorer_artifact_sha256)?;
                if let Some(plan) = &window.fault_plan_sha256 {
                    crate::validate_sha256("recovery fault plan", plan)?;
                }
                if target.exists() {
                    let old: WindowState = crate::read_window_state(&target)?;
                    if old.status == WindowStatus::Published
                        && std::fs::read(&target).map_err(|e| e.to_string())?
                            != entry.text.as_bytes()
                    {
                        return Err("recovery would rewrite a published window".to_string());
                    }
                }
            }
            Some(BOARD_FILE) | Some(BOARD_MD_FILE) => {
                if target.file_name().is_some_and(|name| name == BOARD_FILE) {
                    serde_json::from_str::<PublicChain>(&entry.text)
                        .map_err(|e| format!("invalid recovery board: {e}"))?;
                }
                let window_path = target
                    .parent()
                    .expect("validated window target")
                    .join(WINDOW_FILE);
                if window_path.exists()
                    && crate::read_window_state(&window_path)?.status == WindowStatus::Published
                    && (!target.exists()
                        || std::fs::read(&target).map_err(|e| e.to_string())?
                            != entry.text.as_bytes())
                {
                    return Err("recovery would rewrite a published board".to_string());
                }
            }
            _ => return Err("unsupported recovery content".to_string()),
        }
        targets.push(target);
    }
    // Validate the prospective complete snapshot, not only each payload. A
    // well-formed state must not advance to a missing window/board, and a redo
    // must never alter the bytes named by the supersession ledger.
    let state: StateFile = serde_json::from_str(&redo.entries.last().unwrap().text)
        .map_err(|e| format!("invalid recovery state: {e}"))?;
    let prospective = |relative: &str| -> Result<Vec<u8>, String> {
        let target = validated_target(root, relative)?;
        if let Some(entry) = redo.entries.iter().find(|entry| entry.path == relative) {
            Ok(entry.text.as_bytes().to_vec())
        } else {
            std::fs::read(&target)
                .map_err(|e| format!("cannot read prospective {}: {e}", target.display()))
        }
    };
    let window_at = |id: &str| -> Result<WindowState, String> {
        validate_window_id(id)?;
        let bytes = prospective(&format!("{WINDOWS_DIR}/{id}/{WINDOW_FILE}"))?;
        let window: WindowState = serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid prospective window `{id}`: {e}"))?;
        if window.id != id {
            return Err("prospective window id differs from state".to_string());
        }
        Ok(window)
    };
    let mut active = BTreeSet::new();
    for id in &state.window_order {
        if !active.insert(id) {
            return Err("duplicate prospective active window".to_string());
        }
        window_at(id)?;
    }
    let mut published = BTreeSet::new();
    for id in &state.published_order {
        if !active.contains(id)
            || !published.insert(id)
            || window_at(id)?.status != WindowStatus::Published
        {
            return Err("invalid prospective published window".to_string());
        }
        serde_json::from_slice::<PublicChain>(&prospective(&format!(
            "{WINDOWS_DIR}/{id}/{BOARD_FILE}"
        ))?)
        .map_err(|e| format!("invalid prospective board `{id}`: {e}"))?;
    }
    for historical in &state.superseded {
        if active.contains(&historical.window_id)
            || content_digest(&prospective(&format!(
                "{WINDOWS_DIR}/{}/{WINDOW_FILE}",
                historical.window_id
            ))?) != historical.historical_window_sha256
        {
            return Err("recovery would alter superseded history".to_string());
        }
        if let (Some(id), Some(digest)) = (
            &historical.replacement_window_id,
            &historical.replacement_score_config_sha256,
        ) {
            let replacement = window_at(id)?;
            if &replacement.score_config_sha256 != digest
                || replacement.fault_plan_sha256 != historical.replacement_fault_plan_sha256
            {
                return Err("recovery replacement differs from supersession ledger".to_string());
            }
        }
    }
    Ok(targets)
}

fn apply(lock: &Lock, redo: &Redo, targets: &[PathBuf]) -> Result<(), String> {
    for (entry, target) in redo.entries.iter().zip(targets) {
        if entry.path == STATE_FILE {
            if lock.root.join(WINDOWS_DIR).exists() {
                sync_directory(&lock.root.join(WINDOWS_DIR))?;
            }
            sync_directory(&lock.root)?;
        }
        atomic_replace(target, entry.text.as_bytes())?;
    }
    std::fs::remove_file(lock.root.join(REDO_FILE))
        .map_err(|e| format!("remove completed recovery journal: {e}"))?;
    sync_directory(&lock.root)
}

pub(crate) fn require_no_pending(root: &Path) -> Result<(), String> {
    if root
        .join(REDO_FILE)
        .try_exists()
        .map_err(|e| format!("inspect recovery journal: {e}"))?
    {
        Err("arena has a pending transaction; load it to recover before verification or another mutation".to_string())
    } else {
        Ok(())
    }
}

pub(crate) fn recover(lock: &Lock) -> Result<(), String> {
    let path = lock.root.join(REDO_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("read recovery journal: {error}")),
    };
    let redo: Redo =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid recovery journal: {e}"))?;
    let targets = validate_redo(&lock.root, &redo)?;
    apply(lock, &redo, &targets)
}

pub(crate) fn commit(lock: &Lock, entries: Vec<Replacement>) -> Result<(), String> {
    require_no_pending(&lock.root)?;
    let redo = Redo {
        version: 1,
        entries,
    };
    let targets = validate_redo(&lock.root, &redo)?;
    let bytes =
        serde_json::to_vec_pretty(&redo).map_err(|e| format!("serialize recovery journal: {e}"))?;
    atomic_replace(&lock.root.join(REDO_FILE), &bytes)?;
    apply(lock, &redo, &targets)
}

/// Identity of files controlled by this protocol, including a pending journal.
pub(crate) fn snapshot(root: &Path) -> Result<Option<String>, String> {
    let state_path = root.join(STATE_FILE);
    let bytes = match std::fs::read(&state_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read arena snapshot: {error}")),
    };
    let state: StateFile =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid arena snapshot: {e}"))?;
    let mut paths = BTreeSet::from([STATE_FILE.to_string()]);
    for id in state
        .window_order
        .iter()
        .chain(state.superseded.iter().map(|record| &record.window_id))
    {
        validate_window_id(id)?;
        paths.insert(format!("{WINDOWS_DIR}/{id}/{WINDOW_FILE}"));
    }
    for id in &state.published_order {
        validate_window_id(id)?;
        paths.insert(format!("{WINDOWS_DIR}/{id}/{BOARD_FILE}"));
        paths.insert(format!("{WINDOWS_DIR}/{id}/{BOARD_MD_FILE}"));
    }
    let mut identity = Vec::new();
    for relative in paths {
        let target = validated_target(root, &relative)?;
        match std::fs::read(&target) {
            Ok(bytes) => identity.push((relative, content_digest(&bytes))),
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && target.file_name().is_some_and(|name| name == BOARD_MD_FILE) =>
            {
                identity.push((relative, "missing cosmetic board".to_string()));
            }
            Err(error) => {
                return Err(format!(
                    "cannot read snapshot {}: {error}",
                    target.display()
                ))
            }
        }
    }
    if root.join(REDO_FILE).exists() {
        let bytes = std::fs::read(root.join(REDO_FILE))
            .map_err(|e| format!("read pending snapshot: {e}"))?;
        identity.push((REDO_FILE.to_string(), content_digest(&bytes)));
    }
    serde_json::to_vec(&identity)
        .map(|bytes| Some(content_digest(&bytes)))
        .map_err(|e| format!("serialize snapshot: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_partial_disk_full_preserves_previous_complete_file() {
        let root = std::env::temp_dir().join(format!(
            "sharpe-atomic-full-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("state.json");
        std::fs::write(&target, b"previous complete bytes").unwrap();
        SIMULATE_DISK_FULL.with(|flag| flag.set(true));
        assert!(atomic_replace(&target, b"new bytes that cannot all be written").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"previous complete bytes");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
