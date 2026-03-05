// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol health utilities — heartbeat, timeout detection, and graceful
//! shutdown signaling.

use std::time::{Duration, Instant};

use tokio::sync::watch;

/// Default heartbeat interval (30 seconds).
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Default connection timeout (90 seconds — 3 missed heartbeats).
pub const DEFAULT_CONNECTION_TIMEOUT: Duration = Duration::from_secs(90);

/// Tracks the liveness of a protocol connection via heartbeats.
///
/// # Examples
///
/// ```
/// use abp_sidecar_utils::health::ProtocolHealth;
/// use std::time::Duration;
///
/// let mut health = ProtocolHealth::new(
///     Duration::from_secs(30),
///     Duration::from_secs(90),
/// );
/// assert!(!health.is_timed_out());
///
/// health.record_heartbeat();
/// assert!(!health.is_timed_out());
/// ```
#[derive(Debug)]
pub struct ProtocolHealth {
    heartbeat_interval: Duration,
    connection_timeout: Duration,
    last_heartbeat: Instant,
    heartbeats_received: u64,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl ProtocolHealth {
    /// Create a new health tracker with the given heartbeat interval and
    /// connection timeout.
    pub fn new(heartbeat_interval: Duration, connection_timeout: Duration) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            heartbeat_interval,
            connection_timeout,
            last_heartbeat: Instant::now(),
            heartbeats_received: 0,
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Create a health tracker with default settings (30s heartbeat, 90s timeout).
    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_HEARTBEAT_INTERVAL, DEFAULT_CONNECTION_TIMEOUT)
    }

    /// Record that a heartbeat (or any protocol activity) was received.
    pub fn record_heartbeat(&mut self) {
        self.last_heartbeat = Instant::now();
        self.heartbeats_received += 1;
    }

    /// Time elapsed since the last heartbeat.
    #[must_use]
    pub fn time_since_last_heartbeat(&self) -> Duration {
        self.last_heartbeat.elapsed()
    }

    /// Returns `true` if the connection has timed out (no heartbeat within
    /// the configured timeout).
    #[must_use]
    pub fn is_timed_out(&self) -> bool {
        self.last_heartbeat.elapsed() > self.connection_timeout
    }

    /// Returns `true` if a heartbeat is overdue (past the interval but not
    /// yet timed out).
    #[must_use]
    pub fn is_heartbeat_overdue(&self) -> bool {
        let elapsed = self.last_heartbeat.elapsed();
        elapsed > self.heartbeat_interval && elapsed <= self.connection_timeout
    }

    /// Total heartbeats recorded since creation.
    #[must_use]
    pub fn heartbeats_received(&self) -> u64 {
        self.heartbeats_received
    }

    /// Configured heartbeat interval.
    #[must_use]
    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }

    /// Configured connection timeout.
    #[must_use]
    pub fn connection_timeout(&self) -> Duration {
        self.connection_timeout
    }

    /// Signal a graceful shutdown.
    ///
    /// All receivers obtained via [`shutdown_receiver`](Self::shutdown_receiver)
    /// will be notified.
    pub fn signal_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Returns `true` if shutdown has been signalled.
    #[must_use]
    pub fn is_shutdown_signalled(&self) -> bool {
        *self.shutdown_rx.borrow()
    }

    /// Obtain a receiver that resolves when shutdown is signalled.
    ///
    /// Useful for `tokio::select!` loops.
    #[must_use]
    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_rx.clone()
    }

    /// Wait asynchronously until a heartbeat is due (sleeps for the
    /// remaining time in the current heartbeat interval).
    pub async fn wait_for_next_heartbeat(&self) {
        let elapsed = self.last_heartbeat.elapsed();
        if elapsed < self.heartbeat_interval {
            tokio::time::sleep(self.heartbeat_interval - elapsed).await;
        }
    }
}

impl Default for ProtocolHealth {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let h = ProtocolHealth::with_defaults();
        assert!(!h.is_timed_out());
        assert!(!h.is_heartbeat_overdue());
        assert_eq!(h.heartbeats_received(), 0);
        assert!(!h.is_shutdown_signalled());
    }

    #[test]
    fn record_heartbeat_updates() {
        let mut h = ProtocolHealth::with_defaults();
        h.record_heartbeat();
        assert_eq!(h.heartbeats_received(), 1);
        h.record_heartbeat();
        assert_eq!(h.heartbeats_received(), 2);
    }

    #[test]
    fn timeout_detection() {
        let h = ProtocolHealth::new(Duration::from_millis(10), Duration::from_millis(1));
        // Immediately after creation the timeout might already be exceeded
        // because Instant::now() moves forward during test execution.
        // Use a very short timeout and a small sleep to reliably trigger.
        std::thread::sleep(Duration::from_millis(5));
        assert!(h.is_timed_out());
    }

    #[test]
    fn heartbeat_resets_timeout() {
        let mut h = ProtocolHealth::new(Duration::from_secs(60), Duration::from_secs(120));
        assert!(!h.is_timed_out());
        h.record_heartbeat();
        assert!(!h.is_timed_out());
    }

    #[test]
    fn shutdown_signaling() {
        let h = ProtocolHealth::with_defaults();
        assert!(!h.is_shutdown_signalled());
        let mut rx = h.shutdown_receiver();
        h.signal_shutdown();
        assert!(h.is_shutdown_signalled());
        // The receiver should have the new value.
        assert!(*rx.borrow_and_update());
    }

    #[test]
    fn config_accessors() {
        let h = ProtocolHealth::new(Duration::from_secs(15), Duration::from_secs(45));
        assert_eq!(h.heartbeat_interval(), Duration::from_secs(15));
        assert_eq!(h.connection_timeout(), Duration::from_secs(45));
    }

    #[test]
    fn default_trait() {
        let h = ProtocolHealth::default();
        assert_eq!(h.heartbeat_interval(), DEFAULT_HEARTBEAT_INTERVAL);
        assert_eq!(h.connection_timeout(), DEFAULT_CONNECTION_TIMEOUT);
    }

    #[tokio::test]
    async fn wait_for_next_heartbeat_returns() {
        let h = ProtocolHealth::new(Duration::from_millis(10), Duration::from_secs(60));
        // Should return quickly since the interval is only 10ms.
        h.wait_for_next_heartbeat().await;
    }
}
