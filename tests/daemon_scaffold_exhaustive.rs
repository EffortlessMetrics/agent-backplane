#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive tests for the daemon scaffold module including HTTP API types,
//! state management, configuration, routing, middleware, queue, and versioning.

use abp_config::{BackendEntry, BackplaneConfig};
use abp_core::{AgentEvent, Receipt, WorkOrder, WorkOrderBuilder};
use abp_daemon::api::{
    ApiError as DaemonApiError, ApiRequest, ApiResponse, BackendDetail,
    BackendInfo as ApiBackendInfo, ErrorResponse, HealthResponse as ApiHealthResponse,
    ListBackendsResponse, RunInfo, RunRequest as ApiRunRequest, RunResponse as ApiRunResponse,
    RunStatus as ApiRunStatus,
};
use abp_daemon::handler::{
    BackendInfo, BackendsResponse, HealthResponse, RunRequest, RunResponse, RunState, RunStatus,
};
use abp_daemon::middleware::{CorsConfig, RateLimiter, RequestId};
use abp_daemon::queue::{QueueError, QueuePriority, QueueStats, QueuedRun, RunQueue};
use abp_daemon::routes::{
    Endpoint, MatchResult, Method, Route, RouteError, RouteTable, api_routes,
};
use abp_daemon::server::{DaemonServer, VersionResponse};
use abp_daemon::state::{
    BackendList, RegistryError, RunPhase, RunRecord, RunRegistry, ServerState,
};
use abp_daemon::validation::RequestValidator;
use abp_daemon::versioning::{
    ApiVersion, ApiVersionError, ApiVersionRegistry, VersionNegotiator, VersionedEndpoint,
};
use abp_daemon::{
    AppState, DaemonConfig, DaemonError, DaemonState, RunMetrics, RunStatus as LibRunStatus,
    RunTracker, StatusResponse, ValidationResponse,
};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

// ===========================================================================
// DaemonConfig — creation, defaults, serde, bind_string
// ===========================================================================

#[test]
fn daemon_config_default_bind_address() {
    let cfg = DaemonConfig::default();
    assert_eq!(cfg.bind_address, "127.0.0.1");
}

#[test]
fn daemon_config_default_port() {
    let cfg = DaemonConfig::default();
    assert_eq!(cfg.port, 8088);
}

#[test]
fn daemon_config_default_auth_token_is_none() {
    let cfg = DaemonConfig::default();
    assert!(cfg.auth_token.is_none());
}

#[test]
fn daemon_config_bind_string_default() {
    let cfg = DaemonConfig::default();
    assert_eq!(cfg.bind_string(), "127.0.0.1:8088");
}

#[test]
fn daemon_config_bind_string_custom_port() {
    let cfg = DaemonConfig {
        port: 9090,
        ..Default::default()
    };
    assert_eq!(cfg.bind_string(), "127.0.0.1:9090");
}

#[test]
fn daemon_config_bind_string_custom_address() {
    let cfg = DaemonConfig {
        bind_address: "0.0.0.0".into(),
        ..Default::default()
    };
    assert_eq!(cfg.bind_string(), "0.0.0.0:8088");
}

#[test]
fn daemon_config_bind_string_ipv6() {
    let cfg = DaemonConfig {
        bind_address: "::1".into(),
        port: 3000,
        auth_token: None,
    };
    assert_eq!(cfg.bind_string(), "::1:3000");
}

#[test]
fn daemon_config_serde_roundtrip() {
    let cfg = DaemonConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: DaemonConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.bind_address, cfg.bind_address);
    assert_eq!(back.port, cfg.port);
    assert_eq!(back.auth_token, cfg.auth_token);
}

#[test]
fn daemon_config_serde_with_auth_token() {
    let cfg = DaemonConfig {
        bind_address: "127.0.0.1".into(),
        port: 8088,
        auth_token: Some("secret".into()),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: DaemonConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.auth_token, Some("secret".into()));
}

#[test]
fn daemon_config_auth_token_skipped_when_none() {
    let cfg = DaemonConfig::default();
    let val = serde_json::to_value(&cfg).unwrap();
    assert!(val.get("auth_token").is_none());
}

#[test]
fn daemon_config_auth_token_present_when_some() {
    let cfg = DaemonConfig {
        auth_token: Some("tok".into()),
        ..Default::default()
    };
    let val = serde_json::to_value(&cfg).unwrap();
    assert_eq!(val["auth_token"], "tok");
}

#[test]
fn daemon_config_clone() {
    let cfg = DaemonConfig {
        bind_address: "10.0.0.1".into(),
        port: 443,
        auth_token: Some("key".into()),
    };
    let cloned = cfg.clone();
    assert_eq!(cloned.bind_address, "10.0.0.1");
    assert_eq!(cloned.port, 443);
    assert_eq!(cloned.auth_token, Some("key".into()));
}

#[test]
fn daemon_config_debug_format() {
    let cfg = DaemonConfig::default();
    let dbg = format!("{:?}", cfg);
    assert!(dbg.contains("DaemonConfig"));
    assert!(dbg.contains("127.0.0.1"));
}

#[test]
fn daemon_config_port_zero() {
    let cfg = DaemonConfig {
        port: 0,
        ..Default::default()
    };
    assert_eq!(cfg.bind_string(), "127.0.0.1:0");
}

#[test]
fn daemon_config_port_max() {
    let cfg = DaemonConfig {
        port: u16::MAX,
        ..Default::default()
    };
    assert_eq!(cfg.bind_string(), "127.0.0.1:65535");
}

#[test]
fn daemon_config_deserialize_minimal_json() {
    let json = r#"{"bind_address":"localhost","port":80}"#;
    let cfg: DaemonConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.bind_address, "localhost");
    assert_eq!(cfg.port, 80);
    assert!(cfg.auth_token.is_none());
}

#[test]
fn daemon_config_empty_address() {
    let cfg = DaemonConfig {
        bind_address: "".into(),
        port: 8088,
        auth_token: None,
    };
    assert_eq!(cfg.bind_string(), ":8088");
}

