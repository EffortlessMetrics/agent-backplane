// SPDX-License-Identifier: MIT OR Apache-2.0
//! Keep-alive heartbeat for sidecar liveness detection.
//!
//! The host periodically sends `ping` envelopes; the sidecar responds with
//! `pong`. [`HeartbeatMonitor`] tracks the round-trip and declares a sidecar
//! stalled when too many consecutive pings go unanswered.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// HeartbeatConfig
// ---------------------------------------------------------------------------

/// Configuration for heartbeat monitoring.
///
/// # Examples
///
/// ```
/// use abp_protocol::heartbeat::HeartbeatConfig;
/// use std::time::Duration;
///
/// let cfg = HeartbeatConfig::new(Duration::from_secs(10), Duration::from_secs(5), 3);
/// assert_eq!(cfg.interval(), Duration::from_secs(10));
/// assert_eq!(cfg.timeout(), Duration::from_secs(5));
/// assert_eq!(cfg.max_missed(), 3);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Interval between pings in milliseconds.
    interval_ms: u64,
    /// Timeout per individual ping in milliseconds.
    timeout_ms: u64,
    /// Maximum consecutive missed pongs before declaring stalled.
    max_missed: u32,
}

impl HeartbeatConfig {
    /// Create a new heartbeat configuration.
    #[must_use]
    pub fn new(interval: Duration, timeout: Duration, max_missed: u32) -> Self {
        Self {
            interval_ms: interval.as_millis() as u64,
            timeout_ms: timeout.as_millis() as u64,
            max_missed,
        }
    }

    /// Interval between pings.
    #[must_use]
    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }

    /// Timeout for a single ping.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    /// Max consecutive missed pongs.
    #[must_use]
    pub fn max_missed(&self) -> u32 {
        self.max_missed
    }

    /// Total duration before declaring a sidecar stalled.
    #[must_use]
    pub fn stall_threshold(&self) -> Duration {
        self.timeout() * self.max_missed
    }
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self::new(Duration::from_secs(30), Duration::from_secs(10), 3)
    }
}

// ---------------------------------------------------------------------------
// Ping / Pong
// ---------------------------------------------------------------------------

/// A ping envelope sent by the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ping {
    /// Monotonically increasing sequence number.
    pub seq: u64,
    /// Timestamp of when the ping was sent (milliseconds since epoch).
    pub timestamp_ms: u64,
}

/// A pong envelope sent by the sidecar in reply to a [`Ping`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pong {
    /// Sequence number — must match the ping's seq.
    pub seq: u64,
    /// Timestamp of when the pong was sent (milliseconds since epoch).
    pub timestamp_ms: u64,
}

// ---------------------------------------------------------------------------
// HeartbeatState
// ---------------------------------------------------------------------------

/// Current liveness state of a monitored sidecar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatState {
    /// No heartbeats sent yet.
    Idle,
    /// Normal operation — pongs arriving on time.
    Alive,
    /// One or more pongs missed but not yet at the threshold.
    Degraded {
        /// Number of consecutive missed pongs.
        missed: u32,
    },
    /// The sidecar is considered stalled (exceeded `max_missed`).
    Stalled {
        /// Number of consecutive missed pongs.
        missed: u32,
    },
}

// ---------------------------------------------------------------------------
// HeartbeatMonitor
// ---------------------------------------------------------------------------

/// Monitors heartbeat round-trips for a single sidecar.
///
/// # Examples
///
/// ```
/// use abp_protocol::heartbeat::{HeartbeatConfig, HeartbeatMonitor, HeartbeatState};
/// use std::time::Duration;
///
/// let cfg = HeartbeatConfig::new(Duration::from_secs(5), Duration::from_secs(2), 3);
/// let mut mon = HeartbeatMonitor::new(cfg);
///
/// assert_eq!(*mon.state(), HeartbeatState::Idle);
///
/// let ping = mon.next_ping();
/// assert_eq!(ping.seq, 0);
///
/// mon.record_pong(ping.seq);
/// assert_eq!(*mon.state(), HeartbeatState::Alive);
/// ```
#[derive(Debug)]
pub struct HeartbeatMonitor {
    config: HeartbeatConfig,
    state: HeartbeatState,
    next_seq: u64,
    consecutive_missed: u32,
    last_pong_at: Option<Instant>,
    last_ping_at: Option<Instant>,
    pending_ping_seq: Option<u64>,
    total_pings: u64,
    total_pongs: u64,
}

impl HeartbeatMonitor {
    /// Create a new monitor with the given configuration.
    #[must_use]
    pub fn new(config: HeartbeatConfig) -> Self {
        Self {
            config,
            state: HeartbeatState::Idle,
            next_seq: 0,
            consecutive_missed: 0,
            last_pong_at: None,
            last_ping_at: None,
            pending_ping_seq: None,
            total_pings: 0,
            total_pongs: 0,
        }
    }

