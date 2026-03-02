// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the daemon/HTTP API layer covering:
//! 1. Daemon builder construction
//! 2. Route registration
//! 3. Middleware configuration
//! 4. Health check endpoint
//! 5. Work order submission endpoint
//! 6. Status/monitoring endpoints
//! 7. Serde for API request/response types
//! 8. Edge cases: invalid requests, concurrent connections

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, Receipt, ReceiptBuilder,
    RuntimeConfig, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::api::{
    ApiError as ApiErrorType, ApiRequest, ApiResponse, BackendDetail, HealthResponse, RunInfo,
    RunStatus as ApiRunStatus,
};
use abp_daemon::middleware::{CorsConfig, RateLimiter, RequestId};
use abp_daemon::queue::{QueueError, QueuePriority, QueueStats, QueuedRun, RunQueue};
use abp_daemon::validation::RequestValidator;
use abp_daemon::versioning::{
    ApiVersion, ApiVersionError, ApiVersionRegistry, VersionNegotiator, VersionedEndpoint,
};
use abp_daemon::{
    AppState, BackendInfo, RunMetrics, RunRequest, RunResponse, RunStatus as TrackerRunStatus,
    RunTracker, build_app,
};
use abp_integrations::MockBackend;
use abp_runtime::Runtime;
use axum::body::Body;
use axum::http::{Request, StatusCode};
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

fn test_receipt() -> Receipt {
    ReceiptBuilder::new("mock").build()
}

fn test_state(receipts_dir: &std::path::Path) -> Arc<AppState> {
    let mut runtime = Runtime::new();
    runtime.register_backend("mock", MockBackend);
    Arc::new(AppState {
        runtime: Arc::new(runtime),
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: receipts_dir.to_path_buf(),
        run_tracker: RunTracker::new(),
    })
}

fn empty_state(receipts_dir: &std::path::Path) -> Arc<AppState> {
    let runtime = Runtime::new();
    Arc::new(AppState {
        runtime: Arc::new(runtime),
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: receipts_dir.to_path_buf(),
        run_tracker: RunTracker::new(),
    })
}

async fn get_json(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

async fn post_json(
    app: axum::Router,
    uri: &str,
    body: &impl serde::Serialize,
) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

async fn delete_json(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

fn make_queued_run(id: &str, priority: QueuePriority) -> QueuedRun {
    QueuedRun {
        id: id.into(),
        work_order_id: Uuid::new_v4().to_string(),
        priority,
        queued_at: "2025-01-01T00:00:00Z".into(),
        backend: Some("mock".into()),
        metadata: BTreeMap::new(),
    }
}

// ===========================================================================
// 1. Daemon builder construction
// ===========================================================================

#[tokio::test]
async fn app_state_can_be_constructed() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    assert!(!state.runtime.backend_names().is_empty());
}

#[tokio::test]
async fn app_state_with_empty_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let state = empty_state(tmp.path());
    assert!(state.runtime.backend_names().is_empty());
}

#[tokio::test]
async fn app_state_clone_shares_tracker() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let state2 = state.clone();
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();
    assert!(state2.run_tracker.get_run_status(id).await.is_some());
}

#[tokio::test]
async fn app_state_clone_shares_receipts() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let state2 = state.clone();
    let id = Uuid::new_v4();
    state.receipts.write().await.insert(id, test_receipt());
    assert!(state2.receipts.read().await.contains_key(&id));
}

#[tokio::test]
async fn build_app_returns_router() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let _app = build_app(state);
}

// ===========================================================================
// 2. Route registration — all routes respond (not 404/405)
// ===========================================================================

#[tokio::test]
async fn route_health_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/health").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_metrics_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/metrics").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_backends_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/backends").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_capabilities_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/capabilities").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_config_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/config").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_runs_get_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/runs").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_receipts_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/receipts").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_schema_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/schema/work_order").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_validate_post_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, _) = post_json(app, "/validate", &req).await;
    assert_ne!(status, StatusCode::NOT_FOUND);
    assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn route_nonexistent_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/this-does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// 3. Middleware configuration
// ===========================================================================

#[test]
fn request_id_is_unique() {
    let a = RequestId(Uuid::new_v4());
    let b = RequestId(Uuid::new_v4());
    assert_ne!(a, b);
}

#[test]
fn request_id_debug_format() {
    let id = RequestId(Uuid::nil());
    let debug = format!("{id:?}");
    assert!(debug.contains("RequestId"));
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
    assert!(limiter.check().await.is_ok());
    assert!(limiter.check().await.is_ok());
    assert_eq!(limiter.check().await, Err(StatusCode::TOO_MANY_REQUESTS));
}

#[tokio::test]
async fn rate_limiter_window_expiry() {
    let limiter = RateLimiter::new(1, Duration::from_millis(50));
    assert!(limiter.check().await.is_ok());
    assert!(limiter.check().await.is_err());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(limiter.check().await.is_ok());
}

#[test]
fn rate_limiter_into_layer_does_not_panic() {
    let limiter = RateLimiter::new(10, Duration::from_secs(60));
    let _ = limiter.into_layer();
}

