//! sharpebench-arena - the forward-league driver.
//!
//! Every attestation primitive already exists in this workspace: commitments and
//! the epoch-locked [`sharpebench_attest::registry::Registry`], sealed datasets,
//! HMAC and Ed25519 result chains, and board rendering in
//! `sharpebench_leaderboard`. Nothing drove them. This crate is the driver: a
//! file-backed [`Arena`] that walks evaluation windows through
//! open -> committed -> scoring -> published, refusing late commitments and
//! failed reveals with the exact semantics the registry already defines.
//!
//! **The kernel stays clock-free.** Time inside the arena is an explicit integer
//! epoch, exactly as in `registry.rs`. The arena maps epochs to wall time ONLY at
//! this layer, and only through an explicit [`Arena::advance`] call made by the
//! operator or an external scheduler (cron, CI). No function in this crate reads
//! a clock; given the same epochs and inputs, every state transition and every
//! published byte is deterministic.
//!
//! **Cross-window chaining.** Each published window is an Ed25519
//! [`PublicChain`] board. The existing `PublicChain` API is genesis-anchored per
//! document and cannot express cross-document chaining without modification, so
//! chaining is implemented at the arena layer: the first signed payload of
//! window N+1 (its [`WindowHeader`]) carries the final signature of window N's
//! board. Altering window N's board changes (or invalidates) that final
//! signature, so [`verify_arena`] catches it either as a broken chain in N or as
//! an anchor mismatch in N+1. The whole arena history is one verifiable chain,
//! checkable with only the public key.
#![forbid(unsafe_code)]

pub mod sandbox;

pub use sandbox::{
    docker_available, resolve_launch, run_external_sandboxed, Launch, SandboxError, SandboxOptions,
};
// Re-exported so a driver (the CLI) can sign and pin without a direct attest dep.
pub use sharpebench_attest::{SigningKey, VerifyingKey};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use sharpebench_attest::registry::Registry;
use sharpebench_attest::{
    content_digest, publish_public_chain, verify_public_chain, verify_public_chain_with,
    Commitment, PublicChain,
};
use sharpebench_core::{rank, AgentSubmission, CompositeScore, ScoreConfig};

pub const STATE_FILE: &str = "state.json";
pub const WINDOWS_DIR: &str = "windows";
pub const WINDOW_FILE: &str = "window.json";
pub const BOARD_FILE: &str = "board.json";
pub const BOARD_MD_FILE: &str = "board.md";
/// Anchor value carried by the first published window's header.
pub const GENESIS_ANCHOR: &str = "genesis";
/// `kind` tag on a [`WindowHeader`] payload.
pub const WINDOW_HEADER_KIND: &str = "sharpebench-arena-window";

/// Lifecycle status of one evaluation window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowStatus {
    /// Accepting commitments (until the commit deadline).
    Open,
    /// Commit deadline passed; waiting for the data-reveal epoch.
    Committed,
    /// Reveals verified and the field scored; waiting for publish.
    Scoring,
    /// Board signed and written; immutable from the arena's point of view.
    Published,
}

/// A recorded refusal: an entry that was not scored, and why. Refusals are part
/// of the window's permanent record (and of the signed board header), so a
/// failed reveal cannot be silently dropped from history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub agent_id: String,
    pub reason: String,
}

/// Persistent state of one evaluation window (`windows/<id>/window.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowState {
    pub id: String,
    /// Epoch at (and after) which new commitments are refused.
    pub commit_deadline: u64,
    /// Epoch at (and after) which the dataset may be revealed and scored.
    pub data_reveal_epoch: u64,
    pub status: WindowStatus,
    /// The scoring rules, fixed at `open_window` time, before any entry exists.
    pub score_config: ScoreConfig,
    /// Optional SHA-256 commitment to the secret salt used to derive a
    /// SharpeArena sealed-evaluation seed set. The salt itself never enters the
    /// arena state; after scoring it can be revealed and independently checked
    /// against this pre-entry commitment.
    #[serde(default)]
    pub sealed_eval_salt_sha256: Option<String>,
    pub commitments: Vec<Commitment>,
    #[serde(default)]
    pub refusals: Vec<Refusal>,
    /// SHA-256 hex of the revealed dataset bytes, recorded at scoring time.
    #[serde(default)]
    pub dataset_hash: Option<String>,
    #[serde(default)]
    pub scores: Vec<CompositeScore>,
}