// ===========================================================================
// DaemonState — creation, backends, runs
// ===========================================================================

#[tokio::test]
async fn daemon_state_new_is_empty() {
    let state = DaemonState::new();
    assert!(state.backend_names().await.is_empty());
    assert!(state.run_ids().await.is_empty());
}

#[tokio::test]
async fn daemon_state_default_is_empty() {
    let state = DaemonState::default();
    assert!(state.backend_names().await.is_empty());
}

#[tokio::test]
async fn daemon_state_register_backend() {
    let state = DaemonState::new();
    state.register_backend("mock".into()).await;
    assert_eq!(state.backend_names().await, vec!["mock".to_string()]);
}

#[tokio::test]
async fn daemon_state_register_backend_dedup() {
    let state = DaemonState::new();
    state.register_backend("mock".into()).await;
    state.register_backend("mock".into()).await;
    assert_eq!(state.backend_names().await.len(), 1);
}

#[tokio::test]
async fn daemon_state_register_multiple_backends() {
    let state = DaemonState::new();
    state.register_backend("a".into()).await;
    state.register_backend("b".into()).await;
    state.register_backend("c".into()).await;
    assert_eq!(state.backend_names().await.len(), 3);
}

#[tokio::test]
async fn daemon_state_set_and_get_run_status() {
    let state = DaemonState::new();
    let id = Uuid::new_v4();
    let status = abp_daemon::handler::RunStatus {
        id,
        state: RunState::Running,
        receipt: None,
    };
    state.set_run_status(id, status.clone()).await;
    let got = state.get_run_status(id).await.unwrap();
    assert_eq!(got.id, id);
}

#[tokio::test]
async fn daemon_state_get_run_status_missing() {
    let state = DaemonState::new();
    assert!(state.get_run_status(Uuid::new_v4()).await.is_none());
}

#[tokio::test]
async fn daemon_state_run_ids() {
    let state = DaemonState::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let status = abp_daemon::handler::RunStatus {
        id: id1,
        state: RunState::Pending,
        receipt: None,
    };
    state.set_run_status(id1, status.clone()).await;
    state
        .set_run_status(
            id2,
            abp_daemon::handler::RunStatus {
                id: id2,
                state: RunState::Completed,
                receipt: None,
            },
        )
        .await;
    let ids = state.run_ids().await;
    assert_eq!(ids.len(), 2);
}

#[tokio::test]
async fn daemon_state_clone_shares_state() {
    let state = DaemonState::new();
    let cloned = state.clone();
    state.register_backend("shared".into()).await;
    assert_eq!(cloned.backend_names().await, vec!["shared".to_string()]);
}

// ===========================================================================
// RunTracker — lifecycle transitions
// ===========================================================================

#[tokio::test]
async fn run_tracker_new_is_empty() {
    let tracker = RunTracker::new();
    assert!(tracker.list_runs().await.is_empty());
}

#[tokio::test]
async fn run_tracker_start_run() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, LibRunStatus::Running));
}

#[tokio::test]
async fn run_tracker_start_run_duplicate_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert!(tracker.start_run(id).await.is_err());
}

#[tokio::test]
async fn run_tracker_complete_run() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    tracker.complete_run(id, receipt).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, LibRunStatus::Completed { .. }));
}

#[tokio::test]
async fn run_tracker_complete_untracked_fails() {
    let tracker = RunTracker::new();
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    assert!(tracker.complete_run(Uuid::new_v4(), receipt).await.is_err());
}

#[tokio::test]
async fn run_tracker_fail_run() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    tracker.fail_run(id, "boom".into()).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, LibRunStatus::Failed { .. }));
}

#[tokio::test]
async fn run_tracker_fail_untracked_fails() {
    let tracker = RunTracker::new();
    assert!(
        tracker
            .fail_run(Uuid::new_v4(), "err".into())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn run_tracker_cancel_running() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    tracker.cancel_run(id).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, LibRunStatus::Cancelled));
}

#[tokio::test]
async fn run_tracker_cancel_completed_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    tracker.complete_run(id, receipt).await.unwrap();
    assert!(tracker.cancel_run(id).await.is_err());
}

#[tokio::test]
async fn run_tracker_cancel_untracked_fails() {
    let tracker = RunTracker::new();
    assert!(tracker.cancel_run(Uuid::new_v4()).await.is_err());
}

#[tokio::test]
async fn run_tracker_remove_completed() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    tracker.complete_run(id, receipt).await.unwrap();
    let removed = tracker.remove_run(id).await.unwrap();
    assert!(matches!(removed, LibRunStatus::Completed { .. }));
    assert!(tracker.get_run_status(id).await.is_none());
}

#[tokio::test]
async fn run_tracker_remove_running_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert_eq!(tracker.remove_run(id).await.unwrap_err(), "conflict");
}

#[tokio::test]
async fn run_tracker_remove_not_found() {
    let tracker = RunTracker::new();
    assert_eq!(
        tracker.remove_run(Uuid::new_v4()).await.unwrap_err(),
        "not found"
    );
}

#[tokio::test]
async fn run_tracker_list_runs() {
    let tracker = RunTracker::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    tracker.start_run(id1).await.unwrap();
    tracker.start_run(id2).await.unwrap();
    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 2);
}

// ===========================================================================
// DaemonError — status codes and IntoResponse
// ===========================================================================

#[test]
fn daemon_error_not_found_status() {
    let err = DaemonError::NotFound("x".into());
    assert_eq!(err.status_code(), axum::http::StatusCode::NOT_FOUND);
}

#[test]
fn daemon_error_bad_request_status() {
    let err = DaemonError::BadRequest("x".into());
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[test]
fn daemon_error_conflict_status() {
    let err = DaemonError::Conflict("x".into());
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);
}

