// SPDX-License-Identifier: MIT OR Apache-2.0
//! Host-side graceful shutdown coordinator for sidecar processes.
//!
//! Builds on the protocol-level shutdown types to manage the complete
//! shutdown lifecycle: send request → wait for ack → force kill on timeout.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// ShutdownConfig
// ---------------------------------------------------------------------------

/// Host configuration for sidecar shutdown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownConfig {
    /// How long to wait for the sidecar's goodbye before force-killing.
    pub grace_period_ms: u64,
    /// Whether to attempt sending a shutdown request at all (vs. immediate
    /// kill).
    pub graceful: bool,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            grace_period_ms: 30_000,
            graceful: true,
        }
    }
}

impl ShutdownConfig {
    /// Grace period as a [`Duration`].
    #[must_use]
    pub fn grace_period(&self) -> Duration {
        Duration::from_millis(self.grace_period_ms)
    }
}

// ---------------------------------------------------------------------------
// ShutdownPhase
// ---------------------------------------------------------------------------

/// Phases of the host-side shutdown coordinator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShutdownPhase {
    /// No shutdown in progress.
    Idle,
    /// Shutdown request sent, waiting for ack.
    WaitingForAck,
    /// Sidecar acknowledged.
    Acknowledged,
    /// Grace period expired, force-killing.
    ForceKill,
}

// ---------------------------------------------------------------------------
// ShutdownSession
// ---------------------------------------------------------------------------

/// Tracks a single shutdown exchange between host and sidecar.
#[derive(Debug)]
pub struct ShutdownSession {
    config: ShutdownConfig,
    phase: ShutdownPhase,
    started_at: Option<Instant>,
    ack_received: bool,
}

impl ShutdownSession {
    /// Create a new session.
    #[must_use]
    pub fn new(config: ShutdownConfig) -> Self {
        Self {
            config,
            phase: ShutdownPhase::Idle,
            started_at: None,
            ack_received: false,
        }
    }

    /// Begin the shutdown sequence.
    pub fn begin(&mut self) {
        self.phase = if self.config.graceful {
            ShutdownPhase::WaitingForAck
        } else {
            ShutdownPhase::ForceKill
        };
        self.started_at = Some(Instant::now());
    }

    /// Record that the sidecar acknowledged the shutdown.
    pub fn record_ack(&mut self) {
        self.ack_received = true;
        self.phase = ShutdownPhase::Acknowledged;
    }

    /// Check whether the grace period has expired.
    pub fn check_timeout(&mut self) -> bool {
        if self.phase == ShutdownPhase::WaitingForAck {
            if let Some(started) = self.started_at {
                if started.elapsed() >= self.config.grace_period() {
                    self.phase = ShutdownPhase::ForceKill;
                    return true;
                }
            }
        }
        false
    }

    /// Current phase.
    #[must_use]
    pub fn phase(&self) -> &ShutdownPhase {
        &self.phase
    }

    /// Whether the sidecar has acknowledged.
    #[must_use]
    pub fn ack_received(&self) -> bool {
        self.ack_received
    }

    /// Time remaining in the grace period, or zero.
    #[must_use]
    pub fn time_remaining(&self) -> Duration {
        match self.started_at {
            Some(t) => self.config.grace_period().saturating_sub(t.elapsed()),
            None => self.config.grace_period(),
        }
    }

    /// Whether the session has reached a terminal phase.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.phase,
            ShutdownPhase::Acknowledged | ShutdownPhase::ForceKill
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ShutdownConfig {
        ShutdownConfig {
            grace_period_ms: 5000,
            graceful: true,
        }
    }

    #[test]
    fn session_initial() {
        let s = ShutdownSession::new(test_config());
        assert_eq!(*s.phase(), ShutdownPhase::Idle);
        assert!(!s.is_terminal());
    }

    #[test]
    fn begin_graceful() {
        let mut s = ShutdownSession::new(test_config());
        s.begin();
        assert_eq!(*s.phase(), ShutdownPhase::WaitingForAck);
    }

    #[test]
    fn begin_non_graceful() {
        let mut cfg = test_config();
        cfg.graceful = false;
        let mut s = ShutdownSession::new(cfg);
        s.begin();
        assert_eq!(*s.phase(), ShutdownPhase::ForceKill);
        assert!(s.is_terminal());
    }

    #[test]
    fn record_ack() {
        let mut s = ShutdownSession::new(test_config());
        s.begin();
        s.record_ack();
        assert!(s.ack_received());
        assert_eq!(*s.phase(), ShutdownPhase::Acknowledged);
        assert!(s.is_terminal());
    }

    #[test]
    fn check_timeout_not_expired() {
        let mut s = ShutdownSession::new(test_config());
        s.begin();
        assert!(!s.check_timeout());
    }

    #[test]
    fn time_remaining_before_begin() {
        let s = ShutdownSession::new(test_config());
        assert_eq!(s.time_remaining(), Duration::from_millis(5000));
    }

    #[test]
    fn config_default() {
        let cfg = ShutdownConfig::default();
        assert_eq!(cfg.grace_period_ms, 30_000);
        assert!(cfg.graceful);
    }

    #[test]
    fn config_serde_roundtrip() {
        let cfg = test_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: ShutdownConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, decoded);
    }
}
