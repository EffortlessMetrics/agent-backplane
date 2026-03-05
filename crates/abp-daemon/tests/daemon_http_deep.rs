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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Comprehensive deep tests for the daemon HTTP API surface.
//!
//! Covers both the lightweight `server::router` (backed by `ServerState`) and
//! the full `build_app` / `build_versioned_app` routers (backed by `AppState`
//! with a real `Runtime` + `MockBackend`).

use abp_core::{
    CONTRACT_VERSION, CapabilityRequirements, ContextPacket, ExecutionLane, Outcome, PolicyProfile,
    ReceiptBuilder, RuntimeConfig, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::api::{
    HealthResponse as V1HealthResponse, ListBackendsResponse, RunRequest as V1RunRequest,
};
use abp_daemon::middleware::{CorsConfig, RateLimiter};
use abp_daemon::queue::{QueuePriority, QueuedRun, RunQueue};
use abp_daemon::routes::{Endpoint, MatchResult, Method, RouteTable, api_routes};
use abp_daemon::server::{DaemonServer, VersionResponse, router as server_router};
use abp_daemon::state::{BackendList, RunPhase, RunRegistry, ServerState};
use abp_daemon::validation::RequestValidator;
use abp_daemon::versioning::{ApiVersion, VersionNegotiator};
use abp_daemon::{
    AppState, BackendInfo, DaemonConfig, DaemonError, DaemonState, RunMetrics, RunRequest,
    RunStatus, RunTracker, StatusResponse, ValidationResponse, build_app, build_versioned_app,
};
use abp_integrations::MockBackend;
use abp_runtime::Runtime;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use http_body_util::BodyExt;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tower::ServiceExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn server_state(backends: Vec<&str>) -> Arc<ServerState> {
    Arc::new(ServerState::new(
        backends.into_iter().map(String::from).collect(),
    ))
}

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

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "http deep test task".into(),
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

fn test_run_request() -> RunRequest {
    RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    }
}

async fn send_get(router: axum::Router, uri: &str) -> http::Response<Body> {
    router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn send_post_json(
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

async fn send_post_raw(router: axum::Router, uri: &str, body: &str) -> http::Response<Body> {
    router
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(uri)
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn send_post_no_content_type(
    router: axum::Router,
    uri: &str,
    body: &str,
) -> http::Response<Body> {
    router
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(uri)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn send_delete(router: axum::Router, uri: &str) -> http::Response<Body> {
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

async fn body_json(resp: http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_deser<T: serde::de::DeserializeOwned>(resp: http::Response<Body>) -> T {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ===========================================================================
// 1. Health endpoint (GET /health)
// ===========================================================================

#[tokio::test]
async fn health_returns_200() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_status_is_ok() {
    let json =
        body_json(send_get(server_router(server_state(vec!["mock"])), "/health").await).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_contract_version_matches() {
    let json =
        body_json(send_get(server_router(server_state(vec!["mock"])), "/health").await).await;
    assert_eq!(json["version"], CONTRACT_VERSION);
}

#[tokio::test]
async fn health_has_uptime_secs() {
    let json =
        body_json(send_get(server_router(server_state(vec!["mock"])), "/health").await).await;
    assert!(json["uptime_secs"].is_number());
}

#[tokio::test]
async fn health_lists_backends() {
    let state = server_state(vec!["mock", "sidecar:node"]);
    let json = body_json(send_get(server_router(state), "/health").await).await;
    assert_eq!(json["backends"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn health_content_type_is_json() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/health").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn health_deserializes_to_typed_response() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/health").await;
    let health: V1HealthResponse = body_deser(resp).await;
    assert_eq!(health.status, "ok");
    assert_eq!(health.version, CONTRACT_VERSION);
}

#[tokio::test]
async fn health_no_backends_returns_empty_array() {
    let json = body_json(send_get(server_router(server_state(vec![])), "/health").await).await;
    assert!(json["backends"].as_array().unwrap().is_empty());
}

// --- build_app health ---
#[tokio::test]
async fn build_app_health_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn build_app_health_has_time_field() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/health").await).await;
    assert!(json.get("time").is_some());
}

// ===========================================================================
// 2. Run endpoint (POST /run)
// ===========================================================================

#[tokio::test]
async fn run_with_mock_backend_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_post_json(build_app(state), "/run", &test_run_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn run_response_has_run_id() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_post_json(build_app(state), "/run", &test_run_request()).await).await;
    assert!(json.get("run_id").is_some());
}

#[tokio::test]
async fn run_response_has_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_post_json(build_app(state), "/run", &test_run_request()).await).await;
    assert!(json.get("receipt").is_some());
}

#[tokio::test]
async fn run_response_has_events_array() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_post_json(build_app(state), "/run", &test_run_request()).await).await;
    assert!(json["events"].is_array());
}

#[tokio::test]
async fn run_response_receipt_has_meta() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_post_json(build_app(state), "/run", &test_run_request()).await).await;
    assert!(json["receipt"]["meta"].is_object());
}

#[tokio::test]
async fn run_persists_receipt_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_post_json(build_app(state), "/run", &test_run_request()).await).await;
    let run_id = json["run_id"].as_str().unwrap();
    let path = dir.path().join(format!("{run_id}.json"));
    assert!(path.exists());
}

