//! Append-only money journal for the host-observed model gateway.
//!
//! Every spending decision the gateway makes is a record appended to this
//! journal, and every derived number (spend, outstanding reservations, calls
//! started) is a fold over those records. Nothing is ever rewritten in place,
//! so recovery cannot selectively erase a spent attempt: the only way to make a
//! reservation disappear is to append a settlement that says what happened to
//! it, and a reservation with no settlement folds as consumed at its reserved
//! amount rather than as a free retry.
//!
//! Amounts are integer USD nanodollars carried as decimal strings, matching the
//! rate-card accounting in [`crate::accounting`]. They are never repriced: a
//! resumed journal folds the amounts that were written, not a fresh quote
//! against whatever rate card the process happens to hold now.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::accounting::{MonetarySummary, RateCard};

pub const JOURNAL_SCHEMA_VERSION: &str = "sharpebench.gateway-journal.v1";

/// Schema of the document a [`JournalLock`] writes. A lock is not a journal:
/// it lives at a different path, carries a different schema version and is
/// refused by [`GatewayJournal::load_bound`] like any other foreign document.
pub const JOURNAL_LOCK_SCHEMA_VERSION: &str = "sharpebench.gateway-journal-lock.v1";

/// The largest journal accepted from disk. A journal is one record per call
/// under a bounded call ceiling, so this is generous; it exists so a corrupt or
/// hostile file cannot be read into memory unbounded.
pub const MAX_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;

/// Why a reservation was released without spending it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseReason {
    /// The host refused to dispatch after reserving (a bound or a shutdown), so
    /// no request ever reached a provider.
    NeverDispatched,
    /// The provider refused the request before doing any work it could bill:
    /// the connection was refused, or the endpoint answered a rate-limit
    /// rejection with no completion started.
    ProviderRefusedBeforeWork,
}

/// Why a call's cost cannot be known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownCostReason {
    /// The provider answered, but reported no usage. The completion happened;
    /// what it cost did not come back.
    UsageAbsent,
    /// The exchange failed after the request was committed: a read timeout, a
    /// dropped connection, an unparseable or oversized body. The provider may
    /// have completed and billed the call.
    AmbiguousAfterCommit,
    /// The transport returned only after the wall-clock deadline the host handed
    /// it had already passed. The answer is refused, but the request was on the
    /// wire, so the provider may have completed and billed it.
    AdapterDeadlineExceeded,
    /// A reservation with no settlement was found on resume. The process died
    /// somewhere around the call.
    InterruptedBeforeSettlement,
}

/// One settlement of one reservation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "settlement", rename_all = "snake_case")]
pub enum Settlement {
    /// The provider reported usage and the host priced it once, here.
    Priced {
        input_tokens: u64,
        output_tokens: u64,
        usd_nanos: String,
    },
    /// The call is charged at its full reservation because its true cost is not
    /// knowable. Never a refund and never a zero.
    Unknown { reason: UnknownCostReason },
    /// Nothing was spent, and the journal says why.
    Released { reason: ReleaseReason },
}

/// One append-only journal record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
pub enum JournalRecord {
    /// Written, and made durable, before the call it pays for is dispatched.
    Reserved {
        ordinal: u32,
        alias: String,
        provider: String,
        model: String,
        revision: String,
        rate_card_sha256: String,
        reserved_usd_nanos: String,
    },
    Settled {
        ordinal: u32,
        #[serde(flatten)]
        settlement: Settlement,
    },
}

/// The host-owned spending ceiling for one sweep. Both limits are hard: a call
/// that would cross either one is refused before anything is dispatched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayBudget {
    /// Total money the sweep may commit, in integer USD nanodollars.
    pub max_usd_nanos: u128,
    /// Total provider calls the sweep may start, retries included.
    pub max_calls: u32,
}

/// What the journal is bound to. A journal is resumable only against an
/// identical binding, so a changed model, revision, rate card or budget cannot
/// silently continue an existing spend record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalIdentity {
    pub schema_version: String,
    /// Digest of the frozen route table: aliases, models, revisions, rate-card
    /// digests and destinations. Never credentials.
    pub route_table_sha256: String,
    pub budget: GatewayBudget,
    /// The sweep this journal pays for, when a sweep owns it: a digest over the
    /// entrant id and the checkpoint contract. Absent for a journal no sweep
    /// is bound to, which keeps such a document byte for byte what it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_sha256: Option<String>,
}

impl JournalIdentity {
    pub fn new(route_table_sha256: String, budget: GatewayBudget) -> Self {
        Self {
            schema_version: JOURNAL_SCHEMA_VERSION.to_string(),
            route_table_sha256,
            budget,
            sweep_sha256: None,
        }
    }

    /// Bind this identity to one sweep. A journal written under it then
    /// refuses to resume under any other sweep, even one with the same routes
    /// and budget, so one sweep's spend cannot be reported as another's.
    pub fn for_sweep(mut self, sweep_sha256: String) -> Self {
        self.sweep_sha256 = Some(sweep_sha256);
        self
    }
}

/// Derived spend state. Every field is a fold over the records; none of it is
/// stored, so it cannot drift from the append-only truth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpendState {
    /// Priced settlements: the provider reported usage and it was quoted once.
    pub priced_usd_nanos: u128,
    /// Reservations consumed at face value because their real cost is unknown.
    pub unknown_usd_nanos: u128,
    /// Reservations that are neither settled nor released: calls in flight, or
    /// interrupted ones a fold has not yet charged.
    pub outstanding_usd_nanos: u128,
    /// Calls started, retries included. Released reservations still count: the
    /// call ceiling bounds attempts, not successes.
    pub calls_started: u32,
    /// Settlements whose amount is a reservation rather than a measurement.
    pub unknown_calls: u32,
    pub priced_calls: u32,
    pub released_calls: u32,
    /// Money priced above what its call reserved, summed over every such call.
    /// A reservation bounds what the host authorized, not what the provider
    /// billed, so this is the measured gap between the two and it is never
    /// folded away into the priced total.
    pub overspent_usd_nanos: u128,
    /// Calls whose observed price exceeded their reservation.
    pub overspent_calls: u32,
}

impl SpendState {
    /// Everything the budget considers committed.
    pub fn committed_usd_nanos(&self) -> u128 {
        self.priced_usd_nanos
            .saturating_add(self.unknown_usd_nanos)
            .saturating_add(self.outstanding_usd_nanos)
    }

    /// Whether any amount in this state is a reservation standing in for an
    /// unmeasured cost. A total containing one is a partial total.
    pub fn is_partial(&self) -> bool {
        self.unknown_calls > 0 || self.outstanding_usd_nanos > 0
    }
}

/// Why a journal could not be persisted. A conflict is a distinct outcome from
/// an I/O failure: the write was refused because someone else owns the record,
/// not because the disk would not take it.
#[derive(Debug)]
pub enum JournalSaveError {
    /// The document on disk is not the one this snapshot was derived from, so
    /// writing would replace a record this process never read. The in-memory
    /// journal is left intact: the caller still holds every record it appended.
    Conflict {
        expected: u64,
        found: u64,
    },
    /// The record reached the journal path and its durability is unconfirmed:
    /// the rename landed, and the parent directory sync that makes the new
    /// directory entry survive a power loss did not. The disk is a version
    /// ahead of where it was, so this snapshot keeps the version it wrote and
    /// the next save compares against the file rather than being refused as
    /// another writer's. Distinct from [`JournalSaveError::Io`] because the
    /// two need opposite bookkeeping and publish different diagnoses: this one
    /// is an I/O fault by the sole owner, not lost ownership.
    Unsynced(std::io::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for JournalSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict { expected, found } => write!(
                f,
                "gateway journal on disk is at version {found}, this snapshot is at version {expected}"
            ),
            Self::Unsynced(error) => write!(
                f,
                "gateway journal was written and its durability is unconfirmed: {error}"
            ),
            Self::Io(error) => write!(f, "gateway journal could not be written: {error}"),
        }
    }
}

