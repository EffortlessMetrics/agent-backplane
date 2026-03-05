// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar-side heartbeat responder.
//!
//! The host sends periodic pings; this module provides a [`PingResponder`]
//! that tracks ping sequence numbers and produces matching pong replies.

use serde::{Deserialize, Serialize};
use std::time::Duration;

// ---------------------------------------------------------------------------
// PingMessage / PongMessage
// ---------------------------------------------------------------------------

/// A ping received from the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingMessage {
    /// Monotonically increasing sequence number.
    pub seq: u64,
    /// Timestamp (ms since epoch) when the ping was sent.
    pub timestamp_ms: u64,
}

/// A pong reply sent by the sidecar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PongMessage {
    /// Must match the ping's `seq`.
    pub seq: u64,
    /// Timestamp (ms since epoch) when the pong was sent.
    pub timestamp_ms: u64,
}

// ---------------------------------------------------------------------------
// PingResponder
// ---------------------------------------------------------------------------

/// Tracks incoming pings and produces matching pong replies.
///
/// The responder also records basic statistics that the sidecar can
/// expose through its hello metadata or diagnostics endpoint.
#[derive(Debug)]
pub struct PingResponder {
    pings_received: u64,
    pongs_sent: u64,
    last_ping_seq: Option<u64>,
    max_response_time: Option<Duration>,
}

impl PingResponder {
    /// Create a new responder with zeroed counters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pings_received: 0,
            pongs_sent: 0,
            last_ping_seq: None,
            max_response_time: None,
        }
    }

    /// Handle an incoming ping and produce a pong reply.
    #[must_use]
    pub fn respond(&mut self, ping: &PingMessage) -> PongMessage {
        self.pings_received += 1;
        self.last_ping_seq = Some(ping.seq);

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let rtt = Duration::from_millis(now_ms.saturating_sub(ping.timestamp_ms));
        self.max_response_time = Some(match self.max_response_time {
            Some(prev) => prev.max(rtt),
            None => rtt,
        });

        self.pongs_sent += 1;

        PongMessage {
            seq: ping.seq,
            timestamp_ms: now_ms,
        }
    }

    /// Total pings received.
    #[must_use]
    pub fn pings_received(&self) -> u64 {
        self.pings_received
    }

    /// Total pongs sent.
    #[must_use]
    pub fn pongs_sent(&self) -> u64 {
        self.pongs_sent
    }

    /// Sequence number of the last ping received.
    #[must_use]
    pub fn last_ping_seq(&self) -> Option<u64> {
        self.last_ping_seq
    }

    /// Worst-case response time observed.
    #[must_use]
    pub fn max_response_time(&self) -> Option<Duration> {
        self.max_response_time
    }

    /// Reset all counters.
    pub fn reset(&mut self) {
        self.pings_received = 0;
        self.pongs_sent = 0;
        self.last_ping_seq = None;
        self.max_response_time = None;
    }
}

impl Default for PingResponder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ping(seq: u64) -> PingMessage {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        PingMessage {
            seq,
            timestamp_ms: ts,
        }
    }

    #[test]
    fn responder_initial_state() {
        let r = PingResponder::new();
        assert_eq!(r.pings_received(), 0);
        assert_eq!(r.pongs_sent(), 0);
        assert!(r.last_ping_seq().is_none());
        assert!(r.max_response_time().is_none());
    }

    #[test]
    fn respond_produces_matching_seq() {
        let mut r = PingResponder::new();
        let ping = make_ping(42);
        let pong = r.respond(&ping);
        assert_eq!(pong.seq, 42);
    }

    #[test]
    fn respond_increments_counters() {
        let mut r = PingResponder::new();
        r.respond(&make_ping(0));
        r.respond(&make_ping(1));
        r.respond(&make_ping(2));
        assert_eq!(r.pings_received(), 3);
        assert_eq!(r.pongs_sent(), 3);
        assert_eq!(r.last_ping_seq(), Some(2));
    }

    #[test]
    fn respond_tracks_max_response_time() {
        let mut r = PingResponder::new();
        r.respond(&make_ping(0));
        assert!(r.max_response_time().is_some());
    }

    #[test]
    fn reset_clears_state() {
        let mut r = PingResponder::new();
        r.respond(&make_ping(0));
        r.reset();
        assert_eq!(r.pings_received(), 0);
        assert_eq!(r.pongs_sent(), 0);
        assert!(r.last_ping_seq().is_none());
    }

    #[test]
    fn ping_serde_roundtrip() {
        let ping = PingMessage {
            seq: 7,
            timestamp_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&ping).unwrap();
        let decoded: PingMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(ping, decoded);
    }

    #[test]
    fn pong_serde_roundtrip() {
        let pong = PongMessage {
            seq: 7,
            timestamp_ms: 1_700_000_000_001,
        };
        let json = serde_json::to_string(&pong).unwrap();
        let decoded: PongMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(pong, decoded);
    }

    #[test]
    fn default_impl() {
        let r = PingResponder::default();
        assert_eq!(r.pings_received(), 0);
    }
}
