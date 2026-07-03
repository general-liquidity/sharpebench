//! Transport-integrity primitives for external agents.
//!
//! An external agent speaks JSON over a container/HTTP boundary, and any such
//! boundary can blip: a connection resets, stdout closes, an endpoint stalls. The
//! old behaviour degraded *every* such blip to an empty-orders hold - silently
//! biasing the run's return series toward flat and making a transient failure
//! indistinguishable from a deliberate hold. In a luck-robust *honesty* benchmark
//! that is an eval-integrity hole.
//!
//! These primitives close it: a bounded per-decision **retry** recovers a transient
//! blip, a per-endpoint **circuit breaker** fails a dead endpoint explicitly instead
//! of emitting a stream of masked holds, and a rolling [`TransportHealth`] records
//! every fault as a *distinct outcome* so the harness can surface it as a typed
//! failure (never a silent hold). The failure *taxonomy* proper lives in the harness
//! (`FailureKind`); the crate boundary forbids `sharpebench-sim` depending on it, so
//! the transport layer speaks [`DecideError`] and the harness maps it across.

use sharpebench_protocol::Decision;

/// Why one external-agent decision attempt failed at the wire, as distinct from a
/// deliberate hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecideError {
    /// Connection reset, stdout closed, write failed, malformed framing - a runtime
    /// transport error (retryable).
    Transport,
    /// The endpoint did not answer within the wall-clock budget (retryable).
    Timeout,
    /// The agent answered, but the bytes were not a parseable [`Decision`]. The
    /// agent's own protocol violation - **not** retryable.
    Protocol,
}

impl DecideError {
    /// Whether this is a retryable runtime error (transport / timeout) rather than a
    /// final agent protocol fault.
    pub fn is_retryable(&self) -> bool {
        matches!(self, DecideError::Transport | DecideError::Timeout)
    }
}

/// Retry a fallible transport decision on retryable runtime errors up to
/// `max_retries` *extra* attempts; a protocol fault returns immediately (it is the
/// agent's own fault, not the harness's). Pure over the injected `attempt`, so a
/// transient blip that recovers on retry yields `Ok`, while a persistent fault
/// surfaces the last error to the caller.
pub fn decide_with_retry<F>(max_retries: u32, mut attempt: F) -> Result<Decision, DecideError>
where
    F: FnMut() -> Result<Decision, DecideError>,
{
    let mut tries = 0u32;
    loop {
        tries += 1;
        match attempt() {
            Ok(d) => return Ok(d),
            Err(e) if e.is_retryable() && tries <= max_retries => { /* retry */ }
            Err(e) => return Err(e),
        }
    }
}

/// A per-endpoint circuit breaker: after `max_consecutive` consecutive unrecovered
/// transport faults it trips, so a dead endpoint fails the run **explicitly** instead
/// of emitting an indistinguishable stream of holds for the rest of the window. A
/// single clean decision resets the streak.
#[derive(Clone, Debug)]
pub struct CircuitBreaker {
    consecutive: u32,
    max_consecutive: u32,
    tripped: bool,
}

impl CircuitBreaker {
    /// A breaker that trips after `max_consecutive` consecutive faults (clamped to
    /// at least 1).
    pub fn new(max_consecutive: u32) -> Self {
        Self {
            consecutive: 0,
            max_consecutive: max_consecutive.max(1),
            tripped: false,
        }
    }

    /// Record a clean decision - clears the consecutive-fault streak.
    pub fn record_success(&mut self) {
        self.consecutive = 0;
    }

    /// Record a transport fault; returns whether the breaker is now tripped.
    pub fn record_fault(&mut self) -> bool {
        self.consecutive += 1;
        if self.consecutive >= self.max_consecutive {
            self.tripped = true;
        }
        self.tripped
    }

    /// Whether the breaker has tripped (the endpoint is considered dead for the run).
    pub fn is_tripped(&self) -> bool {
        self.tripped
    }
}

