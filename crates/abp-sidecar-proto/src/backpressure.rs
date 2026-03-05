// SPDX-License-Identifier: MIT OR Apache-2.0
//! Credit-based backpressure for high-throughput event streams.
//!
//! The host grants the sidecar a number of *credits* — each credit permits
//! one event envelope to be sent. When the sidecar exhausts its credits it
//! must pause until the host grants more. This prevents the host from being
//! overwhelmed by fast-producing sidecars.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CreditGrant / CreditRequest
// ---------------------------------------------------------------------------

/// Sent by the host to grant additional send credits to the sidecar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditGrant {
    /// Number of additional credits being granted.
    pub credits: u64,
}

/// Sent by the sidecar to request more credits from the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditRequest {
    /// Number of credits requested.
    pub requested: u64,
}

// ---------------------------------------------------------------------------
// BackpressureConfig
// ---------------------------------------------------------------------------

/// Configuration for backpressure behaviour.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackpressureConfig {
    /// Initial number of credits granted on connection.
    pub initial_credits: u64,
    /// Low-water mark: the sidecar should request more credits when its
    /// available credits fall to this level.
    pub low_watermark: u64,
    /// Number of credits to request at a time.
    pub refill_amount: u64,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            initial_credits: 256,
            low_watermark: 64,
            refill_amount: 128,
        }
    }
}

// ---------------------------------------------------------------------------
// CreditWindow (sidecar side)
// ---------------------------------------------------------------------------

/// Tracks available send credits on the sidecar side.
///
/// Each call to [`try_consume`](Self::try_consume) decrements the credit
/// counter. When credits reach zero the sidecar must wait for a
/// [`CreditGrant`] from the host.
#[derive(Debug)]
pub struct CreditWindow {
    available: u64,
    total_consumed: u64,
    total_granted: u64,
    config: BackpressureConfig,
}

impl CreditWindow {
    /// Create a new window with the configured initial credits.
    #[must_use]
    pub fn new(config: BackpressureConfig) -> Self {
        let initial = config.initial_credits;
        Self {
            available: initial,
            total_consumed: 0,
            total_granted: initial,
            config,
        }
    }

    /// Try to consume one credit. Returns `true` if a credit was available.
    pub fn try_consume(&mut self) -> bool {
        if self.available > 0 {
            self.available -= 1;
            self.total_consumed += 1;
            true
        } else {
            false
        }
    }

    /// Apply a credit grant from the host.
    pub fn grant(&mut self, grant: &CreditGrant) {
        self.available += grant.credits;
        self.total_granted += grant.credits;
    }

    /// Currently available credits.
    #[must_use]
    pub fn available(&self) -> u64 {
        self.available
    }

    /// Total credits consumed since creation.
    #[must_use]
    pub fn total_consumed(&self) -> u64 {
        self.total_consumed
    }

    /// Total credits granted (including initial) since creation.
    #[must_use]
    pub fn total_granted(&self) -> u64 {
        self.total_granted
    }

    /// Returns `true` if the sidecar should request more credits.
    #[must_use]
    pub fn needs_refill(&self) -> bool {
        self.available <= self.config.low_watermark
    }

    /// Build a [`CreditRequest`] using the configured refill amount.
    #[must_use]
    pub fn make_request(&self) -> CreditRequest {
        CreditRequest {
            requested: self.config.refill_amount,
        }
    }

    /// Reset the window to its initial state.
    pub fn reset(&mut self) {
        self.available = self.config.initial_credits;
        self.total_consumed = 0;
        self.total_granted = self.config.initial_credits;
    }
}

// ---------------------------------------------------------------------------
// BackpressureController (host side)
// ---------------------------------------------------------------------------

/// Host-side controller that decides when to issue credit grants.
#[derive(Debug)]
pub struct BackpressureController {
    config: BackpressureConfig,
    outstanding: u64,
    total_granted: u64,
    total_events_received: u64,
}

impl BackpressureController {
    /// Create a new controller with the given config. The initial credits
    /// are pre-loaded into `outstanding`.
    #[must_use]
    pub fn new(config: BackpressureConfig) -> Self {
        let initial = config.initial_credits;
        Self {
            config,
            outstanding: initial,
            total_granted: initial,
            total_events_received: 0,
        }
    }

    /// Record that the host received one event from the sidecar.
    pub fn record_event(&mut self) {
        self.outstanding = self.outstanding.saturating_sub(1);
        self.total_events_received += 1;
    }

