// SPDX-License-Identifier: MIT OR Apache-2.0
//! Graceful shutdown protocol for sidecars.
//!
//! The host sends a [`ShutdownRequest`] envelope to ask the sidecar to
//! wind down. The sidecar has a configurable deadline to finish in-flight
//! work and respond with a [`GoodbyeResponse`]. If the deadline expires
//! the host may forcibly terminate the process.

use serde::{Deserialize, Serialize};
use std::time::Duration;

// ---------------------------------------------------------------------------
// ShutdownReason
// ---------------------------------------------------------------------------

/// Why the host is requesting a shutdown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownReason {
    /// Normal shutdown (e.g. user request, run complete).
    Normal,
    /// The sidecar exceeded a resource limit.
    ResourceLimit,
    /// The sidecar is being replaced by a new instance.
    Replacement,
    /// The host is shutting down entirely.
    HostShutdown,
    /// A policy violation was detected.
    PolicyViolation,
    /// A custom reason with a description.
    Custom(String),
}

// ---------------------------------------------------------------------------
// ShutdownRequest
// ---------------------------------------------------------------------------

/// A request from the host for the sidecar to shut down gracefully.
///
/// # Examples
///
/// ```
/// use abp_protocol::graceful_shutdown::{ShutdownRequest, ShutdownReason};
/// use std::time::Duration;
///
/// let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(30));
/// assert_eq!(req.reason(), &ShutdownReason::Normal);
/// assert_eq!(req.deadline(), Duration::from_secs(30));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownRequest {
    /// Reason for the shutdown.
    reason: ShutdownReason,
    /// Duration in milliseconds the sidecar has to finish before force-kill.
    deadline_ms: u64,
    /// Optional human-readable message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl ShutdownRequest {
    /// Create a new shutdown request with the given reason and deadline.
    #[must_use]
    pub fn new(reason: ShutdownReason, deadline: Duration) -> Self {
        Self {
            reason,
            deadline_ms: deadline.as_millis() as u64,
            message: None,
        }
    }

    /// Create a shutdown request with an additional human-readable message.
    #[must_use]
    pub fn with_message(mut self, msg: impl Into<String>) -> Self {
        self.message = Some(msg.into());
        self
    }

    /// The reason for this shutdown.
    #[must_use]
    pub fn reason(&self) -> &ShutdownReason {
        &self.reason
    }

    /// The deadline as a [`Duration`].
    #[must_use]
    pub fn deadline(&self) -> Duration {
        Duration::from_millis(self.deadline_ms)
    }

    /// Optional message from the host.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Returns `true` if the deadline has been exceeded given elapsed time.
    #[must_use]
    pub fn is_expired(&self, elapsed: Duration) -> bool {
        elapsed >= self.deadline()
    }
}

// ---------------------------------------------------------------------------
// GoodbyeStatus
// ---------------------------------------------------------------------------

/// How the sidecar concluded after receiving a shutdown request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoodbyeStatus {
    /// All in-flight work completed successfully.
    Clean,
    /// Shutdown occurred but some work was abandoned.
    Partial,
    /// An error occurred during shutdown.
    Error,
}

// ---------------------------------------------------------------------------
// GoodbyeResponse
// ---------------------------------------------------------------------------

/// The sidecar's response to a shutdown request.
///
/// # Examples
///
/// ```
/// use abp_protocol::graceful_shutdown::{GoodbyeResponse, GoodbyeStatus};
///
/// let resp = GoodbyeResponse::new(GoodbyeStatus::Clean);
/// assert_eq!(resp.status(), &GoodbyeStatus::Clean);
/// assert!(resp.is_clean());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoodbyeResponse {
    /// Final status of the shutdown process.
    status: GoodbyeStatus,
    /// Number of in-flight requests that were completed.
    #[serde(default)]
    completed_requests: u64,
    /// Number of in-flight requests that were abandoned.
    #[serde(default)]
    abandoned_requests: u64,
    /// Optional error message if status is `Error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl GoodbyeResponse {
    /// Create a goodbye with the given status.
    #[must_use]
    pub fn new(status: GoodbyeStatus) -> Self {
        Self {
            status,
            completed_requests: 0,
            abandoned_requests: 0,
            error: None,
        }
    }

    /// Set the number of completed in-flight requests.
    #[must_use]
    pub fn with_completed(mut self, n: u64) -> Self {
        self.completed_requests = n;
        self
    }

    /// Set the number of abandoned in-flight requests.
    #[must_use]
    pub fn with_abandoned(mut self, n: u64) -> Self {
        self.abandoned_requests = n;
        self
    }

    /// Set an error message.
    #[must_use]
    pub fn with_error(mut self, msg: impl Into<String>) -> Self {
        self.error = Some(msg.into());
        self
    }

    /// The goodbye status.
    #[must_use]
    pub fn status(&self) -> &GoodbyeStatus {
        &self.status
    }

    /// Number of completed in-flight requests.
    #[must_use]
    pub fn completed_requests(&self) -> u64 {
        self.completed_requests
    }

    /// Number of abandoned in-flight requests.
    #[must_use]
    pub fn abandoned_requests(&self) -> u64 {
        self.abandoned_requests
    }

    /// Error message, if any.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// `true` if the shutdown was clean.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.status == GoodbyeStatus::Clean
    }
}

