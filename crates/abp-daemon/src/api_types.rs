#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Request and response types for the `/api/v1` HTTP endpoints.
//!
//! All types derive `Debug`, `Clone`, `Serialize`, and `Deserialize` for
//! deterministic wire-format compatibility. `BTreeMap` is used where key
//! ordering must be stable (important for canonical JSON hashing).

use abp_core::{AgentEvent, Receipt, WorkOrder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Submit run
// ---------------------------------------------------------------------------

/// Request body for `POST /api/v1/runs`.
///
/// Wraps a [`WorkOrder`] with backend selection and optional overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitRunRequest {
    /// The work order to execute.
    pub work_order: WorkOrder,
    /// Target backend name (e.g. `"mock"`, `"sidecar:node"`).
    pub backend: String,
    /// Arbitrary key-value overrides applied before dispatch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, serde_json::Value>,
}

/// Response body for `POST /api/v1/runs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitRunResponse {
    /// Assigned run identifier.
    pub run_id: Uuid,
    /// Current status of the newly created run.
    pub status: RunStatusKind,
}

// ---------------------------------------------------------------------------
// Run status
// ---------------------------------------------------------------------------

/// Lifecycle status of a single run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatusKind {
    /// The run is queued and waiting to start.
    Queued,
    /// The run is actively executing.
    Running,
    /// The run completed successfully.
    Completed,
    /// The run terminated with an error.
    Failed,
    /// The run was cancelled by a user request.
    Cancelled,
}

/// Response body for `GET /api/v1/runs/:id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStatusResponse {
    /// Unique run identifier.
    pub run_id: Uuid,
    /// Current lifecycle status.
    pub status: RunStatusKind,
    /// Final receipt, present only when status is `completed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Box<Receipt>>,
    /// Number of events emitted so far.
    pub events_count: usize,
    /// Backend that is (or was) executing this run.
    pub backend: String,
    /// Timestamp when the run was created.
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Backend list
// ---------------------------------------------------------------------------

/// Information about a single registered backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfoEntry {
    /// Backend identifier / name.
    pub name: String,
    /// The type of backend (e.g. `"mock"`, `"sidecar"`).
    pub backend_type: String,
    /// Current operational status.
    pub status: String,
}