// --- server::router POST /run ---
#[tokio::test]
async fn server_run_empty_task_returns_400() {
    let state = server_state(vec!["mock"]);
    let req = V1RunRequest {
        task: "".into(),
        backend: None,
        config: None,
    };
    let resp = send_post_json(server_router(state), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn server_run_valid_task_returns_201() {
    let state = server_state(vec!["mock"]);
    let req = V1RunRequest {
        task: "do something".into(),
        backend: None,
        config: None,
    };
    let resp = send_post_json(server_router(state), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn server_run_unknown_backend_returns_400() {
    let state = server_state(vec!["mock"]);
    let req = V1RunRequest {
        task: "do something".into(),
        backend: Some("nonexistent".into()),
        config: None,
    };
    let resp = send_post_json(server_router(state), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn server_run_response_has_run_id() {
    let state = server_state(vec!["mock"]);
    let req = V1RunRequest {
        task: "do something".into(),
        backend: None,
        config: None,
    };
    let json = body_json(send_post_json(server_router(state), "/run", &req).await).await;
    assert!(json.get("run_id").is_some());
}

// ===========================================================================
// 3. Backends endpoint (GET /backends)
// ===========================================================================

#[tokio::test]
async fn backends_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn backends_returns_registered_names() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/backends").await).await;
    let arr = json.as_array().unwrap();
    assert!(arr.iter().any(|v| v.as_str() == Some("mock")));
}

#[tokio::test]
async fn server_backends_returns_200() {
    let state = server_state(vec!["mock", "sidecar:node"]);
    let resp = send_get(server_router(state), "/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn server_backends_deserializes_to_list() {
    let state = server_state(vec!["alpha", "beta"]);
    let list: ListBackendsResponse =
        body_deser(send_get(server_router(state), "/backends").await).await;
    assert_eq!(list.backends.len(), 2);
}

#[tokio::test]
async fn server_backends_has_name_field() {
    let state = server_state(vec!["mock"]);
    let list: ListBackendsResponse =
        body_deser(send_get(server_router(state), "/backends").await).await;
    assert_eq!(list.backends[0].name, "mock");
}

// ===========================================================================
// 4. Receipt endpoint (GET /receipts/:id)
// ===========================================================================

#[tokio::test]
async fn receipt_not_found_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let id = Uuid::new_v4();
    let resp = send_get(build_app(state), &format!("/receipts/{id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn receipt_after_run_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let run_resp =
        body_json(send_post_json(build_app(state.clone()), "/run", &test_run_request()).await)
            .await;
    let run_id = run_resp["run_id"].as_str().unwrap();
    let resp = send_get(build_app(state), &format!("/receipts/{run_id}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn receipt_has_meta_run_id() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let run_resp =
        body_json(send_post_json(build_app(state.clone()), "/run", &test_run_request()).await)
            .await;
    let run_id = run_resp["run_id"].as_str().unwrap();
    let json = body_json(send_get(build_app(state), &format!("/receipts/{run_id}")).await).await;
    assert!(json["meta"]["run_id"].is_string());
}

#[tokio::test]
async fn receipt_has_outcome_field() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let run_resp =
        body_json(send_post_json(build_app(state.clone()), "/run", &test_run_request()).await)
            .await;
    let run_id = run_resp["run_id"].as_str().unwrap();
    let json = body_json(send_get(build_app(state), &format!("/receipts/{run_id}")).await).await;
    assert!(json.get("outcome").is_some());
}

// --- receipts list ---
#[tokio::test]
async fn receipts_list_empty_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/receipts").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn receipts_list_with_limit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/receipts?limit=0").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json.as_array().unwrap().is_empty());
}

// ===========================================================================
// 5. Validate endpoint (POST /validate)
// ===========================================================================

#[tokio::test]
async fn validate_valid_request_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let req = test_run_request();
    let resp = send_post_json(build_app(state), "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn validate_valid_request_returns_valid_true() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let req = test_run_request();
    let json = body_json(send_post_json(build_app(state), "/validate", &req).await).await;
    assert_eq!(json["valid"], true);
}

#[tokio::test]
async fn validate_unknown_backend_returns_400() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let mut req = test_run_request();
    req.backend = "nonexistent".into();
    let resp = send_post_json(build_app(state), "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn validate_empty_task_returns_400() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let mut wo = test_work_order();
    wo.task = "".into();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = send_post_json(build_app(state), "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --- v1 validate ---
#[tokio::test]
async fn validate_v1_valid_request_returns_valid() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let req = test_run_request();
    let json =
        body_json(send_post_json(build_versioned_app(state), "/api/v1/validate", &req).await).await;
    assert_eq!(json["valid"], true);
    assert!(json.get("errors").is_none() || json["errors"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn validate_v1_bad_backend_returns_errors() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let mut req = test_run_request();
    req.backend = "unknown_backend".into();
    let resp: ValidationResponse =
        body_deser(send_post_json(build_versioned_app(state), "/api/v1/validate", &req).await)
            .await;
    assert!(!resp.valid);
    assert!(!resp.errors.is_empty());
}

// ===========================================================================
// 6. Schema endpoint (GET /schemas/:type)
// ===========================================================================

#[tokio::test]
async fn schema_work_order_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/schema/work_order").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_work_order_has_title() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/schema/work_order").await).await;
    assert!(json.get("title").is_some() || json.get("$schema").is_some());
}

#[tokio::test]
async fn schema_receipt_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/schema/receipt").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_capability_requirements_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/schema/capability_requirements").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_backplane_config_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/schema/backplane_config").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_unknown_type_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/schema/nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn schema_response_is_valid_json_schema() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/schema/work_order").await).await;
    // JSON Schema must have "type" or "$ref" or "properties" at root
    assert!(
        json.get("type").is_some()
            || json.get("$ref").is_some()
            || json.get("properties").is_some()
            || json.get("$schema").is_some()
    );
}

// ===========================================================================
// 7. Config endpoint (GET /config)
// ===========================================================================

#[tokio::test]
async fn config_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/config").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_has_backends() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/config").await).await;
    assert!(json.get("backends").is_some());
}