/// One revealed entry at scoring time: the agent's scored submission plus the
/// pre-image (artifact digest + salt) of the commitment it registered before the
/// deadline. The commitment itself is already on file in the window.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevealedEntry {
    pub submission: AgentSubmission,
    pub artifact_digest: String,
    pub salt: String,
}

/// The first signed payload of every published board. Binding the window's
/// rules, dataset hash, refusals, and the previous board's final signature into
/// the chain means none of them can be altered after publication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowHeader {
    /// Always [`WINDOW_HEADER_KIND`].
    pub kind: String,
    pub window_id: String,
    pub commit_deadline: u64,
    pub data_reveal_epoch: u64,
    pub dataset_hash: String,
    pub score_config: ScoreConfig,
    /// The pre-entry sealed-evaluation salt commitment, when this window uses
    /// SharpeArena's commit-reveal seed protocol.
    #[serde(default)]
    pub sealed_eval_salt_sha256: Option<String>,
    pub refusals: Vec<Refusal>,
    /// Final signature of the previously published window's board, or
    /// [`GENESIS_ANCHOR`] for the arena's first published window.
    pub prev_final_signature: String,
}

/// Arena-level persistent state (`state.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StateFile {
    current_epoch: u64,
    /// Windows in open order.
    window_order: Vec<String>,
    /// Windows in publish order - the order the cross-window chain runs in.
    published_order: Vec<String>,
}

/// Verification result for one published board.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowVerification {
    pub window_id: String,
    /// The board's own Ed25519 chain recomputes end to end.
    pub chain_ok: bool,
    /// The header's `prev_final_signature` matches the previous published
    /// board's actual final signature (the cross-window link).
    pub anchor_ok: bool,
    /// The board is signed under the same verifying key as the rest of the
    /// arena (and the pinned key, when one is supplied).
    pub key_ok: bool,
    pub detail: String,
}

/// Verification result for a whole arena directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArenaVerification {
    pub ok: bool,
    pub verifying_key: Option<String>,
    pub windows: Vec<WindowVerification>,
}

/// The file-backed arena. All mutating methods persist before returning, so a
/// process crash between calls loses nothing.
pub struct Arena {
    root: PathBuf,
    state: StateFile,
    windows: BTreeMap<String, WindowState>,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("invalid JSON in {}: {e}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// A window id doubles as a directory name; keep it to a safe charset.
fn validate_window_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid window id `{id}`: use ASCII letters, digits, `-`, `_`, `.` (no leading dot)"
        ))
    }
}

impl Arena {
    fn window_dir(&self, id: &str) -> PathBuf {
        self.root.join(WINDOWS_DIR).join(id)
    }

    fn save(&self) -> Result<(), String> {
        write_json(&self.root.join(STATE_FILE), &self.state)?;
        for (id, w) in &self.windows {
            let dir = self.window_dir(id);
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            write_json(&dir.join(WINDOW_FILE), w)?;
        }
        Ok(())
    }

    /// Create a new arena directory. Fails if `dir` already holds one.
    pub fn init(dir: &Path) -> Result<Self, String> {
        let state_path = dir.join(STATE_FILE);
        if state_path.exists() {
            return Err(format!("{} already exists", state_path.display()));
        }
        std::fs::create_dir_all(dir.join(WINDOWS_DIR))
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let arena = Self {
            root: dir.to_path_buf(),
            state: StateFile::default(),
            windows: BTreeMap::new(),
        };
        arena.save()?;
        Ok(arena)
    }