impl std::error::Error for JournalSaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Conflict { .. } => None,
            Self::Unsynced(error) | Self::Io(error) => Some(error),
        }
    }
}

/// The largest lock document read back to name a holder. The document this
/// code writes is a few dozen bytes; the bound exists so a hostile file at the
/// lock path cannot be read into memory unbounded.
const MAX_LOCK_BYTES: u64 = 4096;

/// What a [`JournalLock`] writes about its holder. Enough for an operator to
/// find the process, and nothing else: no journal path, no destination, no
/// credential, nothing that could carry provider material into a file the
/// operator is meant to read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalLockDocument {
    pub schema_version: String,
    /// The process that took the lock, on the host that took it.
    pub pid: u32,
    /// Which lock *value* wrote this file, as distinct from which process. A
    /// process may hold, lose and retake a path, and a displaced holder is
    /// still a live value naming it, so removing a lock by path alone removes
    /// whatever happens to be there. A holder removes only a file still
    /// carrying its own instance. Absent in a document written before the
    /// field existed, which matches no live holder and is therefore never
    /// removed by one.
    #[serde(default)]
    pub instance: String,
    pub acquired_unix_ms: u128,
    /// Present only when this lock displaced another through
    /// [`JournalLock::take_over`]: the operator's stated reason, and the pid
    /// the takeover displaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded: Option<SupersededHolder>,
}

/// The holder a takeover displaced, recorded in the lock that displaced it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupersededHolder {
    pub pid: u32,
    pub acquired_unix_ms: u128,
    /// Why an operator decided the earlier holder was gone. Free text, written
    /// by the operator, kept so the decision is not invisible afterwards.
    pub reason: String,
}

/// Why exclusive ownership of a journal path could not be taken.
#[derive(Debug)]
pub enum JournalLockError {
    /// Someone else holds this journal, or a process that crashed while
    /// holding it left its lock behind. Never broken automatically: breaking a
    /// lock nobody can prove is dead puts two writers back on one budget,
    /// which is the whole defect the lock exists to prevent.
    Held {
        lock_path: PathBuf,
        holder: String,
    },
    /// A takeover named a lock that is not there.
    NotHeld {
        lock_path: PathBuf,
    },
    Io(std::io::Error),
}

impl std::fmt::Display for JournalLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Held { lock_path, holder } => write!(
                f,
                "the gateway journal is locked by {holder}; its lock file is {}. \
                 Check whether that process is still running before doing anything else: \
                 if it is, this gateway must not spend the same budget. \
                 If it is gone, the lock is stale and an operator must take it over \
                 deliberately, which records who was displaced and why.",
                lock_path.display()
            ),
            Self::NotHeld { lock_path } => write!(
                f,
                "no gateway journal lock is held at {}, so there is nothing to take over",
                lock_path.display()
            ),
            Self::Io(error) => write!(f, "gateway journal lock could not be taken: {error}"),
        }
    }
}

impl std::error::Error for JournalLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Held { .. } | Self::NotHeld { .. } => None,
            Self::Io(error) => Some(error),
        }
    }
}

impl From<JournalLockError> for std::io::Error {
    fn from(error: JournalLockError) -> Self {
        match error {
            held @ JournalLockError::Held { .. } => {
                Self::new(std::io::ErrorKind::AlreadyExists, held)
            }
            absent @ JournalLockError::NotHeld { .. } => {
                Self::new(std::io::ErrorKind::NotFound, absent)
            }
            JournalLockError::Io(error) => error,
        }
    }
}

/// Exclusive ownership of one journal path, held for as long as the gateway
/// that spends it lives.
///
/// The lock is a sibling file named after the journal with `.lock` appended,
/// created with `create_new`, which is `O_EXCL | O_CREAT` on Unix and
/// `CREATE_NEW` on Windows: the file system decides the winner, and a second
/// attempt is refused as [`JournalLockError::Held`] rather than queued. There
/// is no wait and no timeout, because two gateways over one budget is not a
/// situation that improves by waiting.
///
/// # What it does not cover
///
/// One host. Two hosts reaching the same path over a network file system are
/// not separated by this: `create_new` is only as exclusive as the remote
/// server's create semantics, and NFS in particular does not guarantee them.
///
/// A crash leaves the lock behind, and that is deliberate. Nothing on disk can
/// distinguish a dead holder from a live one, so the stale lock is refused with
/// a message naming the file, and clearing it is an operator decision taken
/// through [`JournalLock::take_over`], which records who was displaced and why.
///
/// # One journal reached under two names
///
/// A lock named after the journal's spelling is one lock per name, not one per
/// journal, so two directory entries for one journal would otherwise admit two
/// gateways. A second lock therefore keys on the journal document's own
/// [`GatewayJournal::journal_id`], which every name for that document reports
/// alike and which survives the rename a save persists through:
/// `sb-gateway-journal-<id>.lock`, beside the journal. Both are held, and both
/// have to be free.
///
/// What that leaves open, precisely: the identity lock is a sibling of the
/// journal, so it separates aliases that share a directory and not aliases in
/// different directories; and a journal document written before the id field
/// existed names no id, so a gateway assigns one and saves before it binds,
/// which two gateways opening such a document under two names would each do
/// separately.
///
/// # Detecting displacement
///
/// A holder can tell whether it still holds its lock, through
/// [`JournalLock::is_still_held`], because the file carries the instance that
/// wrote it. Nothing polls it: the check is a question a caller may ask, not a
/// watchdog. What stops a displaced holder from spending is the journal's
/// compare-and-swap, which refuses the second of the two writers to save.
#[derive(Debug)]
pub struct JournalLock {
    path: PathBuf,
    /// The journal-identity lock, held alongside `path` once the document names
    /// an id. `None` for a journal that does not exist yet, or one written
    /// before the field existed, until [`JournalLock::bind_journal_id`].
    identity_path: Option<PathBuf>,
    /// What this value wrote into both files, so its drop can tell its own lock
    /// from whatever else may be at those paths.
    instance: String,
}

/// A name no other live lock or journal on this host reuses: the process, a
/// counter within it and the wall clock, hashed to a fixed-width filename
/// component. An identifier, never a secret: nothing authorises on it.
fn fresh_token() -> String {
    use sha2::{Digest as _, Sha256};
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut hash = Sha256::new();
    hash.update(std::process::id().to_le_bytes());
    hash.update(
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_le_bytes(),
    );
    hash.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0)
            .to_le_bytes(),
    );
    format!("{:x}", hash.finalize())[..32].to_string()
}