#[test]
fn cors_config_to_layer_basic() {
    let config = CorsConfig {
        allowed_origins: vec!["http://localhost:3000".into()],
        allowed_methods: vec!["GET".into(), "POST".into()],
        allowed_headers: vec!["Content-Type".into()],
    };
    let _ = config.to_cors_layer();
}

#[test]
fn cors_config_to_layer_empty() {
    let config = CorsConfig {
        allowed_origins: vec![],
        allowed_methods: vec![],
        allowed_headers: vec![],
    };
    let _ = config.to_cors_layer();
}

#[test]
fn cors_config_to_layer_invalid_origin_skipped() {
    let config = CorsConfig {
        allowed_origins: vec!["not a valid\x00 header".into()],
        allowed_methods: vec!["GET".into()],
        allowed_headers: vec![],
    };
    let _ = config.to_cors_layer();
}

// ===========================================================================
// 4. Health check endpoint
// ===========================================================================

#[tokio::test]
async fn health_returns_ok_status() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_has_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (_, json) = get_json(app, "/health").await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_has_time_field() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (_, json) = get_json(app, "/health").await;
    let time_str = json["time"].as_str().unwrap();
    assert!(chrono::DateTime::parse_from_rfc3339(time_str).is_ok());
}

#[tokio::test]
async fn health_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

#[tokio::test]
async fn health_post_method_not_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// ===========================================================================
// 5. Work order submission endpoint
// ===========================================================================

#[tokio::test]
async fn run_valid_work_order_returns_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.get("run_id").is_some());
    assert!(json.get("receipt").is_some());
    assert!(json.get("events").is_some());
    assert_eq!(json["backend"], "mock");
}

#[tokio::test]
async fn run_receipt_has_sha256_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let run_resp: RunResponse = serde_json::from_slice(&body).unwrap();
    assert!(run_resp.receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn run_persists_receipt_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let run_resp: RunResponse = serde_json::from_slice(&body).unwrap();

    let path = tmp.path().join(format!("{}.json", run_resp.run_id));
    assert!(path.exists());
    let raw = std::fs::read(&path).unwrap();
    let receipt: Receipt = serde_json::from_slice(&raw).unwrap();
    assert_eq!(receipt.meta.run_id, run_resp.run_id);
}

#[tokio::test]
async fn run_events_are_nonempty() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let run_resp: RunResponse = serde_json::from_slice(&body).unwrap();
    assert!(!run_resp.events.is_empty());
}

#[tokio::test]
async fn run_empty_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, json) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn run_whitespace_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let mut wo = test_work_order();
    wo.task = "   \t\n".into();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_unknown_backend_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn run_invalid_json_body_returns_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_empty_body_returns_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_via_runs_endpoint_also_works() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/runs", &req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.get("run_id").is_some());
}

// ===========================================================================
// 6. Status/monitoring endpoints
// ===========================================================================

// --- Metrics ---

#[tokio::test]
async fn metrics_initially_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let m: RunMetrics = serde_json::from_slice(&body).unwrap();
    assert_eq!(m.total_runs, 0);
    assert_eq!(m.running, 0);
    assert_eq!(m.completed, 0);
    assert_eq!(m.failed, 0);
}

#[tokio::test]
async fn metrics_after_run_shows_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    // Complete a run
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, _) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::OK);

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let m: RunMetrics = serde_json::from_slice(&body).unwrap();
    assert!(m.completed >= 1);
    assert!(m.total_runs >= 1);
}

#[tokio::test]
async fn metrics_running_count() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let m: RunMetrics = serde_json::from_slice(&body).unwrap();
    assert!(m.running >= 1);
}

// --- Backends ---

#[tokio::test]
async fn backends_lists_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/backends").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<String> = serde_json::from_value(json).unwrap();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn backends_empty_runtime_returns_empty_list() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(empty_state(tmp.path()));
    let (status, json) = get_json(app, "/backends").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<String> = serde_json::from_value(json).unwrap();
    assert!(names.is_empty());
}

// --- Capabilities ---

#[tokio::test]
async fn capabilities_lists_all_backends() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/capabilities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let infos: Vec<BackendInfo> = serde_json::from_slice(&body).unwrap();
    assert!(!infos.is_empty());
}

#[tokio::test]
async fn capabilities_filter_by_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/capabilities?backend=mock")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let infos: Vec<BackendInfo> = serde_json::from_slice(&body).unwrap();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].id, "mock");
}

#[tokio::test]
async fn capabilities_unknown_backend_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/capabilities?backend=doesnotexist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- Config ---

#[tokio::test]
async fn config_returns_json_with_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/config").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.get("backends").is_some());
    assert!(json.get("contract_version").is_some());
    assert!(json.get("receipts_dir").is_some());
}

#[tokio::test]
async fn config_contract_version_matches() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (_, json) = get_json(app, "/config").await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

// --- Runs list ---

#[tokio::test]
async fn list_runs_initially_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/runs").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn list_runs_after_run_includes_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let wo = test_work_order();
    let wo_id = wo.id;
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::OK);

    let app = build_app(state.clone());
    let (_, json) = get_json(app, "/runs").await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&wo_id));
}