#[tokio::test]
async fn config_has_contract_version() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/config").await).await;
    assert_eq!(json["contract_version"], CONTRACT_VERSION);
}

#[tokio::test]
async fn config_has_receipts_dir() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/config").await).await;
    assert!(json.get("receipts_dir").is_some());
}

// ===========================================================================
// 8. Error responses (400, 404, 500)
// ===========================================================================

#[tokio::test]
async fn not_found_route_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_run_nonexistent_id_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let id = Uuid::new_v4();
    let resp = send_get(build_app(state), &format!("/runs/{id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_nonexistent_run_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let id = Uuid::new_v4();
    let resp = send_delete(build_app(state), &format!("/runs/{id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn daemon_error_not_found_status_code() {
    let err = DaemonError::NotFound("gone".into());
    assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn daemon_error_bad_request_status_code() {
    let err = DaemonError::BadRequest("bad".into());
    assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn daemon_error_conflict_status_code() {
    let err = DaemonError::Conflict("conflict".into());
    assert_eq!(err.status_code(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn daemon_error_internal_status_code() {
    let err = DaemonError::Internal(anyhow::anyhow!("boom"));
    assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ===========================================================================
// 9. Request validation (missing fields, invalid JSON)
// ===========================================================================

#[tokio::test]
async fn invalid_json_body_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_post_raw(build_app(state), "/run", "not valid json").await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn missing_backend_field_in_run_request_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = serde_json::json!({"work_order": {}});
    let resp = send_post_raw(build_app(state), "/run", &json.to_string()).await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn empty_object_body_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_post_raw(build_app(state), "/run", "{}").await;
    assert!(resp.status().is_client_error());
}

#[test]
fn validate_run_id_empty_rejected() {
    assert!(RequestValidator::validate_run_id("").is_err());
}

#[test]
fn validate_run_id_invalid_format_rejected() {
    assert!(RequestValidator::validate_run_id("not-uuid").is_err());
}

#[test]
fn validate_run_id_valid_uuid_accepted() {
    assert!(RequestValidator::validate_run_id(&Uuid::new_v4().to_string()).is_ok());
}

#[test]
fn validate_backend_name_empty_rejected() {
    assert!(RequestValidator::validate_backend_name("", &["mock".into()]).is_err());
}

#[test]
fn validate_backend_name_unknown_rejected() {
    assert!(RequestValidator::validate_backend_name("x", &["mock".into()]).is_err());
}

#[test]
fn validate_work_order_empty_task_rejected() {
    let mut wo = test_work_order();
    wo.task = "".into();
    assert!(RequestValidator::validate_work_order(&wo).is_err());
}

#[test]
fn validate_work_order_whitespace_task_rejected() {
    let mut wo = test_work_order();
    wo.task = "   ".into();
    assert!(RequestValidator::validate_work_order(&wo).is_err());
}

#[test]
fn validate_work_order_empty_root_rejected() {
    let mut wo = test_work_order();
    wo.workspace.root = "".into();
    assert!(RequestValidator::validate_work_order(&wo).is_err());
}

#[test]
fn validate_work_order_negative_budget_rejected() {
    let mut wo = test_work_order();
    wo.config.max_budget_usd = Some(-1.0);
    assert!(RequestValidator::validate_work_order(&wo).is_err());
}

#[test]
fn validate_config_non_object_rejected() {
    assert!(RequestValidator::validate_config(&serde_json::json!("string")).is_err());
}

#[test]
fn validate_config_array_rejected() {
    assert!(RequestValidator::validate_config(&serde_json::json!([1, 2])).is_err());
}

#[test]
fn validate_config_valid_object_accepted() {
    assert!(RequestValidator::validate_config(&serde_json::json!({"a": 1})).is_ok());
}

// ===========================================================================
// 10. Content-Type handling
// ===========================================================================

#[tokio::test]
async fn post_without_content_type_still_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let body = serde_json::to_string(&test_run_request()).unwrap();
    let resp = send_post_no_content_type(build_app(state), "/run", &body).await;
    // Axum may reject or accept depending on extractor; just ensure no panic
    assert!(resp.status().is_client_error() || resp.status().is_success());
}

#[tokio::test]
async fn post_with_wrong_content_type_handled() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let body = serde_json::to_string(&test_run_request()).unwrap();
    let resp = build_app(state)
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/run")
                .header(http::header::CONTENT_TYPE, "text/plain")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    // Axum Json extractor rejects non-json content types
    assert!(resp.status().is_client_error());
}

// ===========================================================================
// 11. WebSocket (ws) endpoint wiring
// ===========================================================================

#[tokio::test]
async fn ws_endpoint_requires_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    // A plain GET to /ws without upgrade headers should not succeed as ws
    let resp = send_get(build_app(state), "/ws").await;
    // Non-upgrade GET should fail
    assert!(resp.status().is_client_error() || resp.status().is_server_error());
}

// ===========================================================================
// 12. Concurrent requests
// ===========================================================================

#[tokio::test]
async fn concurrent_health_requests() {
    let state = server_state(vec!["mock"]);
    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let resp = send_get(server_router(s), "/health").await;
            resp.status()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), StatusCode::OK);
    }
}

#[tokio::test]
async fn concurrent_run_requests() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let mut handles = Vec::new();
    for _ in 0..5 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let resp = send_post_json(build_app(s), "/run", &test_run_request()).await;
            resp.status()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), StatusCode::OK);
    }
}

