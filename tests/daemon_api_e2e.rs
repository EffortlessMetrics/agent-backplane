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
//! Comprehensive end-to-end tests for the `abp-daemon` crate covering:
//! 1. Daemon builder/configuration (AppState, build_app)
//! 2. HTTP API route handling
//! 3. Request validation
//! 4. Queue management
//! 5. Middleware stack
//! 6. WebSocket/SSE support
//! 7. Serde roundtrip for API types
//! 8. Edge cases: invalid requests, concurrent access

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, Receipt, RuntimeConfig,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::api::{
    ApiError as ApiApiError, ApiRequest, ApiResponse, BackendDetail, HealthResponse, RunInfo,
    RunStatus as ApiRunStatus,
};
use abp_daemon::middleware::{CorsConfig, RateLimiter, RequestId};
use abp_daemon::queue::{QueueError, QueuePriority, QueueStats, QueuedRun, RunQueue};
use abp_daemon::validation::RequestValidator;
use abp_daemon::versioning::{
    ApiVersion, ApiVersionError, ApiVersionRegistry, VersionNegotiator, VersionedEndpoint,
};
use abp_daemon::{AppState, BackendInfo, RunMetrics, RunRequest, RunStatus, RunTracker, build_app};
use abp_integrations::MockBackend;
use abp_runtime::Runtime;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
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
        task: "e2e daemon test task".into(),
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
// 1. Daemon builder/configuration
// ===========================================================================

#[tokio::test]
async fn app_state_default_construction() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    assert!(!state.runtime.backend_names().is_empty());
    assert!(state.receipts.read().await.is_empty());
    assert_eq!(state.receipts_dir, tmp.path());
}

#[tokio::test]
async fn build_app_returns_router() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state);
    // Router responds to /health
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn app_state_with_empty_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let runtime = Runtime::new();
    let state = Arc::new(AppState {
        runtime: Arc::new(runtime),
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: tmp.path().to_path_buf(),
        run_tracker: RunTracker::new(),
    });
    assert!(state.runtime.backend_names().is_empty());
}

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

// ===========================================================================
// 2. HTTP API route handling
// ===========================================================================

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
    assert!(json.get("time").is_some());
}

#[tokio::test]
async fn metrics_endpoint_initially_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/metrics").await;
    assert_eq!(status, StatusCode::OK);
    let metrics: RunMetrics = serde_json::from_value(json).unwrap();
    assert_eq!(metrics.total_runs, 0);
    assert_eq!(metrics.running, 0);
    assert_eq!(metrics.completed, 0);
    assert_eq!(metrics.failed, 0);
}

#[tokio::test]
async fn metrics_reflects_running_count() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    state.run_tracker.start_run(Uuid::new_v4()).await.unwrap();
    state.run_tracker.start_run(Uuid::new_v4()).await.unwrap();

    let app = build_app(state);
    let (_, json) = get_json(app, "/metrics").await;
    let metrics: RunMetrics = serde_json::from_value(json).unwrap();
    assert_eq!(metrics.running, 2);
}

#[tokio::test]
async fn backends_endpoint_lists_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/backends").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<String> = serde_json::from_value(json).unwrap();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn capabilities_endpoint_returns_backend_info() {
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
async fn capabilities_filter_by_backend() {
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
    let (status, _) = get_json(app, "/capabilities?backend=nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn config_endpoint_returns_backend_list() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/config").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
    assert!(json.get("backends").is_some());
    assert!(json.get("receipts_dir").is_some());
}

#[tokio::test]
async fn run_endpoint_valid_work_order() {
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
    assert_eq!(json["backend"], "mock");
    assert!(json.get("receipt").is_some());
    assert!(json.get("events").is_some());
}

#[tokio::test]
async fn run_endpoint_unknown_backend_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let req = RunRequest {
        backend: "unknown_backend".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn run_endpoint_empty_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_endpoint_persists_receipt_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, json) = post_json(app, "/run", &req).await;
    let run_id = json["run_id"].as_str().unwrap();
    let path = tmp.path().join(format!("{run_id}.json"));
    assert!(path.exists());
}