/// Where a journal's siblings live: its own parent, or the working directory
/// for a bare filename.
fn journal_parent(journal: &Path) -> &Path {
    journal
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

impl JournalLock {
    /// Where the lock for `journal` lives: the journal's own name with `.lock`
    /// appended, so it sorts beside the journal and can never be opened as one.
    pub fn lock_path(journal: &Path) -> Result<PathBuf, JournalLockError> {
        let Some(name) = journal.file_name() else {
            return Err(JournalLockError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "gateway journal path needs a filename",
            )));
        };
        let mut lock_name = name.to_os_string();
        lock_name.push(".lock");
        Ok(journal_parent(journal).join(lock_name))
    }

    /// Where the lock for the journal *document* `journal_id` lives: beside the
    /// journal, named after the document rather than after the directory entry
    /// it was reached through, so every name for one document names one lock.
    pub fn identity_lock_path(journal: &Path, journal_id: &str) -> PathBuf {
        journal_parent(journal).join(format!("sb-gateway-journal-{journal_id}.lock"))
    }

    /// Take exclusive ownership of `journal`, or refuse.
    ///
    /// Both the spelling and, when the document on disk names one, the
    /// document's identity have to be free. A journal that does not exist yet
    /// names no document; its opener binds the identity once it has written one
    /// through [`JournalLock::bind_journal_id`].
    pub fn acquire(journal: &Path) -> Result<Self, JournalLockError> {
        let mut lock = Self::create(Self::lock_path(journal)?, None, fresh_token())?;
        if let Some(id) = journal_id_on_disk(journal) {
            // A refusal here drops `lock`, which releases the spelling lock it
            // just took: a gateway that cannot own the document owns nothing.
            lock.bind_journal_id(journal, &id)?;
        }
        Ok(lock)
    }

    /// Extend this lock over the journal document `journal_id`, once such a
    /// document exists at `journal`. Refused, leaving this lock as it was, when
    /// another holder already owns that document under another name.
    pub fn bind_journal_id(
        &mut self,
        journal: &Path,
        journal_id: &str,
    ) -> Result<(), JournalLockError> {
        let identity_path = Self::identity_lock_path(journal, journal_id);
        write_lock_file(&identity_path, None, &self.instance)?;
        self.identity_path = Some(identity_path);
        Ok(())
    }

    /// The identity of the journal document at `journal`, as the lock reads it.
    /// `None` when there is no journal there yet, or when its document names no
    /// usable identity.
    pub fn document_id(journal: &Path) -> Option<String> {
        journal_id_on_disk(journal)
    }

    /// Whether this lock covers the journal *document* as well as the path
    /// spelling it was taken on. False until a document exists to name.
    pub fn covers_document(&self) -> bool {
        self.identity_path.is_some()
    }

    /// Whether the file this lock created is still the file at its path. A
    /// takeover replaces it, so a displaced holder can find out that it was
    /// displaced rather than discovering it by writing.
    pub fn is_still_held(&self) -> bool {
        instance_at(&self.path).as_deref() == Some(self.instance.as_str())
    }

    /// Displace the holder of `journal`'s lock, recording `reason` and the pid
    /// displaced in the lock that replaces it.
    ///
    /// This is the deliberate act an operator takes after establishing that the
    /// earlier holder is gone. It is never taken automatically, and it is not a
    /// way around a live gateway: a takeover of a lock a running process holds
    /// puts two writers back on one budget. What it no longer does is leave the
    /// path unlocked afterwards: the displaced holder's drop removes only a
    /// file still carrying its own instance, and the lock it is looking at is
    /// the taker's.
    ///
    /// The document's identity lock is displaced with it, so the taker holds
    /// what the incumbent held rather than half of it.
    pub fn take_over(journal: &Path, reason: &str) -> Result<Self, JournalLockError> {
        let lock_path = Self::lock_path(journal)?;
        let bytes = match read_bounded(&lock_path, MAX_LOCK_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(JournalLockError::NotHeld { lock_path });
            }
            Err(error) => return Err(JournalLockError::Io(error)),
        };
        let displaced = serde_json::from_slice::<JournalLockDocument>(&bytes)
            .ok()
            .map(|document| SupersededHolder {
                pid: document.pid,
                acquired_unix_ms: document.acquired_unix_ms,
                reason: reason.to_string(),
            })
            .unwrap_or(SupersededHolder {
                pid: 0,
                acquired_unix_ms: 0,
                reason: reason.to_string(),
            });
        std::fs::remove_file(&lock_path).map_err(JournalLockError::Io)?;
        let mut taken = Self::create(lock_path, Some(displaced), fresh_token())?;
        if let Some(id) = journal_id_on_disk(journal) {
            let identity_path = Self::identity_lock_path(journal, &id);
            match std::fs::remove_file(&identity_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(JournalLockError::Io(error)),
            }
            taken.bind_journal_id(journal, &id)?;
        }
        Ok(taken)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn create(
        lock_path: PathBuf,
        superseded: Option<SupersededHolder>,
        instance: String,
    ) -> Result<Self, JournalLockError> {
        write_lock_file(&lock_path, superseded, &instance)?;
        Ok(Self {
            path: lock_path,
            identity_path: None,
            instance,
        })
    }
}

fn write_lock_file(
    lock_path: &Path,
    superseded: Option<SupersededHolder>,
    instance: &str,
) -> Result<(), JournalLockError> {
    use std::io::Write as _;
    let document = JournalLockDocument {
        schema_version: JOURNAL_LOCK_SCHEMA_VERSION.to_string(),
        pid: std::process::id(),
        instance: instance.to_string(),
        acquired_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis())
            .unwrap_or(0),
        superseded,
    };
    let payload = serde_json::to_vec(&document)
        .map_err(|error| JournalLockError::Io(std::io::Error::other(error)))?;
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let holder = describe_holder(lock_path);
            return Err(JournalLockError::Held {
                lock_path: lock_path.to_path_buf(),
                holder,
            });
        }
        Err(error) => return Err(JournalLockError::Io(error)),
    };
    // A lock whose document never landed would name no holder, so a failure
    // here releases rather than leaving an anonymous file behind.
    if let Err(error) = file.write_all(&payload).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(lock_path);
        return Err(JournalLockError::Io(error));
    }
    Ok(())
}

/// Which lock value wrote the file at `lock_path`, if it is a lock document at
/// all. `None` covers every way of not being this holder's lock: absent,
/// unreadable, foreign, or written before the instance field existed.
fn instance_at(lock_path: &Path) -> Option<String> {
    let bytes = read_bounded(lock_path, MAX_LOCK_BYTES).ok()?;
    let document = serde_json::from_slice::<JournalLockDocument>(&bytes).ok()?;
    (!document.instance.is_empty()).then_some(document.instance)
}