#[test]
fn daemon_error_internal_status() {
    let err = DaemonError::Internal(anyhow::anyhow!("x"));
    assert_eq!(
        err.status_code(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[test]
fn daemon_error_display_not_found() {
    let err = DaemonError::NotFound("missing".into());
    assert!(err.to_string().contains("missing"));
}

#[test]
fn daemon_error_display_bad_request() {
    let err = DaemonError::BadRequest("invalid".into());
    assert!(err.to_string().contains("invalid"));
}

#[test]
fn daemon_error_display_conflict() {
    let err = DaemonError::Conflict("state".into());
    assert!(err.to_string().contains("state"));
}

// ===========================================================================
// LibRunStatus — serde
// ===========================================================================

#[test]
fn lib_run_status_pending_serde() {
    let s = LibRunStatus::Pending;
    let json = serde_json::to_string(&s).unwrap();
    let back: LibRunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, LibRunStatus::Pending));
}

#[test]
fn lib_run_status_running_serde() {
    let s = LibRunStatus::Running;
    let json = serde_json::to_string(&s).unwrap();
    let back: LibRunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, LibRunStatus::Running));
}

#[test]
fn lib_run_status_cancelled_serde() {
    let s = LibRunStatus::Cancelled;
    let json = serde_json::to_string(&s).unwrap();
    let back: LibRunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, LibRunStatus::Cancelled));
}

#[test]
fn lib_run_status_failed_serde() {
    let s = LibRunStatus::Failed {
        error: "oops".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("oops"));
    let back: LibRunStatus = serde_json::from_str(&json).unwrap();
    match back {
        LibRunStatus::Failed { error } => assert_eq!(error, "oops"),
        _ => panic!("wrong variant"),
    }
}

// ===========================================================================
// RunMetrics / StatusResponse / ValidationResponse — serde
// ===========================================================================

#[test]
fn run_metrics_serde_roundtrip() {
    let m = RunMetrics {
        total_runs: 10,
        running: 3,
        completed: 5,
        failed: 2,
    };
    let json = serde_json::to_string(&m).unwrap();
    let back: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_runs, 10);
    assert_eq!(back.running, 3);
    assert_eq!(back.completed, 5);
    assert_eq!(back.failed, 2);
}

#[test]
fn status_response_serde_roundtrip() {
    let s = StatusResponse {
        status: "ok".into(),
        contract_version: "abp/v0.1".into(),
        backends: vec!["mock".into()],
        active_runs: vec![],
        total_runs: 1,
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: StatusResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.status, "ok");
    assert_eq!(back.backends, vec!["mock"]);
}

#[test]
fn validation_response_valid() {
    let v = ValidationResponse {
        valid: true,
        errors: vec![],
    };
    let json = serde_json::to_string(&v).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["valid"], true);
    // errors should be skipped when empty
    assert!(val.get("errors").is_none());
}

#[test]
fn validation_response_invalid() {
    let v = ValidationResponse {
        valid: false,
        errors: vec!["bad task".into()],
    };
    let json = serde_json::to_string(&v).unwrap();
    let back: ValidationResponse = serde_json::from_str(&json).unwrap();
    assert!(!back.valid);
    assert_eq!(back.errors.len(), 1);
}

// ===========================================================================
// Route definitions — api_routes()
// ===========================================================================

#[test]
fn api_routes_has_expected_count() {
    let routes = api_routes();
    assert!(routes.len() >= 6);
}

#[test]
fn api_routes_contains_health() {
    let routes = api_routes();
    assert!(routes.iter().any(|r| r.path.contains("health")));
}

#[test]
fn api_routes_contains_backends() {
    let routes = api_routes();
    assert!(routes.iter().any(|r| r.path.contains("backends")));
}

#[test]
fn api_routes_contains_run() {
    let routes = api_routes();
    assert!(
        routes
            .iter()
            .any(|r| r.path.contains("run") && r.method == "POST")
    );
}

#[test]
fn api_routes_contains_events() {
    let routes = api_routes();
    assert!(routes.iter().any(|r| r.path.contains("events")));
}

#[test]
fn api_routes_contains_receipt() {
    let routes = api_routes();
    assert!(routes.iter().any(|r| r.path.contains("receipt")));
}

#[test]
fn route_serde_roundtrip() {
    let route = Route {
        method: "GET".into(),
        path: "/health".into(),
        description: "Health check".into(),
    };
    let json = serde_json::to_string(&route).unwrap();
    let back: Route = serde_json::from_str(&json).unwrap();
    assert_eq!(back.method, "GET");
    assert_eq!(back.path, "/health");
}

// ===========================================================================
// RouteTable — matching
// ===========================================================================

#[test]
fn route_table_health_get() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Get, "/api/v1/health");
    assert_eq!(m, MatchResult::Matched(Endpoint::Health));
}

#[test]
fn route_table_health_post_not_allowed() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Post, "/api/v1/health");
    assert_eq!(m, MatchResult::MethodNotAllowed);
}

#[test]
fn route_table_backends_get() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Get, "/api/v1/backends");
    assert_eq!(m, MatchResult::Matched(Endpoint::ListBackends));
}

#[test]
fn route_table_runs_post() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Post, "/api/v1/runs");
    assert_eq!(m, MatchResult::Matched(Endpoint::SubmitRun));
}

#[test]
fn route_table_runs_get_by_id() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Get, "/api/v1/runs/abc-123");
    assert_eq!(
        m,
        MatchResult::Matched(Endpoint::GetRun {
            run_id: "abc-123".into()
        })
    );
}

#[test]
fn route_table_runs_delete_by_id() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Delete, "/api/v1/runs/abc-123");
    assert_eq!(
        m,
        MatchResult::Matched(Endpoint::DeleteRun {
            run_id: "abc-123".into()
        })
    );
}

#[test]
fn route_table_runs_events() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Get, "/api/v1/runs/abc/events");
    assert_eq!(
        m,
        MatchResult::Matched(Endpoint::GetRunEvents {
            run_id: "abc".into()
        })
    );
}

#[test]
fn route_table_runs_cancel() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Post, "/api/v1/runs/abc/cancel");
    assert_eq!(
        m,
        MatchResult::Matched(Endpoint::CancelRun {
            run_id: "abc".into()
        })
    );
}

