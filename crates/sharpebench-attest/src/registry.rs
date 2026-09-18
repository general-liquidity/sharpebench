//! Forward-attestation registry with an explicit operator-driven epoch lock.
//!
//! The deterministic core of a forward league: agents register a commitment for
//! a declared window, and the pre-image can only be revealed once the registry
//! *unlocks*. Time is an explicit integer **epoch** (no wall clock, so refusals
//! are reproducible in tests). The operator supplies that epoch. This crate does
//! not establish its correspondence to wall time, data availability, or prior
//! non-observation; the live feed, custody, scheduler, and hosting are out of
//! scope.

use std::collections::HashMap;

use crate::{verify_commitment_under_fault_plan, Commitment};

/// A registered commitment and the epoch at which its window unlocks.
#[derive(Clone, Debug)]
pub struct Registration {
    pub commitment: Commitment,
    pub unlock_epoch: u64,
    pub revealed: bool,
}

/// A registry of commitments under a monotonic, operator-supplied epoch counter.
#[derive(Default)]
pub struct Registry {
    current_epoch: u64,
    regs: HashMap<String, Registration>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance (or set) the current epoch.
    pub fn set_epoch(&mut self, epoch: u64) {
        self.current_epoch = epoch;
    }

    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    fn key(agent_id: &str, target_window: &str) -> String {
        format!("{agent_id}\u{1}{target_window}")
    }

    /// Register a commitment. Fails if the window has already unlocked (too late
    /// to commit) or the agent already registered for that window.
    pub fn register(&mut self, commitment: Commitment, unlock_epoch: u64) -> Result<(), String> {
        if self.current_epoch >= unlock_epoch {
            return Err("window already unlocked — too late to commit".to_string());
        }
        let k = Self::key(&commitment.agent_id, &commitment.target_window);
        if self.regs.contains_key(&k) {
            return Err("already registered for this window".to_string());
        }
        self.regs.insert(
            k,
            Registration {
                commitment,
                unlock_epoch,
                revealed: false,
            },
        );
        Ok(())
    }

    /// Reveal the pre-image. Succeeds only if the window has unlocked, the
    /// pre-image matches the registered commitment, and the commitment has not
    /// been opened already.
    pub fn reveal(
        &mut self,
        agent_id: &str,
        target_window: &str,
        artifact_digest: &str,
        salt: &str,
    ) -> Result<(), String> {
        self.reveal_under_fault_plan(agent_id, target_window, artifact_digest, salt, None)
    }

    /// [`Registry::reveal`] for a window scored under `fault_plan_sha256`. A
    /// commitment that bound another plan, a plan when this is `None`, or no
    /// plan when this names one, does not match and is refused.
    ///
    /// A commitment opens once. Its pre-image is public from the moment it is
    /// revealed, so anyone who reads the reveal can replay it; without this the
    /// registry answers every replay with `Ok` and a caller that compares the
    /// opened entries cannot tell the entrant's own from a stranger's copy. The
    /// refusal therefore falls on every reveal after the first, never on the
    /// first.
    pub fn reveal_under_fault_plan(
        &mut self,
        agent_id: &str,
        target_window: &str,
        artifact_digest: &str,
        salt: &str,
        fault_plan_sha256: Option<&str>,
    ) -> Result<(), String> {
        let verified = self.check_reveal_under_fault_plan(
            agent_id,
            target_window,
            artifact_digest,
            salt,
            fault_plan_sha256,
        )?;
        self.open(verified);
        Ok(())
    }