#[tokio::test]
async fn list_runs_includes_tracker_only_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();

    let app = build_app(state.clone());
    let (_, json) = get_json(app, "/runs").await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&id));
}

// --- Get run ---

#[tokio::test]
async fn get_run_completed_shows_status() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let wo = test_work_order();
    let wo_id = wo.id;
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (s, _) = post_json(app, "/run", &req).await;
    assert_eq!(s, StatusCode::OK);

    let app = build_app(state.clone());
    let (status, json) = get_json(app, &format!("/runs/{wo_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["run_id"], wo_id.to_string());
}

#[tokio::test]
async fn get_run_running_shows_status() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();

    let app = build_app(state.clone());
    let (status, json) = get_json(app, &format!("/runs/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["run_id"], id.to_string());
}

#[tokio::test]
async fn get_run_not_found_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let id = Uuid::new_v4();
    let (status, _) = get_json(app, &format!("/runs/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- Delete run ---

#[tokio::test]
async fn delete_completed_run_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (s, json) = post_json(app, "/run", &req).await;
    assert_eq!(s, StatusCode::OK);
    let run_id = json["run_id"].as_str().unwrap();

    let app = build_app(state.clone());
    let (status, del_json) = delete_json(app, &format!("/runs/{run_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(del_json["deleted"], run_id);
}

#[tokio::test]
async fn delete_running_run_returns_409() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();

    let app = build_app(state.clone());
    let (status, _) = delete_json(app, &format!("/runs/{id}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn delete_unknown_run_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let id = Uuid::new_v4();
    let (status, _) = delete_json(app, &format!("/runs/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- Cancel run ---

#[tokio::test]
async fn cancel_running_run_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{id}/cancel"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "cancelled");
}

#[tokio::test]
async fn cancel_unknown_run_returns_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{id}/cancel"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

// --- Run receipt ---

#[tokio::test]
async fn get_run_receipt_for_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (s, json) = post_json(app, "/run", &req).await;
    assert_eq!(s, StatusCode::OK);
    let run_id = json["run_id"].as_str().unwrap();

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/receipt"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let receipt: Receipt = serde_json::from_slice(&body).unwrap();
    assert_eq!(receipt.meta.run_id.to_string(), run_id);
}

#[tokio::test]
async fn get_run_receipt_running_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();

    let app = build_app(state.clone());
    let (status, _) = get_json(app, &format!("/runs/{id}/receipt")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_run_receipt_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let id = Uuid::new_v4();
    let (status, _) = get_json(app, &format!("/runs/{id}/receipt")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- Receipts list ---

#[tokio::test]
async fn list_receipts_initially_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/receipts").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn list_receipts_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (s, run_json) = post_json(app, "/run", &req).await;
    assert_eq!(s, StatusCode::OK);
    let run_id: Uuid = serde_json::from_value(run_json["run_id"].clone()).unwrap();

    let app = build_app(state.clone());
    let (status, json) = get_json(app, "/receipts").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&run_id));
}

#[tokio::test]
async fn list_receipts_with_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    // Add two runs
    for _ in 0..2 {
        let app = build_app(state.clone());
        let req = RunRequest {
            backend: "mock".into(),
            work_order: test_work_order(),
        };
        let (s, _) = post_json(app, "/run", &req).await;
        assert_eq!(s, StatusCode::OK);
    }

    let app = build_app(state.clone());
    let (status, json) = get_json(app, "/receipts?limit=1").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert_eq!(ids.len(), 1);
}

#[tokio::test]
async fn list_receipts_limit_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (s, _) = post_json(app, "/run", &req).await;
    assert_eq!(s, StatusCode::OK);

    let app = build_app(state.clone());
    let (status, json) = get_json(app, "/receipts?limit=0").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

// --- Get receipt ---

#[tokio::test]
async fn get_receipt_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (s, run_json) = post_json(app, "/run", &req).await;
    assert_eq!(s, StatusCode::OK);
    let run_id = run_json["run_id"].as_str().unwrap();

    let app = build_app(state.clone());
    let (status, _) = get_json(app, &format!("/receipts/{run_id}")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn get_receipt_not_found_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let id = Uuid::new_v4();
    let (status, _) = get_json(app, &format!("/receipts/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- Validate ---

#[tokio::test]
async fn validate_valid_request_returns_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/validate", &req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["valid"], true);
    assert_eq!(json["backend"], "mock");
}

#[tokio::test]
async fn validate_empty_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/validate", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn validate_unknown_backend_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let req = RunRequest {
        backend: "nope".into(),
        work_order: test_work_order(),
    };
    let (status, _) = post_json(app, "/validate", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn validate_returns_work_order_id() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let wo = test_work_order();
    let wo_id = wo.id;
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, json) = post_json(app, "/validate", &req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["work_order_id"], wo_id.to_string());
}

// --- Schema ---

#[tokio::test]
async fn schema_work_order_returns_valid_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/work_order").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.is_object());
}

#[tokio::test]
async fn schema_receipt_returns_valid_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/receipt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.is_object());
}

#[tokio::test]
async fn schema_capability_requirements_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/schema/capability_requirements").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn schema_backplane_config_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/schema/backplane_config").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn schema_unknown_type_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/schema/nonexistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- SSE events ---

#[tokio::test]
async fn sse_events_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{id}/events"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// 7. Serde for API request/response types
// ===========================================================================

// --- api::RunStatus ---

#[test]
fn api_run_status_serde_roundtrip() {
    for status in [
        ApiRunStatus::Queued,
        ApiRunStatus::Running,
        ApiRunStatus::Completed,
        ApiRunStatus::Failed,
        ApiRunStatus::Cancelled,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let back: ApiRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }
}

#[test]
fn api_run_status_is_terminal() {
    assert!(ApiRunStatus::Completed.is_terminal());
    assert!(ApiRunStatus::Failed.is_terminal());
    assert!(ApiRunStatus::Cancelled.is_terminal());
    assert!(!ApiRunStatus::Queued.is_terminal());
    assert!(!ApiRunStatus::Running.is_terminal());
}

#[test]
fn api_run_status_valid_transitions() {
    assert!(ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Running));
    assert!(ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Cancelled));
    assert!(!ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Completed));
    assert!(ApiRunStatus::Running.can_transition_to(ApiRunStatus::Completed));
    assert!(ApiRunStatus::Running.can_transition_to(ApiRunStatus::Failed));
    assert!(ApiRunStatus::Completed.valid_transitions().is_empty());
}