    /// Load an existing arena directory.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let state: StateFile = read_json(&dir.join(STATE_FILE))?;
        let mut windows = BTreeMap::new();
        for id in &state.window_order {
            let w: WindowState = read_json(&dir.join(WINDOWS_DIR).join(id).join(WINDOW_FILE))?;
            windows.insert(id.clone(), w);
        }
        Ok(Self {
            root: dir.to_path_buf(),
            state,
            windows,
        })
    }

    pub fn current_epoch(&self) -> u64 {
        self.state.current_epoch
    }

    pub fn window(&self, id: &str) -> Option<&WindowState> {
        self.windows.get(id)
    }

    pub fn window_ids(&self) -> &[String] {
        &self.state.window_order
    }

    pub fn published_ids(&self) -> &[String] {
        &self.state.published_order
    }

    /// Advance the arena's epoch. This is the ONLY place wall time enters the
    /// system: the caller (an operator, cron, or CI) decides what epoch "now"
    /// is. Monotonic - moving backwards is refused. Any open window whose commit
    /// deadline has passed flips to `Committed`.
    pub fn advance(&mut self, now_epoch: u64) -> Result<(), String> {
        if now_epoch < self.state.current_epoch {
            return Err(format!(
                "epoch cannot move backwards ({} -> {now_epoch})",
                self.state.current_epoch
            ));
        }
        self.state.current_epoch = now_epoch;
        for w in self.windows.values_mut() {
            if w.status == WindowStatus::Open && now_epoch >= w.commit_deadline {
                w.status = WindowStatus::Committed;
            }
        }
        self.save()
    }

    /// Open a window. Nothing is sealed yet, but the [`ScoreConfig`] the window
    /// will be scored under is recorded now, so the rules are fixed before any
    /// entry exists.
    pub fn open_window(
        &mut self,
        id: &str,
        commit_deadline: u64,
        data_reveal_epoch: u64,
        config: ScoreConfig,
    ) -> Result<(), String> {
        self.open_window_with_sealed_eval_commitment(
            id,
            commit_deadline,
            data_reveal_epoch,
            config,
            None,
        )
    }

    /// Open a window with an optional SHA-256 commitment to a SharpeArena
    /// sealed-evaluation salt. The commitment is public and checked here for
    /// shape only; the secret salt remains outside this repository until the
    /// post-score reveal. It is part of the signed window header, so an operator
    /// cannot replace it after entries have committed.
    pub fn open_window_with_sealed_eval_commitment(
        &mut self,
        id: &str,
        commit_deadline: u64,
        data_reveal_epoch: u64,
        config: ScoreConfig,
        sealed_eval_salt_sha256: Option<String>,
    ) -> Result<(), String> {
        validate_window_id(id)?;
        if self.windows.contains_key(id) {
            return Err(format!("window `{id}` already exists"));
        }
        if commit_deadline <= self.state.current_epoch {
            return Err(format!(
                "commit deadline {commit_deadline} is not after the current epoch {}",
                self.state.current_epoch
            ));
        }
        if data_reveal_epoch < commit_deadline {
            return Err(format!(
                "data-reveal epoch {data_reveal_epoch} is before the commit deadline {commit_deadline}"
            ));
        }
        if let Some(commitment) = &sealed_eval_salt_sha256 {
            let valid = commitment.len() == 64
                && commitment
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
            if !valid {
                return Err(
                    "sealed evaluation commitment must be a 64-character lowercase SHA-256 hex digest"
                        .to_string(),
                );
            }
        }
        self.windows.insert(
            id.to_string(),
            WindowState {
                id: id.to_string(),
                commit_deadline,
                data_reveal_epoch,
                status: WindowStatus::Open,
                score_config: config,
                sealed_eval_salt_sha256,
                commitments: Vec::new(),
                refusals: Vec::new(),
                dataset_hash: None,
                scores: Vec::new(),
            },
        );
        self.state.window_order.push(id.to_string());
        self.save()
    }

    /// Rebuild an attest [`Registry`] holding this window's commitments, each
    /// registered with `unlock_epoch`, and positioned at the arena's current
    /// epoch. The registry has one unlock epoch per registration serving both as
    /// the commit cutoff and the reveal lock; the arena has two distinct epochs,
    /// so it wraps the registry twice: register-side with the commit deadline,
    /// reveal-side with the data-reveal epoch.
    fn registry_for(&self, w: &WindowState, unlock_epoch: u64) -> Result<Registry, String> {
        let mut reg = Registry::new();
        // Epoch 0 is always below a valid unlock epoch (open_window enforces
        // commit_deadline > current >= 0), so re-registering history succeeds.
        reg.set_epoch(0);
        for c in &w.commitments {
            reg.register(c.clone(), unlock_epoch)
                .map_err(|e| format!("internal: re-registering stored commitment: {e}"))?;
        }
        reg.set_epoch(self.state.current_epoch);
        Ok(reg)
    }

    /// Register a commitment for a window. Refused at or after the commit
    /// deadline epoch, and refused for a duplicate (agent, window) pair - both
    /// checks are the attest registry's own semantics, wrapped, not re-derived.
    pub fn register_entry(
        &mut self,
        window_id: &str,
        commitment: Commitment,
    ) -> Result<(), String> {
        let current = self.state.current_epoch;
        let w = self
            .windows
            .get(window_id)
            .ok_or_else(|| format!("no such window `{window_id}`"))?;
        if w.status != WindowStatus::Open {
            return Err(format!(
                "window `{window_id}` is not open (status: {:?})",
                w.status
            ));
        }
        if commitment.target_window != window_id {
            return Err(format!(
                "commitment targets window `{}`, not `{window_id}`",
                commitment.target_window
            ));
        }
        let mut reg = self.registry_for(w, w.commit_deadline)?;
        reg.set_epoch(current);
        reg.register(commitment.clone(), w.commit_deadline)?;
        // The registry accepted it; persist.
        let w = self.windows.get_mut(window_id).expect("checked above");
        w.commitments.push(commitment);
        self.save()
    }

    /// Reveal and score a window. Each entry's pre-image is verified against its
    /// registered commitment through the attest registry's `reveal` (which also
    /// enforces the data-reveal lock). An entry whose commitment does not
    /// verify, or that never committed, is refused and recorded; the rest of the
    /// field is scored with `sharpebench_core::rank` under the ScoreConfig
    /// recorded at open time. The dataset bytes are hashed into the window
    /// record so the published header binds the exact revealed data.
    pub fn reveal_and_score(
        &mut self,
        window_id: &str,
        dataset_path: &Path,
        entries: &[RevealedEntry],
    ) -> Result<Vec<CompositeScore>, String> {
        let w = self
            .windows
            .get(window_id)
            .ok_or_else(|| format!("no such window `{window_id}`"))?;
        if w.status != WindowStatus::Committed {
            return Err(format!(
                "window `{window_id}` is not ready to score (status: {:?}; advance past the commit deadline first)",
                w.status
            ));
        }
        if self.state.current_epoch < w.data_reveal_epoch {
            return Err(format!(
                "window `{window_id}` data is still locked (reveal epoch {}, current {})",
                w.data_reveal_epoch, self.state.current_epoch
            ));
        }
        let dataset_bytes = std::fs::read(dataset_path)
            .map_err(|e| format!("cannot read dataset {}: {e}", dataset_path.display()))?;
        let dataset_hash = content_digest(&dataset_bytes);

        let mut reg = self.registry_for(w, w.data_reveal_epoch)?;
        let mut refusals = Vec::new();
        let mut field = Vec::new();
        for e in entries {
            let agent_id = e.submission.agent_id.clone();
            match reg.reveal(&agent_id, window_id, &e.artifact_digest, &e.salt) {
                Ok(()) => field.push(e.submission.clone()),
                Err(reason) => refusals.push(Refusal { agent_id, reason }),
            }
        }
        let scores = rank(&field, &w.score_config);

        let w = self.windows.get_mut(window_id).expect("checked above");
        w.dataset_hash = Some(dataset_hash);
        w.refusals.extend(refusals);
        w.scores = scores.clone();
        w.status = WindowStatus::Scoring;
        self.save()?;
        Ok(scores)
    }

    /// Final signature of the most recently published board, or the genesis
    /// anchor. Read back from the published document itself, not from memory,
    /// so the recorded anchor is always what a verifier will recompute against.
    fn prev_final_signature(&self) -> Result<String, String> {
        let Some(last) = self.state.published_order.last() else {
            return Ok(GENESIS_ANCHOR.to_string());
        };
        let board: PublicChain = read_json(&self.window_dir(last).join(BOARD_FILE))?;
        board
            .chain
            .last()
            .map(|link| link.signature.clone())
            .ok_or_else(|| format!("published board for `{last}` has an empty chain"))
    }

    /// Publish a scored window: sign a header + one link per scored entry into
    /// an Ed25519 [`PublicChain`] (`board.json`) plus a human-readable
    /// `board.md`, both inside the window directory. The header carries the
    /// previous published board's final signature, chaining the arena's whole
    /// history (see the module docs).
    pub fn publish(&mut self, window_id: &str, key: &SigningKey) -> Result<PathBuf, String> {
        let prev_final_signature = self.prev_final_signature()?;
        let w = self
            .windows
            .get(window_id)
            .ok_or_else(|| format!("no such window `{window_id}`"))?;
        if w.status != WindowStatus::Scoring {
            return Err(format!(
                "window `{window_id}` is not scored yet (status: {:?})",
                w.status
            ));
        }
        let header = WindowHeader {
            kind: WINDOW_HEADER_KIND.to_string(),
            window_id: w.id.clone(),
            commit_deadline: w.commit_deadline,
            data_reveal_epoch: w.data_reveal_epoch,
            dataset_hash: w.dataset_hash.clone().unwrap_or_default(),
            score_config: w.score_config.clone(),
            sealed_eval_salt_sha256: w.sealed_eval_salt_sha256.clone(),
            refusals: w.refusals.clone(),
            prev_final_signature,
        };
        let mut payloads =
            vec![serde_json::to_string(&header).map_err(|e| format!("serialize header: {e}"))?];
        for s in &w.scores {
            payloads.push(serde_json::to_string(s).map_err(|e| format!("serialize score: {e}"))?);
        }
        let board = publish_public_chain(&payloads, key);

        let dir = self.window_dir(window_id);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let board_path = dir.join(BOARD_FILE);
        write_json(&board_path, &board)?;
        std::fs::write(dir.join(BOARD_MD_FILE), render_markdown(&header, &w.scores))
            .map_err(|e| format!("cannot write board.md: {e}"))?;

        let w = self.windows.get_mut(window_id).expect("checked above");
        w.status = WindowStatus::Published;
        self.state.published_order.push(window_id.to_string());
        self.save()?;
        Ok(board_path)
    }
}

