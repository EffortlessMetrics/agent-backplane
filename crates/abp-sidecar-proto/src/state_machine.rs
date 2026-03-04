// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol sequence state machine for sidecar lifecycle tracking.
//!
//! Provides a typed state machine ensuring sidecars follow the correct
//! envelope sequence: `hello` → `run` → `event`* → `final`/`fatal`.

use std::fmt;

use abp_protocol::Envelope;
use thiserror::Error;

// ---------------------------------------------------------------------------
// ProtocolState
// ---------------------------------------------------------------------------

/// The current phase of the sidecar protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolState {
    /// Initial state — waiting for the `hello` envelope.
    AwaitingHello,
    /// Hello received — waiting for the `run` envelope.
    AwaitingRun,
    /// Run dispatched — streaming events.
    Streaming,
    /// A terminal (`final` or `fatal`) envelope has been received.
    Terminated,
    /// The protocol entered an invalid state due to a sequence violation.
    Error,
}

impl ProtocolState {
    /// Whether this state is terminal (no further transitions expected).
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminated | Self::Error)
    }
}

impl fmt::Display for ProtocolState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::AwaitingHello => "awaiting_hello",
            Self::AwaitingRun => "awaiting_run",
            Self::Streaming => "streaming",
            Self::Terminated => "terminated",
            Self::Error => "error",
        };
        f.write_str(name)
    }
}

// ---------------------------------------------------------------------------
// TransitionError
// ---------------------------------------------------------------------------

/// Error produced when an envelope does not match the expected protocol phase.
#[derive(Debug, Error)]
#[error("invalid transition: cannot accept {envelope_type} in state {state}")]
pub struct TransitionError {
    /// The current state when the bad transition was attempted.
    pub state: ProtocolState,
    /// The envelope type that caused the error.
    pub envelope_type: String,
}

// ---------------------------------------------------------------------------
// ProtocolStateMachine
// ---------------------------------------------------------------------------

/// Tracks and validates the sidecar protocol sequence.
///
/// Feed envelopes via [`advance`](Self::advance) and the machine will
/// either transition to the next valid state or return a
/// [`TransitionError`].
///
/// # Examples
///
/// ```
/// use abp_sidecar_proto::state_machine::{ProtocolStateMachine, ProtocolState};
/// use abp_protocol::Envelope;
/// use abp_core::{BackendIdentity, CapabilityManifest};
///
/// let mut sm = ProtocolStateMachine::new();
/// assert_eq!(sm.state(), ProtocolState::AwaitingHello);
///
/// let hello = Envelope::hello(
///     BackendIdentity { id: "test".into(), backend_version: None, adapter_version: None },
///     CapabilityManifest::new(),
/// );
/// sm.advance(&hello).unwrap();
/// assert_eq!(sm.state(), ProtocolState::AwaitingRun);
/// ```
#[derive(Debug)]
pub struct ProtocolStateMachine {
    state: ProtocolState,
    events_seen: u64,
}

impl ProtocolStateMachine {
    /// Create a new state machine in the [`AwaitingHello`](ProtocolState::AwaitingHello) state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: ProtocolState::AwaitingHello,
            events_seen: 0,
        }
    }

    /// The current protocol state.
    #[must_use]
    pub fn state(&self) -> ProtocolState {
        self.state
    }

    /// Number of `Event` envelopes processed since the last `Run`.
    #[must_use]
    pub fn events_seen(&self) -> u64 {
        self.events_seen
    }

    /// Advance the state machine with the given envelope.
    ///
    /// # Errors
    ///
    /// Returns [`TransitionError`] if the envelope is not valid for the
    /// current state. On error the machine transitions to
    /// [`ProtocolState::Error`].
    pub fn advance(&mut self, envelope: &Envelope) -> Result<ProtocolState, TransitionError> {
        let envelope_type = envelope_type_name(envelope);
        let next = match (&self.state, envelope) {
            (ProtocolState::AwaitingHello, Envelope::Hello { .. }) => ProtocolState::AwaitingRun,
            (ProtocolState::AwaitingRun, Envelope::Run { .. }) => {
                self.events_seen = 0;
                ProtocolState::Streaming
            }
            (ProtocolState::Streaming, Envelope::Event { .. }) => {
                self.events_seen += 1;
                ProtocolState::Streaming
            }
            (ProtocolState::Streaming, Envelope::Final { .. }) => ProtocolState::Terminated,
            (ProtocolState::Streaming, Envelope::Fatal { .. }) => ProtocolState::Terminated,
            // Allow fatal at any point before terminal (e.g. early abort)
            (ProtocolState::AwaitingRun, Envelope::Fatal { .. }) => ProtocolState::Terminated,
            (ProtocolState::AwaitingHello, Envelope::Fatal { .. }) => ProtocolState::Terminated,
            _ => {
                self.state = ProtocolState::Error;
                return Err(TransitionError {
                    state: self.state,
                    envelope_type: envelope_type.to_string(),
                });
            }
        };

        self.state = next;
        Ok(next)
    }

    /// Reset the machine back to [`AwaitingHello`](ProtocolState::AwaitingHello).
    pub fn reset(&mut self) {
        self.state = ProtocolState::AwaitingHello;
        self.events_seen = 0;
    }
}

