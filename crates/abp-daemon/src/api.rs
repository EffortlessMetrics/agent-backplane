// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP control-plane API types and handler signatures.
//!
//! This module defines the request/response envelopes, resource
//! representations, and error types used by the daemon REST API.

use abp_core::{CapabilityManifest, WorkOrder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Run lifecycle
// ---------------------------------------------------------------------------

/// API-facing run status (no embedded payloads).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// The run is waiting in the queue.
    Queued,
    /// The run is currently executing.
    Running,
    /// The run completed successfully.
    Completed,
    /// The run failed.
    Failed,
    /// The run was cancelled by a user request.
    Cancelled,
}

impl RunStatus {
    /// Returns `true` if this status represents a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Returns the set of statuses that are valid successors of `self`.
    pub fn valid_transitions(&self) -> &'static [RunStatus] {
        match self {
            Self::Queued => &[Self::Running, Self::Cancelled],
            Self::Running => &[Self::Completed, Self::Failed, Self::Cancelled],
            Self::Completed | Self::Failed | Self::Cancelled => &[],
        }
    }

    /// Returns `true` if transitioning from `self` to `next` is valid.
    pub fn can_transition_to(&self, next: RunStatus) -> bool {
        self.valid_transitions().contains(&next)
    }
}

/// Summary information about a tracked run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunInfo {
    /// Unique run identifier.
    pub id: Uuid,
    /// Current status of the run.
    pub status: RunStatus,
    /// Backend that is (or was) executing the run.
    pub backend: String,
    /// Timestamp when the run was created.
    pub created_at: DateTime<Utc>,
    /// Number of events emitted so far.
    pub events_count: usize,
}

// ---------------------------------------------------------------------------
// API request / response envelopes
// ---------------------------------------------------------------------------

/// Discriminated union of all API request bodies.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiRequest {
    /// Submit a new work order (`POST /runs`).
    SubmitRun {
        /// Target backend name.
        backend: String,
        /// The work order to execute.
        work_order: Box<WorkOrder>,
    },
    /// Cancel a running work order (`POST /runs/{id}/cancel`).
    CancelRun {
        /// Run identifier.
        run_id: Uuid,
    },
}

/// Discriminated union of all successful API response bodies.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiResponse {
    /// Response to `POST /runs`.
    RunCreated {
        /// Newly assigned run identifier.
        run_id: Uuid,
    },
    /// Response to `GET /runs/{id}`.
    RunDetails {
        /// Run information.
        run: RunInfo,
    },
    /// Response to `GET /backends`.
    BackendList {
        /// Available backends with capabilities.
        backends: Vec<BackendDetail>,
    },
    /// Response to `GET /health`.
    Health(HealthResponse),
    /// Response to `POST /runs/{id}/cancel`.
    RunCancelled {
        /// The cancelled run identifier.
        run_id: Uuid,
    },
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Response body for `GET /health`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Server status (e.g. `"ok"`).
    pub status: String,
    /// Contract version reported by the server.
    pub version: String,
    /// Server uptime in seconds.
    pub uptime_seconds: u64,
    /// Number of registered backends.
    pub backends_count: usize,
}

// ---------------------------------------------------------------------------
// Backends
// ---------------------------------------------------------------------------

/// Extended backend information returned by `GET /backends`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackendDetail {
    /// Backend identifier.
    pub id: String,
    /// Capability manifest reported by this backend.
    pub capabilities: CapabilityManifest,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Structured API error returned on failure.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    /// Machine-readable error code (e.g. `"not_found"`, `"invalid_request"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// Create an error with no additional details.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// Attach additional details to this error.
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    // -- Convenience constructors for stable error codes ---------------------

    /// 404 — resource not found.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    /// 400 — the request was malformed or invalid.
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }

    /// 409 — conflicting state (e.g. cancelling a completed run).
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("conflict", message)
    }

    /// 500 — unexpected internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

// ---------------------------------------------------------------------------
// Handler signatures (types only — no actual HTTP server wiring)
// ---------------------------------------------------------------------------

/// Handler signature: `POST /runs` — submit a work order.
///
/// Accepts [`ApiRequest::SubmitRun`] and returns [`ApiResponse::RunCreated`]
/// on success or [`ApiError`] on failure.
pub type SubmitRunHandler = fn(request: ApiRequest) -> Result<ApiResponse, ApiError>;

/// Handler signature: `GET /runs/{id}` — get run status and events.
pub type GetRunHandler = fn(run_id: Uuid) -> Result<ApiResponse, ApiError>;

/// Handler signature: `GET /runs/{id}/receipt` — get run receipt.
pub type GetRunReceiptHandler = fn(run_id: Uuid) -> Result<serde_json::Value, ApiError>;

/// Handler signature: `GET /backends` — list available backends.
pub type ListBackendsHandler = fn() -> Result<ApiResponse, ApiError>;

/// Handler signature: `GET /health` — health check.
pub type HealthCheckHandler = fn() -> Result<ApiResponse, ApiError>;