impl Drop for JournalLock {
    /// Release only what this value still holds. The file at a lock's path can
    /// be a *different* lock by the time the holder drops, because
    /// [`JournalLock::take_over`] replaces it while the displaced holder is
    /// still alive; removing it by path would unlock the journal under the
    /// taker while the taker is spending.
    ///
    /// The read and the removal are not one operation, so a takeover landing
    /// between them can still lose the taker's lock. That window is a few
    /// microseconds against an operator action taken by hand, where the
    /// previous behaviour was certain rather than unlikely.
    fn drop(&mut self) {
        for path in std::iter::once(&self.path).chain(self.identity_path.as_ref()) {
            if instance_at(path).as_deref() == Some(self.instance.as_str()) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// The document identity a journal file names, if it names a usable one. The
/// value becomes a filename component, so anything that is not a plain
/// hexadecimal token is treated as no identity at all rather than joined onto a
/// path.
fn journal_id_on_disk(journal: &Path) -> Option<String> {
    let bytes = read_bounded(journal, MAX_JOURNAL_BYTES).ok()?;
    let document: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let id = document.get("journal_id")?.as_str()?;
    let usable = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit());
    usable.then(|| id.to_string())
}

/// A short operator-facing description of whoever holds a lock. Never fails:
/// an unreadable or foreign document still has to produce a refusal message.
fn describe_holder(lock_path: &Path) -> String {
    let Ok(bytes) = read_bounded(lock_path, MAX_LOCK_BYTES) else {
        return "a holder whose lock document could not be read".to_string();
    };
    match serde_json::from_slice::<JournalLockDocument>(&bytes) {
        Ok(document) => format!(
            "pid {} since unix ms {}",
            document.pid, document.acquired_unix_ms
        ),
        Err(_) => "a holder whose lock document is not a gateway lock".to_string(),
    }
}

fn read_bounded(path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max_bytes)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// An append-only sequence of spending records, persisted as one JSON document.
///
/// # Ownership
///
/// A journal that is spent from is owned exclusively. A gateway takes a
/// [`JournalLock`] when it binds a path and holds it until it drops, so a
/// second writer on the same host is refused at open with a typed
/// [`JournalLockError::Held`] rather than admitted to the same budget. The lock
/// is what makes concurrent writers impossible; nothing below it is a
/// substitute for it.
///
/// A snapshot also carries the `version` it was loaded at, and
/// [`GatewayJournal::save`] is a compare-and-swap against that version: a save
/// from a snapshot that does not match what is on disk is refused as
/// [`JournalSaveError::Conflict`] rather than replacing a record this process
/// never read. That check is now a second line of defence rather than the only
/// one: it catches a journal that moved under a single writer, such as a file
/// restored from a backup or edited by hand mid-sweep.
///
/// # What ownership does not cover
///
/// Two hosts sharing one journal path over a network file system. The lock's
/// exclusivity is the local file system's `create_new`, and a remote server
/// need not honour it; NFS in particular does not. One host per journal path
/// is a deployment rule, not something this code can enforce.
///
/// A holder that crashed. Its lock stays, and the next gateway is refused
/// rather than admitted, because no file on disk distinguishes a dead holder
/// from a live one. Clearing it is [`JournalLock::take_over`], which an
/// operator performs deliberately and which records who was displaced and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayJournal {
    pub identity: JournalIdentity,
    /// Bumped by every successful save. Absent in a document written before the
    /// field existed, which folds to the pre-first-save value.
    #[serde(default)]
    version: u64,
    /// What this document is, as against where it is. Assigned once, carried
    /// through every save, and reported alike by every directory entry that
    /// reaches it, so ownership can be taken on the journal rather than on the
    /// name a gateway happened to be given. Absent in a document written before
    /// the field existed; its next opener assigns one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    journal_id: Option<String>,
    records: Vec<JournalRecord>,
}

impl GatewayJournal {
    pub fn new(identity: JournalIdentity) -> Self {
        Self {
            identity,
            version: 0,
            journal_id: Some(fresh_token()),
            records: Vec::new(),
        }
    }

    /// The identity of this document, or `None` for one loaded from a file
    /// written before the field existed.
    pub fn journal_id(&self) -> Option<&str> {
        self.journal_id.as_deref()
    }

    /// The identity of this document, assigning one if it has none. The
    /// assignment is in memory: it is on disk, and therefore lockable, only
    /// after the next save.
    pub fn ensure_journal_id(&mut self) -> String {
        self.journal_id.get_or_insert_with(fresh_token).clone()
    }

    pub fn records(&self) -> &[JournalRecord] {
        &self.records
    }

    /// The version this snapshot will compare against on its next save.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Load a journal and refuse one that is not bound to this experiment. A
    /// journal whose binding differs is never truncated, merged or reused.
    pub fn load_bound(path: &Path, identity: &JournalIdentity) -> std::io::Result<Self> {
        Self::load_checked(path, |found| found == identity)
    }

    /// Load a journal bound to these routes and this budget, whichever sweep
    /// it belongs to. For inspection only: a gateway that spends must use
    /// [`GatewayJournal::load_bound`] with its full identity.
    pub fn load_for_routes(
        path: &Path,
        route_table_sha256: &str,
        budget: GatewayBudget,
    ) -> std::io::Result<Self> {
        Self::load_checked(path, |found| {
            found.schema_version == JOURNAL_SCHEMA_VERSION
                && found.route_table_sha256 == route_table_sha256
                && found.budget == budget
        })
    }