/// Response body for `GET /api/v1/backends`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendListResponse {
    /// Registered backends.
    pub backends: Vec<BackendInfoEntry>,
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Response body for `GET /api/v1/health`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Server status (e.g. `"ok"`).
    pub status: String,
    /// Contract version reported by the server.
    pub version: String,
    /// Server uptime in whole seconds.
    pub uptime_secs: u64,
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Structured error response returned on failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Machine-readable error code (e.g. `"not_found"`, `"invalid_request"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ErrorResponse {
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

    /// 404 — resource not found.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    /// 400 — invalid request.
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }

    /// 409 — conflicting state.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("conflict", message)
    }

    /// 500 — unexpected internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }
}

// ---------------------------------------------------------------------------
// Cancel response
// ---------------------------------------------------------------------------

/// Response body for `DELETE /api/v1/runs/:id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelRunResponse {
    /// The cancelled run identifier.
    pub run_id: Uuid,
    /// Status after cancellation.
    pub status: RunStatusKind,
}

// ---------------------------------------------------------------------------
// SSE event wrapper
// ---------------------------------------------------------------------------

/// Wrapper for an agent event sent over the SSE stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseEventData {
    /// Sequence number of this event within the run.
    pub seq: usize,
    /// The agent event payload.
    pub event: AgentEvent,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::WorkOrderBuilder;

    // -- SubmitRunRequest ---------------------------------------------------

    #[test]
    fn submit_run_request_serde_roundtrip() {
        let wo = WorkOrderBuilder::new("test task").build();
        let req = SubmitRunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: BTreeMap::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: SubmitRunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backend, "mock");
        assert_eq!(back.work_order.task, "test task");
        assert!(back.overrides.is_empty());
    }

    #[test]
    fn submit_run_request_omits_empty_overrides() {
        let wo = WorkOrderBuilder::new("task").build();
        let req = SubmitRunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: BTreeMap::new(),
        };
        let val = serde_json::to_value(&req).unwrap();
        assert!(val.get("overrides").is_none());
    }

    #[test]
    fn submit_run_request_with_overrides() {
        let wo = WorkOrderBuilder::new("task").build();
        let mut overrides = BTreeMap::new();
        overrides.insert("model".into(), serde_json::json!("gpt-4"));
        let req = SubmitRunRequest {
            work_order: wo,
            backend: "sidecar:node".into(),
            overrides,
        };
        let val = serde_json::to_value(&req).unwrap();
        assert_eq!(val["overrides"]["model"], "gpt-4");
    }

    // -- SubmitRunResponse --------------------------------------------------

    #[test]
    fn submit_run_response_serde_roundtrip() {
        let resp = SubmitRunResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Queued,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: SubmitRunResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, Uuid::nil());
        assert_eq!(back.status, RunStatusKind::Queued);
    }

    // -- RunStatusKind ------------------------------------------------------

    #[test]
    fn run_status_kind_all_variants_roundtrip() {
        for status in [
            RunStatusKind::Queued,
            RunStatusKind::Running,
            RunStatusKind::Completed,
            RunStatusKind::Failed,
            RunStatusKind::Cancelled,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: RunStatusKind = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn run_status_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&RunStatusKind::Queued).unwrap(),
            "\"queued\""
        );
        assert_eq!(
            serde_json::to_string(&RunStatusKind::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    // -- RunStatusResponse --------------------------------------------------

    #[test]
    fn run_status_response_omits_none_receipt() {
        let resp = RunStatusResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Running,
            receipt: None,
            events_count: 5,
            backend: "mock".into(),
            created_at: Utc::now(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val.get("receipt").is_none());
        assert_eq!(val["events_count"], 5);
    }

    #[test]
    fn run_status_response_includes_receipt() {
        use abp_core::{Outcome, ReceiptBuilder};
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let run_id = receipt.meta.run_id;
        let resp = RunStatusResponse {
            run_id,
            status: RunStatusKind::Completed,
            receipt: Some(Box::new(receipt)),
            events_count: 10,
            backend: "mock".into(),
            created_at: Utc::now(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val.get("receipt").is_some());
        assert_eq!(val["status"], "completed");
    }

    // -- BackendListResponse ------------------------------------------------

    #[test]
    fn backend_list_response_serde_roundtrip() {
        let resp = BackendListResponse {
            backends: vec![BackendInfoEntry {
                name: "mock".into(),
                backend_type: "mock".into(),
                status: "available".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: BackendListResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backends.len(), 1);
        assert_eq!(back.backends[0].name, "mock");
    }

    #[test]
    fn backend_list_response_empty() {
        let resp = BackendListResponse { backends: vec![] };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val["backends"].as_array().unwrap().is_empty());
    }

    // -- HealthResponse -----------------------------------------------------

    #[test]
    fn health_response_serde_roundtrip() {
        let resp = HealthResponse {
            status: "ok".into(),
            version: abp_core::CONTRACT_VERSION.into(),
            uptime_secs: 42,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, "ok");
        assert_eq!(back.version, abp_core::CONTRACT_VERSION);
        assert_eq!(back.uptime_secs, 42);
    }

    #[test]
    fn health_response_zero_uptime() {
        let resp = HealthResponse {
            status: "ok".into(),
            version: "abp/v0.1".into(),
            uptime_secs: 0,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"uptime_secs\":0"));
    }

    // -- ErrorResponse ------------------------------------------------------

    #[test]
    fn error_response_serde_roundtrip() {
        let err = ErrorResponse::not_found("run xyz not found");
        let json = serde_json::to_string(&err).unwrap();
        let back: ErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, "not_found");
        assert_eq!(back.message, "run xyz not found");
        assert!(back.details.is_none());
    }

    #[test]
    fn error_response_stable_codes() {
        assert_eq!(ErrorResponse::not_found("x").code, "not_found");
        assert_eq!(ErrorResponse::invalid_request("x").code, "invalid_request");
        assert_eq!(ErrorResponse::conflict("x").code, "conflict");
        assert_eq!(ErrorResponse::internal("x").code, "internal_error");
    }

    #[test]
    fn error_response_with_details() {
        let err = ErrorResponse::invalid_request("bad field")
            .with_details(serde_json::json!({"field": "id"}));
        let val = serde_json::to_value(&err).unwrap();
        assert_eq!(val["details"]["field"], "id");
    }

    #[test]
    fn error_response_omits_null_details() {
        let err = ErrorResponse::not_found("gone");
        let val = serde_json::to_value(&err).unwrap();
        assert!(val.get("details").is_none());
    }

    // -- CancelRunResponse --------------------------------------------------

    #[test]
    fn cancel_run_response_serde_roundtrip() {
        let resp = CancelRunResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Cancelled,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CancelRunResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, Uuid::nil());
        assert_eq!(back.status, RunStatusKind::Cancelled);
    }

    // -- SseEventData -------------------------------------------------------

    #[test]
    fn sse_event_data_serde_roundtrip() {
        use abp_core::AgentEventKind;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "hello".into(),
            },
            ext: None,
        };
        let data = SseEventData {
            seq: 0,
            event: event.clone(),
        };
        let json = serde_json::to_string(&data).unwrap();
        let back: SseEventData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.seq, 0);
    }
}
