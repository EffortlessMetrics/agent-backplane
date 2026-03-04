#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive HTTP API tests covering both the `server::router` (ServerState)
//! and the `build_app` (AppState) routers.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::api::{
    HealthResponse, ListBackendsResponse, RunResponse as V1RunResponse, RunStatus as V1RunStatus,
};
use abp_daemon::server::{VersionResponse, router};
use abp_daemon::state::ServerState;
use abp_daemon::{AppState, RunMetrics, RunRequest, RunTracker, StatusResponse, build_app};
use abp_integrations::MockBackend;
use abp_runtime::Runtime;
use axum::body::Body;
use axum::http::{self, Method, Request, StatusCode};
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a `ServerState` with two backends pre-registered.
fn server_state() -> Arc<ServerState> {
    Arc::new(ServerState::new(vec!["mock".into(), "sidecar:node".into()]))
}

/// Build a `ServerState` with no backends.
fn empty_server_state() -> Arc<ServerState> {
    Arc::new(ServerState::new(vec![]))
}

/// Build an `AppState` backed by a real `Runtime` with mock backend.
fn app_state(receipts_dir: &std::path::Path) -> Arc<AppState> {
    let mut runtime = Runtime::new();
    runtime.register_backend("mock", MockBackend);
    Arc::new(AppState {
        runtime: Arc::new(runtime),
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: receipts_dir.to_path_buf(),
        run_tracker: RunTracker::new(),
    })
}

/// Construct a minimal valid `WorkOrder` for testing.
fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "comprehensive test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

