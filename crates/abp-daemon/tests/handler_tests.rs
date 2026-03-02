// SPDX-License-Identifier: MIT OR Apache-2.0
//! Focused handler tests for the five core daemon endpoints:
//! GET /health, GET /backends, POST /run, GET /runs/{id}, GET /schema/{type}.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, Receipt, RuntimeConfig,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::{AppState, BackendInfo, RunRequest, RunResponse, RunTracker, build_app};
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "handler test task".into(),
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

async fn do_run(state: &Arc<AppState>) -> RunResponse {
    let app = build_app(state.clone());
    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, _) = post_json(app, "/run", &req_body).await;
    assert_eq!(status, StatusCode::OK);

    // Re-do to get typed response (avoid double-parsing the value)
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
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

// ===========================================================================
// 1. GET /health
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
async fn health_contains_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (_, json) = get_json(app, "/health").await;

    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_contains_time_field() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (_, json) = get_json(app, "/health").await;

    let time_str = json["time"].as_str().expect("time should be a string");
    // Should parse as RFC 3339
    assert!(
        chrono::DateTime::parse_from_rfc3339(time_str).is_ok(),
        "time field should be RFC 3339: {time_str}"
    );
}

#[tokio::test]
async fn health_content_type_is_application_json() {
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
        .expect("missing content-type header")
        .to_str()
        .unwrap();
    assert!(
        ct.contains("application/json"),
        "expected application/json, got: {ct}"
    );
}

// ===========================================================================
// 2. GET /backends — list registered backends with capabilities
// ===========================================================================

#[tokio::test]
async fn backends_returns_200_with_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/backends").await;

    assert_eq!(status, StatusCode::OK);
    let names: Vec<String> = serde_json::from_value(json).unwrap();
    assert!(
        names.contains(&"mock".to_string()),
        "mock backend should be listed"
    );
}

#[tokio::test]
async fn capabilities_lists_backends_with_capability_manifests() {
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

    assert!(!infos.is_empty(), "should have at least one backend");
    let mock_info = infos.iter().find(|i| i.id == "mock").expect("mock missing");
    assert!(
        !mock_info.capabilities.is_empty(),
        "mock should report capabilities"
    );
}

#[tokio::test]
async fn capabilities_filter_by_single_backend() {
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

// ===========================================================================
// 3. POST /run — accept WorkOrder JSON, validate, return receipt
// ===========================================================================

#[tokio::test]
async fn run_valid_work_order_returns_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());

    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/run", &req_body).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json.get("run_id").is_some(), "response should have run_id");
    assert_eq!(json["backend"], "mock");
    assert!(
        json.get("receipt").is_some(),
        "response should have receipt"
    );
    assert!(json.get("events").is_some(), "response should have events");
}

#[tokio::test]
async fn run_receipt_has_sha256_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_resp = do_run(&state).await;

    assert!(
        run_resp.receipt.receipt_sha256.is_some(),
        "receipt should contain sha256 hash"
    );
}

#[tokio::test]
async fn run_receipt_is_persisted_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_resp = do_run(&state).await;

    let path = tmp.path().join(format!("{}.json", run_resp.run_id));
    assert!(path.exists(), "receipt file should be persisted to disk");

    let raw = std::fs::read(&path).unwrap();
    let receipt: Receipt = serde_json::from_slice(&raw).unwrap();
    assert_eq!(receipt.meta.run_id, run_resp.run_id);
}

#[tokio::test]
async fn run_unknown_backend_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let req_body = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/run", &req_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json.get("error").is_some(),
        "error response should have error field"
    );
}