#[tokio::test]
async fn validate_endpoint_valid_request() {
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
async fn validate_endpoint_invalid_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };
    let (status, _) = post_json(app, "/validate", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn validate_endpoint_empty_task() {
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
async fn list_runs_endpoint_initially_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/runs").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn list_runs_includes_completed_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, json) = post_json(app, "/run", &req).await;
    let run_id: Uuid = serde_json::from_value(json["run_id"].clone()).unwrap();

    let app = build_app(state.clone());
    let (_, json) = get_json(app, "/runs").await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&run_id));
}

#[tokio::test]
async fn get_run_for_completed_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, json) = post_json(app, "/run", &req).await;
    let run_id = json["run_id"].as_str().unwrap();

    let app = build_app(state.clone());
    let (status, json) = get_json(app, &format!("/runs/{run_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["run_id"], run_id);
}

#[tokio::test]
async fn get_run_nonexistent_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, &format!("/runs/{}", Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_run_receipt_for_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, run_json) = post_json(app, "/run", &req).await;
    let run_id = run_json["run_id"].as_str().unwrap();

    let app = build_app(state.clone());
    let (status, _) = get_json(app, &format!("/runs/{run_id}/receipt")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn get_run_receipt_for_running_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_id = Uuid::new_v4();
    state.run_tracker.start_run(run_id).await.unwrap();

    let app = build_app(state.clone());
    let (status, _) = get_json(app, &format!("/runs/{run_id}/receipt")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_running_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_id = Uuid::new_v4();
    state.run_tracker.start_run(run_id).await.unwrap();

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{run_id}/cancel"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn cancel_nonexistent_run_returns_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{}/cancel", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn delete_completed_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, json) = post_json(app, "/run", &req).await;
    let run_id = json["run_id"].as_str().unwrap();

    let app = build_app(state.clone());
    let (status, json) = delete_json(app, &format!("/runs/{run_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["deleted"], run_id);
}

#[tokio::test]
async fn delete_running_run_returns_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_id = Uuid::new_v4();
    state.run_tracker.start_run(run_id).await.unwrap();

    let app = build_app(state.clone());
    let (status, _) = delete_json(app, &format!("/runs/{run_id}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn delete_nonexistent_run_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = delete_json(app, &format!("/runs/{}", Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

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
async fn list_receipts_with_limit_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/receipts?limit=0").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn list_receipts_with_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    // Create two runs
    for _ in 0..2 {
        let app = build_app(state.clone());
        let req = RunRequest {
            backend: "mock".into(),
            work_order: test_work_order(),
        };
        post_json(app, "/run", &req).await;
    }

    let app = build_app(state.clone());
    let (_, json) = get_json(app, "/receipts?limit=1").await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert_eq!(ids.len(), 1);
}

#[tokio::test]
async fn get_receipt_by_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, json) = post_json(app, "/run", &req).await;
    let run_id = json["run_id"].as_str().unwrap();

    let app = build_app(state.clone());
    let (status, _) = get_json(app, &format!("/receipts/{run_id}")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn get_receipt_nonexistent_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, &format!("/receipts/{}", Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn schema_work_order_endpoint() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/work_order").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.is_object());
}

#[tokio::test]
async fn schema_receipt_endpoint() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/receipt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.is_object());
}

#[tokio::test]
async fn schema_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, _) = get_json(app, "/schema/nonexistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sse_events_endpoint_returns_response() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_id = Uuid::new_v4();
    state.run_tracker.start_run(run_id).await.unwrap();

    let app = build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/events"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn invalid_json_body_returns_client_error() {
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
async fn post_to_runs_also_creates_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, _) = post_json(app, "/runs", &req).await;
    assert_eq!(status, StatusCode::OK);
}

// ===========================================================================
// 3. Request validation
// ===========================================================================

#[test]
fn validate_valid_work_order() {
    let wo = test_work_order();
    assert!(RequestValidator::validate_work_order(&wo).is_ok());
}

#[test]
fn validate_empty_task_rejected() {
    let mut wo = test_work_order();
    wo.task = String::new();
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("task")));
}