    /// Check a pre-image against its registered commitment without opening it:
    /// the window has unlocked, the pre-image matches, and the commitment is
    /// still unopened. Hand the [`VerifiedReveal`] to [`Registry::open`] once
    /// the caller has decided the reveal counts.
    ///
    /// A caller that admits a reveal only after further checks needs this
    /// split. A commitment opens once, and the pre-image is public from the
    /// moment it is revealed, so a stranger can construct an entry that matches
    /// it. If matching alone opened the commitment, that stranger's entry would
    /// consume the entrant's single reveal on its way to being refused, and the
    /// entrant's own reveal would arrive too late.
    pub fn check_reveal_under_fault_plan(
        &self,
        agent_id: &str,
        target_window: &str,
        artifact_digest: &str,
        salt: &str,
        fault_plan_sha256: Option<&str>,
    ) -> Result<VerifiedReveal, String> {
        let k = Self::key(agent_id, target_window);
        let reg = self.regs.get(&k).ok_or("no such commitment")?;
        if self.current_epoch < reg.unlock_epoch {
            return Err("window still locked".to_string());
        }
        if !verify_commitment_under_fault_plan(
            &reg.commitment,
            agent_id,
            target_window,
            artifact_digest,
            salt,
            fault_plan_sha256,
        ) {
            return Err("reveal does not match commitment".to_string());
        }
        if reg.revealed {
            return Err("commitment already revealed; one commitment opens once".to_string());
        }
        Ok(VerifiedReveal { key: k })
    }

    /// Open a checked commitment, spending its single reveal. Every later
    /// reveal of it is refused.
    pub fn open(&mut self, verified: VerifiedReveal) {
        self.regs
            .get_mut(&verified.key)
            .expect("a checked reveal names a registered commitment")
            .revealed = true;
    }
}

/// A pre-image that matched an unopened commitment, and the right to open it.
///
/// Carried rather than re-derived so the decision to open cannot drift from the
/// check that permitted it. Dropping one leaves the commitment unopened, which
/// is why it must not be discarded silently.
#[must_use = "a checked reveal opens no commitment until it is passed to Registry::open"]
pub struct VerifiedReveal {
    key: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_commitment;

    fn commit() -> Commitment {
        make_commitment("gordon", "2025-Q4", "digest", "salt")
    }

    #[test]
    fn cannot_commit_after_unlock() {
        let mut r = Registry::new();
        r.set_epoch(10);
        assert!(r.register(commit(), 5).is_err());
    }

    #[test]
    fn cannot_reveal_while_locked() {
        let mut r = Registry::new();
        r.register(commit(), 100).unwrap();
        assert!(r.reveal("gordon", "2025-Q4", "digest", "salt").is_err());
    }

    #[test]
    fn reveal_after_unlock_with_correct_preimage() {
        let mut r = Registry::new();
        r.register(commit(), 100).unwrap();
        r.set_epoch(100);
        assert!(r.reveal("gordon", "2025-Q4", "digest", "salt").is_ok());
        // wrong pre-image fails
        let mut r2 = Registry::new();
        r2.register(commit(), 100).unwrap();
        r2.set_epoch(100);
        assert!(r2.reveal("gordon", "2025-Q4", "WRONG", "salt").is_err());
    }

    /// The pre-image is public once it is revealed, so the registry must count
    /// reveals rather than trust that only its owner holds one.
    #[test]
    fn a_commitment_opens_once() {
        let mut r = Registry::new();
        r.register(commit(), 100).unwrap();
        r.set_epoch(100);
        assert!(r.reveal("gordon", "2025-Q4", "digest", "salt").is_ok());
        let again = r.reveal("gordon", "2025-Q4", "digest", "salt");
        assert_eq!(
            again,
            Err("commitment already revealed; one commitment opens once".to_string())
        );
    }

    /// Checking a pre-image is not opening it: a caller that checks and then
    /// refuses the entry leaves the commitment for the next reveal.
    #[test]
    fn a_checked_reveal_that_is_never_opened_leaves_the_commitment_unopened() {
        let mut r = Registry::new();
        r.register(commit(), 100).unwrap();
        r.set_epoch(100);
        let checked = r
            .check_reveal_under_fault_plan("gordon", "2025-Q4", "digest", "salt", None)
            .unwrap();
        drop(checked);
        assert!(r.reveal("gordon", "2025-Q4", "digest", "salt").is_ok());
    }
}