#[test]
fn route_table_not_found() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Get, "/api/v1/nonexistent");
    assert_eq!(m, MatchResult::NotFound);
}

#[test]
fn route_table_trailing_slash() {
    let rt = RouteTable::new("/api/v1");
    let m = rt.match_route(Method::Get, "/api/v1/health/");
    assert_eq!(m, MatchResult::Matched(Endpoint::Health));
}

#[test]
fn route_table_empty_prefix() {
    let rt = RouteTable::new("");
    let m = rt.match_route(Method::Get, "/health");
    assert_eq!(m, MatchResult::Matched(Endpoint::Health));
}

// ===========================================================================
// RouteError
// ===========================================================================

#[test]
fn route_error_bad_request_fields() {
    let err = RouteError::bad_request("field missing");
    assert_eq!(err.status, 400);
    assert_eq!(err.code, "bad_request");
}

#[test]
fn route_error_not_found_fields() {
    let err = RouteError::not_found("gone");
    assert_eq!(err.status, 404);
}

#[test]
fn route_error_conflict_fields() {
    let err = RouteError::conflict("already done");
    assert_eq!(err.status, 409);
}

#[test]
fn route_error_internal_fields() {
    let err = RouteError::internal("boom");
    assert_eq!(err.status, 500);
}

// ===========================================================================
// Method display
// ===========================================================================

#[test]
fn method_display_get() {
    assert_eq!(Method::Get.to_string(), "GET");
}

#[test]
fn method_display_post() {
    assert_eq!(Method::Post.to_string(), "POST");
}

#[test]
fn method_display_delete() {
    assert_eq!(Method::Delete.to_string(), "DELETE");
}

// ===========================================================================
// RunState (handler)
// ===========================================================================

#[test]
fn run_state_terminal_completed() {
    assert!(RunState::Completed.is_terminal());
}

#[test]
fn run_state_terminal_failed() {
    assert!(RunState::Failed.is_terminal());
}

#[test]
fn run_state_not_terminal_pending() {
    assert!(!RunState::Pending.is_terminal());
}

#[test]
fn run_state_not_terminal_running() {
    assert!(!RunState::Running.is_terminal());
}

#[test]
fn run_state_serde_snake_case() {
    assert_eq!(
        serde_json::to_string(&RunState::Pending).unwrap(),
        "\"pending\""
    );
    assert_eq!(
        serde_json::to_string(&RunState::Running).unwrap(),
        "\"running\""
    );
}

// ===========================================================================
// Config parsing from TOML
// ===========================================================================

#[test]
fn config_parse_example_toml_structure() {
    let toml_str = r#"
[backends.mock]
type = "mock"

[backends.openai]
type = "sidecar"
command = "node"
args = ["path/to/openai-sidecar.js"]
"#;
    let cfg: BackplaneConfig = abp_config::parse_toml(toml_str).unwrap();
    assert!(cfg.backends.contains_key("mock"));
    assert!(cfg.backends.contains_key("openai"));
}

#[test]
fn config_parse_mock_backend() {
    let toml_str = r#"
[backends.test]
type = "mock"
"#;
    let cfg: BackplaneConfig = abp_config::parse_toml(toml_str).unwrap();
    assert!(matches!(cfg.backends["test"], BackendEntry::Mock {}));
}

#[test]
fn config_parse_sidecar_backend() {
    let toml_str = r#"
[backends.node]
type = "sidecar"
command = "node"
args = ["host.js"]
timeout_secs = 300
"#;
    let cfg: BackplaneConfig = abp_config::parse_toml(toml_str).unwrap();
    match &cfg.backends["node"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
            assert_eq!(*timeout_secs, Some(300));
        }
        _ => panic!("expected Sidecar"),
    }
}

#[test]
fn config_default_has_empty_backends() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.backends.is_empty());
}

#[test]
fn config_default_log_level_is_info() {
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.log_level, Some("info".into()));
}