#[test]
fn validate_whitespace_only_task_rejected() {
    let mut wo = test_work_order();
    wo.task = "   \t\n".into();
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("whitespace")));
}

#[test]
fn validate_empty_workspace_root_rejected() {
    let mut wo = test_work_order();
    wo.workspace.root = String::new();
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("workspace")));
}

#[test]
fn validate_negative_budget_rejected() {
    let mut wo = test_work_order();
    wo.config.max_budget_usd = Some(-1.0);
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("budget")));
}

#[test]
fn validate_nan_budget_rejected() {
    let mut wo = test_work_order();
    wo.config.max_budget_usd = Some(f64::NAN);
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("finite")));
}

#[test]
fn validate_infinite_budget_rejected() {
    let mut wo = test_work_order();
    wo.config.max_budget_usd = Some(f64::INFINITY);
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("finite")));
}

#[test]
fn validate_run_id_valid_uuid() {
    assert!(RequestValidator::validate_run_id(&Uuid::new_v4().to_string()).is_ok());
}

#[test]
fn validate_run_id_empty_rejected() {
    assert!(RequestValidator::validate_run_id("").is_err());
}

#[test]
fn validate_run_id_invalid_format() {
    assert!(RequestValidator::validate_run_id("not-a-uuid").is_err());
}

#[test]
fn validate_backend_name_known() {
    let backends = vec!["mock".into()];
    assert!(RequestValidator::validate_backend_name("mock", &backends).is_ok());
}

#[test]
fn validate_backend_name_unknown() {
    let backends = vec!["mock".into()];
    assert!(RequestValidator::validate_backend_name("other", &backends).is_err());
}

#[test]
fn validate_backend_name_empty() {
    let backends = vec!["mock".into()];
    assert!(RequestValidator::validate_backend_name("", &backends).is_err());
}

#[test]
fn validate_backend_name_too_long() {
    let backends = vec!["mock".into()];
    let long_name = "a".repeat(257);
    assert!(RequestValidator::validate_backend_name(&long_name, &backends).is_err());
}

#[test]
fn validate_config_valid_object() {
    let config = json!({"key": "value"});
    assert!(RequestValidator::validate_config(&config).is_ok());
}

#[test]
fn validate_config_non_object_rejected() {
    assert!(RequestValidator::validate_config(&json!("string")).is_err());
    assert!(RequestValidator::validate_config(&json!([1, 2])).is_err());
    assert!(RequestValidator::validate_config(&json!(42)).is_err());
}

#[test]
fn validate_config_deeply_nested_rejected() {
    let mut val = json!({"a": 1});
    for _ in 0..12 {
        val = json!({"nested": val});
    }
    assert!(RequestValidator::validate_config(&val).is_err());
}

// ===========================================================================
// 4. Queue management
// ===========================================================================

#[test]
fn queue_new_is_empty() {
    let q = RunQueue::new(10);
    assert!(q.is_empty());
    assert_eq!(q.len(), 0);
    assert!(!q.is_full());
}

#[test]
fn queue_enqueue_and_dequeue_fifo() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    q.enqueue(make_queued_run("b", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.len(), 2);
    assert_eq!(q.dequeue().unwrap().id, "a");
    assert_eq!(q.dequeue().unwrap().id, "b");
    assert!(q.dequeue().is_none());
}

#[test]
fn queue_priority_ordering() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("low", QueuePriority::Low))
        .unwrap();
    q.enqueue(make_queued_run("crit", QueuePriority::Critical))
        .unwrap();
    q.enqueue(make_queued_run("high", QueuePriority::High))
        .unwrap();
    q.enqueue(make_queued_run("norm", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.dequeue().unwrap().id, "crit");
    assert_eq!(q.dequeue().unwrap().id, "high");
    assert_eq!(q.dequeue().unwrap().id, "norm");
    assert_eq!(q.dequeue().unwrap().id, "low");
}

#[test]
fn queue_full_rejects_enqueue() {
    let mut q = RunQueue::new(2);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    q.enqueue(make_queued_run("b", QueuePriority::Normal))
        .unwrap();
    assert!(q.is_full());
    let err = q
        .enqueue(make_queued_run("c", QueuePriority::Normal))
        .unwrap_err();
    assert!(matches!(err, QueueError::Full { max: 2 }));
}