    /// Current heartbeat state.
    #[must_use]
    pub fn state(&self) -> &HeartbeatState {
        &self.state
    }

    /// The configuration this monitor is using.
    #[must_use]
    pub fn config(&self) -> &HeartbeatConfig {
        &self.config
    }

    /// Generate the next ping to send.
    pub fn next_ping(&mut self) -> Ping {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.total_pings += 1;
        self.last_ping_at = Some(Instant::now());
        self.pending_ping_seq = Some(seq);

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ping { seq, timestamp_ms }
    }

    /// Record a received pong, resetting the missed counter on match.
    pub fn record_pong(&mut self, seq: u64) {
        self.total_pongs += 1;
        if self.pending_ping_seq == Some(seq) {
            self.consecutive_missed = 0;
            self.last_pong_at = Some(Instant::now());
            self.pending_ping_seq = None;
            self.state = HeartbeatState::Alive;
        }
    }

    /// Record a missed pong (timeout on the pending ping).
    pub fn record_miss(&mut self) {
        self.consecutive_missed += 1;
        self.pending_ping_seq = None;
        if self.consecutive_missed >= self.config.max_missed {
            self.state = HeartbeatState::Stalled {
                missed: self.consecutive_missed,
            };
        } else {
            self.state = HeartbeatState::Degraded {
                missed: self.consecutive_missed,
            };
        }
    }

    /// Returns `true` if the sidecar is considered stalled.
    #[must_use]
    pub fn is_stalled(&self) -> bool {
        matches!(self.state, HeartbeatState::Stalled { .. })
    }

