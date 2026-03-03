// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the `/api/v1` versioned HTTP endpoints.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::{
    AppState, RunRequest, RunResponse, RunTracker, ValidationResponse, build_versioned_app,
};
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
        task: "v1 integration test task".into(),
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

// ===========================================================================
// Helpers
// ===========================================================================

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

// ===========================================================================
// 1. GET /api/v1/health — returns 200 with status field
// ===========================================================================

#[tokio::test]
async fn v1_health_returns_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let (status, json) = get_json(app, "/api/v1/health").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
}

// ===========================================================================
// 2. Health endpoint returns contract version
// ===========================================================================

#[tokio::test]
async fn v1_health_returns_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let (_, json) = get_json(app, "/api/v1/health").await;

    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

// ===========================================================================
// 3. Health endpoint response is JSON content type
// ===========================================================================

#[tokio::test]
async fn v1_health_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
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

// ===========================================================================
// 4. GET /api/v1/backends — returns 200
// ===========================================================================

#[tokio::test]
async fn v1_backends_returns_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let (status, _) = get_json(app, "/api/v1/backends").await;

    assert_eq!(status, StatusCode::OK);
}

// ===========================================================================
// 5. Backends endpoint returns an array containing mock
// ===========================================================================

#[tokio::test]
async fn v1_backends_includes_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/backends")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let names: Vec<String> = serde_json::from_slice(&body).unwrap();
    assert!(names.contains(&"mock".to_string()));
}

// ===========================================================================
// 6. POST /api/v1/run — accepts valid work order
// ===========================================================================

#[tokio::test]
async fn v1_run_accepts_valid_request() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, _) = post_json(app, "/api/v1/run", &req).await;

    assert_eq!(status, StatusCode::OK);
}

// ===========================================================================
// 7. Run endpoint returns receipt with hash
// ===========================================================================

#[tokio::test]
async fn v1_run_returns_receipt_with_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, json) = post_json(app, "/api/v1/run", &req).await;

    assert!(json["receipt"]["receipt_sha256"].is_string());
}

// ===========================================================================
// 8. Run endpoint returns run_id
// ===========================================================================

#[tokio::test]
async fn v1_run_returns_run_id() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (_, json) = post_json(app, "/api/v1/run", &req).await;

    assert!(json["run_id"].is_string());
}

// ===========================================================================
// 9. Run endpoint rejects unknown backend
// ===========================================================================

#[tokio::test]
async fn v1_run_rejects_unknown_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };
    let (status, _) = post_json(app, "/api/v1/run", &req).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// 10. Run endpoint rejects empty task
// ===========================================================================

#[tokio::test]
async fn v1_run_rejects_empty_task() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, _) = post_json(app, "/api/v1/run", &req).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ===========================================================================
// 11. GET /api/v1/status/:run_id — 404 for unknown run
// ===========================================================================

#[tokio::test]
async fn v1_status_unknown_run_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let (status, _) = get_json(app, &format!("/api/v1/status/{}", Uuid::new_v4())).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// 12. Status endpoint returns run info after submission
// ===========================================================================

#[tokio::test]
async fn v1_status_returns_info_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Submit a run.
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let app = build_versioned_app(state.clone());
    let (_, run_json) = post_json(app, "/api/v1/run", &req).await;
    let run_id = run_json["run_id"].as_str().unwrap();

    // Query status.
    let app = build_versioned_app(state);
    let (status, json) = get_json(app, &format!("/api/v1/status/{run_id}")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["run_id"], run_id);
}

// ===========================================================================
// 13. POST /api/v1/validate — accepts valid work order
// ===========================================================================

#[tokio::test]
async fn v1_validate_accepts_valid_work_order() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/api/v1/validate", &req).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["valid"], true);
}

// ===========================================================================
// 14. Validate endpoint rejects empty task
// ===========================================================================

#[tokio::test]
async fn v1_validate_rejects_empty_task() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, json) = post_json(app, "/api/v1/validate", &req).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["valid"], false);
    assert!(!json["errors"].as_array().unwrap().is_empty());
}

// ===========================================================================
// 15. Validate endpoint rejects unknown backend
// ===========================================================================

