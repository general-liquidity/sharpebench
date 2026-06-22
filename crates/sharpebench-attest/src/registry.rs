//! Forward-attestation registry with explicit time-lock.
//!
//! The deterministic core of a forward league: agents register a commitment for
//! a future window, and the pre-image can only be revealed once the window
//! *unlocks*. Time is an explicit integer **epoch** (no wall clock → reproducible
//! in tests and verifiable by anyone). The live data feed and hosting that drive
//! the epoch forward are out of scope for this crate.

use std::collections::HashMap;

use crate::{verify_commitment, Commitment};

/// A registered commitment and the epoch at which its window unlocks.
#[derive(Clone, Debug)]
pub struct Registration {
    pub commitment: Commitment,
    pub unlock_epoch: u64,
    pub revealed: bool,
}

/// A registry of forward-attestation commitments under a monotonic epoch clock.
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

    /// Reveal the pre-image. Succeeds only if the window has unlocked AND the
    /// pre-image matches the registered commitment.
    pub fn reveal(
        &mut self,
        agent_id: &str,
        target_window: &str,
        artifact_digest: &str,
        salt: &str,
    ) -> Result<(), String> {
        let k = Self::key(agent_id, target_window);
        let unlocked = self.current_epoch;
        let reg = self.regs.get_mut(&k).ok_or("no such commitment")?;
        if unlocked < reg.unlock_epoch {
            return Err("window still locked".to_string());
        }
        if !verify_commitment(
            &reg.commitment,
            agent_id,
            target_window,
            artifact_digest,
            salt,
        ) {
            return Err("reveal does not match commitment".to_string());
        }
        reg.revealed = true;
        Ok(())
    }
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
}