// --- api::RunInfo ---

#[test]
fn api_run_info_serde_roundtrip() {
    let info = RunInfo {
        id: Uuid::nil(),
        status: ApiRunStatus::Running,
        backend: "mock".into(),
        created_at: chrono::Utc::now(),
        events_count: 42,
    };
    let json = serde_json::to_string(&info).unwrap();
    let back: RunInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, info.id);
    assert_eq!(back.status, info.status);
    assert_eq!(back.events_count, 42);
}

// --- api::HealthResponse ---

#[test]
fn api_health_response_serde_roundtrip() {
    let resp = HealthResponse {
        status: "ok".into(),
        version: abp_core::CONTRACT_VERSION.into(),
        uptime_seconds: 123,
        backends_count: 3,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: HealthResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.status, "ok");
    assert_eq!(back.uptime_seconds, 123);
    assert_eq!(back.backends_count, 3);
}

// --- api::BackendDetail ---

#[test]
fn api_backend_detail_serde_roundtrip() {
    let detail = BackendDetail {
        id: "mock".into(),
        capabilities: BTreeMap::new(),
    };
    let json = serde_json::to_string(&detail).unwrap();
    let back: BackendDetail = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "mock");
}

// --- api::ApiError ---

#[test]
fn api_error_serde_roundtrip() {
    let err = ApiErrorType::not_found("run xyz not found");
    let json = serde_json::to_string(&err).unwrap();
    let back: ApiErrorType = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, "not_found");
    assert_eq!(back.message, "run xyz not found");
    assert!(back.details.is_none());
}

#[test]
fn api_error_stable_codes() {
    assert_eq!(ApiErrorType::not_found("x").code, "not_found");
    assert_eq!(ApiErrorType::invalid_request("x").code, "invalid_request");
    assert_eq!(ApiErrorType::conflict("x").code, "conflict");
    assert_eq!(ApiErrorType::internal("x").code, "internal_error");
}

#[test]
fn api_error_with_details() {
    let err =
        ApiErrorType::invalid_request("bad field").with_details(serde_json::json!({"field": "id"}));
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["details"]["field"], "id");
}

#[test]
fn api_error_omits_null_details() {
    let err = ApiErrorType::not_found("gone");
    let json = serde_json::to_value(&err).unwrap();
    assert!(json.get("details").is_none());
}

#[test]
fn api_error_display() {
    let err = ApiErrorType::not_found("missing");
    assert_eq!(format!("{err}"), "not_found: missing");
}

// --- api::ApiRequest ---