#[test]
fn config_parse_empty_toml() {
    let cfg: BackplaneConfig = abp_config::parse_toml("").unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn config_serde_roundtrip() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("./receipts".into()),
        bind_address: Some("0.0.0.0".into()),
        port: Some(9999),
        policy_profiles: vec!["default.toml".into()],
        backends: BTreeMap::new(),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn config_backplane_example_toml_exists() {
    let path = std::path::Path::new("backplane.example.toml");
    assert!(
        path.exists(),
        "backplane.example.toml should exist in repo root"
    );
}

#[test]
fn config_backplane_example_toml_parseable() {
    let content = std::fs::read_to_string("backplane.example.toml").unwrap();
    let result = abp_config::parse_toml(&content);
    assert!(
        result.is_ok(),
        "backplane.example.toml should parse: {result:?}"
    );
}

#[test]
fn config_backplane_example_has_mock_backend() {
    let content = std::fs::read_to_string("backplane.example.toml").unwrap();
    let cfg = abp_config::parse_toml(&content).unwrap();
    assert!(
        cfg.backends.contains_key("mock"),
        "example config should define a mock backend"
    );
}

#[test]
fn config_validate_default_succeeds() {
    let cfg = BackplaneConfig::default();
    let result = abp_config::validate_config(&cfg);
    assert!(result.is_ok());
}

// ===========================================================================
// Validation — work order and backend
// ===========================================================================

#[test]
fn validate_valid_uuid() {
    assert!(RequestValidator::validate_run_id(&Uuid::new_v4().to_string()).is_ok());
}

#[test]
fn validate_nil_uuid() {
    assert!(RequestValidator::validate_run_id(&Uuid::nil().to_string()).is_ok());
}

#[test]
fn validate_invalid_uuid() {
    assert!(RequestValidator::validate_run_id("not-a-uuid").is_err());
}

#[test]
fn validate_empty_uuid() {
    assert!(RequestValidator::validate_run_id("").is_err());
}

#[test]
fn validate_empty_backend_name() {
    assert!(RequestValidator::validate_backend_name("", &["mock".into()]).is_err());
}

#[test]
fn validate_unknown_backend_name() {
    let err = RequestValidator::validate_backend_name("nope", &["mock".into()]).unwrap_err();
    assert!(err.contains("unknown backend"));
}

#[test]
fn validate_known_backend_name() {
    assert!(RequestValidator::validate_backend_name("mock", &["mock".into()]).is_ok());
}

#[test]
fn validate_backend_name_max_length() {
    let long = "a".repeat(257);
    assert!(RequestValidator::validate_backend_name(&long, &[long.clone()]).is_err());
}

#[test]
fn validate_valid_config_object() {
    let cfg = json!({"key": "value"});
    assert!(RequestValidator::validate_config(&cfg).is_ok());
}

#[test]
fn validate_non_object_config() {
    let cfg = json!("string");
    assert!(RequestValidator::validate_config(&cfg).is_err());
}

#[test]
fn validate_array_config() {
    let cfg = json!([1, 2, 3]);
    assert!(RequestValidator::validate_config(&cfg).is_err());
}

#[test]
fn validate_work_order_valid() {
    let wo = WorkOrderBuilder::new("hello world").build();
    assert!(RequestValidator::validate_work_order(&wo).is_ok());
}

#[test]
fn validate_work_order_empty_task() {
    let mut wo = WorkOrderBuilder::new("x").build();
    wo.task = "".into();
    assert!(RequestValidator::validate_work_order(&wo).is_err());
}

#[test]
fn validate_work_order_whitespace_task() {
    let mut wo = WorkOrderBuilder::new("x").build();
    wo.task = "   ".into();
    assert!(RequestValidator::validate_work_order(&wo).is_err());
}

// ===========================================================================
// State module — RunPhase, RunRegistry, BackendList, ServerState
// ===========================================================================

#[test]
fn run_phase_terminal_completed() {
    assert!(RunPhase::Completed.is_terminal());
}

#[test]
fn run_phase_terminal_failed() {
    assert!(RunPhase::Failed.is_terminal());
}

#[test]
fn run_phase_terminal_cancelled() {
    assert!(RunPhase::Cancelled.is_terminal());
}

#[test]
fn run_phase_not_terminal_queued() {
    assert!(!RunPhase::Queued.is_terminal());
}

#[test]
fn run_phase_not_terminal_running() {
    assert!(!RunPhase::Running.is_terminal());
}

#[test]
fn run_phase_transitions_queued_to_running() {
    assert!(RunPhase::Queued.can_transition_to(RunPhase::Running));
}

#[test]
fn run_phase_transitions_queued_to_cancelled() {
    assert!(RunPhase::Queued.can_transition_to(RunPhase::Cancelled));
}

#[test]
fn run_phase_transitions_running_to_completed() {
    assert!(RunPhase::Running.can_transition_to(RunPhase::Completed));
}

#[test]
fn run_phase_transitions_running_to_failed() {
    assert!(RunPhase::Running.can_transition_to(RunPhase::Failed));
}

#[test]
fn run_phase_transitions_completed_nowhere() {
    assert!(!RunPhase::Completed.can_transition_to(RunPhase::Running));
    assert!(!RunPhase::Completed.can_transition_to(RunPhase::Failed));
}

#[test]
fn run_phase_transitions_invalid_queued_to_completed() {
    assert!(!RunPhase::Queued.can_transition_to(RunPhase::Completed));
}

#[test]
fn run_phase_serde_roundtrip() {
    for phase in [
        RunPhase::Queued,
        RunPhase::Running,
        RunPhase::Completed,
        RunPhase::Failed,
        RunPhase::Cancelled,
    ] {
        let json = serde_json::to_string(&phase).unwrap();
        let back: RunPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(phase, back);
    }
}

#[tokio::test]
async fn run_registry_create_and_get() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Queued);
    assert_eq!(record.backend, "mock");
}

#[tokio::test]
async fn run_registry_duplicate_id() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let err = reg.create_run(id, "mock".into()).await.unwrap_err();
    assert!(matches!(err, RegistryError::DuplicateId(_)));
}

#[tokio::test]
async fn run_registry_transition() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Running);
}

#[tokio::test]
async fn run_registry_invalid_transition() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let err = reg.transition(id, RunPhase::Completed).await.unwrap_err();
    assert!(matches!(err, RegistryError::InvalidTransition { .. }));
}

#[tokio::test]
async fn run_registry_complete() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    reg.complete(id, receipt).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Completed);
    assert!(record.receipt.is_some());
}

#[tokio::test]
async fn run_registry_fail() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    reg.fail(id, "oops".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Failed);
    assert_eq!(record.error.as_deref(), Some("oops"));
}

#[tokio::test]
async fn run_registry_cancel() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.cancel(id).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Cancelled);
}

#[tokio::test]
async fn run_registry_list_and_count() {
    let reg = RunRegistry::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    reg.create_run(id1, "a".into()).await.unwrap();
    reg.create_run(id2, "b".into()).await.unwrap();
    assert_eq!(reg.len().await, 2);
    assert!(!reg.is_empty().await);
    assert_eq!(reg.count_by_phase(RunPhase::Queued).await, 2);
}

#[tokio::test]
async fn run_registry_remove() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.cancel(id).await.unwrap();
    let removed = reg.remove(id).await.unwrap();
    assert_eq!(removed.phase, RunPhase::Cancelled);
    assert!(reg.get(id).await.is_none());
}

#[tokio::test]
async fn run_registry_remove_non_terminal_fails() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let err = reg.remove(id).await.unwrap_err();
    assert!(matches!(err, RegistryError::InvalidTransition { .. }));
}

#[tokio::test]
async fn run_registry_push_event() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let event = abp_core::AgentEvent {
        ts: chrono::Utc::now(),
        kind: abp_core::AgentEventKind::RunStarted {
            message: "hello".into(),
        },
        ext: None,
    };
    let count = reg.push_event(id, event).await.unwrap();
    assert_eq!(count, 1);
    let events = reg.events(id).await.unwrap();
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn backend_list_new_is_empty() {
    let bl = BackendList::new();
    assert!(bl.is_empty().await);
    assert_eq!(bl.len().await, 0);
}