#[test]
fn queue_duplicate_id_rejected() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("dup", QueuePriority::Normal))
        .unwrap();
    let err = q
        .enqueue(make_queued_run("dup", QueuePriority::High))
        .unwrap_err();
    assert!(matches!(err, QueueError::DuplicateId(_)));
}

#[test]
fn queue_peek_does_not_remove() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.peek().unwrap().id, "a");
    assert_eq!(q.len(), 1);
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
    assert!(q.remove("nonexistent").is_none());
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
fn queue_by_priority_filter() {
    let mut q = RunQueue::new(10);
    q.enqueue(make_queued_run("h1", QueuePriority::High))
        .unwrap();
    q.enqueue(make_queued_run("n1", QueuePriority::Normal))
        .unwrap();
    q.enqueue(make_queued_run("h2", QueuePriority::High))
        .unwrap();
    let high = q.by_priority(QueuePriority::High);
    assert_eq!(high.len(), 2);
    let normal = q.by_priority(QueuePriority::Normal);
    assert_eq!(normal.len(), 1);
}

#[test]
fn queue_stats_snapshot() {
    let mut q = RunQueue::new(100);
    q.enqueue(make_queued_run("a", QueuePriority::High))
        .unwrap();
    q.enqueue(make_queued_run("b", QueuePriority::Low)).unwrap();
    q.enqueue(make_queued_run("c", QueuePriority::High))
        .unwrap();
    let stats = q.stats();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.max, 100);
    assert_eq!(*stats.by_priority.get("high").unwrap(), 2);
    assert_eq!(*stats.by_priority.get("low").unwrap(), 1);
}

#[test]
fn queue_stats_empty() {
    let q = RunQueue::new(5);
    let stats = q.stats();
    assert_eq!(stats.total, 0);
    assert_eq!(stats.max, 5);
    assert!(stats.by_priority.is_empty());
}

#[test]
fn queue_capacity_one() {
    let mut q = RunQueue::new(1);
    q.enqueue(make_queued_run("a", QueuePriority::Normal))
        .unwrap();
    assert!(q.is_full());
    assert!(
        q.enqueue(make_queued_run("b", QueuePriority::Normal))
            .is_err()
    );
    q.dequeue();
    assert!(!q.is_full());
    q.enqueue(make_queued_run("c", QueuePriority::Normal))
        .unwrap();
    assert_eq!(q.len(), 1);
}

// ===========================================================================
// 5. Middleware stack
// ===========================================================================

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
    assert_eq!(
        limiter.check().await.unwrap_err(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn rate_limiter_window_expiry() {
    let limiter = RateLimiter::new(1, Duration::from_millis(50));
    limiter.check().await.unwrap();
    assert!(limiter.check().await.is_err());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(limiter.check().await.is_ok());
}

#[test]
fn cors_config_to_layer() {
    let config = CorsConfig {
        allowed_origins: vec!["http://localhost:3000".into()],
        allowed_methods: vec!["GET".into(), "POST".into()],
        allowed_headers: vec!["content-type".into()],
    };
    // Should not panic
    let _layer = config.to_cors_layer();
}

#[test]
fn cors_config_empty_origins() {
    let config = CorsConfig {
        allowed_origins: vec![],
        allowed_methods: vec![],
        allowed_headers: vec![],
    };
    let _layer = config.to_cors_layer();
}

#[test]
fn request_id_debug_and_clone() {
    let id = RequestId(Uuid::new_v4());
    let cloned = id;
    assert_eq!(id, cloned);
    let _debug = format!("{:?}", id);
}

// ===========================================================================
// 6. WebSocket/SSE support
// ===========================================================================

// WebSocket upgrade requires a real TCP connection; we verify the route exists.
#[tokio::test]
async fn ws_route_exists_returns_upgrade_required() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    // A plain GET without upgrade headers won't upgrade but the route exists.
    let resp = app
        .oneshot(Request::builder().uri("/ws").body(Body::empty()).unwrap())
        .await
        .unwrap();
    // axum returns 400 or similar when WS upgrade headers are missing
    assert!(resp.status().is_client_error() || resp.status().is_server_error());
}

// ===========================================================================
// 7. Serde roundtrip for API types
// ===========================================================================

#[test]
fn run_request_serde_roundtrip() {
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: RunRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.backend, "mock");
    assert_eq!(back.work_order.task, "e2e daemon test task");
}