#[tokio::test]
async fn concurrent_backend_list_requests() {
    let state = server_state(vec!["a", "b", "c"]);
    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let resp = send_get(server_router(s), "/backends").await;
            resp.status()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), StatusCode::OK);
    }
}

// ===========================================================================
// 13. Graceful shutdown
// ===========================================================================

#[tokio::test]
async fn daemon_server_builds_router() {
    let state = server_state(vec!["mock"]);
    let server = DaemonServer::new(state);
    let router = server.router();
    let resp = send_get(router, "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn daemon_server_state_accessible() {
    let state = server_state(vec!["mock"]);
    let server = DaemonServer::new(state.clone());
    assert_eq!(server.state().uptime_secs(), state.uptime_secs());
}

// ===========================================================================
// 14. CORS headers
// ===========================================================================

#[test]
fn cors_config_to_layer_does_not_panic() {
    let config = CorsConfig {
        allowed_origins: vec!["http://localhost:3000".into()],
        allowed_methods: vec!["GET".into(), "POST".into()],
        allowed_headers: vec!["content-type".into(), "authorization".into()],
    };
    let _layer = config.to_cors_layer();
}

#[test]
fn cors_config_empty_origins() {
    let config = CorsConfig {
        allowed_origins: vec![],
        allowed_methods: vec!["GET".into()],
        allowed_headers: vec![],
    };
    let _layer = config.to_cors_layer();
}

#[test]
fn cors_config_multiple_origins() {
    let config = CorsConfig {
        allowed_origins: vec!["http://localhost:3000".into(), "https://example.com".into()],
        allowed_methods: vec!["GET".into(), "POST".into(), "DELETE".into()],
        allowed_headers: vec!["content-type".into()],
    };
    let _layer = config.to_cors_layer();
}

// ===========================================================================
// Additional: Status endpoint (GET /status)
// ===========================================================================

#[tokio::test]
async fn status_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/status").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn status_has_contract_version() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/status").await).await;
    assert_eq!(json["contract_version"], CONTRACT_VERSION);
}

