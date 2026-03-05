// SPDX-License-Identifier: MIT OR Apache-2.0
//! Host-side backpressure controller for sidecar event streams.
//!
//! Wraps the credit-based flow control logic: the host issues an initial
//! credit grant and tops up when the sidecar requests more. The host also
//! tracks per-run throughput to detect fast producers.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// HostBackpressureConfig
// ---------------------------------------------------------------------------

/// Configuration for host-side backpressure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostBackpressureConfig {
    /// Initial credits granted to the sidecar.
    pub initial_credits: u64,
    /// Credits to grant per refill.
    pub refill_amount: u64,
    /// Maximum events per second before the host considers the stream
    /// too fast and slows refills.
    pub max_events_per_sec: u64,
}

impl Default for HostBackpressureConfig {
    fn default() -> Self {
        Self {
            initial_credits: 256,
            refill_amount: 128,
            max_events_per_sec: 10_000,
        }
    }
}

// ---------------------------------------------------------------------------
// HostFlowController
// ---------------------------------------------------------------------------

/// Host-side flow controller for a single sidecar connection.
#[derive(Debug)]
pub struct HostFlowController {
    config: HostBackpressureConfig,
    outstanding_credits: u64,
    events_received: u64,
    started_at: Option<Instant>,
    paused: bool,
}

impl HostFlowController {
    /// Create a new controller with the given configuration.
    #[must_use]
    pub fn new(config: HostBackpressureConfig) -> Self {
        let initial = config.initial_credits;
        Self {
            config,
            outstanding_credits: initial,
            events_received: 0,
            started_at: None,
            paused: false,
        }
    }

    /// Record that a run has started (resets the throughput timer).
    pub fn on_run_start(&mut self) {
        self.started_at = Some(Instant::now());
        self.events_received = 0;
    }

    /// Record reception of one event.
    pub fn on_event(&mut self) {
        self.events_received += 1;
        self.outstanding_credits = self.outstanding_credits.saturating_sub(1);

        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
        }
    }

    /// Current events-per-second throughput.
    #[must_use]
    pub fn throughput(&self) -> f64 {
        let elapsed = self
            .started_at
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO);
        if elapsed.as_secs_f64() < 0.001 {
            return 0.0;
        }
        self.events_received as f64 / elapsed.as_secs_f64()
    }

    /// Whether the stream is producing events faster than the configured
    /// limit.
    #[must_use]
    pub fn is_over_limit(&self) -> bool {
        self.throughput() > self.config.max_events_per_sec as f64
    }

    /// Issue a refill grant. Returns `None` if the host has paused refills
    /// due to rate-limiting.
    #[must_use]
    pub fn issue_grant(&mut self) -> Option<u64> {
        if self.paused {
            return None;
        }
        let credits = self.config.refill_amount;
        self.outstanding_credits += credits;
        Some(credits)
    }

    /// Pause credit issuance (e.g. because of back-pressure).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume credit issuance.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Whether the controller is currently paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Outstanding credits the sidecar is believed to hold.
    #[must_use]
    pub fn outstanding_credits(&self) -> u64 {
        self.outstanding_credits
    }

    /// Total events received.
    #[must_use]
    pub fn events_received(&self) -> u64 {
        self.events_received
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> HostBackpressureConfig {
        HostBackpressureConfig {
            initial_credits: 10,
            refill_amount: 5,
            max_events_per_sec: 1000,
        }
    }

    #[test]
    fn initial_state() {
        let c = HostFlowController::new(test_config());
        assert_eq!(c.outstanding_credits(), 10);
        assert_eq!(c.events_received(), 0);
        assert!(!c.is_paused());
    }

    #[test]
    fn on_event_decrements() {
        let mut c = HostFlowController::new(test_config());
        c.on_event();
        assert_eq!(c.outstanding_credits(), 9);
        assert_eq!(c.events_received(), 1);
    }

    #[test]
    fn issue_grant_adds_credits() {
        let mut c = HostFlowController::new(test_config());
        let credits = c.issue_grant().unwrap();
        assert_eq!(credits, 5);
        assert_eq!(c.outstanding_credits(), 15);
    }

    #[test]
    fn pause_blocks_grants() {
        let mut c = HostFlowController::new(test_config());
        c.pause();
        assert!(c.issue_grant().is_none());
        c.resume();
        assert!(c.issue_grant().is_some());
    }

    #[test]
    fn throughput_zero_initially() {
        let c = HostFlowController::new(test_config());
        assert!(c.throughput() < f64::EPSILON);
    }

    #[test]
    fn config_default() {
        let cfg = HostBackpressureConfig::default();
        assert_eq!(cfg.initial_credits, 256);
        assert_eq!(cfg.refill_amount, 128);
        assert_eq!(cfg.max_events_per_sec, 10_000);
    }

    #[test]
    fn config_serde_roundtrip() {
        let cfg = test_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: HostBackpressureConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, decoded);
    }
}