#[test]
fn run_metrics_serde_roundtrip() {
    let metrics = RunMetrics {
        total_runs: 10,
        running: 3,
        completed: 5,
        failed: 2,
    };
    let json = serde_json::to_string(&metrics).unwrap();
    let back: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_runs, 10);
    assert_eq!(back.running, 3);
    assert_eq!(back.completed, 5);
    assert_eq!(back.failed, 2);
}

#[test]
fn api_run_status_serde_all_variants() {
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
fn api_run_status_terminal_states() {
    assert!(ApiRunStatus::Completed.is_terminal());
    assert!(ApiRunStatus::Failed.is_terminal());
    assert!(ApiRunStatus::Cancelled.is_terminal());
    assert!(!ApiRunStatus::Queued.is_terminal());
    assert!(!ApiRunStatus::Running.is_terminal());
}

#[test]
fn api_run_status_transitions() {
    assert!(ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Running));
    assert!(ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Cancelled));
    assert!(!ApiRunStatus::Queued.can_transition_to(ApiRunStatus::Completed));
    assert!(ApiRunStatus::Running.can_transition_to(ApiRunStatus::Completed));
    assert!(ApiRunStatus::Running.can_transition_to(ApiRunStatus::Failed));
    assert!(!ApiRunStatus::Completed.can_transition_to(ApiRunStatus::Running));
}

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
    assert_eq!(back.id, Uuid::nil());
    assert_eq!(back.status, ApiRunStatus::Running);
    assert_eq!(back.events_count, 42);
}