#[test]
fn api_request_cancel_run_serde() {
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
fn api_request_submit_run_serde() {
    let wo = test_work_order();
    let req = ApiRequest::SubmitRun {
        backend: "mock".into(),
        work_order: Box::new(wo),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ApiRequest = serde_json::from_str(&json).unwrap();
    match back {
        ApiRequest::SubmitRun { backend, .. } => assert_eq!(backend, "mock"),
        _ => panic!("wrong variant"),
    }
}

// --- api::ApiResponse ---

#[test]
fn api_response_run_created_serde() {
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

#[test]
fn api_response_health_serde() {
    let resp = ApiResponse::Health(HealthResponse {
        status: "ok".into(),
        version: "v0.1".into(),
        uptime_seconds: 0,
        backends_count: 1,
    });
    let json = serde_json::to_string(&resp).unwrap();
    let back: ApiResponse = serde_json::from_str(&json).unwrap();
    match back {
        ApiResponse::Health(h) => assert_eq!(h.status, "ok"),
        _ => panic!("wrong variant"),
    }
}

// --- lib RunMetrics serde ---

#[test]
fn run_metrics_serde_roundtrip() {
    let m = RunMetrics {
        total_runs: 10,
        running: 2,
        completed: 7,
        failed: 1,
    };
    let json = serde_json::to_string(&m).unwrap();
    let back: RunMetrics = serde_json::from_slice(json.as_bytes()).unwrap();
    assert_eq!(back.total_runs, 10);
    assert_eq!(back.running, 2);
    assert_eq!(back.completed, 7);
    assert_eq!(back.failed, 1);
}

// --- lib RunRequest serde ---

#[test]
fn run_request_serde_roundtrip() {
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: RunRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.backend, "mock");
}

// --- lib BackendInfo serde ---

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

// --- lib TrackerRunStatus serde ---

#[test]
fn tracker_run_status_pending_serde() {
    let s = TrackerRunStatus::Pending;
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("pending"));
    let back: TrackerRunStatus = serde_json::from_str(&json).unwrap();
    matches!(back, TrackerRunStatus::Pending);
}

#[test]
fn tracker_run_status_cancelled_serde() {
    let s = TrackerRunStatus::Cancelled;
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("cancelled"));
}

#[test]
fn tracker_run_status_failed_serde() {
    let s = TrackerRunStatus::Failed {
        error: "boom".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("boom"));
    let back: TrackerRunStatus = serde_json::from_str(&json).unwrap();
    match back {
        TrackerRunStatus::Failed { error } => assert_eq!(error, "boom"),
        _ => panic!("wrong variant"),
    }
}

// ===========================================================================
// RunTracker unit tests
// ===========================================================================

#[tokio::test]
async fn run_tracker_new_is_empty() {
    let tracker = RunTracker::new();
    assert!(tracker.list_runs().await.is_empty());
}

#[tokio::test]
async fn run_tracker_default_is_empty() {
    let tracker = RunTracker::default();
    assert!(tracker.list_runs().await.is_empty());
}

#[tokio::test]
async fn run_tracker_start_run_succeeds() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    assert!(tracker.start_run(id).await.is_ok());
    let status = tracker.get_run_status(id).await.unwrap();
    matches!(status, TrackerRunStatus::Running);
}

#[tokio::test]
async fn run_tracker_start_run_duplicate_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert!(tracker.start_run(id).await.is_err());
}

#[tokio::test]
async fn run_tracker_complete_run_succeeds() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let receipt = test_receipt();
    assert!(tracker.complete_run(id, receipt).await.is_ok());
}

#[tokio::test]
async fn run_tracker_complete_untracked_run_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    assert!(tracker.complete_run(id, test_receipt()).await.is_err());
}

#[tokio::test]
async fn run_tracker_fail_run_succeeds() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert!(tracker.fail_run(id, "oops".into()).await.is_ok());
    let status = tracker.get_run_status(id).await.unwrap();
    match status {
        TrackerRunStatus::Failed { error } => assert_eq!(error, "oops"),
        _ => panic!("expected failed"),
    }
}

#[tokio::test]
async fn run_tracker_fail_untracked_run_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    assert!(tracker.fail_run(id, "oops".into()).await.is_err());
}

#[tokio::test]
async fn run_tracker_cancel_running_run_succeeds() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert!(tracker.cancel_run(id).await.is_ok());
    let status = tracker.get_run_status(id).await.unwrap();
    matches!(status, TrackerRunStatus::Cancelled);
}

#[tokio::test]
async fn run_tracker_cancel_completed_run_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, test_receipt()).await.unwrap();
    assert!(tracker.cancel_run(id).await.is_err());
}

#[tokio::test]
async fn run_tracker_cancel_untracked_run_fails() {
    let tracker = RunTracker::new();
    assert!(tracker.cancel_run(Uuid::new_v4()).await.is_err());
}

#[tokio::test]
async fn run_tracker_get_status_unknown_is_none() {
    let tracker = RunTracker::new();
    assert!(tracker.get_run_status(Uuid::new_v4()).await.is_none());
}

#[tokio::test]
async fn run_tracker_remove_completed_succeeds() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, test_receipt()).await.unwrap();
    let status = tracker.remove_run(id).await.unwrap();
    matches!(status, TrackerRunStatus::Completed { .. });
    assert!(tracker.get_run_status(id).await.is_none());
}

#[tokio::test]
async fn run_tracker_remove_running_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert!(tracker.remove_run(id).await.is_err());
}

#[tokio::test]
async fn run_tracker_remove_unknown_fails() {
    let tracker = RunTracker::new();
    assert!(tracker.remove_run(Uuid::new_v4()).await.is_err());
}

#[tokio::test]
async fn run_tracker_list_runs_returns_all() {
    let tracker = RunTracker::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    tracker.start_run(id1).await.unwrap();
    tracker.start_run(id2).await.unwrap();
    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 2);
}

// ===========================================================================
// Queue tests
// ===========================================================================

#[test]
fn queue_new_is_empty() {
    let q = RunQueue::new(10);
    assert!(q.is_empty());
    assert_eq!(q.len(), 0);
}

