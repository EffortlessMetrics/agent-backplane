// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol state machine for the ABP JSONL sidecar protocol.
//!
//! Tracks the expected message sequence (`hello → run → events → final/fatal`)
//! and detects protocol violations such as events arriving before the handshake.

use super::error::SidecarError;
use super::frame::Frame;

/// States of the ABP sidecar protocol state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolPhase {
    /// Waiting for the sidecar `hello` handshake.
    AwaitingHello,
    /// Hello received; waiting for a `run` command from the control plane.
    AwaitingRun,
    /// Run dispatched; streaming events until a terminal frame.
    Streaming,
    /// A `final` or `fatal` frame has been received — the run is complete.
    Completed,
    /// The protocol encountered an unrecoverable violation.
    Faulted,
}

/// Tracks the ABP JSONL protocol state machine.
///
/// Feed frames via [`advance`](Self::advance) and inspect the current
/// [`phase`](Self::phase). The machine enforces the canonical ordering
/// `Hello → Run → Event* → (Final | Fatal)`.
#[derive(Debug, Clone)]
pub struct ProtocolState {
    phase: ProtocolPhase,
    run_id: Option<String>,
    events_seen: u64,
    fault_reason: Option<String>,
}

impl Default for ProtocolState {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolState {
    /// Create a new state machine in the `AwaitingHello` phase.
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: ProtocolPhase::AwaitingHello,
            run_id: None,
            events_seen: 0,
            fault_reason: None,
        }
    }

    /// Current phase of the protocol.
    #[must_use]
    pub fn phase(&self) -> ProtocolPhase {
        self.phase
    }

    /// The run identifier, if a `Run` frame has been seen.
    #[must_use]
    pub fn run_id(&self) -> Option<&str> {
        self.run_id.as_deref()
    }

    /// Number of `Event` frames seen since the last `Run`.
    #[must_use]
    pub fn events_seen(&self) -> u64 {
        self.events_seen
    }

    /// If the machine is in the [`Faulted`](ProtocolPhase::Faulted) phase,
    /// returns the reason string.
    #[must_use]
    pub fn fault_reason(&self) -> Option<&str> {
        self.fault_reason.as_deref()
    }

    /// Returns `true` if the protocol has reached a terminal state
    /// (`Completed` or `Faulted`).
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.phase,
            ProtocolPhase::Completed | ProtocolPhase::Faulted
        )
    }

    /// Feed a frame into the state machine.
    ///
    /// Returns `Ok(())` if the transition is valid, or `Err` describing
    /// the protocol violation. On error the machine moves to `Faulted`.
    pub fn advance(&mut self, frame: &Frame) -> Result<(), SidecarError> {
        if self.phase == ProtocolPhase::Faulted {
            return Err(SidecarError::Protocol(
                "state machine is faulted; call reset() first".into(),
            ));
        }

        match (&self.phase, frame) {
            // ── AwaitingHello ───────────────────────────────────────
            (ProtocolPhase::AwaitingHello, Frame::Hello { .. }) => {
                self.phase = ProtocolPhase::AwaitingRun;
                Ok(())
            }
            (ProtocolPhase::AwaitingHello, other) => {
                self.fault(format!("expected hello, got {}", frame_tag_name(other)))
            }

            // ── AwaitingRun ─────────────────────────────────────────
            (ProtocolPhase::AwaitingRun, Frame::Run { id, .. }) => {
                self.run_id = Some(id.clone());
                self.events_seen = 0;
                self.phase = ProtocolPhase::Streaming;
                Ok(())
            }
            (ProtocolPhase::AwaitingRun, Frame::Fatal { .. }) => {
                self.phase = ProtocolPhase::Completed;
                Ok(())
            }
            (ProtocolPhase::AwaitingRun, other) => self.fault(format!(
                "expected run or fatal, got {}",
                frame_tag_name(other)
            )),

            // ── Streaming ───────────────────────────────────────────
            (ProtocolPhase::Streaming, Frame::Event { ref_id, .. }) => {
                self.check_ref_id(ref_id)?;
                self.events_seen += 1;
                Ok(())
            }
            (ProtocolPhase::Streaming, Frame::Final { ref_id, .. }) => {
                self.check_ref_id(ref_id)?;
                self.phase = ProtocolPhase::Completed;
                Ok(())
            }
            (ProtocolPhase::Streaming, Frame::Fatal { ref_id, .. }) => {
                if let Some(rid) = ref_id {
                    self.check_ref_id(rid)?;
                }
                self.phase = ProtocolPhase::Completed;
                Ok(())
            }
            (ProtocolPhase::Streaming, Frame::Ping { .. })
            | (ProtocolPhase::Streaming, Frame::Pong { .. }) => {
                // Keep-alive frames are allowed in any streaming phase.
                Ok(())
            }
            (ProtocolPhase::Streaming, other) => self.fault(format!(
                "expected event/final/fatal during streaming, got {}",
                frame_tag_name(other)
            )),

            // ── Completed ───────────────────────────────────────────
            (ProtocolPhase::Completed, other) => self.fault(format!(
                "protocol already completed, unexpected {}",
                frame_tag_name(other)
            )),

            // Faulted is handled by the early return above, but the
            // compiler requires exhaustive matching.
            (ProtocolPhase::Faulted, _) => unreachable!("handled by early return"),
        }
    }

    /// Reset the state machine to `AwaitingHello`, clearing all tracked
    /// state.
    pub fn reset(&mut self) {
        self.phase = ProtocolPhase::AwaitingHello;
        self.run_id = None;
        self.events_seen = 0;
        self.fault_reason = None;
    }

    // ── internal ────────────────────────────────────────────────────

    fn fault(&mut self, reason: String) -> Result<(), SidecarError> {
        self.phase = ProtocolPhase::Faulted;
        self.fault_reason = Some(reason.clone());
        Err(SidecarError::Protocol(reason))
    }

    fn check_ref_id(&self, ref_id: &str) -> Result<(), SidecarError> {
        if let Some(expected) = &self.run_id {
            if ref_id != expected {
                return Err(SidecarError::Protocol(format!(
                    "ref_id mismatch: expected \"{expected}\", got \"{ref_id}\""
                )));
            }
        }
        Ok(())
    }
}

/// Return the tag name for a frame variant (matches the `"t"` discriminator).
fn frame_tag_name(frame: &Frame) -> &'static str {
    match frame {
        Frame::Hello { .. } => "hello",
        Frame::Run { .. } => "run",
        Frame::Event { .. } => "event",
        Frame::Final { .. } => "final",
        Frame::Fatal { .. } => "fatal",
        Frame::Cancel { .. } => "cancel",
        Frame::Ping { .. } => "ping",
        Frame::Pong { .. } => "pong",
    }
}