#[test]
fn api_request_submit_run_roundtrip() {
    let req = ApiRequest::SubmitRun {
        backend: "mock".into(),
        work_order: Box::new(test_work_order()),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ApiRequest = serde_json::from_str(&json).unwrap();
    match back {
        ApiRequest::SubmitRun { backend, .. } => assert_eq!(backend, "mock"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn api_request_cancel_run_roundtrip() {
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

#[test]
fn api_response_health_roundtrip() {
    let resp = ApiResponse::Health(HealthResponse {
        status: "ok".into(),
        version: abp_core::CONTRACT_VERSION.into(),
        uptime_secs: 123,
        backends: vec!["mock".into(), "sidecar".into()],
    });
    let json = serde_json::to_string(&resp).unwrap();
    let back: ApiResponse = serde_json::from_str(&json).unwrap();
    match back {
        ApiResponse::Health(h) => {
            assert_eq!(h.status, "ok");
            assert_eq!(h.uptime_secs, 123);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn api_error_constructors() {
    assert_eq!(ApiApiError::not_found("x").code, "not_found");
    assert_eq!(ApiApiError::invalid_request("x").code, "invalid_request");
    assert_eq!(ApiApiError::conflict("x").code, "conflict");
    assert_eq!(ApiApiError::internal("x").code, "internal_error");
}

#[test]
fn api_error_with_details() {
    let err = ApiApiError::invalid_request("bad").with_details(json!({"field": "id"}));
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["details"]["field"], "id");
}

#[test]
fn api_error_omits_null_details() {
    let err = ApiApiError::not_found("gone");
    let json = serde_json::to_value(&err).unwrap();
    assert!(json.get("details").is_none());
}

#[test]
fn api_error_display() {
    let err = ApiApiError::not_found("run xyz");
    assert_eq!(err.to_string(), "not_found: run xyz");
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
fn queue_priority_ord_values() {
    assert!(QueuePriority::Low < QueuePriority::Normal);
    assert!(QueuePriority::Normal < QueuePriority::High);
    assert!(QueuePriority::High < QueuePriority::Critical);
}

#[test]
fn queued_run_serde_roundtrip() {
    let run = make_queued_run("test-id", QueuePriority::High);
    let json = serde_json::to_string(&run).unwrap();
    let back: QueuedRun = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "test-id");
    assert_eq!(back.priority, QueuePriority::High);
}

#[test]
fn queue_stats_serde_roundtrip() {
    let stats = QueueStats {
        total: 5,
        max: 100,
        by_priority: {
            let mut m = BTreeMap::new();
            m.insert("high".into(), 3);
            m.insert("low".into(), 2);
            m
        },
    };
    let json = serde_json::to_string(&stats).unwrap();
    let back: QueueStats = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total, 5);
    assert_eq!(back.max, 100);
}

#[test]
fn queue_error_display() {
    let full = QueueError::Full { max: 10 };
    assert!(full.to_string().contains("10"));
    let dup = QueueError::DuplicateId("abc".into());
    assert!(dup.to_string().contains("abc"));
}

#[test]
fn backend_detail_serde_roundtrip() {
    let detail = BackendDetail {
        id: "mock".into(),
        capabilities: BTreeMap::new(),
    };
    let json = serde_json::to_string(&detail).unwrap();
    let back: BackendDetail = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "mock");
}

// ---------------------------------------------------------------------------
// Versioning serde roundtrip
// ---------------------------------------------------------------------------

#[test]
fn api_version_parse_simple() {
    let v = ApiVersion::parse("v1").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 0);
}

#[test]
fn api_version_parse_with_minor() {
    let v = ApiVersion::parse("v2.3").unwrap();
    assert_eq!(v.major, 2);
    assert_eq!(v.minor, 3);
}

#[test]
fn api_version_parse_without_prefix() {
    let v = ApiVersion::parse("1.0").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 0);
}

#[test]
fn api_version_parse_empty_rejected() {
    assert!(ApiVersion::parse("").is_err());
    assert!(ApiVersion::parse("v").is_err());
}

#[test]
fn api_version_parse_invalid_rejected() {
    assert!(ApiVersion::parse("abc").is_err());
    assert!(ApiVersion::parse("v1.abc").is_err());
}

#[test]
fn api_version_display() {
    let v = ApiVersion { major: 1, minor: 2 };
    assert_eq!(v.to_string(), "v1.2");
}

#[test]
fn api_version_compatibility() {
    let v1_0 = ApiVersion { major: 1, minor: 0 };
    let v1_2 = ApiVersion { major: 1, minor: 2 };
    let v2_0 = ApiVersion { major: 2, minor: 0 };
    assert!(v1_0.is_compatible(&v1_2));
    assert!(!v1_0.is_compatible(&v2_0));
}

#[test]
fn api_version_ordering() {
    let v1_0 = ApiVersion { major: 1, minor: 0 };
    let v1_2 = ApiVersion { major: 1, minor: 2 };
    let v2_0 = ApiVersion { major: 2, minor: 0 };
    assert!(v1_0 < v1_2);
    assert!(v1_2 < v2_0);
}

#[test]
fn api_version_serde_roundtrip() {
    let v = ApiVersion { major: 1, minor: 2 };
    let json = serde_json::to_string(&v).unwrap();
    let back: ApiVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

#[test]
fn api_version_error_display() {
    let err = ApiVersionError::InvalidFormat("bad".into());
    assert!(err.to_string().contains("bad"));
    let err = ApiVersionError::UnsupportedVersion(ApiVersion {
        major: 99,
        minor: 0,
    });
    assert!(err.to_string().contains("99"));
}

#[test]
fn version_negotiator_picks_highest_compatible() {
    let requested = ApiVersion { major: 1, minor: 5 };
    let supported = vec![
        ApiVersion { major: 1, minor: 0 },
        ApiVersion { major: 1, minor: 3 },
        ApiVersion { major: 2, minor: 0 },
    ];
    let result = VersionNegotiator::negotiate(&requested, &supported).unwrap();
    assert_eq!(result, ApiVersion { major: 1, minor: 3 });
}

#[test]
fn version_negotiator_no_compatible_returns_none() {
    let requested = ApiVersion { major: 3, minor: 0 };
    let supported = vec![
        ApiVersion { major: 1, minor: 0 },
        ApiVersion { major: 2, minor: 0 },
    ];
    assert!(VersionNegotiator::negotiate(&requested, &supported).is_none());
}

#[test]
fn version_registry_is_supported() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 1, minor: 2 });
    reg.register(VersionedEndpoint {
        path: "/health".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    assert!(reg.is_supported("/health", &ApiVersion { major: 1, minor: 0 }));
    assert!(reg.is_supported("/health", &ApiVersion { major: 1, minor: 2 }));
    assert!(!reg.is_supported("/unknown", &ApiVersion { major: 1, minor: 0 }));
}

#[test]
fn version_registry_deprecated_endpoints() {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 2, minor: 0 });
    reg.register(VersionedEndpoint {
        path: "/old".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: Some(ApiVersion { major: 1, minor: 9 }),
        deprecated: true,
        deprecated_message: Some("use /new instead".into()),
    });
    reg.register(VersionedEndpoint {
        path: "/new".into(),
        min_version: ApiVersion { major: 2, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });
    let deprecated = reg.deprecated_endpoints();
    assert_eq!(deprecated.len(), 1);
    assert_eq!(deprecated[0].path, "/old");
}

#[test]
fn version_registry_current_version() {
    let reg = ApiVersionRegistry::new(ApiVersion { major: 3, minor: 1 });
    assert_eq!(*reg.current_version(), ApiVersion { major: 3, minor: 1 });
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
    assert!(versions.contains(&ApiVersion { major: 2, minor: 0 }));
    assert!(versions.contains(&ApiVersion { major: 1, minor: 0 }));
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
    let eps = reg.endpoints_for_version(&ApiVersion { major: 2, minor: 0 });
    assert_eq!(eps.len(), 2);
}

#[test]
fn versioned_endpoint_serde_roundtrip() {
    let ep = VersionedEndpoint {
        path: "/test".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: Some(ApiVersion { major: 2, minor: 0 }),
        deprecated: true,
        deprecated_message: Some("old".into()),
    };
    let json = serde_json::to_string(&ep).unwrap();
    let back: VersionedEndpoint = serde_json::from_str(&json).unwrap();
    assert_eq!(back.path, "/test");
    assert!(back.deprecated);
}

// ---------------------------------------------------------------------------
// RunStatus (lib.rs) serde
// ---------------------------------------------------------------------------

#[test]
fn run_status_pending_serde() {
    let s = RunStatus::Pending;
    let json = serde_json::to_string(&s).unwrap();
    let back: RunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, RunStatus::Pending));
}

