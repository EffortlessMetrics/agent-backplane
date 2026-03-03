// SPDX-License-Identifier: MIT OR Apache-2.0
//! Request and response types for the daemon HTTP control-plane API.
//!
//! These types are framework-agnostic and designed for deterministic
//! serialization (using `BTreeMap` where order matters).

use abp_core::{CapabilityManifest, Receipt, WorkOrder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Response body for `GET /health`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthResponse {
    /// Server status (e.g. `"ok"`).
    pub status: String,
    /// Contract version reported by the server.
    pub version: String,
    /// Server uptime in whole seconds.
    pub uptime_secs: u64,
}

// ---------------------------------------------------------------------------
// Backends
// ---------------------------------------------------------------------------

/// Information about a registered backend returned by `GET /backends`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfo {
    /// Human-readable backend name (e.g. `"mock"`, `"sidecar:node"`).
    pub name: String,
    /// The type of backend (e.g. `"mock"`, `"sidecar"`).
    pub backend_type: String,
    /// Capability manifest advertised by this backend.
    pub capabilities: CapabilityManifest,
}

// ---------------------------------------------------------------------------
// Run request / response
// ---------------------------------------------------------------------------

/// Request body for `POST /run`.
///
/// Wraps a [`WorkOrder`] with optional per-request overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    /// The work order to execute.
    pub work_order: WorkOrder,
    /// Target backend name override (if not embedded in the work order config).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_override: Option<String>,
    /// Arbitrary key-value overrides applied before dispatch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, serde_json::Value>,
}

/// Status of a tracked run, returned by `GET /runs/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStatus {
    /// Unique run identifier.
    pub id: Uuid,
    /// Current state of the run.
    pub state: RunState,
    /// Final receipt, present only when state is `completed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Receipt>,
}

/// Lifecycle state of a single run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// The run is queued and waiting to start.
    Pending,
    /// The run is actively executing.
    Running,
    /// The run finished successfully.
    Completed,
    /// The run terminated with an error.
    Failed,
}

impl RunState {
    /// Returns `true` for terminal states (`Completed` or `Failed`).
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

// ---------------------------------------------------------------------------
// Backends list response
// ---------------------------------------------------------------------------

/// Response wrapper for `GET /backends`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendsResponse {
    /// Registered backends.
    pub backends: Vec<BackendInfo>,
}

// ---------------------------------------------------------------------------
// Run response (full)
// ---------------------------------------------------------------------------

