#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! End-to-end tests for the ABP daemon HTTP API.
//!
//! Tests both the `server::router` (lightweight ServerState-based) and the
//! full `build_app` / `build_versioned_app` routers backed by a real Runtime.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::api::{
    ErrorResponse, HealthResponse, ListBackendsResponse, RunResponse as ApiRunResponse,
    RunStatus as ApiRunStatus,
};
use abp_daemon::server::{router, VersionResponse};
use abp_daemon::state::{RunPhase, RunRegistry, ServerState};
use abp_daemon::{
    build_app, build_versioned_app, AppState, RunMetrics, RunRequest, RunTracker, StatusResponse,
};
use abp_integrations::MockBackend;
use abp_runtime::Runtime;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

/// Create a `ServerState` with two pre-registered backends.
fn server_state() -> Arc<ServerState> {
    Arc::new(ServerState::new(vec!["mock".into(), "sidecar:node".into()]))
}

/// Create a `ServerState` with no backends.
fn empty_server_state() -> Arc<ServerState> {
    Arc::new(ServerState::new(vec![]))
}

/// Create a full `AppState` backed by a real `Runtime` with a mock backend.
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

/// Build a valid `WorkOrder` for testing.
fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "e2e test task".into(),
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

/// Send a GET request to the given `router` and return the response.
async fn get(router: axum::Router, uri: &str) -> http::Response<Body> {
    router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Send a POST with JSON body.
async fn post_json(
    router: axum::Router,
    uri: &str,
    body: &impl serde::Serialize,
) -> http::Response<Body> {
    let json = serde_json::to_string(body).unwrap();
    router
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(uri)
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Send a POST with a raw string body and content-type header.
async fn post_raw(
    router: axum::Router,
    uri: &str,
    content_type: &str,
    body: &str,
) -> http::Response<Body> {
    router
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(uri)
                .header(http::header::CONTENT_TYPE, content_type)
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Send a DELETE request.
async fn delete(router: axum::Router, uri: &str) -> http::Response<Body> {
    router
        .oneshot(
            Request::builder()
                .method(http::Method::DELETE)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Extract body as `serde_json::Value`.
async fn body_json(resp: http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Deserialize the response body into a concrete type.
async fn body_as<T: serde::de::DeserializeOwned>(resp: http::Response<Body>) -> T {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ===========================================================================
// 1. Health endpoint (server::router)
// ===========================================================================

#[tokio::test]
async fn health_returns_200() {
    let resp = get(router(server_state()), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_includes_uptime_secs() {
    let resp = get(router(server_state()), "/health").await;
    let json = body_json(resp).await;
    assert!(json["uptime_secs"].is_number(), "missing uptime_secs");
}

#[tokio::test]
async fn health_includes_version() {
    let resp = get(router(server_state()), "/health").await;
    let json = body_json(resp).await;
    assert_eq!(json["version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_status_is_ok() {
    let resp = get(router(server_state()), "/health").await;
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_content_type_is_json() {
    let resp = get(router(server_state()), "/health").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn health_lists_registered_backends() {
    let resp = get(router(server_state()), "/health").await;
    let json = body_json(resp).await;
    let backends = json["backends"].as_array().unwrap();
    assert_eq!(backends.len(), 2);
}

#[tokio::test]
async fn health_deserializes_to_health_response() {
    let resp = get(router(server_state()), "/health").await;
    let health: HealthResponse = body_as(resp).await;
    assert_eq!(health.status, "ok");
    assert_eq!(health.version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_uptime_is_non_negative() {
    let resp = get(router(server_state()), "/health").await;
    let health: HealthResponse = body_as(resp).await;
    // uptime_secs is u64 so always >= 0; verify low value.
    assert!(health.uptime_secs < 5);
}

// ===========================================================================
// 2. Backend management (server::router)
// ===========================================================================

#[tokio::test]
async fn backends_returns_200() {
    let resp = get(router(server_state()), "/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn backends_lists_all_registered() {
    let resp = get(router(server_state()), "/backends").await;
    let list: ListBackendsResponse = body_as(resp).await;
    let names: Vec<&str> = list.backends.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"mock"));
    assert!(names.contains(&"sidecar:node"));
}

#[tokio::test]
async fn backends_includes_dialect_field() {
    let resp = get(router(server_state()), "/backends").await;
    let list: ListBackendsResponse = body_as(resp).await;
    for b in &list.backends {
        assert!(!b.dialect.is_empty(), "dialect should not be empty");
    }
}

#[tokio::test]
async fn backends_includes_status_field() {
    let resp = get(router(server_state()), "/backends").await;
    let list: ListBackendsResponse = body_as(resp).await;
    for b in &list.backends {
        assert_eq!(b.status, "available");
    }
}

#[tokio::test]
async fn backends_empty_when_none_registered() {
    let resp = get(router(empty_server_state()), "/backends").await;
    let list: ListBackendsResponse = body_as(resp).await;
    assert!(list.backends.is_empty());
}

#[tokio::test]
async fn backends_returns_json_content_type() {
    let resp = get(router(server_state()), "/backends").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn backend_registration_reflected_in_list() {
    let state = Arc::new(ServerState::default());
    state.backends.register("dynamic-backend".into()).await;
    let resp = get(router(state), "/backends").await;
    let list: ListBackendsResponse = body_as(resp).await;
    assert!(list.backends.iter().any(|b| b.name == "dynamic-backend"));
}

#[tokio::test]
async fn backend_duplicate_registration_idempotent() {
    let state = Arc::new(ServerState::default());
    state.backends.register("dup".into()).await;
    state.backends.register("dup".into()).await;
    assert_eq!(state.backends.len().await, 1);
}

#[tokio::test]
async fn backend_list_preserves_insertion_order() {
    let state = Arc::new(ServerState::default());
    state.backends.register("alpha".into()).await;
    state.backends.register("beta".into()).await;
    state.backends.register("gamma".into()).await;
    let names = state.backends.list().await;
    assert_eq!(names, vec!["alpha", "beta", "gamma"]);
}

#[tokio::test]
async fn backend_contains_check_works() {
    let state = Arc::new(ServerState::default());
    state.backends.register("present".into()).await;
    assert!(state.backends.contains("present").await);
    assert!(!state.backends.contains("absent").await);
}

// ===========================================================================
// 3. Run endpoint (server::router)
// ===========================================================================

#[tokio::test]
async fn run_valid_request_returns_201() {
    let body = serde_json::json!({"task": "hello world"});
    let resp = post_json(router(server_state()), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_returns_valid_uuid() {
    let body = serde_json::json!({"task": "hello"});
    let resp = post_json(router(server_state()), "/run", &body).await;
    let json = body_json(resp).await;
    let run_id = json["run_id"].as_str().unwrap();
    assert!(run_id.parse::<Uuid>().is_ok());
}

#[tokio::test]
async fn run_returns_queued_status() {
    let body = serde_json::json!({"task": "test"});
    let resp = post_json(router(server_state()), "/run", &body).await;
    let run_resp: ApiRunResponse = body_as(resp).await;
    assert_eq!(run_resp.status, ApiRunStatus::Queued);
}

#[tokio::test]
async fn run_empty_task_returns_400() {
    let body = serde_json::json!({"task": ""});
    let resp = post_json(router(server_state()), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_empty_task_error_mentions_task() {
    let body = serde_json::json!({"task": ""});
    let resp = post_json(router(server_state()), "/run", &body).await;
    let json = body_json(resp).await;
    assert!(json["error"].as_str().unwrap().contains("task"));
}

#[tokio::test]
async fn run_invalid_json_returns_400() {
    let resp = post_raw(
        router(server_state()),
        "/run",
        "application/json",
        "not valid json",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_missing_task_field_returns_422() {
    let body = serde_json::json!({"not_task": "hello"});
    let resp = post_json(router(server_state()), "/run", &body).await;
    // axum returns 422 for missing required fields during deserialization.
    assert!(
        resp.status() == StatusCode::UNPROCESSABLE_ENTITY
            || resp.status() == StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn run_unknown_backend_returns_400() {
    let body = serde_json::json!({"task": "hello", "backend": "nonexistent"});
    let resp = post_json(router(server_state()), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_unknown_backend_error_mentions_backend_name() {
    let body = serde_json::json!({"task": "hello", "backend": "nonexistent"});
    let resp = post_json(router(server_state()), "/run", &body).await;
    let json = body_json(resp).await;
    assert!(json["error"].as_str().unwrap().contains("nonexistent"));
}

#[tokio::test]
async fn run_without_backend_defaults_ok() {
    let body = serde_json::json!({"task": "test"});
    let resp = post_json(router(server_state()), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_known_backend_accepted() {
    let body = serde_json::json!({"task": "test", "backend": "mock"});
    let resp = post_json(router(server_state()), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_missing_content_type_returns_415() {
    let resp = router(server_state())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/run")
                .body(Body::from(r#"{"task":"hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn run_with_config_field_accepted() {
    let body = serde_json::json!({"task": "test", "config": {"model": "gpt-4"}});
    let resp = post_json(router(server_state()), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_records_run_in_registry() {
    let state = server_state();
    let body = serde_json::json!({"task": "test"});
    let resp = post_json(router(state.clone()), "/run", &body).await;
    let json = body_json(resp).await;
    let run_id: Uuid = json["run_id"].as_str().unwrap().parse().unwrap();
    let record = state.registry.get(run_id).await;
    assert!(record.is_some(), "run should be tracked in registry");
}

#[tokio::test]
async fn run_consecutive_ids_are_unique() {
    let state = server_state();
    let body = serde_json::json!({"task": "test"});
    let resp1 = post_json(router(state.clone()), "/run", &body).await;
    let id1 = body_json(resp1).await["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    let resp2 = post_json(router(state.clone()), "/run", &body).await;
    let id2 = body_json(resp2).await["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(id1, id2);
}

// ===========================================================================
// 4. Full-app run endpoint (build_app with Runtime + MockBackend)
// ===========================================================================

#[tokio::test]
async fn full_run_with_mock_backend_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = post_json(build_app(state), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn full_run_response_includes_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = post_json(build_app(state), "/run", &req).await;
    let json = body_json(resp).await;
    assert!(
        json.get("receipt").is_some(),
        "response should include receipt"
    );
}

#[tokio::test]
async fn full_run_response_includes_run_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = post_json(build_app(state), "/run", &req).await;
    let json = body_json(resp).await;
    assert!(json.get("run_id").is_some());
}

#[tokio::test]
async fn full_run_response_includes_backend_name() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = post_json(build_app(state), "/run", &req).await;
    let json = body_json(resp).await;
    assert_eq!(json["backend"], "mock");
}

// ===========================================================================
// 5. Status and metrics (build_app)
// ===========================================================================

#[tokio::test]
async fn status_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/status").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn status_includes_status_field() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/status").await;
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn status_includes_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/status").await;
    let json = body_json(resp).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn status_includes_backends_list() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/status").await;
    let json = body_json(resp).await;
    let backends = json["backends"].as_array().unwrap();
    assert!(backends.contains(&serde_json::json!("mock")));
}

#[tokio::test]
async fn status_initially_has_no_active_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/status").await;
    let json = body_json(resp).await;
    assert_eq!(json["active_runs"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn status_total_runs_starts_at_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/status").await;
    let json = body_json(resp).await;
    assert_eq!(json["total_runs"], 0);
}

#[tokio::test]
async fn status_deserializes_to_status_response() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/status").await;
    let status: StatusResponse = body_as(resp).await;
    assert_eq!(status.status, "ok");
}

#[tokio::test]
async fn metrics_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/metrics").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_initially_all_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/metrics").await;
    let m: RunMetrics = body_as(resp).await;
    assert_eq!(m.total_runs, 0);
    assert_eq!(m.running, 0);
    assert_eq!(m.completed, 0);
    assert_eq!(m.failed, 0);
}

#[tokio::test]
async fn metrics_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/metrics").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn metrics_update_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = post_json(build_app(state.clone()), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = get(build_app(state), "/metrics").await;
    let m: RunMetrics = body_as(resp).await;
    assert!(m.completed >= 1);
    assert!(m.total_runs >= 1);
}

// ===========================================================================
// 6. Version endpoint (server::router)
// ===========================================================================

#[tokio::test]
async fn version_returns_200() {
    let resp = get(router(server_state()), "/version").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn version_includes_contract_version() {
    let resp = get(router(server_state()), "/version").await;
    let json = body_json(resp).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn version_includes_crate_version() {
    let resp = get(router(server_state()), "/version").await;
    let json = body_json(resp).await;
    assert!(!json["version"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn version_deserializes_to_version_response() {
    let resp = get(router(server_state()), "/version").await;
    let ver: VersionResponse = body_as(resp).await;
    assert_eq!(ver.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn version_content_type_is_json() {
    let resp = get(router(server_state()), "/version").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

// ===========================================================================
// 7. Error handling and routing
// ===========================================================================

#[tokio::test]
async fn unknown_route_returns_404() {
    let resp = get(router(server_state()), "/nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_to_health_returns_405() {
    let body = serde_json::json!({});
    let resp = post_json(router(server_state()), "/health", &body).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn get_to_run_returns_405() {
    let resp = get(router(server_state()), "/run").await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn post_to_backends_returns_405() {
    let body = serde_json::json!({});
    let resp = post_json(router(server_state()), "/backends", &body).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn post_to_version_returns_405() {
    let body = serde_json::json!({});
    let resp = post_json(router(server_state()), "/version", &body).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn run_error_response_has_error_field() {
    let body = serde_json::json!({"task": ""});
    let resp = post_json(router(server_state()), "/run", &body).await;
    let json = body_json(resp).await;
    assert!(
        json.get("error").is_some(),
        "error response must have 'error' field"
    );
}

#[tokio::test]
async fn run_error_response_has_code_field() {
    let body = serde_json::json!({"task": ""});
    let resp = post_json(router(server_state()), "/run", &body).await;
    let json = body_json(resp).await;
    let err_resp: ErrorResponse = serde_json::from_value(json).unwrap();
    assert!(
        err_resp.code.is_some(),
        "error response should include code"
    );
}

#[tokio::test]
async fn run_error_response_deserializes_to_error_response() {
    let body = serde_json::json!({"task": ""});
    let resp = post_json(router(server_state()), "/run", &body).await;
    let err: ErrorResponse = body_as(resp).await;
    assert!(!err.error.is_empty());
}

#[tokio::test]
async fn error_response_content_type_is_json() {
    let body = serde_json::json!({"task": ""});
    let resp = post_json(router(server_state()), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

// ===========================================================================
// 8. Full-app additional routes
// ===========================================================================

#[tokio::test]
async fn full_app_backends_returns_json_array() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/backends").await;
    let json = body_json(resp).await;
    assert!(json.is_array());
    let arr = json.as_array().unwrap();
    assert!(arr.contains(&serde_json::json!("mock")));
}

#[tokio::test]
async fn full_app_health_returns_time_field() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/health").await;
    let json = body_json(resp).await;
    assert!(json.get("time").is_some());
}

#[tokio::test]
async fn full_app_config_returns_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/config").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn full_app_config_returns_backends() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/config").await;
    let json = body_json(resp).await;
    assert!(json["backends"].is_array());
}

#[tokio::test]
async fn full_app_runs_list_initially_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/runs").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn full_app_receipts_list_initially_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/receipts").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn full_app_receipts_with_limit_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/receipts?limit=0").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn full_app_unknown_run_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = get(build_app(app_state(tmp.path())), &format!("/runs/{id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn full_app_unknown_receipt_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = get(build_app(app_state(tmp.path())), &format!("/receipts/{id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn full_app_delete_unknown_run_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = delete(build_app(app_state(tmp.path())), &format!("/runs/{id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn full_app_cancel_unknown_run_returns_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = post_json(
        build_app(app_state(tmp.path())),
        &format!("/runs/{id}/cancel"),
        &serde_json::json!({}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

// ===========================================================================
// 9. Schema endpoint
// ===========================================================================

#[tokio::test]
async fn schema_work_order_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/schema/work_order").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_receipt_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/schema/receipt").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_unknown_type_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/schema/bogus").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn schema_response_is_valid_json_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(app_state(tmp.path())), "/schema/work_order").await;
    let json = body_json(resp).await;
    // All JSON schemas must have a "$schema" or "type" or "properties" key.
    assert!(
        json.get("$schema").is_some()
            || json.get("type").is_some()
            || json.get("properties").is_some(),
        "response should look like a JSON schema"
    );
}

// ===========================================================================
// 10. Versioned API (build_versioned_app)
// ===========================================================================

#[tokio::test]
async fn v1_health_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_versioned_app(app_state(tmp.path())), "/api/v1/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn v1_backends_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(
        build_versioned_app(app_state(tmp.path())),
        "/api/v1/backends",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn v1_legacy_health_still_works() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_versioned_app(app_state(tmp.path())), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn v1_validate_valid_request() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = post_json(build_versioned_app(state), "/api/v1/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["valid"], true);
}

#[tokio::test]
async fn v1_validate_empty_task_returns_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = post_json(build_versioned_app(state), "/api/v1/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["valid"], false);
    assert!(!json["errors"].as_array().unwrap().is_empty());
}

// ===========================================================================
// 11. Run lifecycle (RunRegistry unit tests through API)
// ===========================================================================

#[tokio::test]
async fn run_registry_create_and_get() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Queued);
}

#[tokio::test]
async fn run_registry_duplicate_id_rejected() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    assert!(reg.create_run(id, "mock".into()).await.is_err());
}

#[tokio::test]
async fn run_registry_transition_queued_to_running() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let phase = reg.transition(id, RunPhase::Running).await.unwrap();
    assert_eq!(phase, RunPhase::Running);
}

#[tokio::test]
async fn run_registry_invalid_transition_rejected() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    // Queued → Completed is invalid (must go through Running).
    assert!(reg.transition(id, RunPhase::Completed).await.is_err());
}

#[tokio::test]
async fn run_registry_cancel_from_queued() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.cancel(id).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Cancelled);
}

#[tokio::test]
async fn run_registry_complete_from_running() {
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
async fn run_registry_fail_from_running() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    reg.fail(id, "something broke".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Failed);
    assert_eq!(record.error.as_deref(), Some("something broke"));
}

#[tokio::test]
async fn run_registry_remove_completed() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    reg.complete(id, receipt).await.unwrap();
    let removed = reg.remove(id).await.unwrap();
    assert_eq!(removed.phase, RunPhase::Completed);
    assert!(reg.get(id).await.is_none());
}

#[tokio::test]
async fn run_registry_remove_running_rejected() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    assert!(reg.remove(id).await.is_err());
}

#[tokio::test]
async fn run_registry_count_by_phase() {
    let reg = RunRegistry::new();
    for _ in 0..3 {
        reg.create_run(Uuid::new_v4(), "mock".into()).await.unwrap();
    }
    assert_eq!(reg.count_by_phase(RunPhase::Queued).await, 3);
    assert_eq!(reg.count_by_phase(RunPhase::Running).await, 0);
}

// ===========================================================================
// 12. Concurrent requests
// ===========================================================================

#[tokio::test]
async fn concurrent_health_checks_all_succeed() {
    let state = server_state();
    let mut handles = Vec::new();
    for _ in 0..20 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let resp = get(router(s), "/health").await;
            resp.status()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), StatusCode::OK);
    }
}

#[tokio::test]
async fn concurrent_runs_all_get_unique_ids() {
    let state = server_state();
    let mut handles = Vec::new();
    for i in 0..10 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let body = serde_json::json!({"task": format!("task-{i}")});
            let resp = post_json(router(s), "/run", &body).await;
            let json = body_json(resp).await;
            json["run_id"].as_str().unwrap().to_string()
        }));
    }
    let mut ids = std::collections::HashSet::new();
    for h in handles {
        let id = h.await.unwrap();
        assert!(ids.insert(id), "duplicate run_id detected");
    }
    assert_eq!(ids.len(), 10);
}