#[test]
fn run_status_running_serde() {
    let s = RunStatus::Running;
    let json = serde_json::to_string(&s).unwrap();
    let back: RunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, RunStatus::Running));
}

#[test]
fn run_status_failed_serde() {
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
fn run_status_cancelled_serde() {
    let s = RunStatus::Cancelled;
    let json = serde_json::to_string(&s).unwrap();
    let back: RunStatus = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, RunStatus::Cancelled));
}

// ===========================================================================
// 8. Edge cases: invalid requests, concurrent access
// ===========================================================================

#[tokio::test]
async fn run_tracker_start_duplicate_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert!(tracker.start_run(id).await.is_err());
}

#[tokio::test]
async fn run_tracker_complete_untracked_fails() {
    let tracker = RunTracker::new();
    let receipt = serde_json::from_value::<Receipt>(json!({
        "meta": {"run_id": Uuid::nil(), "work_order_id": Uuid::nil(), "contract_version": "abp/v0.1", "started_at": "2025-01-01T00:00:00Z", "finished_at": "2025-01-01T00:00:01Z", "duration_ms": 1000},
        "backend": {"id": "mock", "backend_version": null, "adapter_version": null},
        "capabilities": {},
        "mode": "mapped",
        "usage_raw": {},
        "usage": {"input_tokens": null, "output_tokens": null, "cache_read_tokens": null, "cache_write_tokens": null, "request_units": null, "estimated_cost_usd": null},
        "trace": [],
        "artifacts": [],
        "verification": {"git_diff": null, "git_status": null, "harness_ok": false},
        "outcome": "complete",
        "receipt_sha256": null
    })).unwrap();
    assert!(tracker.complete_run(Uuid::new_v4(), receipt).await.is_err());
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
async fn run_tracker_cancel_untracked_fails() {
    let tracker = RunTracker::new();
    assert!(tracker.cancel_run(Uuid::new_v4()).await.is_err());
}

#[tokio::test]
async fn run_tracker_cancel_completed_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    let receipt = serde_json::from_value::<Receipt>(json!({
        "meta": {"run_id": id, "work_order_id": id, "contract_version": "abp/v0.1", "started_at": "2025-01-01T00:00:00Z", "finished_at": "2025-01-01T00:00:01Z", "duration_ms": 1000},
        "backend": {"id": "mock", "backend_version": null, "adapter_version": null},
        "capabilities": {},
        "mode": "mapped",
        "usage_raw": {},
        "usage": {"input_tokens": null, "output_tokens": null, "cache_read_tokens": null, "cache_write_tokens": null, "request_units": null, "estimated_cost_usd": null},
        "trace": [],
        "artifacts": [],
        "verification": {"git_diff": null, "git_status": null, "harness_ok": false},
        "outcome": "complete",
        "receipt_sha256": null
    })).unwrap();
    tracker.complete_run(id, receipt).await.unwrap();
    assert!(tracker.cancel_run(id).await.is_err());
}