#[tokio::test]
async fn backend_list_register_and_list() {
    let bl = BackendList::new();
    bl.register("mock".into()).await;
    assert!(bl.contains("mock").await);
    assert_eq!(bl.list().await, vec!["mock".to_string()]);
}

#[tokio::test]
async fn backend_list_dedup() {
    let bl = BackendList::new();
    bl.register("mock".into()).await;
    bl.register("mock".into()).await;
    assert_eq!(bl.len().await, 1);
}

#[tokio::test]
async fn backend_list_from_names() {
    let bl = BackendList::from_names(vec!["a".into(), "b".into()]);
    assert_eq!(bl.len().await, 2);
    assert!(bl.contains("a").await);
    assert!(bl.contains("b").await);
}

#[tokio::test]
async fn server_state_new_with_backends() {
    let st = ServerState::new(vec!["mock".into()]);
    assert!(st.backends.contains("mock").await);
    assert!(st.registry.is_empty().await);
}

#[tokio::test]
async fn server_state_uptime_secs() {
    let st = ServerState::new(vec![]);
    // Should be 0 or very close to 0 right after creation
    assert!(st.uptime_secs() < 2);
}

#[tokio::test]
async fn server_state_default_empty() {
    let st = ServerState::default();
    assert!(st.backends.is_empty().await);
}

// ===========================================================================
// Concurrent state access
// ===========================================================================

#[tokio::test]
async fn concurrent_backend_registration() {
    let state = DaemonState::new();
    let mut handles = vec![];
    for i in 0..20 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            s.register_backend(format!("backend-{i}")).await;
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(state.backend_names().await.len(), 20);
}

#[tokio::test]
async fn concurrent_run_tracking() {
    let tracker = RunTracker::new();
    let mut handles = vec![];
    for _ in 0..20 {
        let t = tracker.clone();
        handles.push(tokio::spawn(async move {
            let id = Uuid::new_v4();
            t.start_run(id).await.unwrap();
            id
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(tracker.list_runs().await.len(), 20);
}

#[tokio::test]
async fn concurrent_registry_operations() {
    let reg = RunRegistry::new();
    let mut handles = vec![];
    for _ in 0..10 {
        let r = reg.clone();
        handles.push(tokio::spawn(async move {
            let id = Uuid::new_v4();
            r.create_run(id, "mock".into()).await.unwrap();
            r.transition(id, RunPhase::Running).await.unwrap();
            r.fail(id, "test".into()).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(reg.len().await, 10);
    assert_eq!(reg.count_by_phase(RunPhase::Failed).await, 10);
}

// ===========================================================================
// Queue — priority, enqueue, dequeue
// ===========================================================================

fn make_queued_run(id: &str, priority: QueuePriority) -> QueuedRun {
    QueuedRun {
        id: id.into(),
        work_order_id: Uuid::new_v4().to_string(),
        priority,
        queued_at: chrono::Utc::now().to_rfc3339(),
        backend: None,
        metadata: BTreeMap::new(),
    }
}

#[test]
fn queue_new_is_empty() {
    let q = RunQueue::new(10);
    assert!(q.is_empty());
    assert_eq!(q.len(), 0);
}

#[test]
fn queue_enqueue_and_dequeue() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("r1", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.len(), 1);
    let item = q.dequeue().unwrap();
    assert_eq!(item.id, "r1");
    assert!(q.is_empty());
}

#[test]
fn queue_priority_ordering() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("low", QueuePriority::Low))
        .unwrap();
    q.enqueue(make_queued_run("high", QueuePriority::High))
        .unwrap();
    q.enqueue(make_queued_run("normal", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.dequeue().unwrap().id, "high");
    assert_eq!(q.dequeue().unwrap().id, "normal");
    assert_eq!(q.dequeue().unwrap().id, "low");
}

#[test]
fn queue_critical_priority_first() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("high", QueuePriority::High))
        .unwrap();
    q.enqueue(make_queued_run("crit", QueuePriority::Critical))
        .unwrap();
    assert_eq!(q.dequeue().unwrap().id, "crit");
}

#[test]
fn queue_full_error() {
    let mut q = RunQueue::new(1);
    q.enqueue(make_queued_run("r1", QueuePriority::Normal))
        .unwrap();
    let err = q.enqueue(make_queued_run("r2", QueuePriority::Normal));
    assert!(err.is_err());
}

#[test]
fn queue_duplicate_id_error() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("r1", QueuePriority::Normal))
        .unwrap();
    let err = q.enqueue(make_queued_run("r1", QueuePriority::High));
    assert!(err.is_err());
}

#[test]
fn queue_peek() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("r1", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.peek().unwrap().id, "r1");
    assert_eq!(q.len(), 1); // peek doesn't remove
}

#[test]
fn queue_remove_by_id() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("r1", QueuePriority::Normal))
        .unwrap();
    q.enqueue(make_queued_run("r2", QueuePriority::High))
        .unwrap();
    let removed = q.remove("r1").unwrap();
    assert_eq!(removed.id, "r1");
    assert_eq!(q.len(), 1);
}

#[test]
fn queue_remove_nonexistent() {
    let mut q = RunQueue::new(10);
    assert!(q.remove("nope").is_none());
}

#[test]
fn queue_clear() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("r1", QueuePriority::Normal))
        .unwrap();
    q.enqueue(make_queued_run("r2", QueuePriority::High))
        .unwrap();
    q.clear();
    assert!(q.is_empty());
}

#[test]
fn queue_by_priority() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("r1", QueuePriority::Low))
        .unwrap();
    q.enqueue(make_queued_run("r2", QueuePriority::Low))
        .unwrap();
    q.enqueue(make_queued_run("r3", QueuePriority::High))
        .unwrap();
    assert_eq!(q.by_priority(QueuePriority::Low).len(), 2);
    assert_eq!(q.by_priority(QueuePriority::High).len(), 1);
}

