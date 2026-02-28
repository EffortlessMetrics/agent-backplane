// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive health, metrics, and backends endpoint tests for the daemon.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::{AppState, RunMetrics, RunRequest, RunTracker, build_app};
use abp_integrations::MockBackend;
use abp_runtime::Runtime;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;
use uuid::Uuid;

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "health test task".into(),
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

// ---------------------------------------------------------------------------
// 1. Health endpoint – returns JSON with status, contract_version, time
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_returns_json_with_required_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let (status, json) = get_json(app, "/health").await;

    assert_eq!(status, StatusCode::OK);
    assert!(json.get("status").is_some(), "missing 'status' field");
    assert!(
        json.get("contract_version").is_some(),
        "missing 'contract_version' field"
    );
    assert!(json.get("time").is_some(), "missing 'time' field");
}

// ---------------------------------------------------------------------------
// 2. Health status value – "ok" when the daemon is running
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_status_is_ok_when_running() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let (_, json) = get_json(app, "/health").await;

    assert_eq!(json["status"], "ok");
}

// ---------------------------------------------------------------------------
// 3. Contract version matches abp_core::CONTRACT_VERSION
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_reports_correct_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let (_, json) = get_json(app, "/health").await;

    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

// ---------------------------------------------------------------------------
// 4a. Metrics endpoint – initially shows zero runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn metrics_initially_shows_zero_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let metrics: RunMetrics = serde_json::from_slice(&body).unwrap();

    assert_eq!(metrics.total_runs, 0);
    assert_eq!(metrics.running, 0);
    assert_eq!(metrics.completed, 0);
    assert_eq!(metrics.failed, 0);
}

// ---------------------------------------------------------------------------
// 4b. Metrics endpoint – counts update after a run completes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn metrics_update_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Submit a run via the mock backend.
    {
        let app = build_app(state.clone());
        let req_body = RunRequest {
            backend: "mock".into(),
            work_order: test_work_order(),
        };
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/run")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Now check metrics reflect the completed run.
    let app = build_app(state);
    let resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let metrics: RunMetrics = serde_json::from_slice(&body).unwrap();

    assert!(metrics.total_runs >= 1, "expected at least 1 total run");
    assert!(metrics.completed >= 1, "expected at least 1 completed run");
}

// ---------------------------------------------------------------------------
// 5. Backends endpoint – lists registered backends including mock
// ---------------------------------------------------------------------------

#[tokio::test]
async fn backends_lists_registered_backends() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(Request::builder().uri("/backends").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let names: Vec<String> = serde_json::from_slice(&body).unwrap();
    assert!(
        names.contains(&"mock".to_string()),
        "expected 'mock' in backend list, got: {names:?}"
    );
}

#[tokio::test]
async fn backends_returns_json_array() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let (status, json) = get_json(app, "/backends").await;

    assert_eq!(status, StatusCode::OK);
    assert!(json.is_array(), "expected JSON array from /backends");
}

// ---------------------------------------------------------------------------
// 6. Multiple rapid health checks – 100 calls all succeed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rapid_health_checks_all_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    for i in 0..100 {
        let app = build_app(state.clone());
        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "health check #{i} failed with status {}",
            resp.status()
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Health response time – completes quickly (no hard timing assertion)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_response_completes_quickly() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let start = std::time::Instant::now();
    let resp = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status(), StatusCode::OK);
    // Log elapsed time for observability; no hard assertion to avoid flaky CI.
    eprintln!("health response time: {elapsed:?}");
}

// ---------------------------------------------------------------------------
// Supplementary: health content-type is application/json
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let ct = resp
        .headers()
        .get("content-type")
        .expect("missing content-type")
        .to_str()
        .unwrap();
    assert!(
        ct.contains("application/json"),
        "expected application/json, got: {ct}"
    );
}

// ---------------------------------------------------------------------------
// Supplementary: metrics content-type is application/json
// ---------------------------------------------------------------------------

#[tokio::test]
async fn metrics_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let ct = resp
        .headers()
        .get("content-type")
        .expect("missing content-type")
        .to_str()
        .unwrap();
    assert!(
        ct.contains("application/json"),
        "expected application/json, got: {ct}"
    );
}