/// Response body for `POST /run` when the run completes synchronously.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResponse {
    /// Assigned run identifier.
    pub run_id: Uuid,
    /// Final status of the completed run.
    pub status: RunStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

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
    fn health_response_fields_present_in_json() {
        let resp = HealthResponse {
            status: "ok".into(),
            version: "abp/v0.1".into(),
            uptime_secs: 100,
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["status"], "ok");
        assert_eq!(val["version"], "abp/v0.1");
        assert_eq!(val["uptime_secs"], 100);
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

    // -- BackendInfo --------------------------------------------------------

    #[test]
    fn backend_info_serde_roundtrip() {
        let info = BackendInfo {
            name: "mock".into(),
            backend_type: "mock".into(),
            capabilities: BTreeMap::new(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: BackendInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "mock");
        assert_eq!(back.backend_type, "mock");
        assert!(back.capabilities.is_empty());
    }

    #[test]
    fn backend_info_with_capabilities() {
        use abp_core::{Capability, SupportLevel};
        let mut caps = BTreeMap::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);

        let info = BackendInfo {
            name: "sidecar:node".into(),
            backend_type: "sidecar".into(),
            capabilities: caps,
        };
        let val = serde_json::to_value(&info).unwrap();
        assert_eq!(val["name"], "sidecar:node");
        assert_eq!(val["backend_type"], "sidecar");
        assert!(val["capabilities"].is_object());
    }

    #[test]
    fn backend_info_empty_capabilities_serialized() {
        let info = BackendInfo {
            name: "test".into(),
            backend_type: "mock".into(),
            capabilities: BTreeMap::new(),
        };
        let val = serde_json::to_value(&info).unwrap();
        assert!(val["capabilities"].as_object().unwrap().is_empty());
    }

    // -- RunState -----------------------------------------------------------

    #[test]
    fn run_state_serde_all_variants() {
        for state in [
            RunState::Pending,
            RunState::Running,
            RunState::Completed,
            RunState::Failed,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: RunState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, back);
        }
    }

    #[test]
    fn run_state_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&RunState::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&RunState::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&RunState::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&RunState::Failed).unwrap(),
            "\"failed\""
        );
    }

    #[test]
    fn run_state_terminal_check() {
        assert!(!RunState::Pending.is_terminal());
        assert!(!RunState::Running.is_terminal());
        assert!(RunState::Completed.is_terminal());
        assert!(RunState::Failed.is_terminal());
    }

    // -- RunStatus ----------------------------------------------------------

    #[test]
    fn run_status_serde_roundtrip_pending() {
        let status = RunStatus {
            id: Uuid::nil(),
            state: RunState::Pending,
            receipt: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        let back: RunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, status.id);
        assert_eq!(back.state, status.state);
        assert!(back.receipt.is_none());
    }

    #[test]
    fn run_status_omits_none_receipt() {
        let status = RunStatus {
            id: Uuid::nil(),
            state: RunState::Running,
            receipt: None,
        };
        let val = serde_json::to_value(&status).unwrap();
        assert!(val.get("receipt").is_none());
    }

    #[test]
    fn run_status_includes_receipt_when_present() {
        use abp_core::{Outcome, ReceiptBuilder};
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let status = RunStatus {
            id: receipt.meta.run_id,
            state: RunState::Completed,
            receipt: Some(receipt),
        };
        let val = serde_json::to_value(&status).unwrap();
        assert!(val.get("receipt").is_some());
    }

    // -- RunRequest ---------------------------------------------------------

    #[test]
    fn run_request_minimal_serde_roundtrip() {
        use abp_core::WorkOrderBuilder;
        let wo = WorkOrderBuilder::new("test task").build();
        let req = RunRequest {
            work_order: wo,
            backend_override: None,
            overrides: BTreeMap::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: RunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.work_order.task, "test task");
        assert!(back.backend_override.is_none());
        assert!(back.overrides.is_empty());
    }

    #[test]
    fn run_request_with_overrides() {
        use abp_core::WorkOrderBuilder;
        let wo = WorkOrderBuilder::new("task").build();
        let mut overrides = BTreeMap::new();
        overrides.insert("model".into(), serde_json::json!("gpt-4"));
        let req = RunRequest {
            work_order: wo,
            backend_override: Some("sidecar:node".into()),
            overrides,
        };
        let val = serde_json::to_value(&req).unwrap();
        assert_eq!(val["backend_override"], "sidecar:node");
        assert_eq!(val["overrides"]["model"], "gpt-4");
    }

    #[test]
    fn run_request_omits_empty_overrides() {
        use abp_core::WorkOrderBuilder;
        let wo = WorkOrderBuilder::new("task").build();
        let req = RunRequest {
            work_order: wo,
            backend_override: None,
            overrides: BTreeMap::new(),
        };
        let val = serde_json::to_value(&req).unwrap();
        assert!(val.get("overrides").is_none());
        assert!(val.get("backend_override").is_none());
    }

    #[test]
    fn run_request_overrides_deterministic_order() {
        use abp_core::WorkOrderBuilder;
        let wo = WorkOrderBuilder::new("task").build();
        let mut overrides = BTreeMap::new();
        overrides.insert("z_key".into(), serde_json::json!(1));
        overrides.insert("a_key".into(), serde_json::json!(2));
        let req = RunRequest {
            work_order: wo,
            backend_override: None,
            overrides,
        };
        let json = serde_json::to_string(&req).unwrap();
        let a_pos = json.find("a_key").unwrap();
        let z_pos = json.find("z_key").unwrap();
        assert!(a_pos < z_pos, "BTreeMap should serialize in key order");
    }

    // -- BackendsResponse ---------------------------------------------------

    #[test]
    fn backends_response_serde_roundtrip() {
        let resp = BackendsResponse {
            backends: vec![BackendInfo {
                name: "mock".into(),
                backend_type: "mock".into(),
                capabilities: BTreeMap::new(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: BackendsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backends.len(), 1);
        assert_eq!(back.backends[0].name, "mock");
    }

    #[test]
    fn backends_response_empty_list() {
        let resp = BackendsResponse { backends: vec![] };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val["backends"].as_array().unwrap().is_empty());
    }

    // -- RunResponse --------------------------------------------------------

    #[test]
    fn run_response_serde_roundtrip() {
        let resp = RunResponse {
            run_id: Uuid::nil(),
            status: RunStatus {
                id: Uuid::nil(),
                state: RunState::Completed,
                receipt: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: RunResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, Uuid::nil());
        assert_eq!(back.status.state, RunState::Completed);
    }
}