#[test]
fn queue_stats() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("r1", QueuePriority::Low))
        .unwrap();
    q.enqueue(make_queued_run("r2", QueuePriority::High))
        .unwrap();
    let stats = q.stats();
    assert_eq!(stats.total, 2);
    assert_eq!(stats.max, 10);
    assert_eq!(*stats.by_priority.get("low").unwrap_or(&0), 1);
    assert_eq!(*stats.by_priority.get("high").unwrap_or(&0), 1);
}

#[test]
fn queue_is_full() {
    let mut q = RunQueue::new(2);
    assert!(!q.is_full());
    q.enqueue(make_queued_run("r1", QueuePriority::Normal))
        .unwrap();
    assert!(!q.is_full());
    q.enqueue(make_queued_run("r2", QueuePriority::Normal))
        .unwrap();
    assert!(q.is_full());
}

#[test]
fn queue_priority_serde_roundtrip() {
    for p in [
        QueuePriority::Low,
        QueuePriority::Normal,
        QueuePriority::High,
        QueuePriority::Critical,
    ] {
        let json = serde_json::to_string(&p).unwrap();
        let back: QueuePriority = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}

#[test]
fn queue_priority_ordering_enum() {
    assert!(QueuePriority::Low < QueuePriority::Normal);
    assert!(QueuePriority::Normal < QueuePriority::High);
    assert!(QueuePriority::High < QueuePriority::Critical);
}

// ===========================================================================
// Versioning — ApiVersion parsing, display, compatibility
// ===========================================================================

#[test]
fn api_version_parse_v1() {
    let v = ApiVersion::parse("v1").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 0);
}

#[test]
fn api_version_parse_v1_2() {
    let v = ApiVersion::parse("v1.2").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
}

#[test]
fn api_version_parse_no_prefix() {
    let v = ApiVersion::parse("2.3").unwrap();
    assert_eq!(v.major, 2);
    assert_eq!(v.minor, 3);
}

#[test]
fn api_version_parse_empty_fails() {
    assert!(ApiVersion::parse("").is_err());
    assert!(ApiVersion::parse("v").is_err());
}

#[test]
fn api_version_parse_invalid_fails() {
    assert!(ApiVersion::parse("abc").is_err());
    assert!(ApiVersion::parse("v.1").is_err());
}

#[test]
fn api_version_display() {
    let v = ApiVersion { major: 1, minor: 0 };
    assert_eq!(v.to_string(), "v1.0");
}

#[test]
fn api_version_compatibility() {
    let v1 = ApiVersion { major: 1, minor: 0 };
    let v1_1 = ApiVersion { major: 1, minor: 1 };
    let v2 = ApiVersion { major: 2, minor: 0 };
    assert!(v1.is_compatible(&v1_1));
    assert!(!v1.is_compatible(&v2));
}

#[test]
fn api_version_ordering() {
    let v1_0 = ApiVersion { major: 1, minor: 0 };
    let v1_1 = ApiVersion { major: 1, minor: 1 };
    let v2_0 = ApiVersion { major: 2, minor: 0 };
    assert!(v1_0 < v1_1);
    assert!(v1_1 < v2_0);
}

#[test]
fn api_version_serde_roundtrip() {
    let v = ApiVersion { major: 1, minor: 3 };
    let json = serde_json::to_string(&v).unwrap();
    let back: ApiVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

#[test]
fn version_negotiator_picks_highest_compatible() {
    let requested = ApiVersion { major: 1, minor: 5 };
    let supported = vec![
        ApiVersion { major: 1, minor: 0 },
        ApiVersion { major: 1, minor: 3 },
        ApiVersion { major: 2, minor: 0 },
    ];
    let result = VersionNegotiator::negotiate(&requested, &supported);
    assert_eq!(result, Some(ApiVersion { major: 1, minor: 3 }));
}

#[test]
fn version_negotiator_no_compatible() {
    let requested = ApiVersion { major: 3, minor: 0 };
    let supported = vec![
        ApiVersion { major: 1, minor: 0 },
        ApiVersion { major: 2, minor: 0 },
    ];
    assert!(VersionNegotiator::negotiate(&requested, &supported).is_none());
}

#[test]
fn version_registry_is_supported() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 1, minor: 1 });
    reg.register(VersionedEndpoint {
        path: "/health".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    assert!(reg.is_supported("/health", &ApiVersion { major: 1, minor: 0 }));
    assert!(reg.is_supported("/health", &ApiVersion { major: 1, minor: 1 }));
    assert!(!reg.is_supported("/nope", &ApiVersion { major: 1, minor: 0 }));
}

#[test]
fn version_registry_deprecated_endpoints() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 1, minor: 1 });
    reg.register(VersionedEndpoint {
        path: "/old".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: Some(ApiVersion { major: 1, minor: 0 }),
        deprecated: true,
        deprecated_message: Some("use /new".into()),
    });
    reg.register(VersionedEndpoint {
        path: "/new".into(),
        min_version: ApiVersion { major: 1, minor: 1 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    let deprecated = reg.deprecated_endpoints();
    assert_eq!(deprecated.len(), 1);
    assert_eq!(deprecated[0].path, "/old");
}

#[test]
fn version_registry_supported_versions() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 1, minor: 1 });
    reg.register(VersionedEndpoint {
        path: "/health".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    let versions = reg.supported_versions();
    assert!(versions.contains(&ApiVersion { major: 1, minor: 0 }));
    assert!(versions.contains(&ApiVersion { major: 1, minor: 1 }));
}

// ===========================================================================
// Middleware — RequestId, RateLimiter, CorsConfig
// ===========================================================================

#[test]
fn request_id_equality() {
    let id = Uuid::new_v4();
    let a = RequestId(id);
    let b = RequestId(id);
    assert_eq!(a, b);
}

#[test]
fn request_id_debug() {
    let id = RequestId(Uuid::nil());
    let dbg = format!("{:?}", id);
    assert!(dbg.contains("RequestId"));
}

#[tokio::test]
async fn rate_limiter_allows_under_limit() {
    let limiter = RateLimiter::new(5, Duration::from_secs(60));
    for _ in 0..5 {
        assert!(limiter.check().await.is_ok());
    }
}

#[tokio::test]
async fn rate_limiter_rejects_over_limit() {
    let limiter = RateLimiter::new(2, Duration::from_secs(60));
    limiter.check().await.unwrap();
    limiter.check().await.unwrap();
    assert!(limiter.check().await.is_err());
}

#[test]
fn cors_config_to_layer() {
    let config = CorsConfig {
        allowed_origins: vec!["http://localhost:3000".into()],
        allowed_methods: vec!["GET".into(), "POST".into()],
        allowed_headers: vec!["Content-Type".into()],
    };
    let _layer = config.to_cors_layer();
}

// ===========================================================================
// API module types — serde roundtrips
// ===========================================================================

#[test]
fn api_run_status_serde_all_variants() {
    for s in [
        ApiRunStatus::Queued,
        ApiRunStatus::Running,
        ApiRunStatus::Completed,
        ApiRunStatus::Failed,
        ApiRunStatus::Cancelled,
    ] {
        let json = serde_json::to_string(&s).unwrap();
        let back: ApiRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}

#[test]
fn api_run_status_is_terminal() {
    assert!(!ApiRunStatus::Queued.is_terminal());
    assert!(!ApiRunStatus::Running.is_terminal());
    assert!(ApiRunStatus::Completed.is_terminal());
    assert!(ApiRunStatus::Failed.is_terminal());
    assert!(ApiRunStatus::Cancelled.is_terminal());
}

#[test]
fn api_run_status_transitions() {
    assert!(ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Running));
    assert!(ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Cancelled));
    assert!(!ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Completed));
    assert!(ApiRunStatus::Running.can_transition_to(ApiRunStatus::Completed));
    assert!(ApiRunStatus::Running.can_transition_to(ApiRunStatus::Failed));
    assert!(ApiRunStatus::Completed.valid_transitions().is_empty());
}