#[test]
fn queue_enqueue_dequeue_basic() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.len(), 1);
    let item = q.dequeue().unwrap();
    assert_eq!(item.id, "a");
    assert!(q.is_empty());
}

#[test]
fn queue_priority_ordering() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("low", QueuePriority::Low))
        .unwrap();
    q.enqueue(make_queued_run("critical", QueuePriority::Critical))
        .unwrap();
    q.enqueue(make_queued_run("normal", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.dequeue().unwrap().id, "critical");
    assert_eq!(q.dequeue().unwrap().id, "normal");
    assert_eq!(q.dequeue().unwrap().id, "low");
}

#[test]
fn queue_fifo_within_same_priority() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("first", QueuePriority::Normal))
        .unwrap();
    q.enqueue(make_queued_run("second", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.dequeue().unwrap().id, "first");
    assert_eq!(q.dequeue().unwrap().id, "second");
}

#[test]
fn queue_full_error() {
    let mut q = RunQueue::new(1);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    let err = q
        .enqueue(make_queued_run("b", QueuePriority::Normal))
        .unwrap_err();
    assert!(matches!(err, QueueError::Full { max: 1 }));
}

#[test]
fn queue_duplicate_id_error() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    let err = q
        .enqueue(make_queued_run("a", QueuePriority::High))
        .unwrap_err();
    assert!(matches!(err, QueueError::DuplicateId(_)));
}

#[test]
fn queue_peek_returns_highest_priority() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("low", QueuePriority::Low))
        .unwrap();
    q.enqueue(make_queued_run("high", QueuePriority::High))
        .unwrap();
    assert_eq!(q.peek().unwrap().id, "high");
    assert_eq!(q.len(), 2); // peek doesn't remove
}

#[test]
fn queue_peek_empty_returns_none() {
    let q = RunQueue::new(10);
    assert!(q.peek().is_none());
}

#[test]
fn queue_remove_by_id() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    q.enqueue(make_queued_run("b", QueuePriority::Normal))
        .unwrap();
    let removed = q.remove("a").unwrap();
    assert_eq!(removed.id, "a");
    assert_eq!(q.len(), 1);
}

#[test]
fn queue_remove_nonexistent_returns_none() {
    let mut q = RunQueue::new(10);
    assert!(q.remove("nope").is_none());
}

#[test]
fn queue_clear() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    q.enqueue(make_queued_run("b", QueuePriority::Normal))
        .unwrap();
    q.clear();
    assert!(q.is_empty());
}

#[test]
fn queue_by_priority_filters_correctly() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("low", QueuePriority::Low))
        .unwrap();
    q.enqueue(make_queued_run("high", QueuePriority::High))
        .unwrap();
    q.enqueue(make_queued_run("low2", QueuePriority::Low))
        .unwrap();
    let lows = q.by_priority(QueuePriority::Low);
    assert_eq!(lows.len(), 2);
}

#[test]
fn queue_is_full() {
    let mut q = RunQueue::new(1);
    assert!(!q.is_full());
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    assert!(q.is_full());
}

#[test]
fn queue_stats() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("a", QueuePriority::Low)).unwrap();
    q.enqueue(make_queued_run("b", QueuePriority::High))
        .unwrap();
    q.enqueue(make_queued_run("c", QueuePriority::Low)).unwrap();
    let stats = q.stats();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.max, 10);
    assert_eq!(*stats.by_priority.get("low").unwrap(), 2);
    assert_eq!(*stats.by_priority.get("high").unwrap(), 1);
}

#[test]
fn queue_dequeue_empty_returns_none() {
    let mut q = RunQueue::new(10);
    assert!(q.dequeue().is_none());
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
fn queue_stats_serde_roundtrip() {
    let stats = QueueStats {
        total: 5,
        max: 100,
        by_priority: BTreeMap::from([("normal".into(), 3), ("high".into(), 2)]),
    };
    let json = serde_json::to_string(&stats).unwrap();
    let back: QueueStats = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total, 5);
}

#[test]
fn queued_run_serde_roundtrip() {
    let run = make_queued_run("test", QueuePriority::Normal);
    let json = serde_json::to_string(&run).unwrap();
    let back: QueuedRun = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "test");
    assert_eq!(back.priority, QueuePriority::Normal);
}

#[test]
fn queue_error_display() {
    let full = QueueError::Full { max: 10 };
    assert!(format!("{full}").contains("full"));
    let dup = QueueError::DuplicateId("abc".into());
    assert!(format!("{dup}").contains("abc"));
}

// ===========================================================================
// Validation tests
// ===========================================================================