/// The human-readable board written beside `board.json`. Cosmetic; the signed
/// JSON is the document of record.
fn render_markdown(header: &WindowHeader, scores: &[CompositeScore]) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Arena window `{}`\n\n", header.window_id));
    out.push_str(&format!(
        "- commit deadline: epoch {}\n- data reveal: epoch {}\n- dataset SHA-256: `{}`\n- previous board signature: `{}`\n\n",
        header.commit_deadline, header.data_reveal_epoch, header.dataset_hash, header.prev_final_signature
    ));
    out.push_str("```\n");
    out.push_str(&sharpebench_leaderboard::render(scores));
    out.push_str("```\n");
    if !header.refusals.is_empty() {
        out.push_str("\n## Refused entries\n\n");
        for r in &header.refusals {
            out.push_str(&format!("- `{}`: {}\n", r.agent_id, r.reason));
        }
    }
    out.push_str("\nVerify with `sharpebench arena verify <dir> --pubkey <hex>`.\n");
    out
}

/// Verify every published board in an arena directory, and the cross-window
/// chain, from the documents alone. `pinned` supplies a verifying key the
/// caller trusts out of band; without it each board is checked under its own
/// embedded key (consistency, not identity - see `sharpebench-attest`), and all
/// boards are still required to share one key.
pub fn verify_arena(
    dir: &Path,
    pinned: Option<&VerifyingKey>,
) -> Result<ArenaVerification, String> {
    let state: StateFile = read_json(&dir.join(STATE_FILE))?;
    let mut windows = Vec::new();
    let mut expected_anchor = GENESIS_ANCHOR.to_string();
    let mut arena_key: Option<String> = pinned.map(VerifyingKey::to_hex);
    let mut all_ok = true;

    for id in &state.published_order {
        let board_path = dir.join(WINDOWS_DIR).join(id).join(BOARD_FILE);
        let board: PublicChain = read_json(&board_path)?;

        let chain_ok = match pinned {
            Some(vk) => verify_public_chain_with(&board, vk),
            None => verify_public_chain(&board),
        };
        let key_ok = match &arena_key {
            Some(k) => *k == board.verifying_key,
            None => {
                arena_key = Some(board.verifying_key.clone());
                true
            }
        };
        let (anchor_ok, detail) = match board
            .chain
            .first()
            .ok_or(())
            .and_then(|first| serde_json::from_str::<WindowHeader>(&first.payload).map_err(|_| ()))
        {
            Ok(header) if header.kind != WINDOW_HEADER_KIND => {
                (false, format!("unexpected header kind `{}`", header.kind))
            }
            Ok(header) if header.window_id != *id => (
                false,
                format!("header names window `{}`", header.window_id),
            ),
            Ok(header) if header.prev_final_signature != expected_anchor => (
                false,
                "cross-window anchor mismatch: this board was not signed over the previous board's final signature".to_string(),
            ),
            Ok(_) => (true, String::new()),
            Err(()) => (false, "first link is not a window header".to_string()),
        };
        // The next window must anchor to this board's actual final signature.
        if let Some(last) = board.chain.last() {
            expected_anchor = last.signature.clone();
        }
        let ok = chain_ok && anchor_ok && key_ok;
        all_ok &= ok;
        windows.push(WindowVerification {
            window_id: id.clone(),
            chain_ok,
            anchor_ok,
            key_ok,
            detail: if ok && detail.is_empty() {
                "ok".to_string()
            } else if !chain_ok {
                "Ed25519 chain invalid (tampered, or key mismatch)".to_string()
            } else {
                detail
            },
        });
    }
    Ok(ArenaVerification {
        ok: all_ok,
        verifying_key: arena_key,
        windows,
    })
}