impl Default for ProtocolStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Return a human-readable type name for an envelope.
fn envelope_type_name(envelope: &Envelope) -> &'static str {
    match envelope {
        Envelope::Hello { .. } => "hello",
        Envelope::Run { .. } => "run",
        Envelope::Event { .. } => "event",
        Envelope::Final { .. } => "final",
        Envelope::Fatal { .. } => "fatal",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, ReceiptBuilder,
        WorkOrderBuilder,
    };
    use chrono::Utc;

    fn hello_env() -> Envelope {
        Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        )
    }

    fn run_env() -> Envelope {
        let wo = WorkOrderBuilder::new("task").build();
        Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        }
    }

    fn event_env() -> Envelope {
        Envelope::Event {
            ref_id: "r".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "msg".into() },
                ext: None,
            },
        }
    }

    fn final_env() -> Envelope {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        Envelope::Final {
            ref_id: "r".into(),
            receipt,
        }
    }

    fn fatal_env() -> Envelope {
        Envelope::Fatal {
            ref_id: Some("r".into()),
            error: "boom".into(),
            error_code: None,
        }
    }

    // -- State display -----------------------------------------------------

    #[test]
    fn state_display() {
        assert_eq!(ProtocolState::AwaitingHello.to_string(), "awaiting_hello");
        assert_eq!(ProtocolState::Streaming.to_string(), "streaming");
        assert_eq!(ProtocolState::Terminated.to_string(), "terminated");
    }

    #[test]
    fn state_is_terminal() {
        assert!(!ProtocolState::AwaitingHello.is_terminal());
        assert!(!ProtocolState::Streaming.is_terminal());
        assert!(ProtocolState::Terminated.is_terminal());
        assert!(ProtocolState::Error.is_terminal());
    }

    // -- Happy path --------------------------------------------------------

    #[test]
    fn happy_path_hello_run_events_final() {
        let mut sm = ProtocolStateMachine::new();
        assert_eq!(sm.state(), ProtocolState::AwaitingHello);

        sm.advance(&hello_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::AwaitingRun);

        sm.advance(&run_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::Streaming);
        assert_eq!(sm.events_seen(), 0);

        sm.advance(&event_env()).unwrap();
        sm.advance(&event_env()).unwrap();
        assert_eq!(sm.events_seen(), 2);

        sm.advance(&final_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
        assert!(sm.state().is_terminal());
    }

    #[test]
    fn happy_path_no_events() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&final_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
        assert_eq!(sm.events_seen(), 0);
    }

    #[test]
    fn fatal_terminates_from_streaming() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&event_env()).unwrap();
        sm.advance(&fatal_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
    }

    #[test]
    fn fatal_allowed_before_run() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&fatal_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
    }

    #[test]
    fn fatal_allowed_before_hello() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&fatal_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
    }

    // -- Invalid transitions -----------------------------------------------

    #[test]
    fn event_before_hello_is_error() {
        let mut sm = ProtocolStateMachine::new();
        let err = sm.advance(&event_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
        assert!(err.to_string().contains("event"));
    }

    #[test]
    fn run_before_hello_is_error() {
        let mut sm = ProtocolStateMachine::new();
        let err = sm.advance(&run_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
        assert!(err.to_string().contains("run"));
    }

    #[test]
    fn event_before_run_is_error() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        let err = sm.advance(&event_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
        assert!(err.to_string().contains("event"));
    }

    #[test]
    fn hello_after_hello_is_error() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        let err = sm.advance(&hello_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
        assert!(err.to_string().contains("hello"));
    }

    #[test]
    fn event_after_terminal_is_error() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&final_env()).unwrap();
        let _err = sm.advance(&event_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
    }

    // -- Reset -------------------------------------------------------------

    #[test]
    fn reset_returns_to_initial() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&event_env()).unwrap();
        assert_eq!(sm.events_seen(), 1);

        sm.reset();
        assert_eq!(sm.state(), ProtocolState::AwaitingHello);
        assert_eq!(sm.events_seen(), 0);
    }

    // -- TransitionError display -------------------------------------------

    #[test]
    fn transition_error_display() {
        let err = TransitionError {
            state: ProtocolState::AwaitingHello,
            envelope_type: "event".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("event"));
        assert!(msg.contains("awaiting_hello") || msg.contains("error"));
    }

    // -- Default -----------------------------------------------------------

    #[test]
    fn default_is_awaiting_hello() {
        let sm = ProtocolStateMachine::default();
        assert_eq!(sm.state(), ProtocolState::AwaitingHello);
    }
}