#[test]
fn validate_valid_uuid() {
    let id = Uuid::new_v4().to_string();
    assert!(RequestValidator::validate_run_id(&id).is_ok());
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
fn validate_backend_valid() {
    let backends = vec!["mock".into(), "sidecar:node".into()];
    assert!(RequestValidator::validate_backend_name("mock", &backends).is_ok());
}

#[test]
fn validate_backend_unknown() {
    let backends = vec!["mock".into()];
    let err = RequestValidator::validate_backend_name("nope", &backends).unwrap_err();
    assert!(err.contains("unknown backend"));
}

#[test]
fn validate_backend_empty() {
    let backends = vec!["mock".into()];
    assert!(RequestValidator::validate_backend_name("", &backends).is_err());
}

#[test]
fn validate_config_valid_object() {
    let config = serde_json::json!({"key": "value"});
    assert!(RequestValidator::validate_config(&config).is_ok());
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
fn validate_work_order_valid() {
    let wo = test_work_order();
    assert!(RequestValidator::validate_work_order(&wo).is_ok());
}

#[test]
fn validate_work_order_empty_task() {
    let mut wo = test_work_order();
    wo.task = String::new();
    let errs = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errs.iter().any(|e| e.contains("task")));
}

#[test]
fn validate_work_order_whitespace_task() {
    let mut wo = test_work_order();
    wo.task = "   ".into();
    assert!(RequestValidator::validate_work_order(&wo).is_err());
}

#[test]
fn validate_work_order_empty_root() {
    let mut wo = test_work_order();
    wo.workspace.root = String::new();
    assert!(RequestValidator::validate_work_order(&wo).is_err());
}

// ===========================================================================
// Versioning tests
// ===========================================================================

#[test]
fn version_parse_v1() {
    let v = ApiVersion::parse("v1").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 0);
}

#[test]
fn version_parse_v1_2() {
    let v = ApiVersion::parse("v1.2").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
}

#[test]
fn version_parse_no_prefix() {
    let v = ApiVersion::parse("2.3").unwrap();
    assert_eq!(v.major, 2);
    assert_eq!(v.minor, 3);
}

#[test]
fn version_parse_empty_error() {
    assert!(ApiVersion::parse("").is_err());
    assert!(ApiVersion::parse("v").is_err());
}

#[test]
fn version_parse_invalid_major() {
    assert!(matches!(
        ApiVersion::parse("vX.1"),
        Err(ApiVersionError::InvalidFormat(_))
    ));
}

#[test]
fn version_compatibility() {
    let v1_0 = ApiVersion { major: 1, minor: 0 };
    let v1_2 = ApiVersion { major: 1, minor: 2 };
    let v2_0 = ApiVersion { major: 2, minor: 0 };
    assert!(v1_0.is_compatible(&v1_2));
    assert!(!v1_0.is_compatible(&v2_0));
}

#[test]
fn version_ordering() {
    let v1_0 = ApiVersion { major: 1, minor: 0 };
    let v1_2 = ApiVersion { major: 1, minor: 2 };
    let v2_0 = ApiVersion { major: 2, minor: 0 };
    assert!(v1_0 < v1_2);
    assert!(v1_2 < v2_0);
}

#[test]
fn version_display() {
    let v = ApiVersion { major: 1, minor: 2 };
    assert_eq!(format!("{v}"), "v1.2");
}

#[test]
fn version_serde_roundtrip() {
    let v = ApiVersion { major: 1, minor: 2 };
    let json = serde_json::to_string(&v).unwrap();
    let back: ApiVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

#[test]
fn version_registry_register_and_query() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 1, minor: 0 });
    reg.register(VersionedEndpoint {
        path: "/health".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    assert!(reg.is_supported("/health", &ApiVersion { major: 1, minor: 0 }));
    assert!(!reg.is_supported("/nonexistent", &ApiVersion { major: 1, minor: 0 }));
}

#[test]
fn version_registry_max_version_respected() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 2, minor: 0 });
    reg.register(VersionedEndpoint {
        path: "/old".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: Some(ApiVersion { major: 1, minor: 5 }),
        deprecated: true,
        deprecated_message: Some("use /new".into()),
    });
    assert!(reg.is_supported("/old", &ApiVersion { major: 1, minor: 3 }));
    assert!(!reg.is_supported("/old", &ApiVersion { major: 2, minor: 0 }));
}

#[test]
fn version_registry_deprecated_endpoints() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 1, minor: 0 });
    reg.register(VersionedEndpoint {
        path: "/old".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: None,
        deprecated: true,
        deprecated_message: None,
    });
    reg.register(VersionedEndpoint {
        path: "/new".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    assert_eq!(reg.deprecated_endpoints().len(), 1);
}

#[test]
fn version_registry_current_version() {
    let v = ApiVersion { major: 3, minor: 1 };
    let reg = ApiVersionRegistry::new(v);
    assert_eq!(*reg.current_version(), v);
}

#[test]
fn version_registry_supported_versions() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 2, minor: 0 });
    reg.register(VersionedEndpoint {
        path: "/a".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: Some(ApiVersion { major: 1, minor: 5 }),
        deprecated: false,
        deprecated_message: None,
    });
    let versions = reg.supported_versions();
    assert!(versions.len() >= 2);
}