#[tokio::test]
async fn run_tracker_get_status_nonexistent_returns_none() {
    let tracker = RunTracker::new();
    assert!(tracker.get_run_status(Uuid::new_v4()).await.is_none());
}

#[tokio::test]
async fn run_tracker_remove_running_fails() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    assert_eq!(tracker.remove_run(id).await.unwrap_err(), "conflict");
}

#[tokio::test]
async fn run_tracker_remove_nonexistent_fails() {
    let tracker = RunTracker::new();
    assert_eq!(
        tracker.remove_run(Uuid::new_v4()).await.unwrap_err(),
        "not found"
    );
}

#[tokio::test]
async fn run_tracker_remove_cancelled_succeeds() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    tracker.start_run(id).await.unwrap();
    tracker.cancel_run(id).await.unwrap();
    assert!(tracker.remove_run(id).await.is_ok());
}

#[tokio::test]
async fn run_tracker_list_runs_after_operations() {
    let tracker = RunTracker::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    tracker.start_run(id1).await.unwrap();
    tracker.start_run(id2).await.unwrap();
    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 2);
}

#[tokio::test]
async fn concurrent_run_tracker_access() {
    let tracker = RunTracker::new();
    let mut handles = Vec::new();
    for _ in 0..20 {
        let t = tracker.clone();
        handles.push(tokio::spawn(async move {
            let id = Uuid::new_v4();
            t.start_run(id).await.unwrap();
            t.cancel_run(id).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 20);
}

#[tokio::test]
async fn concurrent_queue_access_via_app() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let mut handles = Vec::new();
    for _ in 0..5 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let app = build_app(s);
            let req = RunRequest {
                backend: "mock".into(),
                work_order: test_work_order(),
            };
            let (status, _) = post_json(app, "/run", &req).await;
            assert_eq!(status, StatusCode::OK);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(state.receipts.read().await.len(), 5);
}

#[tokio::test]
async fn persist_receipt_and_read_back() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, json) = post_json(app, "/run", &req).await;
    let run_id = json["run_id"].as_str().unwrap();

    // Clear in-memory cache
    state.receipts.write().await.clear();

    // Reading from /receipts/{id} should reload from disk
    let app = build_app(state.clone());
    let (status, _) = get_json(app, &format!("/receipts/{run_id}")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn whitespace_only_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let mut wo = test_work_order();
    wo.task = "   ".into();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/run", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn empty_workspace_root_returns_400() {
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

#[test]
fn api_error_is_std_error() {
    let err = ApiApiError::internal("boom");
    let _: &dyn std::error::Error = &err;
}

#[test]
fn queue_error_is_std_error() {
    let err = QueueError::Full { max: 1 };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn api_version_error_is_std_error() {
    let err = ApiVersionError::InvalidFormat("bad".into());
    let _: &dyn std::error::Error = &err;
}