#[tokio::test]
async fn run_empty_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = String::new();
    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, json) = post_json(app, "/run", &req_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn run_whitespace_only_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = "   ".into();
    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/run", &req_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
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
                .body(Body::from("this is not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        resp.status().is_client_error(),
        "expected 4xx for invalid JSON, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn run_events_are_non_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_resp = do_run(&state).await;

    assert!(
        !run_resp.events.is_empty(),
        "mock backend should emit at least one event"
    );
}

// ===========================================================================
// 4. GET /runs/{id} — check status of running/completed work
// ===========================================================================

#[tokio::test]
async fn get_run_completed_shows_status() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_resp = do_run(&state).await;

    let app = build_app(state.clone());
    let (status, json) = get_json(app, &format!("/runs/{}", run_resp.run_id)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["run_id"], run_resp.run_id.to_string());
    // Status should contain completed variant info
    let status_val = &json["status"];
    assert!(
        status_val.get("status").is_some() || status_val.is_string(),
        "status field should be present: {json}"
    );
}

#[tokio::test]
async fn get_run_nonexistent_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let fake_id = Uuid::new_v4();

    let (status, _) = get_json(app, &format!("/runs/{}", fake_id)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_run_running_shows_running_status() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Manually insert a running run.
    let run_id = Uuid::new_v4();
    state.run_tracker.start_run(run_id).await.unwrap();

    let app = build_app(state.clone());
    let (status, json) = get_json(app, &format!("/runs/{}", run_id)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["run_id"], run_id.to_string());
}

#[tokio::test]
async fn get_run_receipt_for_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_resp = do_run(&state).await;

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{}/receipt", run_resp.run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let receipt: Receipt = serde_json::from_slice(&body).unwrap();
    assert_eq!(receipt.meta.run_id, run_resp.run_id);
}

#[tokio::test]
async fn cancel_running_run_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let run_id = Uuid::new_v4();
    state.run_tracker.start_run(run_id).await.unwrap();

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{}/cancel", run_id))
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
async fn list_runs_includes_completed_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_resp = do_run(&state).await;

    let app = build_app(state.clone());
    let (status, json) = get_json(app, "/runs").await;

    assert_eq!(status, StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(
        ids.contains(&run_resp.run_id),
        "completed run should appear in /runs list"
    );
}

// ===========================================================================
// 5. GET /schema/{type} — serve JSON schemas
// ===========================================================================

#[tokio::test]
async fn schema_work_order_returns_valid_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/work_order").await;

    assert_eq!(status, StatusCode::OK);
    assert!(json.is_object(), "schema should be a JSON object");
    // A JSON Schema document typically has title, $schema, or type
    let schema_str = serde_json::to_string(&json).unwrap();
    assert!(
        schema_str.contains("WorkOrder") || schema_str.contains("work_order"),
        "work_order schema should reference WorkOrder type"
    );
}

#[tokio::test]
async fn schema_receipt_returns_valid_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/receipt").await;

    assert_eq!(status, StatusCode::OK);
    let schema_str = serde_json::to_string(&json).unwrap();
    assert!(
        schema_str.contains("Receipt") || schema_str.contains("receipt"),
        "receipt schema should reference Receipt type"
    );
}

#[tokio::test]
async fn schema_capability_requirements_returns_valid_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/capability_requirements").await;

    assert_eq!(status, StatusCode::OK);
    assert!(json.is_object());
}

#[tokio::test]
async fn schema_backplane_config_returns_valid_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/backplane_config").await;

    assert_eq!(status, StatusCode::OK);
    assert!(json.is_object());
}

#[tokio::test]
async fn schema_unknown_type_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (status, json) = get_json(app, "/schema/nonexistent").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(json.get("error").is_some(), "should return error body");
}

#[tokio::test]
async fn schema_has_json_content_type() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/schema/work_order")
                .body(Body::empty())
                .unwrap(),
        )
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
        "schema endpoint should return application/json, got: {ct}"
    );
}

#[tokio::test]
async fn schema_work_order_has_schema_or_type_field() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let (_, json) = get_json(app, "/schema/work_order").await;

    assert!(
        json.get("$schema").is_some() || json.get("type").is_some() || json.get("title").is_some(),
        "JSON Schema should have $schema, type, or title field"
    );
}