#[test]
fn version_registry_endpoints_for_version() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 2, minor: 0 });
    reg.register(VersionedEndpoint {
        path: "/a".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    reg.register(VersionedEndpoint {
        path: "/b".into(),
        min_version: ApiVersion { major: 2, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    let eps = reg.endpoints_for_version(&ApiVersion { major: 1, minor: 0 });
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].path, "/a");
    let eps2 = reg.endpoints_for_version(&ApiVersion { major: 2, minor: 0 });
    assert_eq!(eps2.len(), 2);
}

#[test]
fn version_negotiator_exact_match() {
    let supported = vec![
        ApiVersion { major: 1, minor: 0 },
        ApiVersion { major: 1, minor: 1 },
        ApiVersion { major: 2, minor: 0 },
    ];
    let result = VersionNegotiator::negotiate(&ApiVersion { major: 1, minor: 1 }, &supported);
    assert_eq!(result, Some(ApiVersion { major: 1, minor: 1 }));
}

#[test]
fn version_negotiator_compatible_downgrade() {
    let supported = vec![
        ApiVersion { major: 1, minor: 0 },
        ApiVersion { major: 1, minor: 1 },
    ];
    let result = VersionNegotiator::negotiate(&ApiVersion { major: 1, minor: 5 }, &supported);
    assert_eq!(result, Some(ApiVersion { major: 1, minor: 1 }));
}

#[test]
fn version_negotiator_no_compatible() {
    let supported = vec![ApiVersion { major: 2, minor: 0 }];
    let result = VersionNegotiator::negotiate(&ApiVersion { major: 1, minor: 0 }, &supported);
    assert_eq!(result, None);
}

#[test]
fn version_error_display() {
    let e = ApiVersionError::InvalidFormat("bad".into());
    assert!(format!("{e}").contains("bad"));
    let e2 = ApiVersionError::UnsupportedVersion(ApiVersion {
        major: 99,
        minor: 0,
    });
    assert!(format!("{e2}").contains("99"));
}

// ===========================================================================
// 8. Edge cases
// ===========================================================================

#[tokio::test]
async fn concurrent_runs_do_not_interfere() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let mut handles = vec![];
    for _ in 0..5 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let app = build_app(s);
            let req = RunRequest {
                backend: "mock".into(),
                work_order: test_work_order(),
            };
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/run")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&req).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            resp.status()
        }));
    }
    for h in handles {
        let status = h.await.unwrap();
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn concurrent_health_checks() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let mut handles = vec![];
    for _ in 0..20 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let app = build_app(s);
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/health")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            resp.status()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), StatusCode::OK);
    }
}

#[tokio::test]
async fn run_with_empty_workspace_root_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let mut wo = test_work_order();
    wo.workspace.root = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_with_very_long_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let mut wo = test_work_order();
    wo.task = "x".repeat(200_000);
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_then_get_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (s, run_json) = post_json(app, "/run", &req).await;
    assert_eq!(s, StatusCode::OK);
    let run_id = run_json["run_id"].as_str().unwrap().to_string();

    let app = build_app(state.clone());
    let (ds, _) = delete_json(app, &format!("/runs/{run_id}")).await;
    assert_eq!(ds, StatusCode::OK);

    let app = build_app(state.clone());
    let (gs, _) = get_json(app, &format!("/runs/{run_id}")).await;
    assert_eq!(gs, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_then_delete_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();
    state.run_tracker.cancel_run(id).await.unwrap();

    let app = build_app(state.clone());
    let (status, _) = delete_json(app, &format!("/runs/{id}")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn multiple_schemas_accessible() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    for schema_type in &[
        "work_order",
        "receipt",
        "capability_requirements",
        "backplane_config",
    ] {
        let app = build_app(state.clone());
        let (status, _) = get_json(app, &format!("/schema/{schema_type}")).await;
        assert_eq!(status, StatusCode::OK, "schema {schema_type} failed");
    }
}

#[tokio::test]
async fn run_tracker_concurrent_start_unique_ids() {
    let tracker = RunTracker::new();
    let mut handles = vec![];
    for _ in 0..10 {
        let t = tracker.clone();
        handles.push(tokio::spawn(async move {
            let id = Uuid::new_v4();
            t.start_run(id).await.unwrap();
            id
        }));
    }
    let mut ids = vec![];
    for h in handles {
        ids.push(h.await.unwrap());
    }
    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 10);
}

#[test]
fn queue_priority_ordering_all_levels() {
    assert!(QueuePriority::Low < QueuePriority::Normal);
    assert!(QueuePriority::Normal < QueuePriority::High);
    assert!(QueuePriority::High < QueuePriority::Critical);
}

#[test]
fn validate_backend_too_long_name() {
    let backends = vec!["mock".into()];
    let long_name = "a".repeat(300);
    assert!(RequestValidator::validate_backend_name(&long_name, &backends).is_err());
}

#[test]
fn versioned_endpoint_serde_roundtrip() {
    let ep = VersionedEndpoint {
        path: "/test".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: Some(ApiVersion { major: 2, minor: 0 }),
        deprecated: false,
        deprecated_message: None,
    };
    let json = serde_json::to_string(&ep).unwrap();
    let back: VersionedEndpoint = serde_json::from_str(&json).unwrap();
    assert_eq!(back.path, "/test");
}
