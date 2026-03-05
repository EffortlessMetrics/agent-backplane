// SPDX-License-Identifier: MIT OR Apache-2.0
//! API request/response models for the daemon HTTP control-plane.
//!
//! All types derive `Serialize`, `Deserialize`, and `JsonSchema` for
//! deterministic wire-format compatibility and automatic OpenAPI documentation.

use abp_core::{Receipt, WorkOrder, ir::IrConversation};
use abp_dialect::Dialect;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Run request / response
// ---------------------------------------------------------------------------

/// Request body for `POST /v1/run`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunRequest {
    /// The work order to execute.
    pub work_order: WorkOrder,
    /// Target backend name (e.g. `"mock"`, `"sidecar:node"`).
    pub backend: String,
    /// Arbitrary key-value overrides applied before dispatch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, serde_json::Value>,
}

/// Response body for `POST /v1/run`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunResponse {
    /// Assigned run identifier.
    pub run_id: Uuid,
    /// Current status of the newly created run.
    pub status: RunStatusKind,
    /// Human-readable message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Lifecycle status of a single run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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

/// Response body for `GET /v1/status/:id`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusResponse {
    /// Unique run identifier.
    pub run_id: Uuid,
    /// Current lifecycle status.
    pub status: RunStatusKind,
    /// Backend executing (or that executed) this run.
    pub backend: String,
    /// Timestamp when the run was created.
    pub created_at: DateTime<Utc>,
    /// Number of events emitted so far.
    pub events_count: usize,
    /// Error message, present only when status is `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

/// Response body for `GET /v1/receipt/:id`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptResponse {
    /// Unique run identifier.
    pub run_id: Uuid,
    /// The receipt produced by the completed run.
    pub receipt: Receipt,
}

// ---------------------------------------------------------------------------
// Backends
// ---------------------------------------------------------------------------

/// Information about a registered backend.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackendInfo {
    /// Backend identifier / name.
    pub name: String,
    /// The type of backend (e.g. `"mock"`, `"sidecar"`).
    pub backend_type: String,
    /// Current operational status.
    pub status: String,
}

/// Response body for `GET /v1/backends`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackendsListResponse {
    /// Registered backends.
    pub backends: Vec<BackendInfo>,
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Response body for `GET /v1/health`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct HealthResponse {
    /// Server status (e.g. `"ok"`).
    pub status: String,
    /// Contract version reported by the server.
    pub version: String,
    /// Server uptime in whole seconds.
    pub uptime_secs: u64,
}

// ---------------------------------------------------------------------------
// Cancel
// ---------------------------------------------------------------------------

/// Response body for `POST /v1/cancel/:id`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CancelResponse {
    /// The cancelled run identifier.
    pub run_id: Uuid,
    /// Status after cancellation.
    pub status: RunStatusKind,
    /// Human-readable message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Structured error response returned on failure.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ErrorBody {
    /// Machine-readable error code (e.g. `"not_found"`, `"invalid_request"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

impl ErrorBody {
    /// Create a new error body.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// 404 — resource not found.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    /// 400 — invalid request.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("bad_request", message)
    }

    /// 409 — conflict.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("conflict", message)
    }

    /// 500 — internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }
}

// ---------------------------------------------------------------------------
// Translate
// ---------------------------------------------------------------------------

/// Request body for `POST /v1/translate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateRequest {
    /// Source dialect (e.g. `"openai"`, `"claude"`).
    pub from: Dialect,
    /// Target dialect.
    pub to: Dialect,
    /// The IR conversation to translate.
    pub conversation: IrConversation,
}

/// Response body for `POST /v1/translate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateResponse {
    /// The translated IR conversation.
    pub conversation: IrConversation,
    /// Source dialect.
    pub from: Dialect,
    /// Target dialect.
    pub to: Dialect,
    /// Translation classification.
    pub mode: String,
    /// Detected capability gaps (non-fatal warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<String>,
}

// ---------------------------------------------------------------------------
// Receipts list
// ---------------------------------------------------------------------------

/// Response body for `GET /v1/receipts`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptsListResponse {
    /// Stored receipt summaries.
    pub receipts: Vec<ReceiptSummary>,
}