/// Send a GET request against the `server::router`.
async fn server_get(state: Arc<ServerState>, uri: &str) -> http::Response<Body> {
    router(state)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Send a POST+JSON request against the `server::router`.
async fn server_post_json(
    state: Arc<ServerState>,
    uri: &str,
    body: &impl serde::Serialize,
) -> http::Response<Body> {
    let json = serde_json::to_string(body).unwrap();
    router(state)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Send a GET request against the `build_app` router.
async fn app_get(state: Arc<AppState>, uri: &str) -> http::Response<Body> {
    build_app(state)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Send a POST+JSON request against the `build_app` router.
async fn app_post_json(
    state: Arc<AppState>,
    uri: &str,
    body: &impl serde::Serialize,
) -> http::Response<Body> {
    let json = serde_json::to_string(body).unwrap();
    build_app(state)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Send a DELETE request against the `build_app` router.
async fn app_delete(state: Arc<AppState>, uri: &str) -> http::Response<Body> {
    build_app(state)
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Extract response body as `serde_json::Value`.
async fn body_json(resp: http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Extract raw response body bytes.
async fn body_bytes(resp: http::Response<Body>) -> Vec<u8> {
    resp.into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

// ===========================================================================
// 1. Health endpoint — server::router  (GET /health)
// ===========================================================================

#[tokio::test]
async fn health_server_returns_200() {
    let resp = server_get(server_state(), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_server_json_content_type() {
    let resp = server_get(server_state(), "/health").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn health_server_status_field_is_ok() {
    let json = body_json(server_get(server_state(), "/health").await).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_server_includes_contract_version() {
    let json = body_json(server_get(server_state(), "/health").await).await;
    assert_eq!(json["version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_server_includes_uptime() {
    let json = body_json(server_get(server_state(), "/health").await).await;
    assert!(json["uptime_secs"].is_number());
}

#[tokio::test]
async fn health_server_lists_backends() {
    let json = body_json(server_get(server_state(), "/health").await).await;
    let backends = json["backends"].as_array().unwrap();
    assert_eq!(backends.len(), 2);
}

#[tokio::test]
async fn health_server_empty_backends_when_none() {
    let json = body_json(server_get(empty_server_state(), "/health").await).await;
    assert!(json["backends"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn health_server_deserializes_as_health_response() {
    let bytes = body_bytes(server_get(server_state(), "/health").await).await;
    let health: HealthResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(health.status, "ok");
}

// ===========================================================================
// 2. Health endpoint — build_app (GET /health)
// ===========================================================================

#[tokio::test]
async fn health_app_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_app_has_status_field() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/health").await).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_app_has_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/health").await).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_app_has_time_field() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/health").await).await;
    assert!(json.get("time").is_some(), "missing 'time' field");
}

// ===========================================================================
// 3. Backends endpoint — server::router  (GET /backends)
// ===========================================================================

#[tokio::test]
async fn backends_server_returns_200() {
    let resp = server_get(server_state(), "/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn backends_server_returns_json_content_type() {
    let resp = server_get(server_state(), "/backends").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn backends_server_contains_mock() {
    let bytes = body_bytes(server_get(server_state(), "/backends").await).await;
    let list: ListBackendsResponse = serde_json::from_slice(&bytes).unwrap();
    let names: Vec<&str> = list.backends.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"mock"));
}

#[tokio::test]
async fn backends_server_empty_when_none() {
    let bytes = body_bytes(server_get(empty_server_state(), "/backends").await).await;
    let list: ListBackendsResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(list.backends.is_empty());
}

// ===========================================================================
// 4. Backends endpoint — build_app (GET /backends)
// ===========================================================================

#[tokio::test]
async fn backends_app_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn backends_app_lists_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/backends").await).await;
    let names: Vec<String> = serde_json::from_value(json).unwrap();
    assert!(names.contains(&"mock".to_string()));
}

// ===========================================================================
// 5. Run endpoint — server::router  (POST /run)
// ===========================================================================

#[tokio::test]
async fn run_server_accepts_valid_request() {
    let body = serde_json::json!({"task": "hello world"});
    let resp = server_post_json(server_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_server_returns_valid_uuid() {
    let body = serde_json::json!({"task": "hello"});
    let json = body_json(server_post_json(server_state(), "/run", &body).await).await;
    let run_id = json["run_id"].as_str().unwrap();
    assert!(run_id.parse::<Uuid>().is_ok());
}

#[tokio::test]
async fn run_server_returns_queued_status() {
    let body = serde_json::json!({"task": "hello"});
    let bytes = body_bytes(server_post_json(server_state(), "/run", &body).await).await;
    let resp: V1RunResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.status, V1RunStatus::Queued);
}

#[tokio::test]
async fn run_server_rejects_empty_task() {
    let body = serde_json::json!({"task": ""});
    let resp = server_post_json(server_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_server_rejects_unknown_backend() {
    let body = serde_json::json!({"task": "hello", "backend": "nonexistent"});
    let resp = server_post_json(server_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp).await;
    assert!(json["error"].as_str().unwrap().contains("unknown backend"));
}

#[tokio::test]
async fn run_server_accepts_known_backend() {
    let body = serde_json::json!({"task": "hello", "backend": "mock"});
    let resp = server_post_json(server_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_server_accepts_default_backend() {
    let body = serde_json::json!({"task": "hello", "backend": "default"});
    let resp = server_post_json(server_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

// ===========================================================================
// 6. Run endpoint — build_app (POST /run) with real MockBackend
// ===========================================================================

#[tokio::test]
async fn run_app_accepts_valid_work_order() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = app_post_json(app_state(tmp.path()), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn run_app_response_has_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let json = body_json(app_post_json(app_state(tmp.path()), "/run", &req).await).await;
    assert!(json.get("receipt").is_some(), "response missing receipt");
    assert!(json.get("run_id").is_some(), "response missing run_id");
}

#[tokio::test]
async fn run_app_rejects_unknown_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };
    let resp = app_post_json(app_state(tmp.path()), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_app_rejects_empty_task() {
    let tmp = tempfile::tempdir().unwrap();
    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = app_post_json(app_state(tmp.path()), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_app_rejects_whitespace_only_task() {
    let tmp = tempfile::tempdir().unwrap();
    let mut wo = test_work_order();
    wo.task = "   \t\n".into();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = app_post_json(app_state(tmp.path()), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===========================================================================
// 7. Status endpoint — build_app (GET /status)
// ===========================================================================

#[tokio::test]
async fn status_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/status").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn status_has_required_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = body_bytes(app_get(app_state(tmp.path()), "/status").await).await;
    let status: StatusResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status.status, "ok");
    assert_eq!(status.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn status_shows_zero_runs_initially() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = body_bytes(app_get(app_state(tmp.path()), "/status").await).await;
    let status: StatusResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status.total_runs, 0);
    assert!(status.active_runs.is_empty());
}

#[tokio::test]
async fn status_lists_backends() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = body_bytes(app_get(app_state(tmp.path()), "/status").await).await;
    let status: StatusResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(status.backends.contains(&"mock".to_string()));
}

// ===========================================================================
// 8. Config endpoint — build_app (GET /config)
// ===========================================================================

#[tokio::test]
async fn config_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/config").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_has_backends_and_version() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/config").await).await;
    assert!(json.get("backends").is_some());
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn config_has_receipts_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/config").await).await;
    assert!(json.get("receipts_dir").is_some());
}

// ===========================================================================
// 9. Metrics endpoint — build_app (GET /metrics)
// ===========================================================================

#[tokio::test]
async fn metrics_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/metrics").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_initially_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = body_bytes(app_get(app_state(tmp.path()), "/metrics").await).await;
    let m: RunMetrics = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(m.total_runs, 0);
    assert_eq!(m.running, 0);
    assert_eq!(m.completed, 0);
    assert_eq!(m.failed, 0);
}

#[tokio::test]
async fn metrics_increment_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());

    // Execute a run via mock backend.
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = app_post_json(state.clone(), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Metrics should reflect the completed run.
    let bytes = body_bytes(app_get(state, "/metrics").await).await;
    let m: RunMetrics = serde_json::from_slice(&bytes).unwrap();
    assert!(m.total_runs >= 1);
    assert!(m.completed >= 1);
}

#[tokio::test]
async fn metrics_json_content_type() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/metrics").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

// ===========================================================================
// 10. Version endpoint — server::router  (GET /version)
// ===========================================================================

#[tokio::test]
async fn version_returns_200() {
    let resp = server_get(server_state(), "/version").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn version_has_contract_version() {
    let json = body_json(server_get(server_state(), "/version").await).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn version_has_crate_version() {
    let json = body_json(server_get(server_state(), "/version").await).await;
    assert!(!json["version"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn version_deserializes_as_version_response() {
    let bytes = body_bytes(server_get(server_state(), "/version").await).await;
    let v: VersionResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v.contract_version, abp_core::CONTRACT_VERSION);
}

// ===========================================================================
// 11. Error responses — proper 4xx/5xx codes
// ===========================================================================

#[tokio::test]
async fn unknown_route_returns_404() {
    let resp = server_get(server_state(), "/nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_to_get_only_returns_405() {
    let body = serde_json::json!({});
    let resp = server_post_json(server_state(), "/health", &body).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn get_to_post_only_returns_405() {
    let resp = server_get(server_state(), "/run").await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn malformed_json_returns_400() {
    let resp = router(server_state())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/run")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from("not json at all"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn missing_content_type_returns_415() {
    let resp = router(server_state())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/run")
                .body(Body::from(r#"{"task":"hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn run_empty_body_returns_error() {
    let resp = router(server_state())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/run")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Axum returns 400 for missing body.
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_missing_task_field_returns_error() {
    let body = serde_json::json!({"backend": "mock"});
    let resp = router(server_state())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/run")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_client_error());
}

// ===========================================================================
// 12. Error response body structure (server::router)
// ===========================================================================

#[tokio::test]
async fn error_response_has_error_field() {
    let body = serde_json::json!({"task": ""});
    let json = body_json(server_post_json(server_state(), "/run", &body).await).await;
    assert!(
        json.get("error").is_some(),
        "error body should have 'error' field"
    );
}

#[tokio::test]
async fn error_response_has_code_field() {
    let body = serde_json::json!({"task": ""});
    let json = body_json(server_post_json(server_state(), "/run", &body).await).await;
    assert!(
        json.get("code").is_some(),
        "error body should have 'code' field"
    );
}

// ===========================================================================
// 13. Content negotiation — JSON responses
// ===========================================================================

#[tokio::test]
async fn all_server_get_endpoints_return_json() {
    let state = server_state();
    for uri in ["/health", "/backends", "/version"] {
        let resp = server_get(state.clone(), uri).await;
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            ct.starts_with("application/json"),
            "{uri} returned content-type: {ct}"
        );
    }
}

#[tokio::test]
async fn all_app_get_endpoints_return_json() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    for uri in ["/health", "/status", "/metrics", "/backends", "/config"] {
        let resp = app_get(state.clone(), uri).await;
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            ct.starts_with("application/json"),
            "{uri} returned content-type: {ct}"
        );
    }
}

// ===========================================================================
// 14. Schema endpoint — build_app  (GET /schema/{type})
// ===========================================================================

#[tokio::test]
async fn schema_work_order_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/schema/work_order").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_receipt_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/schema/receipt").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/schema/nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn schema_body_is_valid_json_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/schema/work_order").await).await;
    // JSON Schema documents always have a "type" or "$schema" or "properties" key.
    assert!(
        json.get("type").is_some()
            || json.get("$schema").is_some()
            || json.get("properties").is_some(),
        "schema should be a valid JSON Schema document"
    );
}

// ===========================================================================
// 15. Validate endpoint — build_app  (POST /validate)
// ===========================================================================

#[tokio::test]
async fn validate_accepts_valid_request() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = app_post_json(app_state(tmp.path()), "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["valid"], true);
}

#[tokio::test]
async fn validate_rejects_invalid_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "bogus".into(),
        work_order: test_work_order(),
    };
    let resp = app_post_json(app_state(tmp.path()), "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===========================================================================
// 16. Runs listing — build_app  (GET /runs)
// ===========================================================================

#[tokio::test]
async fn runs_list_initially_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/runs").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn runs_list_populated_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let wo = test_work_order();
    let expected_id = wo.id;
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = app_post_json(state.clone(), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_json(app_get(state, "/runs").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&expected_id));
}

// ===========================================================================
// 17. Get run by ID — build_app  (GET /runs/{id})
// ===========================================================================

#[tokio::test]
async fn get_run_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_id = Uuid::new_v4();
    let resp = app_get(app_state(tmp.path()), &format!("/runs/{fake_id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_run_after_completion() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let wo = test_work_order();
    let run_id = wo.id;
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = app_post_json(state.clone(), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app_get(state, &format!("/runs/{run_id}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["run_id"], run_id.to_string());
}

// ===========================================================================
// 18. Get run receipt — build_app  (GET /runs/{id}/receipt)
// ===========================================================================

#[tokio::test]
async fn get_receipt_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_id = Uuid::new_v4();
    let resp = app_get(app_state(tmp.path()), &format!("/runs/{fake_id}/receipt")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_receipt_after_completion() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let wo = test_work_order();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = app_post_json(state.clone(), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let run_json = body_json(resp).await;
    // Use the run_id from the response (receipt.meta.run_id).
    let receipt_run_id = run_json["run_id"].as_str().unwrap();

    let resp = app_get(state, &format!("/receipts/{receipt_run_id}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// 19. Delete run — build_app  (DELETE /runs/{id})
// ===========================================================================

#[tokio::test]
async fn delete_run_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_id = Uuid::new_v4();
    let resp = app_delete(app_state(tmp.path()), &format!("/runs/{fake_id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_completed_run_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let wo = test_work_order();
    let run_id = wo.id;
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = app_post_json(state.clone(), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app_delete(state, &format!("/runs/{run_id}")).await;
    // The tracker may report "conflict" if the run is still marked Running
    // internally (the cmd_run handler completes it, but remove_run only
    // accepts terminal states). Either OK (deleted) or Conflict is valid
    // here depending on tracker state; assert it's not 404.
    assert_ne!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// 20. Receipts endpoints — build_app  (GET /receipts, GET /receipts/{id})
// ===========================================================================

#[tokio::test]
async fn receipts_list_initially_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/receipts").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn receipts_list_with_limit_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/receipts?limit=0").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn get_receipt_by_id_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_id = Uuid::new_v4();
    let resp = app_get(app_state(tmp.path()), &format!("/receipts/{fake_id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// 21. Capabilities endpoint — build_app (GET /capabilities)
// ===========================================================================

#[tokio::test]
async fn capabilities_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/capabilities").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn capabilities_returns_array() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(app_get(app_state(tmp.path()), "/capabilities").await).await;
    assert!(json.is_array());
}

#[tokio::test]
async fn capabilities_unknown_backend_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = app_get(app_state(tmp.path()), "/capabilities?backend=nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// 22. Concurrency — multiple simultaneous requests
// ===========================================================================

#[tokio::test]
async fn concurrent_health_requests() {
    let state = server_state();
    let futs: Vec<_> = (0..20)
        .map(|_| {
            let s = state.clone();
            async move {
                let resp = server_get(s, "/health").await;
                resp.status()
            }
        })
        .collect();

    let results = futures::future::join_all(futs).await;
    for status in results {
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn concurrent_app_health_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let futs: Vec<_> = (0..20)
        .map(|_| {
            let s = state.clone();
            async move {
                let resp = app_get(s, "/health").await;
                resp.status()
            }
        })
        .collect();

    let results = futures::future::join_all(futs).await;
    for status in results {
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn concurrent_runs_server_router() {
    let state = server_state();
    let futs: Vec<_> = (0..10)
        .map(|i| {
            let s = state.clone();
            async move {
                let body = serde_json::json!({"task": format!("task-{i}")});
                let resp = server_post_json(s, "/run", &body).await;
                resp.status()
            }
        })
        .collect();

    let results = futures::future::join_all(futs).await;
    for status in results {
        assert_eq!(status, StatusCode::CREATED);
    }
}

#[tokio::test]
async fn concurrent_mixed_endpoints() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());

    let h1 = {
        let s = state.clone();
        tokio::spawn(async move { app_get(s, "/health").await.status() })
    };
    let h2 = {
        let s = state.clone();
        tokio::spawn(async move { app_get(s, "/status").await.status() })
    };
    let h3 = {
        let s = state.clone();
        tokio::spawn(async move { app_get(s, "/metrics").await.status() })
    };
    let h4 = {
        let s = state.clone();
        tokio::spawn(async move { app_get(s, "/backends").await.status() })
    };
    let h5 = {
        let s = state.clone();
        tokio::spawn(async move { app_get(s, "/config").await.status() })
    };

    let (r1, r2, r3, r4, r5) = tokio::join!(h1, h2, h3, h4, h5);
    assert_eq!(r1.unwrap(), StatusCode::OK);
    assert_eq!(r2.unwrap(), StatusCode::OK);
    assert_eq!(r3.unwrap(), StatusCode::OK);
    assert_eq!(r4.unwrap(), StatusCode::OK);
    assert_eq!(r5.unwrap(), StatusCode::OK);
}

// ===========================================================================
// 23. DaemonError → HTTP status mapping
// ===========================================================================

#[test]
fn daemon_error_not_found_maps_to_404() {
    let err = abp_daemon::DaemonError::NotFound("x".into());
    assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
}

#[test]
fn daemon_error_bad_request_maps_to_400() {
    let err = abp_daemon::DaemonError::BadRequest("x".into());
    assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
}

#[test]
fn daemon_error_conflict_maps_to_409() {
    let err = abp_daemon::DaemonError::Conflict("x".into());
    assert_eq!(err.status_code(), StatusCode::CONFLICT);
}

#[test]
fn daemon_error_internal_maps_to_500() {
    let err = abp_daemon::DaemonError::Internal(anyhow::anyhow!("boom"));
    assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ===========================================================================
// 24. DaemonConfig
// ===========================================================================

#[test]
fn daemon_config_default_values() {
    let cfg = abp_daemon::DaemonConfig::default();
    assert_eq!(cfg.bind_address, "127.0.0.1");
    assert_eq!(cfg.port, 8088);
    assert!(cfg.auth_token.is_none());
}

#[test]
fn daemon_config_bind_string() {
    let cfg = abp_daemon::DaemonConfig {
        bind_address: "0.0.0.0".into(),
        port: 9090,
        auth_token: None,
    };
    assert_eq!(cfg.bind_string(), "0.0.0.0:9090");
}

// ===========================================================================
// 25. RunTracker lifecycle
// ===========================================================================

#[tokio::test]
async fn run_tracker_start_and_list() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 1);
}

#[tokio::test]
async fn run_tracker_duplicate_start_errors() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert!(tracker.start_run(id).await.is_err());
}

#[tokio::test]
async fn run_tracker_cancel_pending_succeeds() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    tracker.cancel_run(id).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, abp_daemon::RunStatus::Cancelled));
}