#[tokio::test]
async fn status_has_active_runs_list() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/status").await).await;
    assert!(json["active_runs"].is_array());
}

// ===========================================================================
// Additional: Metrics endpoint (GET /metrics)
// ===========================================================================

#[tokio::test]
async fn metrics_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/metrics").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_has_total_runs() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/metrics").await).await;
    assert!(json["total_runs"].is_number());
}

#[tokio::test]
async fn metrics_zero_initially() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/metrics").await).await;
    assert_eq!(json["total_runs"], 0);
    assert_eq!(json["running"], 0);
    assert_eq!(json["failed"], 0);
}

// ===========================================================================
// Additional: Version endpoint (GET /version — server router)
// ===========================================================================

#[tokio::test]
async fn version_returns_200() {
    let state = server_state(vec!["mock"]);
    let resp = send_get(server_router(state), "/version").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn version_has_contract_version() {
    let state = server_state(vec!["mock"]);
    let ver: VersionResponse = body_deser(send_get(server_router(state), "/version").await).await;
    assert_eq!(ver.contract_version, CONTRACT_VERSION);
}

// ===========================================================================
// Additional: Runs lifecycle (list, get, cancel, delete)
// ===========================================================================

#[tokio::test]
async fn runs_list_initially_empty() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let json = body_json(send_get(build_app(state), "/runs").await).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn runs_list_after_run_not_empty() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let _ = send_post_json(build_app(state.clone()), "/run", &test_run_request()).await;
    let json = body_json(send_get(build_app(state), "/runs").await).await;
    assert!(!json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_run_after_completion_returns_status() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let run_resp =
        body_json(send_post_json(build_app(state.clone()), "/run", &test_run_request()).await)
            .await;
    let run_id = run_resp["run_id"].as_str().unwrap();
    let json = body_json(send_get(build_app(state), &format!("/runs/{run_id}")).await).await;
    assert!(json.get("run_id").is_some());
}

#[tokio::test]
async fn cancel_nonexistent_run_returns_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let id = Uuid::new_v4();
    let resp = send_post_json(
        build_app(state),
        &format!("/runs/{id}/cancel"),
        &serde_json::json!({}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn run_receipt_endpoint_returns_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let run_resp =
        body_json(send_post_json(build_app(state.clone()), "/run", &test_run_request()).await)
            .await;
    let run_id = run_resp["run_id"].as_str().unwrap();
    let json =
        body_json(send_get(build_app(state), &format!("/runs/{run_id}/receipt")).await).await;
    assert!(json.get("meta").is_some());
}

// ===========================================================================
// Additional: Serde roundtrip tests for daemon types
// ===========================================================================

#[test]
fn daemon_config_default_serde_roundtrip() {
    let cfg = DaemonConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: DaemonConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.bind_address, "127.0.0.1");
    assert_eq!(back.port, 8088);
    assert!(back.auth_token.is_none());
}

#[test]
fn daemon_config_bind_string() {
    let cfg = DaemonConfig::default();
    assert_eq!(cfg.bind_string(), "127.0.0.1:8088");
}

#[test]
fn daemon_config_with_auth_token() {
    let cfg = DaemonConfig {
        auth_token: Some("secret".into()),
        ..DaemonConfig::default()
    };
    let json = serde_json::to_value(&cfg).unwrap();
    assert_eq!(json["auth_token"], "secret");
}

#[test]
fn run_status_serde_pending_variant() {
    let s = RunStatus::Pending;
    let json = serde_json::to_string(&s).unwrap();
    let back: RunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, RunStatus::Pending));
}

#[test]
fn run_status_serde_completed_variant() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let s = RunStatus::Completed {
        receipt: Box::new(receipt),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: RunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, RunStatus::Completed { .. }));
}