/// Summary of a stored receipt.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReceiptSummary {
    /// Run identifier.
    pub run_id: Uuid,
    /// Backend that produced the receipt.
    pub backend: String,
    /// Outcome of the run.
    pub outcome: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::WorkOrderBuilder;

    // -- RunRequest ---------------------------------------------------------

    #[test]
    fn run_request_serde_roundtrip() {
        let wo = WorkOrderBuilder::new("test task").build();
        let req = RunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: BTreeMap::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: RunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backend, "mock");
        assert_eq!(back.work_order.task, "test task");
    }

    #[test]
    fn run_request_omits_empty_overrides() {
        let wo = WorkOrderBuilder::new("task").build();
        let req = RunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: BTreeMap::new(),
        };
        let val = serde_json::to_value(&req).unwrap();
        assert!(val.get("overrides").is_none());
    }

    #[test]
    fn run_request_with_overrides_deterministic() {
        let wo = WorkOrderBuilder::new("task").build();
        let mut overrides = BTreeMap::new();
        overrides.insert("z_key".into(), serde_json::json!(1));
        overrides.insert("a_key".into(), serde_json::json!(2));
        let req = RunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides,
        };
        let json = serde_json::to_string(&req).unwrap();
        let a_pos = json.find("a_key").unwrap();
        let z_pos = json.find("z_key").unwrap();
        assert!(a_pos < z_pos, "BTreeMap should serialize in key order");
    }

    #[test]
    fn run_request_json_schema_generated() {
        let schema = schemars::schema_for!(RunRequest);
        let val = serde_json::to_value(&schema).unwrap();
        assert!(val.get("$schema").is_some() || val.get("title").is_some());
    }

    // -- RunResponse --------------------------------------------------------

    #[test]
    fn run_response_serde_roundtrip() {
        let resp = RunResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Queued,
            message: Some("queued".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: RunResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, Uuid::nil());
        assert_eq!(back.status, RunStatusKind::Queued);
        assert_eq!(back.message.as_deref(), Some("queued"));
    }

    #[test]
    fn run_response_omits_none_message() {
        let resp = RunResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Queued,
            message: None,
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val.get("message").is_none());
    }

    // -- RunStatusKind ------------------------------------------------------

    #[test]
    fn run_status_kind_all_variants_roundtrip() {
        for s in [
            RunStatusKind::Queued,
            RunStatusKind::Running,
            RunStatusKind::Completed,
            RunStatusKind::Failed,
            RunStatusKind::Cancelled,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: RunStatusKind = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn run_status_kind_snake_case() {
        assert_eq!(
            serde_json::to_string(&RunStatusKind::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    // -- StatusResponse -----------------------------------------------------

    #[test]
    fn status_response_serde_roundtrip() {
        let resp = StatusResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Running,
            backend: "mock".into(),
            created_at: Utc::now(),
            events_count: 5,
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: StatusResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, Uuid::nil());
        assert_eq!(back.status, RunStatusKind::Running);
        assert_eq!(back.events_count, 5);
    }

    #[test]
    fn status_response_omits_none_error() {
        let resp = StatusResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Running,
            backend: "mock".into(),
            created_at: Utc::now(),
            events_count: 0,
            error: None,
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val.get("error").is_none());
    }

    #[test]
    fn status_response_includes_error_when_failed() {
        let resp = StatusResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Failed,
            backend: "mock".into(),
            created_at: Utc::now(),
            events_count: 3,
            error: Some("timeout".into()),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["error"], "timeout");
        assert_eq!(val["status"], "failed");
    }

    // -- ReceiptResponse ----------------------------------------------------

    #[test]
    fn receipt_response_serde_roundtrip() {
        use abp_core::{Outcome, ReceiptBuilder};
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let run_id = receipt.meta.run_id;
        let resp = ReceiptResponse {
            run_id,
            receipt: receipt.clone(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ReceiptResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, run_id);
    }

    // -- BackendInfo --------------------------------------------------------

    #[test]
    fn backend_info_serde_roundtrip() {
        let info = BackendInfo {
            name: "mock".into(),
            backend_type: "mock".into(),
            status: "available".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: BackendInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "mock");
    }

    #[test]
    fn backends_list_response_empty() {
        let resp = BackendsListResponse { backends: vec![] };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val["backends"].as_array().unwrap().is_empty());
    }

    #[test]
    fn backends_list_response_with_entries() {
        let resp = BackendsListResponse {
            backends: vec![
                BackendInfo {
                    name: "mock".into(),
                    backend_type: "mock".into(),
                    status: "available".into(),
                },
                BackendInfo {
                    name: "sidecar:node".into(),
                    backend_type: "sidecar".into(),
                    status: "available".into(),
                },
            ],
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["backends"].as_array().unwrap().len(), 2);
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
        assert_eq!(resp, back);
    }

    #[test]
    fn health_response_json_schema_generated() {
        let schema = schemars::schema_for!(HealthResponse);
        let val = serde_json::to_value(&schema).unwrap();
        assert!(val.get("$schema").is_some() || val.get("title").is_some());
    }

    // -- CancelResponse -----------------------------------------------------

    #[test]
    fn cancel_response_serde_roundtrip() {
        let resp = CancelResponse {
            run_id: Uuid::nil(),
            status: RunStatusKind::Cancelled,
            message: Some("cancelled by user".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CancelResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, Uuid::nil());
        assert_eq!(back.status, RunStatusKind::Cancelled);
    }

    // -- ErrorBody ----------------------------------------------------------

    #[test]
    fn error_body_not_found() {
        let err = ErrorBody::not_found("run xyz not found");
        assert_eq!(err.code, "not_found");
        assert_eq!(err.message, "run xyz not found");
    }

    #[test]
    fn error_body_bad_request() {
        let err = ErrorBody::bad_request("missing field");
        assert_eq!(err.code, "bad_request");
    }

    #[test]
    fn error_body_conflict() {
        let err = ErrorBody::conflict("already completed");
        assert_eq!(err.code, "conflict");
    }

    #[test]
    fn error_body_internal() {
        let err = ErrorBody::internal("unexpected");
        assert_eq!(err.code, "internal_error");
    }

    #[test]
    fn error_body_serde_roundtrip() {
        let err = ErrorBody::not_found("gone");
        let json = serde_json::to_string(&err).unwrap();
        let back: ErrorBody = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, "not_found");
        assert_eq!(back.message, "gone");
    }

    #[test]
    fn error_body_json_schema_generated() {
        let schema = schemars::schema_for!(ErrorBody);
        let val = serde_json::to_value(&schema).unwrap();
        assert!(val.get("$schema").is_some() || val.get("title").is_some());
    }
}