/// Rolling transport health for one external agent across a run, so a transport blip
/// is a recorded, inspectable outcome rather than a silent degrade-to-hold. The
/// harness reads this after a run to decide whether the returns are trustworthy or
/// the run must be surfaced as a typed failure.
#[derive(Clone, Debug, Default)]
pub struct TransportHealth {
    /// Retryable transport / timeout faults that ultimately degraded to a hold.
    pub transport_faults: u32,
    /// Agent protocol faults (unparseable output) - the agent's own fault.
    pub protocol_faults: u32,
    /// Whether the per-endpoint circuit breaker has tripped.
    pub tripped: bool,
    /// The most recent decision-level fault, if any.
    pub last_error: Option<DecideError>,
}

impl TransportHealth {
    /// Fold a decision-level fault into the rolling health, tagged with whether the
    /// circuit breaker tripped on it.
    pub fn record(&mut self, err: DecideError, tripped: bool) {
        match err {
            DecideError::Protocol => self.protocol_faults += 1,
            DecideError::Transport | DecideError::Timeout => self.transport_faults += 1,
        }
        self.last_error = Some(err);
        self.tripped = tripped;
    }

    /// Did any decision in the run fail at the transport / agent layer? I.e. was at
    /// least one hold a masked fault rather than a deliberate hold?
    pub fn degraded(&self) -> bool {
        self.transport_faults > 0 || self.protocol_faults > 0 || self.tripped
    }
}

/// Something whose per-decision transport health can be inspected after a run - the
/// seam the harness uses to convert a masked-hold run into an explicit failure.
pub trait TransportDiagnostics {
    /// The rolling transport health accumulated across the run so far.
    fn health(&self) -> &TransportHealth;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold() -> Decision {
        Decision {
            orders: Vec::new(),
            reasoning: String::new(),
            cost: None,
        }
    }

    #[test]
    fn retry_recovers_a_transient_blip() {
        let mut calls = 0u32;
        let out = decide_with_retry(2, || {
            calls += 1;
            if calls < 3 {
                Err(DecideError::Transport)
            } else {
                Ok(hold())
            }
        });
        assert!(
            out.is_ok(),
            "a blip that clears within the retry budget recovers"
        );
        assert_eq!(calls, 3, "1 initial + 2 retries");
    }

    #[test]
    fn persistent_transport_fault_exhausts_retries() {
        let mut calls = 0u32;
        let out = decide_with_retry(2, || {
            calls += 1;
            Err(DecideError::Timeout)
        });
        assert_eq!(out.unwrap_err(), DecideError::Timeout);
        assert_eq!(
            calls, 3,
            "exhausts the bounded retries then surfaces the error"
        );
    }

    #[test]
    fn protocol_fault_is_not_retried() {
        let mut calls = 0u32;
        let out = decide_with_retry(5, || {
            calls += 1;
            Err(DecideError::Protocol)
        });
        assert_eq!(out.unwrap_err(), DecideError::Protocol);
        assert_eq!(calls, 1, "an agent protocol fault is final, never retried");
    }

    #[test]
    fn breaker_trips_after_consecutive_faults_and_resets_on_success() {
        let mut cb = CircuitBreaker::new(3);
        assert!(!cb.record_fault());
        assert!(!cb.record_fault());
        assert!(
            cb.record_fault(),
            "third consecutive fault trips the breaker"
        );
        assert!(cb.is_tripped());

        // A success clears the streak, but a tripped breaker stays tripped.
        let mut cb2 = CircuitBreaker::new(2);
        assert!(!cb2.record_fault());
        cb2.record_success();
        assert!(!cb2.record_fault(), "streak reset by the success");
        assert!(!cb2.is_tripped());
    }

    #[test]
    fn health_records_faults_distinctly() {
        let mut h = TransportHealth::default();
        assert!(!h.degraded());
        h.record(DecideError::Transport, false);
        h.record(DecideError::Protocol, false);
        assert_eq!(h.transport_faults, 1);
        assert_eq!(h.protocol_faults, 1);
        assert_eq!(h.last_error, Some(DecideError::Protocol));
        assert!(
            h.degraded(),
            "a recorded fault is a distinct outcome, not a hold"
        );
    }
}