    /// Process a credit request from the sidecar and return a grant.
    #[must_use]
    pub fn handle_request(&mut self, _req: &CreditRequest) -> CreditGrant {
        let credits = self.config.refill_amount;
        self.outstanding += credits;
        self.total_granted += credits;
        CreditGrant { credits }
    }

    /// Outstanding credits the sidecar is believed to hold.
    #[must_use]
    pub fn outstanding(&self) -> u64 {
        self.outstanding
    }

    /// Total credits granted since creation.
    #[must_use]
    pub fn total_granted(&self) -> u64 {
        self.total_granted
    }

    /// Total events received from the sidecar.
    #[must_use]
    pub fn total_events_received(&self) -> u64 {
        self.total_events_received
    }

    /// Returns `true` if the sidecar has likely exhausted its credits.
    #[must_use]
    pub fn is_starved(&self) -> bool {
        self.outstanding == 0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BackpressureConfig {
        BackpressureConfig {
            initial_credits: 10,
            low_watermark: 3,
            refill_amount: 5,
        }
    }

    #[test]
    fn credit_window_initial() {
        let w = CreditWindow::new(test_config());
        assert_eq!(w.available(), 10);
        assert_eq!(w.total_consumed(), 0);
        assert_eq!(w.total_granted(), 10);
    }

    #[test]
    fn credit_window_consume() {
        let mut w = CreditWindow::new(test_config());
        assert!(w.try_consume());
        assert_eq!(w.available(), 9);
        assert_eq!(w.total_consumed(), 1);
    }

    #[test]
    fn credit_window_exhaustion() {
        let mut w = CreditWindow::new(test_config());
        for _ in 0..10 {
            assert!(w.try_consume());
        }
        assert!(!w.try_consume());
        assert_eq!(w.available(), 0);
    }

    #[test]
    fn credit_window_grant() {
        let mut w = CreditWindow::new(test_config());
        for _ in 0..10 {
            w.try_consume();
        }
        w.grant(&CreditGrant { credits: 5 });
        assert_eq!(w.available(), 5);
        assert_eq!(w.total_granted(), 15);
    }

    #[test]
    fn credit_window_needs_refill() {
        let mut w = CreditWindow::new(test_config());
        // Consume until low_watermark
        for _ in 0..7 {
            w.try_consume();
        }
        assert_eq!(w.available(), 3);
        assert!(w.needs_refill());
    }

    #[test]
    fn credit_window_make_request() {
        let w = CreditWindow::new(test_config());
        let req = w.make_request();
        assert_eq!(req.requested, 5);
    }

    #[test]
    fn credit_window_reset() {
        let mut w = CreditWindow::new(test_config());
        w.try_consume();
        w.reset();
        assert_eq!(w.available(), 10);
        assert_eq!(w.total_consumed(), 0);
    }

    #[test]
    fn backpressure_controller_initial() {
        let c = BackpressureController::new(test_config());
        assert_eq!(c.outstanding(), 10);
        assert_eq!(c.total_granted(), 10);
        assert_eq!(c.total_events_received(), 0);
    }

    #[test]
    fn backpressure_controller_record_event() {
        let mut c = BackpressureController::new(test_config());
        c.record_event();
        assert_eq!(c.outstanding(), 9);
        assert_eq!(c.total_events_received(), 1);
    }

    #[test]
    fn backpressure_controller_handle_request() {
        let mut c = BackpressureController::new(test_config());
        for _ in 0..10 {
            c.record_event();
        }
        assert!(c.is_starved());
        let req = CreditRequest { requested: 5 };
        let grant = c.handle_request(&req);
        assert_eq!(grant.credits, 5);
        assert_eq!(c.outstanding(), 5);
        assert!(!c.is_starved());
    }

    #[test]
    fn backpressure_config_default() {
        let cfg = BackpressureConfig::default();
        assert_eq!(cfg.initial_credits, 256);
        assert_eq!(cfg.low_watermark, 64);
        assert_eq!(cfg.refill_amount, 128);
    }

    #[test]
    fn credit_grant_serde() {
        let g = CreditGrant { credits: 42 };
        let json = serde_json::to_string(&g).unwrap();
        let decoded: CreditGrant = serde_json::from_str(&json).unwrap();
        assert_eq!(g, decoded);
    }

    #[test]
    fn credit_request_serde() {
        let r = CreditRequest { requested: 100 };
        let json = serde_json::to_string(&r).unwrap();
        let decoded: CreditRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(r, decoded);
    }

    #[test]
    fn backpressure_config_serde() {
        let cfg = test_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: BackpressureConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, decoded);
    }
}