#[test]
fn run_status_serde_failed_variant() {
    let s = RunStatus::Failed {
        error: "boom".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: RunStatus = serde_json::from_str(&json).unwrap();
    match back {
        RunStatus::Failed { error } => assert_eq!(error, "boom"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn run_status_serde_cancelled_variant() {
    let s = RunStatus::Cancelled;
    let json = serde_json::to_string(&s).unwrap();
    let back: RunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, RunStatus::Cancelled));
}

#[test]
fn run_metrics_serde_roundtrip() {
    let m = RunMetrics {
        total_runs: 10,
        running: 2,
        completed: 7,
        failed: 1,
    };
    let json = serde_json::to_string(&m).unwrap();
    let back: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_runs, 10);
    assert_eq!(back.running, 2);
}

#[test]
fn status_response_serde_roundtrip() {
    let s = StatusResponse {
        status: "ok".into(),
        contract_version: CONTRACT_VERSION.into(),
        backends: vec!["mock".into()],
        active_runs: vec![],
        total_runs: 0,
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: StatusResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.status, "ok");
    assert_eq!(back.backends.len(), 1);
}

#[test]
fn validation_response_valid_roundtrip() {
    let v = ValidationResponse {
        valid: true,
        errors: vec![],
    };
    let json = serde_json::to_string(&v).unwrap();
    let back: ValidationResponse = serde_json::from_str(&json).unwrap();
    assert!(back.valid);
}

#[test]
fn validation_response_invalid_roundtrip() {
    let v = ValidationResponse {
        valid: false,
        errors: vec!["task empty".into()],
    };
    let json = serde_json::to_string(&v).unwrap();
    let back: ValidationResponse = serde_json::from_str(&json).unwrap();
    assert!(!back.valid);
    assert_eq!(back.errors.len(), 1);
}

#[test]
fn run_request_serde_roundtrip() {
    let req = test_run_request();
    let json = serde_json::to_string(&req).unwrap();
    let back: RunRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.backend, "mock");
}

#[test]
fn backend_info_serde_roundtrip() {
    let info = BackendInfo {
        id: "mock".into(),
        capabilities: BTreeMap::new(),
    };
    let json = serde_json::to_string(&info).unwrap();
    let back: BackendInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "mock");
}

// ===========================================================================
// Additional: RunTracker unit tests
// ===========================================================================

#[tokio::test]
async fn run_tracker_start_and_complete() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, RunStatus::Running));

    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    tracker.complete_run(id, receipt).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, RunStatus::Completed { .. }));
}

#[tokio::test]
async fn run_tracker_fail_run() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    tracker.fail_run(id, "error".into()).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, RunStatus::Failed { .. }));
}

#[tokio::test]
async fn run_tracker_cancel_run() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    tracker.cancel_run(id).await.unwrap();
    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, RunStatus::Cancelled));
}

#[tokio::test]
async fn run_tracker_duplicate_start_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert!(tracker.start_run(id).await.is_err());
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

#[tokio::test]
async fn run_tracker_remove_completed() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    tracker.complete_run(id, receipt).await.unwrap();
    let removed = tracker.remove_run(id).await;
    assert!(removed.is_ok());
}

#[tokio::test]
async fn run_tracker_remove_running_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let removed = tracker.remove_run(id).await;
    assert!(removed.is_err());
}

// ===========================================================================
// Additional: DaemonState unit tests
// ===========================================================================

#[tokio::test]
async fn daemon_state_register_backend() {
    let state = DaemonState::new();
    state.register_backend("mock".into()).await;
    let names = state.backend_names().await;
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn daemon_state_register_deduplicate() {
    let state = DaemonState::new();
    state.register_backend("mock".into()).await;
    state.register_backend("mock".into()).await;
    assert_eq!(state.backend_names().await.len(), 1);
}

#[tokio::test]
async fn daemon_state_run_status() {
    let state = DaemonState::new();
    let id = Uuid::new_v4();
    state
        .set_run_status(
            id,
            abp_daemon::handler::RunStatus {
                id,
                state: abp_daemon::handler::RunState::Pending,
                receipt: None,
            },
        )
        .await;
    let status = state.get_run_status(id).await;
    assert!(status.is_some());
}

// ===========================================================================
// Additional: Route table matching
// ===========================================================================

#[test]
fn route_table_health_get() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Get, "/api/v1/health");
    assert_eq!(result, MatchResult::Matched(Endpoint::Health));
}

#[test]
fn route_table_health_post_method_not_allowed() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Post, "/api/v1/health");
    assert_eq!(result, MatchResult::MethodNotAllowed);
}

#[test]
fn route_table_backends_get() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Get, "/api/v1/backends");
    assert_eq!(result, MatchResult::Matched(Endpoint::ListBackends));
}

#[test]
fn route_table_runs_post() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Post, "/api/v1/runs");
    assert_eq!(result, MatchResult::Matched(Endpoint::SubmitRun));
}

#[test]
fn route_table_unknown_path() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Get, "/api/v1/nonexistent");
    assert_eq!(result, MatchResult::NotFound);
}