    /// Returns `true` if the sidecar is in the `Alive` state.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.state == HeartbeatState::Alive
    }

    /// Number of consecutive missed pongs.
    #[must_use]
    pub fn consecutive_missed(&self) -> u32 {
        self.consecutive_missed
    }

    /// Total pings sent.
    #[must_use]
    pub fn total_pings(&self) -> u64 {
        self.total_pings
    }

    /// Total pongs received.
    #[must_use]
    pub fn total_pongs(&self) -> u64 {
        self.total_pongs
    }

    /// Time since the last successful pong, if any.
    #[must_use]
    pub fn time_since_last_pong(&self) -> Option<Duration> {
        self.last_pong_at.map(|t| t.elapsed())
    }

    /// Returns `true` if the pending ping has timed out.
    #[must_use]
    pub fn is_pending_timeout(&self) -> bool {
        if self.pending_ping_seq.is_none() {
            return false;
        }
        self.last_ping_at
            .is_some_and(|t| t.elapsed() >= self.config.timeout())
    }

    /// Returns `true` if enough time has elapsed since the last ping to send another.
    #[must_use]
    pub fn should_ping(&self) -> bool {
        match self.last_ping_at {
            None => true,
            Some(t) => t.elapsed() >= self.config.interval(),
        }
    }

    /// Reset the monitor to its initial state.
    pub fn reset(&mut self) {
        self.state = HeartbeatState::Idle;
        self.next_seq = 0;
        self.consecutive_missed = 0;
        self.last_pong_at = None;
        self.last_ping_at = None;
        self.pending_ping_seq = None;
        self.total_pings = 0;
        self.total_pongs = 0;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> HeartbeatConfig {
        HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3)
    }

    #[test]
    fn config_defaults() {
        let cfg = HeartbeatConfig::default();
        assert_eq!(cfg.interval(), Duration::from_secs(30));
        assert_eq!(cfg.timeout(), Duration::from_secs(10));
        assert_eq!(cfg.max_missed(), 3);
    }

    #[test]
    fn config_stall_threshold() {
        let cfg = HeartbeatConfig::new(Duration::from_secs(5), Duration::from_secs(2), 4);
        assert_eq!(cfg.stall_threshold(), Duration::from_secs(8));
    }

    #[test]
    fn monitor_initial_state() {
        let mon = HeartbeatMonitor::new(test_config());
        assert_eq!(*mon.state(), HeartbeatState::Idle);
        assert!(!mon.is_stalled());
        assert!(!mon.is_alive());
        assert_eq!(mon.total_pings(), 0);
        assert_eq!(mon.total_pongs(), 0);
        assert_eq!(mon.consecutive_missed(), 0);
    }

    #[test]
    fn ping_pong_cycle() {
        let mut mon = HeartbeatMonitor::new(test_config());
        let ping = mon.next_ping();
        assert_eq!(ping.seq, 0);
        assert_eq!(mon.total_pings(), 1);

        mon.record_pong(ping.seq);
        assert!(mon.is_alive());
        assert_eq!(mon.total_pongs(), 1);
        assert_eq!(mon.consecutive_missed(), 0);
    }

    #[test]
    fn multiple_ping_pong() {
        let mut mon = HeartbeatMonitor::new(test_config());
        for i in 0..5 {
            let ping = mon.next_ping();
            assert_eq!(ping.seq, i);
            mon.record_pong(ping.seq);
        }
        assert_eq!(mon.total_pings(), 5);
        assert_eq!(mon.total_pongs(), 5);
        assert!(mon.is_alive());
    }

    #[test]
    fn single_miss_becomes_degraded() {
        let mut mon = HeartbeatMonitor::new(test_config());
        let _ping = mon.next_ping();
        mon.record_miss();
        assert_eq!(*mon.state(), HeartbeatState::Degraded { missed: 1 });
        assert!(!mon.is_stalled());
        assert_eq!(mon.consecutive_missed(), 1);
    }

    #[test]
    fn max_misses_becomes_stalled() {
        let mut mon = HeartbeatMonitor::new(test_config());
        for _ in 0..3 {
            let _ping = mon.next_ping();
            mon.record_miss();
        }
        assert!(mon.is_stalled());
        assert_eq!(*mon.state(), HeartbeatState::Stalled { missed: 3 });
    }

    #[test]
    fn pong_resets_after_misses() {
        let mut mon = HeartbeatMonitor::new(test_config());
        // Two misses
        let _p = mon.next_ping();
        mon.record_miss();
        let _p = mon.next_ping();
        mon.record_miss();
        assert_eq!(mon.consecutive_missed(), 2);

        // Then a successful pong
        let ping = mon.next_ping();
        mon.record_pong(ping.seq);
        assert!(mon.is_alive());
        assert_eq!(mon.consecutive_missed(), 0);
    }

    #[test]
    fn wrong_seq_pong_ignored() {
        let mut mon = HeartbeatMonitor::new(test_config());
        let _ping = mon.next_ping();
        // Wrong seq number
        mon.record_pong(999);
        // State should not change to Alive because seq didn't match.
        assert_eq!(*mon.state(), HeartbeatState::Idle);
        assert_eq!(mon.total_pongs(), 1); // still counted
    }

    #[test]
    fn reset_clears_state() {
        let mut mon = HeartbeatMonitor::new(test_config());
        let ping = mon.next_ping();
        mon.record_pong(ping.seq);
        assert!(mon.is_alive());

        mon.reset();
        assert_eq!(*mon.state(), HeartbeatState::Idle);
        assert_eq!(mon.total_pings(), 0);
        assert_eq!(mon.total_pongs(), 0);
    }

    #[test]
    fn time_since_last_pong_none_initially() {
        let mon = HeartbeatMonitor::new(test_config());
        assert!(mon.time_since_last_pong().is_none());
    }

    #[test]
    fn time_since_last_pong_after_success() {
        let mut mon = HeartbeatMonitor::new(test_config());
        let ping = mon.next_ping();
        mon.record_pong(ping.seq);
        let elapsed = mon.time_since_last_pong();
        assert!(elapsed.is_some());
        assert!(elapsed.unwrap() < Duration::from_secs(1));
    }

    #[test]
    fn serde_config_round_trip() {
        let cfg = HeartbeatConfig::new(Duration::from_secs(15), Duration::from_secs(5), 2);
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: HeartbeatConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, decoded);
    }

    #[test]
    fn serde_ping_round_trip() {
        let ping = Ping {
            seq: 42,
            timestamp_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&ping).unwrap();
        let decoded: Ping = serde_json::from_str(&json).unwrap();
        assert_eq!(ping, decoded);
    }

    #[test]
    fn serde_pong_round_trip() {
        let pong = Pong {
            seq: 42,
            timestamp_ms: 1_700_000_000_001,
        };
        let json = serde_json::to_string(&pong).unwrap();
        let decoded: Pong = serde_json::from_str(&json).unwrap();
        assert_eq!(pong, decoded);
    }

    #[test]
    fn serde_state_round_trip() {
        for state in [
            HeartbeatState::Idle,
            HeartbeatState::Alive,
            HeartbeatState::Degraded { missed: 2 },
            HeartbeatState::Stalled { missed: 5 },
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let decoded: HeartbeatState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, decoded);
        }
    }

    #[test]
    fn should_ping_initially_true() {
        let mon = HeartbeatMonitor::new(test_config());
        assert!(mon.should_ping());
    }

    #[test]
    fn is_pending_timeout_false_when_no_pending() {
        let mon = HeartbeatMonitor::new(test_config());
        assert!(!mon.is_pending_timeout());
    }
}