#[tokio::test]
async fn v1_validate_rejects_unknown_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };
    let (status, json) = post_json(app, "/api/v1/validate", &req).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["valid"], false);
    let errors = json["errors"].as_array().unwrap();
    assert!(errors
        .iter()
        .any(|e| e.as_str().unwrap().contains("unknown backend")));
}

// ===========================================================================
// 16. Validate returns valid=false with multiple errors
// ===========================================================================

#[tokio::test]
async fn v1_validate_accumulates_multiple_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: wo,
    };
    let (_, json) = post_json(app, "/api/v1/validate", &req).await;

    assert_eq!(json["valid"], false);
    assert!(json["errors"].as_array().unwrap().len() >= 2);
}

// ===========================================================================
// 17. ValidationResponse serde roundtrip (valid case)
// ===========================================================================

#[test]
fn validation_response_valid_roundtrip() {
    let resp = ValidationResponse {
        valid: true,
        errors: vec![],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ValidationResponse = serde_json::from_str(&json).unwrap();
    assert!(back.valid);
    assert!(back.errors.is_empty());
}

// ===========================================================================
// 18. ValidationResponse serde roundtrip (invalid case)
// ===========================================================================

#[test]
fn validation_response_invalid_roundtrip() {
    let resp = ValidationResponse {
        valid: false,
        errors: vec![
            "task must not be empty".into(),
            "unknown backend: x".into(),
        ],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ValidationResponse = serde_json::from_str(&json).unwrap();
    assert!(!back.valid);
    assert_eq!(back.errors.len(), 2);
}

// ===========================================================================
// 19. ValidationResponse omits empty errors in serialization
// ===========================================================================

#[test]
fn validation_response_omits_empty_errors() {
    let resp = ValidationResponse {
        valid: true,
        errors: vec![],
    };
    let val = serde_json::to_value(&resp).unwrap();
    assert!(val.get("errors").is_none());
}

// ===========================================================================
// 20. ValidationResponse includes errors when present
// ===========================================================================

#[test]
fn validation_response_includes_errors_when_present() {
    let resp = ValidationResponse {
        valid: false,
        errors: vec!["bad field".into()],
    };
    let val = serde_json::to_value(&resp).unwrap();
    assert!(val.get("errors").is_some());
    assert_eq!(val["errors"][0], "bad field");
}

// ===========================================================================
// 21. Legacy routes still work alongside v1
// ===========================================================================

#[tokio::test]
async fn legacy_routes_still_work_alongside_v1() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let (status, json) = get_json(app, "/health").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
}

// ===========================================================================
// 22. V1 health and legacy health return same structure
// ===========================================================================

#[tokio::test]
async fn v1_and_legacy_health_return_same_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let app1 = build_versioned_app(state.clone());
    let (_, v1_json) = get_json(app1, "/api/v1/health").await;

    let app2 = build_versioned_app(state);
    let (_, legacy_json) = get_json(app2, "/health").await;

    assert_eq!(v1_json["status"], legacy_json["status"]);
    assert_eq!(
        v1_json["contract_version"],
        legacy_json["contract_version"]
    );
}

// ===========================================================================
// 23. Run response deserializes into RunResponse
// ===========================================================================

#[tokio::test]
async fn v1_run_response_deserializes_as_run_response() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/run")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let run_resp: RunResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(run_resp.backend, "mock");
    assert!(run_resp.receipt.receipt_sha256.is_some());
}

// ===========================================================================
// 24. Validate with whitespace-only task fails
// ===========================================================================

#[tokio::test]
async fn v1_validate_rejects_whitespace_only_task() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = "   ".into();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let (status, json) = post_json(app, "/api/v1/validate", &req).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["valid"], false);
}

// ===========================================================================
// 25. Backends returns JSON array type
// ===========================================================================

#[tokio::test]
async fn v1_backends_returns_json_array() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(test_state(tmp.path()));

    let (status, json) = get_json(app, "/api/v1/backends").await;

    assert_eq!(status, StatusCode::OK);
    assert!(json.is_array(), "expected JSON array from /api/v1/backends");
}