#[test]
fn route_table_runs_id_get() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Get, "/api/v1/runs/abc-123");
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::GetRun {
            run_id: "abc-123".into()
        })
    );
}

#[test]
fn route_table_runs_id_delete() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Delete, "/api/v1/runs/abc-123");
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::DeleteRun {
            run_id: "abc-123".into()
        })
    );
}

#[test]
fn route_table_runs_id_cancel() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Post, "/api/v1/runs/abc/cancel");
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::CancelRun {
            run_id: "abc".into()
        })
    );
}

#[test]
fn route_table_runs_id_events() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Get, "/api/v1/runs/abc/events");
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::GetRunEvents {
            run_id: "abc".into()
        })
    );
}

// ===========================================================================
// Additional: API versioning
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
fn api_version_parse_invalid() {
    assert!(ApiVersion::parse("").is_err());
    assert!(ApiVersion::parse("abc").is_err());
}

#[test]
fn api_version_compatibility() {
    let v1_0 = ApiVersion::parse("v1.0").unwrap();
    let v1_1 = ApiVersion::parse("v1.1").unwrap();
    let v2_0 = ApiVersion::parse("v2.0").unwrap();
    assert!(v1_0.is_compatible(&v1_1));
    assert!(!v1_0.is_compatible(&v2_0));
}

#[test]
fn api_version_ordering() {
    let v1_0 = ApiVersion::parse("v1.0").unwrap();
    let v1_1 = ApiVersion::parse("v1.1").unwrap();
    assert!(v1_0 < v1_1);
}

#[test]
fn version_negotiator_picks_highest_compatible() {
    let requested = ApiVersion::parse("v1.2").unwrap();
    let supported = vec![
        ApiVersion::parse("v1.0").unwrap(),
        ApiVersion::parse("v1.1").unwrap(),
        ApiVersion::parse("v1.2").unwrap(),
        ApiVersion::parse("v2.0").unwrap(),
    ];
    let result = VersionNegotiator::negotiate(&requested, &supported);
    assert_eq!(result, Some(ApiVersion::parse("v1.2").unwrap()));
}

#[test]
fn version_negotiator_no_compatible() {
    let requested = ApiVersion::parse("v3.0").unwrap();
    let supported = vec![
        ApiVersion::parse("v1.0").unwrap(),
        ApiVersion::parse("v2.0").unwrap(),
    ];
    let result = VersionNegotiator::negotiate(&requested, &supported);
    assert!(result.is_none());
}

// ===========================================================================
// Additional: Run registry (state module)
// ===========================================================================

#[tokio::test]
async fn registry_create_and_get() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Queued);
    assert_eq!(record.backend, "mock");
}

#[tokio::test]
async fn registry_transition_queued_to_running() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Running);
}

#[tokio::test]
async fn registry_invalid_transition_fails() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    // Cannot go directly from Queued to Completed
    let result = reg.transition(id, RunPhase::Completed).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn registry_complete_with_receipt() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    reg.complete(id, receipt).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Completed);
    assert!(record.receipt.is_some());
}

#[tokio::test]
async fn registry_fail_with_error() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    reg.fail(id, "boom".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Failed);
    assert_eq!(record.error.as_deref(), Some("boom"));
}

#[tokio::test]
async fn registry_cancel() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.cancel(id).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Cancelled);
}

#[tokio::test]
async fn registry_duplicate_id_fails() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    assert!(reg.create_run(id, "mock".into()).await.is_err());
}

#[tokio::test]
async fn registry_list_ids() {
    let reg = RunRegistry::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    reg.create_run(id1, "a".into()).await.unwrap();
    reg.create_run(id2, "b".into()).await.unwrap();
    let ids = reg.list_ids().await;
    assert_eq!(ids.len(), 2);
}

#[tokio::test]
async fn registry_count_by_phase() {
    let reg = RunRegistry::new();
    reg.create_run(Uuid::new_v4(), "a".into()).await.unwrap();
    reg.create_run(Uuid::new_v4(), "b".into()).await.unwrap();
    assert_eq!(reg.count_by_phase(RunPhase::Queued).await, 2);
    assert_eq!(reg.count_by_phase(RunPhase::Running).await, 0);
}

#[tokio::test]
async fn registry_remove_terminal() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.cancel(id).await.unwrap();
    let removed = reg.remove(id).await.unwrap();
    assert_eq!(removed.phase, RunPhase::Cancelled);
    assert!(reg.get(id).await.is_none());
}

#[tokio::test]
async fn registry_remove_nonterminal_fails() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    assert!(reg.remove(id).await.is_err());
}