    fn load_checked(
        path: &Path,
        accepts: impl Fn(&JournalIdentity) -> bool,
    ) -> std::io::Result<Self> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gateway journal exceeds the accepted size",
            ));
        }
        let journal: Self = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        if !accepts(&journal.identity) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gateway journal is bound to a different route table, budget or sweep",
            ));
        }
        journal.validate()?;
        Ok(journal)
    }

    /// Structural invariants an append-only journal must satisfy: ordinals are
    /// dense and increasing, no reservation is settled twice, and no settlement
    /// names a reservation that was never written.
    fn validate(&self) -> std::io::Result<()> {
        let invalid = |detail: &str| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("gateway journal is malformed: {detail}"),
            )
        };
        let mut reserved: Vec<u32> = Vec::new();
        let mut settled: Vec<u32> = Vec::new();
        for record in &self.records {
            match record {
                JournalRecord::Reserved {
                    ordinal,
                    reserved_usd_nanos,
                    ..
                } => {
                    if *ordinal as usize != reserved.len() {
                        return Err(invalid("reservation ordinals are not dense"));
                    }
                    if reserved_usd_nanos.parse::<u128>().is_err() {
                        return Err(invalid("a reserved amount is not an integer"));
                    }
                    reserved.push(*ordinal);
                }
                JournalRecord::Settled {
                    ordinal,
                    settlement,
                } => {
                    if !reserved.contains(ordinal) {
                        return Err(invalid("a settlement names no reservation"));
                    }
                    if settled.contains(ordinal) {
                        return Err(invalid("a reservation is settled twice"));
                    }
                    if let Settlement::Priced { usd_nanos, .. } = settlement {
                        if usd_nanos.parse::<u128>().is_err() {
                            return Err(invalid("a priced amount is not an integer"));
                        }
                    }
                    settled.push(*ordinal);
                }
            }
        }
        Ok(())
    }

    /// Fold the records into spend state. Amounts are read, never recomputed:
    /// resuming a journal cannot reprice what an earlier process already paid.
    pub fn spend(&self) -> SpendState {
        let mut state = SpendState::default();
        let mut open: Vec<(u32, u128)> = Vec::new();
        for record in &self.records {
            match record {
                JournalRecord::Reserved {
                    ordinal,
                    reserved_usd_nanos,
                    ..
                } => {
                    state.calls_started = state.calls_started.saturating_add(1);
                    open.push((*ordinal, reserved_usd_nanos.parse().unwrap_or(u128::MAX)));
                }
                JournalRecord::Settled {
                    ordinal,
                    settlement,
                } => {
                    let Some(index) = open.iter().position(|(open, _)| open == ordinal) else {
                        continue;
                    };
                    let (_, reserved) = open.remove(index);
                    match settlement {
                        Settlement::Priced { usd_nanos, .. } => {
                            let priced: u128 = usd_nanos.parse().unwrap_or(reserved);
                            state.priced_calls = state.priced_calls.saturating_add(1);
                            state.priced_usd_nanos = state.priced_usd_nanos.saturating_add(priced);
                            if priced > reserved {
                                state.overspent_calls = state.overspent_calls.saturating_add(1);
                                state.overspent_usd_nanos =
                                    state.overspent_usd_nanos.saturating_add(priced - reserved);
                            }
                        }
                        Settlement::Unknown { .. } => {
                            state.unknown_calls = state.unknown_calls.saturating_add(1);
                            state.unknown_usd_nanos =
                                state.unknown_usd_nanos.saturating_add(reserved);
                        }
                        Settlement::Released { .. } => {
                            state.released_calls = state.released_calls.saturating_add(1);
                        }
                    }
                }
            }
        }
        // A reservation nobody settled is an interrupted call. It is charged at
        // its reservation, which is what makes a crash mid-call neither a free
        // retry nor an automatic refund.
        for (_, reserved) in open {
            state.outstanding_usd_nanos = state.outstanding_usd_nanos.saturating_add(reserved);
        }
        state
    }

    /// Whether committed money has already passed the budget ceiling. Only an
    /// observed price above its reservation can put a journal here, because a
    /// reservation is refused before dispatch when it would not fit. Once true
    /// the sweep must start no further call: the ceiling is the ceiling, and an
    /// overage is not authority to keep spending.
    pub fn ceiling_breached(&self) -> bool {
        self.spend().committed_usd_nanos() > self.identity.budget.max_usd_nanos
    }

    /// Money still available under the budget, after everything committed.
    pub fn available_usd_nanos(&self) -> u128 {
        self.identity
            .budget
            .max_usd_nanos
            .saturating_sub(self.spend().committed_usd_nanos())
    }

    /// Append a reservation. The caller must persist before dispatching.
    pub fn reserve(&mut self, alias: &str, card: &RateCard, reserved_usd_nanos: u128) -> u32 {
        let ordinal = u32::try_from(
            self.records
                .iter()
                .filter(|record| matches!(record, JournalRecord::Reserved { .. }))
                .count(),
        )
        .unwrap_or(u32::MAX);
        self.records.push(JournalRecord::Reserved {
            ordinal,
            alias: alias.to_string(),
            provider: card.provider().to_string(),
            model: card.model().to_string(),
            revision: card.revision().to_string(),
            rate_card_sha256: card.digest(),
            reserved_usd_nanos: reserved_usd_nanos.to_string(),
        });
        ordinal
    }

    pub fn settle(&mut self, ordinal: u32, settlement: Settlement) {
        self.records.push(JournalRecord::Settled {
            ordinal,
            settlement,
        });
    }

    /// Every observed call, priced once, as the rank-neutral summary the rest of
    /// the harness publishes. Host-observed usage is what the host saw the
    /// provider report; it is not a verified invoice.
    pub fn monetary_summary(&self, card: Option<&RateCard>) -> MonetarySummary {
        let state = self.spend();
        if state.calls_started == 0 {
            return MonetarySummary::Unavailable {
                reason: "gateway_journal_has_no_calls",
                known_subtotal_usd_nanos: None,
                rate_card: card.cloned(),
                rate_card_sha256: card.map(RateCard::digest),
            };
        }
        if state.is_partial() {
            return MonetarySummary::Unavailable {
                reason: "host_observed_usage_incomplete",
                known_subtotal_usd_nanos: Some(state.priced_usd_nanos.to_string()),
                rate_card: card.cloned(),
                rate_card_sha256: card.map(RateCard::digest),
            };
        }
        match card {
            Some(card) => MonetarySummary::Estimated {
                rate_card: card.clone(),
                rate_card_sha256: card.digest(),
                usage_source: "host_observed",
                usd_nanos: state.priced_usd_nanos.to_string(),
            },
            None => MonetarySummary::Unavailable {
                reason: "gateway_journal_spans_several_rate_cards",
                known_subtotal_usd_nanos: Some(state.priced_usd_nanos.to_string()),
                rate_card: None,
                rate_card_sha256: None,
            },
        }
    }

    /// The version of the document at `path`, or `None` when no document is
    /// there. A document written before the field existed reads as version 0.
    pub(crate) fn version_on_disk(path: &Path) -> std::io::Result<Option<u64>> {
        use std::io::Read as _;
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gateway journal exceeds the accepted size",
            ));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        Ok(Some(
            value
                .get("version")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        ))
    }

    /// Persist through a sibling temporary file and fsync it before the rename,
    /// so a reservation is durable before the call it pays for is dispatched.
    ///
    /// The write is a compare-and-swap on [`GatewayJournal::version`]: a save
    /// from a snapshot the disk has moved past is refused, and the caller keeps
    /// every record it appended. Concurrent writers are kept apart by the
    /// [`JournalLock`] a gateway holds, not by this check; see the type
    /// documentation for what each one covers.
    pub fn save(&mut self, path: &Path) -> Result<(), JournalSaveError> {
        use std::io::Write as _;
        let found = Self::version_on_disk(path)
            .map_err(JournalSaveError::Io)?
            .unwrap_or(0);
        if found != self.version {
            return Err(JournalSaveError::Conflict {
                expected: self.version,
                found,
            });
        }
        self.version += 1;
        let payload = match serde_json::to_string_pretty(self) {
            Ok(payload) => payload,
            Err(error) => {
                self.version -= 1;
                return Err(JournalSaveError::Io(std::io::Error::other(error)));
            }
        };
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let Some(name) = path.file_name() else {
            self.version -= 1;
            return Err(JournalSaveError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "gateway journal path needs a filename",
            )));
        };
        let mut temp_name = name.to_os_string();
        temp_name.push(format!(".{}.tmp", std::process::id()));
        let tmp = parent.join(temp_name);
        let mut landed = false;
        let result: std::io::Result<()> = (|| {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(payload.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, path)?;
            landed = true;
            sync_parent_directory(parent)
        })();
        match result {
            Ok(()) => Ok(()),
            // The rename already moved the file to the new version. Decrementing
            // here would leave this snapshot a version behind the file it owns,
            // and its next save would be refused as another writer's work: an
            // I/O fault by the sole owner published as lost ownership.
            Err(error) if landed => Err(JournalSaveError::Unsynced(error)),
            Err(error) => {
                let _ = std::fs::remove_file(&tmp);
                // The bump only stands for a write that landed: a failed save
                // must leave this snapshot comparing against what is on disk.
                self.version -= 1;
                Err(JournalSaveError::Io(error))
            }
        }
    }
}

/// A save that lands its rename and fails its durability step. Forcing a real
/// directory fsync to fail is a Unix-only path and not available on every host
/// the suite runs on, and what the finding is about is what the caller does
/// with a half-landed write, so the fault is injected rather than provoked.
/// Thread-local: one test's injection cannot reach another running beside it.
#[cfg(test)]
pub(crate) mod fault_injection {
    // The initializer already is a `const` block. Clippy 0.1.96 reports it as
    // one that "can be made const" all the same, pointing at the macro rather
    // than at the initializer, and the suggested edit is what is written here.
    #[allow(clippy::missing_const_for_thread_local)]
    mod flag {
        use std::cell::Cell;

        thread_local! {
            pub(super) static FAIL_PARENT_SYNC: Cell<bool> = const { Cell::new(false) };
        }
    }

    /// The next save on this thread renames and then reports its durability
    /// unconfirmed. One save, not every save.
    pub(crate) fn fail_next_parent_sync() {
        flag::FAIL_PARENT_SYNC.with(|flag| flag.set(true));
    }

    pub(crate) fn take_injected_parent_sync_failure() -> bool {
        flag::FAIL_PARENT_SYNC.with(|flag| flag.replace(false))
    }
}