#[test]
fn api_error_not_found() {
    let err = DaemonApiError::not_found("run xyz");
    assert_eq!(err.code, "not_found");
    assert_eq!(err.message, "run xyz");
}

#[test]
fn api_error_invalid_request() {
    let err = DaemonApiError::invalid_request("bad");
    assert_eq!(err.code, "invalid_request");
}

#[test]
fn api_error_conflict() {
    let err = DaemonApiError::conflict("state");
    assert_eq!(err.code, "conflict");
}

#[test]
fn api_error_internal() {
    let err = DaemonApiError::internal("boom");
    assert_eq!(err.code, "internal_error");
}

#[test]
fn api_error_with_details() {
    let err = DaemonApiError::not_found("x").with_details(json!({"hint": "try again"}));
    assert!(err.details.is_some());
}

#[test]
fn api_error_display() {
    let err = DaemonApiError::not_found("gone");
    assert!(err.to_string().contains("not_found"));
    assert!(err.to_string().contains("gone"));
}

#[test]
fn error_response_serde() {
    let resp = ErrorResponse {
        error: "bad".into(),
        code: Some("invalid".into()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.error, "bad");
    assert_eq!(back.code, Some("invalid".into()));
}

#[test]
fn error_response_no_code() {
    let resp = ErrorResponse {
        error: "err".into(),
        code: None,
    };
    let val = serde_json::to_value(&resp).unwrap();
    assert!(val.get("code").is_none());
}

#[test]
fn api_request_cancel_serde() {
    let req = ApiRequest::CancelRun {
        run_id: Uuid::nil(),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("cancel_run"));
}

#[test]
fn api_response_run_cancelled_serde() {
    let resp = ApiResponse::RunCancelled {
        run_id: Uuid::nil(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ApiResponse = serde_json::from_str(&json).unwrap();
    match back {
        ApiResponse::RunCancelled { run_id } => assert_eq!(run_id, Uuid::nil()),
        _ => panic!("wrong variant"),
    }
}

// ===========================================================================
// Server module — VersionResponse, router creation
// ===========================================================================

#[test]
fn version_response_serde() {
    let v = VersionResponse {
        version: "0.1.0".into(),
        contract_version: abp_core::CONTRACT_VERSION.into(),
    };
    let json = serde_json::to_string(&v).unwrap();
    let back: VersionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.version, "0.1.0");
    assert_eq!(back.contract_version, abp_core::CONTRACT_VERSION);
}

#[test]
fn daemon_server_creates_router() {
    let state = Arc::new(ServerState::new(vec!["mock".into()]));
    let server = DaemonServer::new(state);
    let _router = server.router();
}

#[test]
fn server_router_builds() {
    let state = Arc::new(ServerState::new(vec![]));
    let _router = abp_daemon::server::router(state);
}

// ===========================================================================
// RegistryError display
// ===========================================================================

#[test]
fn registry_error_not_found_display() {
    let err = RegistryError::NotFound(Uuid::nil());
    assert!(err.to_string().contains("not found"));
}

#[test]
fn registry_error_duplicate_display() {
    let err = RegistryError::DuplicateId(Uuid::nil());
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn registry_error_invalid_transition_display() {
    let err = RegistryError::InvalidTransition {
        run_id: Uuid::nil(),
        from: RunPhase::Queued,
        to: RunPhase::Completed,
    };
    assert!(err.to_string().contains("invalid transition"));
}

// ===========================================================================
// ApiVersionError display
// ===========================================================================

#[test]
fn api_version_error_invalid_format_display() {
    let err = ApiVersionError::InvalidFormat("bad".into());
    assert!(err.to_string().contains("invalid version format"));
}

#[test]
fn api_version_error_unsupported_display() {
    let err = ApiVersionError::UnsupportedVersion(ApiVersion {
        major: 99,
        minor: 0,
    });
    assert!(err.to_string().contains("unsupported"));
}

// ===========================================================================
// QueueError display
// ===========================================================================

#[test]
fn queue_error_full_display() {
    let err = QueueError::Full { max: 10 };
    assert!(err.to_string().contains("full"));
}

#[test]
fn queue_error_duplicate_display() {
    let err = QueueError::DuplicateId("r1".into());
    assert!(err.to_string().contains("duplicate"));
}