// ---------------------------------------------------------------------------
// ShutdownCoordinator
// ---------------------------------------------------------------------------

/// Tracks the state of a graceful shutdown exchange.
#[derive(Debug)]
pub struct ShutdownCoordinator {
    request: ShutdownRequest,
    response: Option<GoodbyeResponse>,
    initiated_at: std::time::Instant,
}

impl ShutdownCoordinator {
    /// Start a shutdown sequence with the given request.
    #[must_use]
    pub fn new(request: ShutdownRequest) -> Self {
        Self {
            request,
            response: None,
            initiated_at: std::time::Instant::now(),
        }
    }

    /// The active shutdown request.
    #[must_use]
    pub fn request(&self) -> &ShutdownRequest {
        &self.request
    }

    /// Record the sidecar's goodbye response.
    pub fn record_response(&mut self, response: GoodbyeResponse) {
        self.response = Some(response);
    }

    /// The goodbye response, if received.
    #[must_use]
    pub fn response(&self) -> Option<&GoodbyeResponse> {
        self.response.as_ref()
    }

    /// How long since the shutdown was initiated.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.initiated_at.elapsed()
    }

    /// Returns `true` if the deadline has been exceeded without a response.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.response.is_none() && self.request.is_expired(self.elapsed())
    }

    /// Returns `true` if a goodbye response has been received.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.response.is_some()
    }

    /// Time remaining until the deadline, or zero if expired.
    #[must_use]
    pub fn time_remaining(&self) -> Duration {
        self.request.deadline().saturating_sub(self.elapsed())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_request_basic() {
        let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(10));
        assert_eq!(req.reason(), &ShutdownReason::Normal);
        assert_eq!(req.deadline(), Duration::from_secs(10));
        assert!(req.message().is_none());
    }

    #[test]
    fn shutdown_request_with_message() {
        let req = ShutdownRequest::new(ShutdownReason::HostShutdown, Duration::from_secs(5))
            .with_message("bye");
        assert_eq!(req.message(), Some("bye"));
    }

    #[test]
    fn shutdown_request_is_expired() {
        let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_millis(10));
        assert!(!req.is_expired(Duration::from_millis(5)));
        assert!(req.is_expired(Duration::from_millis(10)));
        assert!(req.is_expired(Duration::from_millis(15)));
    }

    #[test]
    fn goodbye_response_clean() {
        let resp = GoodbyeResponse::new(GoodbyeStatus::Clean);
        assert!(resp.is_clean());
        assert_eq!(resp.completed_requests(), 0);
        assert_eq!(resp.abandoned_requests(), 0);
        assert!(resp.error().is_none());
    }

    #[test]
    fn goodbye_response_partial() {
        let resp = GoodbyeResponse::new(GoodbyeStatus::Partial)
            .with_completed(3)
            .with_abandoned(1);
        assert!(!resp.is_clean());
        assert_eq!(resp.completed_requests(), 3);
        assert_eq!(resp.abandoned_requests(), 1);
    }

    #[test]
    fn goodbye_response_with_error() {
        let resp = GoodbyeResponse::new(GoodbyeStatus::Error).with_error("disk full");
        assert_eq!(resp.error(), Some("disk full"));
        assert!(!resp.is_clean());
    }

    #[test]
    fn shutdown_coordinator_lifecycle() {
        let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(60));
        let mut coord = ShutdownCoordinator::new(req);

        assert!(!coord.is_complete());
        assert!(!coord.is_expired());

        coord.record_response(GoodbyeResponse::new(GoodbyeStatus::Clean));
        assert!(coord.is_complete());
        assert!(!coord.is_expired()); // response received, never expires
        assert!(coord.response().unwrap().is_clean());
    }

    #[test]
    fn shutdown_coordinator_time_remaining() {
        let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(60));
        let coord = ShutdownCoordinator::new(req);
        // Time remaining should be close to 60s (within tolerance).
        assert!(coord.time_remaining() > Duration::from_secs(59));
    }

    #[test]
    fn serde_shutdown_request_round_trip() {
        let req = ShutdownRequest::new(ShutdownReason::ResourceLimit, Duration::from_secs(30))
            .with_message("memory exceeded");
        let json = serde_json::to_string(&req).unwrap();
        let decoded: ShutdownRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn serde_goodbye_response_round_trip() {
        let resp = GoodbyeResponse::new(GoodbyeStatus::Partial)
            .with_completed(5)
            .with_abandoned(2)
            .with_error("timeout");
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: GoodbyeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, decoded);
    }

    #[test]
    fn serde_shutdown_reason_variants() {
        for reason in [
            ShutdownReason::Normal,
            ShutdownReason::ResourceLimit,
            ShutdownReason::Replacement,
            ShutdownReason::HostShutdown,
            ShutdownReason::PolicyViolation,
            ShutdownReason::Custom("test".into()),
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let decoded: ShutdownReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, decoded);
        }
    }

    #[test]
    fn serde_goodbye_status_variants() {
        for status in [
            GoodbyeStatus::Clean,
            GoodbyeStatus::Partial,
            GoodbyeStatus::Error,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: GoodbyeStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, decoded);
        }
    }

    #[test]
    fn shutdown_request_json_structure() {
        let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(30));
        let v: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(v["reason"], "normal");
        assert_eq!(v["deadline_ms"], 30_000);
        assert!(v.get("message").is_none());
    }
}