// ===========================================================================
// Additional: Backend list
// ===========================================================================

#[tokio::test]
async fn backend_list_register_and_list() {
    let list = BackendList::new();
    list.register("mock".into()).await;
    assert!(list.contains("mock").await);
    assert_eq!(list.len().await, 1);
}

#[tokio::test]
async fn backend_list_deduplicate() {
    let list = BackendList::new();
    list.register("mock".into()).await;
    list.register("mock".into()).await;
    assert_eq!(list.len().await, 1);
}

#[tokio::test]
async fn backend_list_from_names() {
    let list = BackendList::from_names(vec!["a".into(), "b".into()]);
    assert_eq!(list.len().await, 2);
    assert!(list.contains("a").await);
}

// ===========================================================================
// Additional: Queue priority
// ===========================================================================

#[test]
fn queue_enqueue_dequeue_priority_order() {
    let mut q = RunQueue::new(10);
    q.enqueue(QueuedRun {
        id: "low".into(),
        work_order_id: "wo1".into(),
        priority: QueuePriority::Low,
        queued_at: "now".into(),
        backend: None,
        metadata: BTreeMap::new(),
    })
    .unwrap();
    q.enqueue(QueuedRun {
        id: "high".into(),
        work_order_id: "wo2".into(),
        priority: QueuePriority::High,
        queued_at: "now".into(),
        backend: None,
        metadata: BTreeMap::new(),
    })
    .unwrap();
    let first = q.dequeue().unwrap();
    assert_eq!(first.id, "high");
}

#[test]
fn queue_full_rejected() {
    let mut q = RunQueue::new(1);
    q.enqueue(QueuedRun {
        id: "a".into(),
        work_order_id: "wo".into(),
        priority: QueuePriority::Normal,
        queued_at: "now".into(),
        backend: None,
        metadata: BTreeMap::new(),
    })
    .unwrap();
    assert!(
        q.enqueue(QueuedRun {
            id: "b".into(),
            work_order_id: "wo".into(),
            priority: QueuePriority::Normal,
            queued_at: "now".into(),
            backend: None,
            metadata: BTreeMap::new(),
        })
        .is_err()
    );
}

#[test]
fn queue_stats_counts() {
    let mut q = RunQueue::new(10);
    q.enqueue(QueuedRun {
        id: "a".into(),
        work_order_id: "wo".into(),
        priority: QueuePriority::Normal,
        queued_at: "now".into(),
        backend: None,
        metadata: BTreeMap::new(),
    })
    .unwrap();
    let stats = q.stats();
    assert_eq!(stats.total, 1);
    assert_eq!(stats.max, 10);
}

// ===========================================================================
// Additional: Rate limiter
// ===========================================================================

#[tokio::test]
async fn rate_limiter_allows_within_limit() {
    let limiter = RateLimiter::new(5, Duration::from_secs(60));
    for _ in 0..5 {
        assert!(limiter.check().await.is_ok());
    }
}

#[tokio::test]
async fn rate_limiter_rejects_over_limit() {
    let limiter = RateLimiter::new(2, Duration::from_secs(60));
    assert!(limiter.check().await.is_ok());
    assert!(limiter.check().await.is_ok());
    assert!(limiter.check().await.is_err());
}

// ===========================================================================
// Additional: Versioned app routes
// ===========================================================================

#[tokio::test]
async fn versioned_health_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_versioned_app(state), "/api/v1/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn versioned_backends_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_versioned_app(state), "/api/v1/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn versioned_run_with_mock_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_post_json(
        build_versioned_app(state),
        "/api/v1/run",
        &test_run_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// Additional: Capabilities endpoint
// ===========================================================================

#[tokio::test]
async fn capabilities_returns_200() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/capabilities").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn capabilities_with_unknown_backend_returns_404() {
    let dir = tempfile::tempdir().unwrap();
    let state = app_state(dir.path());
    let resp = send_get(build_app(state), "/capabilities?backend=nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// Additional: api_routes descriptor
// ===========================================================================

#[test]
fn api_routes_non_empty() {
    let routes = api_routes();
    assert!(!routes.is_empty());
}

#[test]
fn api_routes_contain_health() {
    let routes = api_routes();
    assert!(routes.iter().any(|r| r.path.contains("health")));
}

#[test]
fn api_routes_contain_run() {
    let routes = api_routes();
    assert!(routes.iter().any(|r| r.path.contains("run")));
}

#[test]
fn api_routes_contain_backends() {
    let routes = api_routes();
    assert!(routes.iter().any(|r| r.path.contains("backends")));
}