/// Make the renamed directory entry durable. Only Unix needs the parent
/// directory synced for that; the step exists as its own function on every
/// platform so the partly landed save, rename done and durability unconfirmed,
/// is reachable in a test wherever the suite runs.
fn sync_parent_directory(parent: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if fault_injection::take_injected_parent_sync_failure() {
        return Err(std::io::Error::other(
            "injected parent directory sync failure",
        ));
    }
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = parent;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::gateway_journal::fault_injection::fail_next_parent_sync;
    use crate::scratch::ScratchDir;

    pub(crate) fn card(input: u64, output: u64) -> RateCard {
        let json = format!(
            r#"{{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":{input},"output_usd_nanos_per_token":{output}}}"#
        );
        RateCard::from_json(json.as_bytes()).expect("a valid test rate card")
    }

    fn identity(budget: GatewayBudget) -> JournalIdentity {
        JournalIdentity::new("a".repeat(64), budget)
    }

    fn budget(max_usd_nanos: u128, max_calls: u32) -> GatewayBudget {
        GatewayBudget {
            max_usd_nanos,
            max_calls,
        }
    }

    /// A reservation nobody settled is an interrupted call. It is charged at its
    /// reservation: not refunded, and not left available for a free retry.
    #[test]
    fn an_unsettled_reservation_is_charged_not_refunded() {
        let mut journal = GatewayJournal::new(identity(budget(1_000, 8)));
        journal.reserve("alias", &card(1, 1), 400);
        let spend = journal.spend();
        assert_eq!(spend.outstanding_usd_nanos, 400);
        assert_eq!(spend.committed_usd_nanos(), 400);
        assert_eq!(journal.available_usd_nanos(), 600);
        assert!(spend.is_partial(), "an unmeasured amount makes it partial");
    }

    /// An unknown-cost settlement keeps the whole reservation. The alternative,
    /// refunding it, would make an ambiguous provider failure free.
    #[test]
    fn an_unknown_cost_keeps_the_full_reservation() {
        let mut journal = GatewayJournal::new(identity(budget(1_000, 8)));
        let ordinal = journal.reserve("alias", &card(1, 1), 400);
        journal.settle(
            ordinal,
            Settlement::Unknown {
                reason: UnknownCostReason::UsageAbsent,
            },
        );
        let spend = journal.spend();
        assert_eq!(spend.unknown_usd_nanos, 400);
        assert_eq!(spend.priced_usd_nanos, 0);
        assert_eq!(journal.available_usd_nanos(), 600);
    }

    /// A released reservation frees its money but still counts as a started
    /// call: the call ceiling bounds attempts, not successes.
    #[test]
    fn a_released_reservation_frees_money_but_still_counts_as_a_call() {
        let mut journal = GatewayJournal::new(identity(budget(1_000, 8)));
        let ordinal = journal.reserve("alias", &card(1, 1), 400);
        journal.settle(
            ordinal,
            Settlement::Released {
                reason: ReleaseReason::ProviderRefusedBeforeWork,
            },
        );
        let spend = journal.spend();
        assert_eq!(spend.committed_usd_nanos(), 0);
        assert_eq!(spend.calls_started, 1);
        assert_eq!(spend.released_calls, 1);
    }

    /// Money is priced once, at settlement. A reloaded journal folds the amount
    /// that was written, so resuming cannot reprice it.
    #[test]
    fn a_resumed_journal_folds_recorded_amounts_and_never_reprices() {
        let dir = ScratchDir::new("journal-reprice");
        let path = dir.join("journal.json");
        let identity = identity(budget(1_000_000, 8));
        let mut journal = GatewayJournal::new(identity.clone());
        let ordinal = journal.reserve("alias", &card(2, 3), 5_000);
        journal.settle(
            ordinal,
            Settlement::Priced {
                input_tokens: 10,
                output_tokens: 10,
                usd_nanos: "50".into(),
            },
        );
        journal.save(&path).expect("save");

        let reloaded = GatewayJournal::load_bound(&path, &identity).expect("resume");
        assert_eq!(reloaded.spend().priced_usd_nanos, 50);
        assert!(!reloaded.spend().is_partial());
        // The recorded 50 is what a resume sees, and it is not recomputed: a
        // different card over the same tokens would produce something else.
        assert_eq!(card(2, 3).quote_nanos(10, 10), Some(50));
        assert_ne!(card(9, 9).quote_nanos(10, 10), Some(50));
    }

    /// A journal bound to different routes or a different budget is refused,
    /// never truncated and never merged into the current experiment.
    #[test]
    fn a_journal_bound_elsewhere_is_refused() {
        let dir = ScratchDir::new("journal-bind");
        let path = dir.join("journal.json");
        let mine = identity(budget(1_000, 8));
        GatewayJournal::new(mine.clone()).save(&path).expect("save");

        let other_routes = JournalIdentity::new("b".repeat(64), budget(1_000, 8));
        assert!(GatewayJournal::load_bound(&path, &other_routes).is_err());
        let other_budget = JournalIdentity::new("a".repeat(64), budget(2_000, 8));
        assert!(GatewayJournal::load_bound(&path, &other_budget).is_err());
        assert!(GatewayJournal::load_bound(&path, &mine).is_ok());
    }

    /// A sweep-bound journal resumes only under its own sweep, while the
    /// inspection load reads it under the same routes and budget. A journal no
    /// sweep owns serializes exactly as it did before the binding existed.
    #[test]
    fn a_sweep_bound_journal_resumes_only_under_its_sweep() {
        let dir = ScratchDir::new("journal-sweep");
        let path = dir.join("journal.json");
        let unbound = identity(budget(1_000, 8));
        let legacy = serde_json::to_string(&GatewayJournal::new(unbound.clone())).expect("json");
        assert!(!legacy.contains("sweep_sha256"), "{legacy}");

        let bound = unbound.clone().for_sweep("1".repeat(64));
        GatewayJournal::new(bound.clone())
            .save(&path)
            .expect("save");
        assert!(GatewayJournal::load_bound(&path, &bound).is_ok());
        assert!(GatewayJournal::load_bound(&path, &unbound).is_err());
        let other = unbound.clone().for_sweep("2".repeat(64));
        assert!(GatewayJournal::load_bound(&path, &other).is_err());
        assert!(GatewayJournal::load_for_routes(&path, &"a".repeat(64), budget(1_000, 8)).is_ok());
        assert!(GatewayJournal::load_for_routes(&path, &"a".repeat(64), budget(2_000, 8)).is_err());
    }

    /// Recovery cannot selectively erase a spent attempt: a hand-edited journal
    /// that drops a reservation, or settles one twice, is refused on load.
    #[test]
    fn a_journal_that_erases_or_double_settles_an_attempt_is_refused() {
        let dir = ScratchDir::new("journal-erase");
        let path = dir.join("journal.json");
        let identity = identity(budget(1_000, 8));
        let mut journal = GatewayJournal::new(identity.clone());
        journal.reserve("alias", &card(1, 1), 100);
        journal.reserve("alias", &card(1, 1), 100);
        journal.settle(
            1,
            Settlement::Unknown {
                reason: UnknownCostReason::UsageAbsent,
            },
        );
        journal.save(&path).expect("save");
        let text = std::fs::read_to_string(&path).expect("read");

        let mut erased: serde_json::Value = serde_json::from_str(&text).expect("json");
        let records = erased["records"].as_array().expect("records").clone();
        erased["records"] = serde_json::Value::Array(records[1..].to_vec());
        std::fs::write(&path, erased.to_string()).expect("write");
        let error = GatewayJournal::load_bound(&path, &identity).expect_err("erased is refused");
        assert!(error.to_string().contains("malformed"), "{error}");

        let mut doubled: serde_json::Value = serde_json::from_str(&text).expect("json");
        let mut records = doubled["records"].as_array().expect("records").clone();
        let last = records.last().expect("a settlement").clone();
        records.push(last);
        doubled["records"] = serde_json::Value::Array(records);
        std::fs::write(&path, doubled.to_string()).expect("write");
        assert!(GatewayJournal::load_bound(&path, &identity).is_err());
    }

    fn lock_dir(tag: &str) -> ScratchDir {
        ScratchDir::new(&format!("journal-{tag}"))
    }

    /// A second holder is refused by type. It is not queued behind the first,
    /// because two gateways over one budget is not a situation waiting improves.
    #[test]
    fn a_second_holder_of_one_journal_is_refused() {
        let dir = lock_dir("lockheld");
        let path = dir.join("journal.json");
        let held = JournalLock::acquire(&path).expect("the first holder takes the path");
        let error = JournalLock::acquire(&path).expect_err("the second holder is refused");
        assert!(
            matches!(&error, JournalLockError::Held { lock_path, .. } if lock_path == held.path()),
            "{error}"
        );
        assert!(
            error
                .to_string()
                .contains(&held.path().display().to_string()),
            "the refusal names the lock file: {error}"
        );
        drop(held);
        JournalLock::acquire(&path).expect("a released path is free again");
    }

    /// A lock left behind by a crashed process is refused, not broken. Nothing
    /// on disk tells a dead holder from a live one, and breaking the lock on a
    /// guess puts two writers back on one budget.
    #[test]
    fn a_stale_lock_is_refused_rather_than_broken() {
        let dir = lock_dir("lockstale");
        let path = dir.join("journal.json");
        let lock_path = JournalLock::lock_path(&path).expect("a lock path");
        // What a process that died mid-sweep leaves behind: the lock file, with
        // no live holder anywhere.
        std::mem::forget(JournalLock::acquire(&path).expect("the crashed holder took the path"));

        let error = JournalLock::acquire(&path).expect_err("a stale lock is still a lock");
        assert!(matches!(error, JournalLockError::Held { .. }), "{error}");
        assert!(
            lock_path.exists(),
            "the refusal leaves the stale lock in place for the operator to judge"
        );
        std::fs::remove_file(&lock_path).ok();
    }

    /// Clearing a stale lock is an explicit act, and it leaves a record: the
    /// lock that replaces it names the holder it displaced and the reason.
    ///
    /// The displaced holder is a document this test wrote, not a lock this
    /// process took, and its pid and timestamp are values this process would
    /// never produce for itself. An assertion on them can be satisfied only by
    /// the takeover having read the displaced document: writing the taker's own
    /// identity, which is the mutation this test exists to catch, fails on the
    /// pid, and leaving the timestamp unwritten fails on the timestamp.
    #[test]
    fn a_takeover_is_explicit_and_records_who_it_displaced() {
        const DISPLACED_PID: u32 = 424_242;
        const DISPLACED_MS: u128 = 1_111_111_111_111;

        let dir = lock_dir("locktakeover");
        let path = dir.join("journal.json");
        let lock_path = JournalLock::lock_path(&path).expect("a lock path");
        assert!(
            matches!(
                JournalLock::take_over(&path, "nothing is running"),
                Err(JournalLockError::NotHeld { .. })
            ),
            "a takeover of an unheld path is refused rather than treated as an acquire"
        );

        // What a holder on another host, or one this process cannot be mistaken
        // for, left behind.
        assert_ne!(DISPLACED_PID, std::process::id());
        std::fs::write(
            &lock_path,
            serde_json::to_vec(&JournalLockDocument {
                schema_version: JOURNAL_LOCK_SCHEMA_VERSION.to_string(),
                pid: DISPLACED_PID,
                instance: "0123456789abcdef0123456789abcdef".to_string(),
                acquired_unix_ms: DISPLACED_MS,
                superseded: None,
            })
            .expect("a lock document"),
        )
        .expect("the crashed holder's lock");

        let taken = JournalLock::take_over(&path, "host rebooted, pid 424242 is gone")
            .expect("an operator may displace a holder deliberately");
        let document: JournalLockDocument =
            serde_json::from_slice(&std::fs::read(taken.path()).expect("the new lock is readable"))
                .expect("the new lock is a lock document");
        let superseded = document.superseded.expect("the displacement is recorded");
        assert_eq!(
            superseded.pid, DISPLACED_PID,
            "the record names the holder that was displaced, not the one that displaced it"
        );
        assert_eq!(
            superseded.acquired_unix_ms, DISPLACED_MS,
            "and when it had held the path since"
        );
        assert_eq!(superseded.reason, "host rebooted, pid 424242 is gone");
        assert_eq!(document.pid, std::process::id());
        assert_ne!(
            document.instance, "0123456789abcdef0123456789abcdef",
            "the lock that replaces it is a different lock, not the same one rewritten"
        );

        drop(taken);
        assert!(!lock_path.exists());
    }

    /// A lock document nothing can parse still displaces, and says so: the
    /// record carries the zeroes that mean "nothing was legible here" rather
    /// than a plausible pid nobody wrote.
    #[test]
    fn a_takeover_of_an_illegible_lock_records_that_it_learned_nothing() {
        let dir = lock_dir("lockillegible");
        let path = dir.join("journal.json");
        let lock_path = JournalLock::lock_path(&path).expect("a lock path");
        std::fs::write(&lock_path, b"not a lock document")
            .expect("a foreign file at the lock path");

        let taken = JournalLock::take_over(&path, "whatever this is, it is not running")
            .expect("an operator may displace it");
        let document: JournalLockDocument =
            serde_json::from_slice(&std::fs::read(taken.path()).expect("readable"))
                .expect("a lock document");
        let superseded = document.superseded.expect("the displacement is recorded");
        assert_eq!(superseded.pid, 0);
        assert_eq!(superseded.acquired_unix_ms, 0);
        assert_eq!(superseded.reason, "whatever this is, it is not running");
    }

    /// A1. A holder removes the lock it holds, and only that. After a takeover
    /// the file at the path is the taker's lock, and the displaced holder's
    /// drop must leave it alone: the journal stays owned while the taker is
    /// live, instead of being left unowned for any number of further gateways.
    ///
    /// What could satisfy this without the repair: nothing removing the file at
    /// all would pass the first assertion and fail the third, which requires the
    /// *taker* to still release it. A drop keyed on the path, which is the
    /// defect, fails the first.
    #[test]
    fn a_displaced_holder_leaves_the_lock_of_the_holder_that_displaced_it() {
        let dir = lock_dir("lockdisplaced");
        let path = dir.join("journal.json");
        let lock_path = JournalLock::lock_path(&path).expect("a lock path");

        let displaced = JournalLock::acquire(&path).expect("the first holder takes the path");
        let taker = JournalLock::take_over(&path, "believed gone, in fact still running")
            .expect("an operator displaces it");
        assert!(
            !displaced.is_still_held(),
            "a displaced holder can tell that the lock at its path is no longer its own"
        );
        assert!(taker.is_still_held());

        // The displaced process was not dead after all, and now exits normally.
        drop(displaced);

        assert!(
            lock_path.exists(),
            "the taker's lock survives the displaced holder's drop"
        );
        assert!(
            matches!(
                JournalLock::acquire(&path),
                Err(JournalLockError::Held { .. })
            ),
            "and no third holder is admitted while the taker is live"
        );

        drop(taker);
        assert!(
            !lock_path.exists(),
            "the holder that does own the lock still releases it"
        );
    }

    /// The lock lives beside the journal and is never mistaken for one: it has
    /// its own name and its own schema, and a journal reader refuses it.
    #[test]
    fn a_lock_is_not_a_journal() {
        let dir = lock_dir("locknotjournal");
        let path = dir.join("journal.json");
        let held = JournalLock::acquire(&path).expect("lock");
        assert_eq!(held.path(), dir.join("journal.json.lock"));
        assert!(!path.exists(), "taking the lock writes no journal");
        let error = GatewayJournal::load_bound(held.path(), &identity(budget(1_000, 8)))
            .expect_err("a lock document is never read as a journal");
        assert_ne!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "the file is there; it is refused for what it is: {error}"
        );
    }

    /// A2. The document's identity is what ownership is keyed on, and it is a
    /// property of the document rather than of the entry it was reached
    /// through: it survives the rename every save persists through, and a
    /// second name for the same document reports the same value.
    ///
    /// Only the id surviving the save can satisfy this: a fresh id per save, or
    /// one derived from the path, changes the value the second read sees.
    #[test]
    fn a_journal_identity_is_the_documents_and_survives_every_save() {
        let dir = lock_dir("journalid");
        let path = dir.join("journal.json");
        let alias = dir.join("journal-copy.json");
        let identity = identity(budget(1_000_000, 8));
        let mut journal = GatewayJournal::new(identity.clone());
        let assigned = journal
            .journal_id()
            .expect("a new journal names itself")
            .to_string();
        journal.save(&path).expect("save");
        assert_eq!(
            JournalLock::document_id(&path).as_deref(),
            Some(assigned.as_str())
        );

        let ordinal = journal.reserve("alias", &card(1, 1), 10);
        journal.settle(
            ordinal,
            Settlement::Released {
                reason: ReleaseReason::NeverDispatched,
            },
        );
        journal
            .save(&path)
            .expect("a second save renames a new file over the old one");
        assert_eq!(
            JournalLock::document_id(&path).as_deref(),
            Some(assigned.as_str()),
            "the identity is the document's, not the inode's"
        );
        assert_eq!(
            GatewayJournal::load_bound(&path, &identity)
                .expect("resume")
                .journal_id(),
            Some(assigned.as_str())
        );

        std::fs::copy(&path, &alias).expect("a second name for the same document");
        assert_eq!(
            JournalLock::document_id(&alias).as_deref(),
            Some(assigned.as_str()),
            "every name for one document reports one identity"
        );
        assert_eq!(
            JournalLock::identity_lock_path(&path, &assigned),
            JournalLock::identity_lock_path(&alias, &assigned),
            "so both names derive one lock"
        );
    }

    /// A lock over a journal document is refused to a second holder whatever
    /// name that holder used, and the spelling lock alone is not what refuses
    /// it: the two names have different spelling locks and the second one is
    /// free.
    #[test]
    fn a_second_name_for_one_journal_document_is_refused_the_lock() {
        let dir = lock_dir("aliaslock");
        let path = dir.join("journal.json");
        let alias = dir.join("journal-copy.json");
        let mut journal = GatewayJournal::new(identity(budget(1_000_000, 8)));
        journal.save(&path).expect("save");
        std::fs::copy(&path, &alias).expect("a second name");

        let held = JournalLock::acquire(&path).expect("the first holder takes the document");
        assert!(held.covers_document());
        assert!(
            !JournalLock::lock_path(&alias)
                .expect("a lock path")
                .exists(),
            "the other name's spelling lock is free, so only the document lock can refuse"
        );
        let error = JournalLock::acquire(&alias).expect_err("the second name is refused");
        let identity_path = JournalLock::identity_lock_path(
            &path,
            &JournalLock::document_id(&path).expect("an identity"),
        );
        assert!(
            matches!(&error, JournalLockError::Held { lock_path, .. } if *lock_path == identity_path),
            "{error}"
        );
        assert!(
            !JournalLock::lock_path(&alias)
                .expect("a lock path")
                .exists(),
            "a refused holder leaves no lock of its own behind"
        );
    }

    /// A7. A save whose rename landed and whose durability step did not is a
    /// distinct outcome, and the snapshot stays level with the file: the next
    /// save is this owner writing again, not another writer refused.
    ///
    /// Two causes could make that next save succeed: this one, and a save that
    /// never bumped the version at all. The first assertions rule the second
    /// out by reading the version off the disk.
    #[test]
    fn a_save_whose_rename_landed_is_unsynced_rather_than_a_later_conflict() {
        let dir = lock_dir("unsynced");
        let path = dir.join("journal.json");
        let mut journal = GatewayJournal::new(identity(budget(1_000_000, 8)));
        journal.save(&path).expect("the first save lands whole");
        assert_eq!(journal.version(), 1);

        fail_next_parent_sync();
        let error = journal
            .save(&path)
            .expect_err("a save whose durability is unconfirmed is not a success");
        assert!(matches!(error, JournalSaveError::Unsynced(_)), "{error}");
        assert_eq!(
            GatewayJournal::version_on_disk(&path).expect("readable"),
            Some(2),
            "the rename landed, so the file is at the new version"
        );
        assert_eq!(
            journal.version(),
            2,
            "and the snapshot stays level with it rather than a version behind"
        );

        journal
            .save(&path)
            .expect("the sole owner's next save is not refused as another writer's");
    }

    /// A total that contains an unmeasured amount stays labelled partial, and
    /// carries the measured part as a subtotal rather than as a total.
    #[test]
    fn a_partial_total_is_labelled_partial_and_publishes_only_a_subtotal() {
        let card = card(1, 1);
        let mut journal = GatewayJournal::new(identity(budget(1_000_000, 8)));
        let priced = journal.reserve("alias", &card, 500);
        journal.settle(
            priced,
            Settlement::Priced {
                input_tokens: 10,
                output_tokens: 10,
                usd_nanos: "20".into(),
            },
        );
        let unknown = journal.reserve("alias", &card, 500);
        journal.settle(
            unknown,
            Settlement::Unknown {
                reason: UnknownCostReason::UsageAbsent,
            },
        );
        match journal.monetary_summary(Some(&card)) {
            MonetarySummary::Unavailable {
                reason,
                known_subtotal_usd_nanos,
                ..
            } => {
                assert_eq!(reason, "host_observed_usage_incomplete");
                assert_eq!(known_subtotal_usd_nanos.as_deref(), Some("20"));
            }
            other => panic!("a partial total must not be published as a total: {other:?}"),
        }
    }

    /// Missing usage is unavailable, never zero. An empty journal has no total
    /// at all rather than a total of nothing.
    #[test]
    fn missing_usage_is_unavailable_rather_than_zero() {
        let card = card(1, 1);
        let journal = GatewayJournal::new(identity(budget(1_000, 8)));
        match journal.monetary_summary(Some(&card)) {
            MonetarySummary::Unavailable {
                reason,
                known_subtotal_usd_nanos,
                ..
            } => {
                assert_eq!(reason, "gateway_journal_has_no_calls");
                assert!(known_subtotal_usd_nanos.is_none(), "no calls is not a zero");
            }
            other => panic!("expected unavailable, got {other:?}"),
        }
    }

    /// A complete host-observed total says so: the source label is what
    /// distinguishes it from the entrant-reported estimate, and neither is a
    /// verified invoice.
    #[test]
    fn a_complete_total_is_labelled_host_observed() {
        let card = card(1, 1);
        let mut journal = GatewayJournal::new(identity(budget(1_000_000, 8)));
        let ordinal = journal.reserve("alias", &card, 500);
        journal.settle(
            ordinal,
            Settlement::Priced {
                input_tokens: 10,
                output_tokens: 10,
                usd_nanos: "20".into(),
            },
        );
        match journal.monetary_summary(Some(&card)) {
            MonetarySummary::Estimated {
                usage_source,
                usd_nanos,
                ..
            } => {
                assert_eq!(usage_source, "host_observed");
                assert_eq!(usd_nanos, "20");
            }
            other => panic!("expected an estimate, got {other:?}"),
        }
    }
}
