// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::validation::RequestValidator;
use abp_daemon::{AppState, RunRequest, RunTracker, build_app};
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
        id: Uuid::nil(),
        task: "test task".into(),
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

// ---------------------------------------------------------------------------
// Unit tests for RequestValidator
// ---------------------------------------------------------------------------

#[test]
fn valid_work_order_passes() {
    let wo = test_work_order();
    assert!(RequestValidator::validate_work_order(&wo).is_ok());
}

#[test]
fn empty_task_rejected() {
    let mut wo = test_work_order();
    wo.task = String::new();
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("task must not be empty")));
}

#[test]
fn whitespace_only_task_rejected() {
    let mut wo = test_work_order();
    wo.task = "   \t\n  ".into();
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("non-whitespace")));
}

#[test]
fn very_long_task_rejected() {
    let mut wo = test_work_order();
    wo.task = "x".repeat(200_000);
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("maximum length")));
}

#[test]
fn empty_workspace_root_rejected() {
    let mut wo = test_work_order();
    wo.workspace.root = String::new();
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("workspace.root")));
}

#[test]
fn negative_budget_rejected() {
    let mut wo = test_work_order();
    wo.config.max_budget_usd = Some(-1.0);
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("negative")));
}

#[test]
fn valid_uuid_format_accepted() {
    let id = Uuid::new_v4().to_string();
    assert!(RequestValidator::validate_run_id(&id).is_ok());
}

#[test]
fn invalid_uuid_format_rejected() {
    let err = RequestValidator::validate_run_id("not-a-valid-uuid").unwrap_err();
    assert!(err.contains("invalid UUID format"));
}

#[test]
fn empty_uuid_rejected() {
    assert!(RequestValidator::validate_run_id("").is_err());
}

#[test]
fn unknown_backend_name_rejected() {
    let backends = vec!["mock".to_string(), "sidecar:node".to_string()];
    let err = RequestValidator::validate_backend_name("nonexistent", &backends).unwrap_err();
    assert!(err.contains("unknown backend"));
}

#[test]
fn valid_backend_name_passes() {
    let backends = vec!["mock".to_string(), "sidecar:node".to_string()];
    assert!(RequestValidator::validate_backend_name("mock", &backends).is_ok());
}

#[test]
fn empty_backend_name_rejected() {
    let backends = vec!["mock".to_string()];
    let err = RequestValidator::validate_backend_name("", &backends).unwrap_err();
    assert!(err.contains("must not be empty"));
}

#[test]
fn very_long_backend_name_rejected() {
    let backends = vec!["mock".to_string()];
    let long_name = "a".repeat(300);
    let err = RequestValidator::validate_backend_name(&long_name, &backends).unwrap_err();
    assert!(err.contains("maximum length"));
}

#[test]
fn valid_config_passes() {
    let config = serde_json::json!({"model": "gpt-4", "temperature": 0.7});
    assert!(RequestValidator::validate_config(&config).is_ok());
}

#[test]
fn non_object_config_rejected() {
    let config = serde_json::json!("just a string");
    let errors = RequestValidator::validate_config(&config).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("must be a JSON object")));
}

#[test]
fn array_config_rejected() {
    let config = serde_json::json!([1, 2, 3]);
    let errors = RequestValidator::validate_config(&config).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("must be a JSON object")));
}

#[test]
fn deeply_nested_config_rejected() {
    // Build a config nested > 10 levels deep.
    let mut val = serde_json::json!("leaf");
    for _ in 0..12 {
        val = serde_json::json!({ "nested": val });
    }
    let errors = RequestValidator::validate_config(&val).unwrap_err();
    assert!(errors.iter().any(|e| e.contains("nesting depth")));
}

#[test]
fn empty_object_config_accepted() {
    let config = serde_json::json!({});
    assert!(RequestValidator::validate_config(&config).is_ok());
}

#[test]
fn multiple_work_order_errors_accumulated() {
    let mut wo = test_work_order();
    wo.task = String::new();
    wo.workspace.root = String::new();
    wo.config.max_budget_usd = Some(-5.0);
    let errors = RequestValidator::validate_work_order(&wo).unwrap_err();
    assert!(
        errors.len() >= 3,
        "expected at least 3 errors, got {}: {:?}",
        errors.len(),
        errors
    );
}

// ---------------------------------------------------------------------------
// Integration tests: validation through HTTP routes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn validate_endpoint_rejects_empty_task() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = String::new();
    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validate")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("task"), "expected task error, got: {error}");
}

#[tokio::test]
async fn validate_endpoint_rejects_unknown_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let req_body = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validate")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let error = json["error"].as_str().unwrap();
    assert!(
        error.contains("unknown backend"),
        "expected backend error, got: {error}"
    );
}

#[tokio::test]
async fn validate_endpoint_accepts_valid_request() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validate")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["valid"], true);
}

#[tokio::test]
async fn run_endpoint_rejects_empty_task() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = String::new();
    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: wo,
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

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