/// Handler signature: `POST /runs/{id}/cancel` — cancel a running work order.
pub type CancelRunHandler = fn(run_id: Uuid) -> Result<ApiResponse, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    // -----------------------------------------------------------------------
    // RunStatus serde roundtrips
    // -----------------------------------------------------------------------

    #[test]
    fn run_status_serde_roundtrip_all_variants() {
        for status in [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: RunStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn run_status_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&RunStatus::Queued).unwrap(),
            "\"queued\""
        );
        assert_eq!(
            serde_json::to_string(&RunStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    // -----------------------------------------------------------------------
    // RunStatus transitions
    // -----------------------------------------------------------------------

    #[test]
    fn queued_can_transition_to_running() {
        assert!(RunStatus::Queued.can_transition_to(RunStatus::Running));
    }

    #[test]
    fn queued_can_transition_to_cancelled() {
        assert!(RunStatus::Queued.can_transition_to(RunStatus::Cancelled));
    }

    #[test]
    fn running_can_transition_to_completed() {
        assert!(RunStatus::Running.can_transition_to(RunStatus::Completed));
    }

    #[test]
    fn running_can_transition_to_failed() {
        assert!(RunStatus::Running.can_transition_to(RunStatus::Failed));
    }

    #[test]
    fn running_can_transition_to_cancelled() {
        assert!(RunStatus::Running.can_transition_to(RunStatus::Cancelled));
    }

    #[test]
    fn terminal_states_have_no_transitions() {
        for status in [
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            assert!(status.valid_transitions().is_empty());
            assert!(status.is_terminal());
        }
    }

    #[test]
    fn non_terminal_states_are_not_terminal() {
        assert!(!RunStatus::Queued.is_terminal());
        assert!(!RunStatus::Running.is_terminal());
    }

    #[test]
    fn invalid_transition_rejected() {
        assert!(!RunStatus::Queued.can_transition_to(RunStatus::Completed));
        assert!(!RunStatus::Completed.can_transition_to(RunStatus::Running));
        assert!(!RunStatus::Failed.can_transition_to(RunStatus::Running));
    }

    // -----------------------------------------------------------------------
    // RunInfo serde roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn run_info_serde_roundtrip() {
        let info = RunInfo {
            id: Uuid::nil(),
            status: RunStatus::Running,
            backend: "mock".into(),
            created_at: Utc::now(),
            events_count: 42,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: RunInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, info.id);
        assert_eq!(back.status, info.status);
        assert_eq!(back.backend, info.backend);
        assert_eq!(back.events_count, 42);
    }

    // -----------------------------------------------------------------------
    // HealthResponse
    // -----------------------------------------------------------------------

    #[test]
    fn health_response_serde_roundtrip() {
        let resp = HealthResponse {
            status: "ok".into(),
            version: abp_core::CONTRACT_VERSION.into(),
            uptime_seconds: 123,
            backends_count: 3,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, "ok");
        assert_eq!(back.version, abp_core::CONTRACT_VERSION);
        assert_eq!(back.uptime_seconds, 123);
        assert_eq!(back.backends_count, 3);
    }

    #[test]
    fn health_response_includes_version() {
        let resp = HealthResponse {
            status: "ok".into(),
            version: abp_core::CONTRACT_VERSION.into(),
            uptime_seconds: 0,
            backends_count: 0,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("version").is_some());
        assert_eq!(json["version"], abp_core::CONTRACT_VERSION);
    }

    // -----------------------------------------------------------------------
    // ApiError
    // -----------------------------------------------------------------------

    #[test]
    fn api_error_serde_roundtrip() {
        let err = ApiError::not_found("run xyz not found");
        let json = serde_json::to_string(&err).unwrap();
        let back: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, "not_found");
        assert_eq!(back.message, "run xyz not found");
        assert!(back.details.is_none());
    }

    #[test]
    fn api_error_stable_codes() {
        assert_eq!(ApiError::not_found("x").code, "not_found");
        assert_eq!(ApiError::invalid_request("x").code, "invalid_request");
        assert_eq!(ApiError::conflict("x").code, "conflict");
        assert_eq!(ApiError::internal("x").code, "internal_error");
    }

    #[test]
    fn api_error_with_details() {
        let err =
            ApiError::invalid_request("bad field").with_details(serde_json::json!({"field": "id"}));
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["details"]["field"], "id");
    }

    #[test]
    fn api_error_omits_null_details() {
        let err = ApiError::not_found("gone");
        let json = serde_json::to_value(&err).unwrap();
        assert!(json.get("details").is_none());
    }

    // -----------------------------------------------------------------------
    // ApiRequest / ApiResponse serde roundtrips
    // -----------------------------------------------------------------------

    #[test]
    fn api_request_cancel_roundtrip() {
        let req = ApiRequest::CancelRun {
            run_id: Uuid::nil(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ApiRequest = serde_json::from_str(&json).unwrap();
        match back {
            ApiRequest::CancelRun { run_id } => assert_eq!(run_id, Uuid::nil()),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn api_response_run_created_roundtrip() {
        let resp = ApiResponse::RunCreated {
            run_id: Uuid::nil(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ApiResponse = serde_json::from_str(&json).unwrap();
        match back {
            ApiResponse::RunCreated { run_id } => assert_eq!(run_id, Uuid::nil()),
            _ => panic!("wrong variant"),
        }
    }
}
