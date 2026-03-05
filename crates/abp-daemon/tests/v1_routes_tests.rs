// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the `/v1` HTTP routes.

use abp_core::{Outcome, ReceiptBuilder, WorkOrderBuilder};
use abp_daemon::models::{BackendsListResponse, RunRequest};
use abp_daemon::routes::v1_routes;
use abp_daemon::state::{RunPhase, ServerState};
use axum::Router;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use http_body_util::BodyExt;
use std::collections::BTreeMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_state() -> Arc<ServerState> {
    Arc::new(ServerState::new(vec!["mock".into(), "sidecar:node".into()]))
}

fn empty_state() -> Arc<ServerState> {
    Arc::new(ServerState::new(vec![]))
}

fn app(state: Arc<ServerState>) -> Router {
    v1_routes(state)
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn run_request_body(task: &str, backend: &str) -> Vec<u8> {
    let wo = WorkOrderBuilder::new(task).build();
    let req = RunRequest {
        work_order: wo,
        backend: backend.into(),
        overrides: BTreeMap::new(),
    };
    serde_json::to_vec(&req).unwrap()
}

// ---------------------------------------------------------------------------
// GET /v1/health
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_health_returns_200() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn v1_health_contains_version() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["version"], abp_core::CONTRACT_VERSION);
    assert!(json["uptime_secs"].is_number());
}

#[tokio::test]
async fn v1_health_ok_with_no_backends() {
    let resp = app(empty_state())
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// GET /v1/backends
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_backends_returns_registered() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .uri("/v1/backends")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let list: BackendsListResponse = serde_json::from_value(json).unwrap();
    assert_eq!(list.backends.len(), 2);
    assert_eq!(list.backends[0].name, "mock");
}

#[tokio::test]
async fn v1_backends_empty() {
    let resp = app(empty_state())
        .oneshot(
            Request::builder()
                .uri("/v1/backends")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    let list: BackendsListResponse = serde_json::from_value(json).unwrap();
    assert!(list.backends.is_empty());
}

#[tokio::test]
async fn v1_backends_sidecar_type() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .uri("/v1/backends")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    let list: BackendsListResponse = serde_json::from_value(json).unwrap();
    let sidecar = list
        .backends
        .iter()
        .find(|b| b.name == "sidecar:node")
        .unwrap();
    assert_eq!(sidecar.backend_type, "sidecar");
}

// ---------------------------------------------------------------------------
// POST /v1/run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_run_returns_201() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/run")
                .header("content-type", "application/json")
                .body(Body::from(run_request_body("hello", "mock")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "queued");
    assert!(json["run_id"].is_string());
}

#[tokio::test]
async fn v1_run_empty_task_400() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/run")
                .header("content-type", "application/json")
                .body(Body::from(run_request_body("", "mock")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn v1_run_unknown_backend_400() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/run")
                .header("content-type", "application/json")
                .body(Body::from(run_request_body("task", "nonexistent")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp).await;
    assert_eq!(json["code"], "bad_request");
}

#[tokio::test]
async fn v1_run_invalid_json_returns_error() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/run")
                .header("content-type", "application/json")
                .body(Body::from(b"not json".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    // Axum returns 4xx for deserialization failures.
    assert!(resp.status().is_client_error());
}

// ---------------------------------------------------------------------------
// GET /v1/status/:id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_status_not_found() {
    let id = Uuid::new_v4();
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .uri(format!("/v1/status/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_status_after_create() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();

    let resp = app(state)
        .oneshot(
            Request::builder()
                .uri(format!("/v1/status/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "queued");
    assert_eq!(json["backend"], "mock");
}

#[tokio::test]
async fn v1_status_running_state() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    state
        .registry
        .transition(run_id, RunPhase::Running)
        .await
        .unwrap();

    let resp = app(state)
        .oneshot(
            Request::builder()
                .uri(format!("/v1/status/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["status"], "running");
}

// ---------------------------------------------------------------------------
// GET /v1/receipt/:id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_receipt_not_found() {
    let id = Uuid::new_v4();
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .uri(format!("/v1/receipt/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_receipt_no_receipt_yet() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();

    let resp = app(state)
        .oneshot(
            Request::builder()
                .uri(format!("/v1/receipt/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let json = body_json(resp).await;
    assert!(json["message"].as_str().unwrap().contains("no receipt"));
}

#[tokio::test]
async fn v1_receipt_after_completion() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    state
        .registry
        .transition(run_id, RunPhase::Running)
        .await
        .unwrap();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    state.registry.complete(run_id, receipt).await.unwrap();

    let resp = app(state)
        .oneshot(
            Request::builder()
                .uri(format!("/v1/receipt/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["run_id"], run_id.to_string());
    assert!(json["receipt"].is_object());
}

// ---------------------------------------------------------------------------
// POST /v1/cancel/:id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_cancel_not_found() {
    let id = Uuid::new_v4();
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/v1/cancel/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_cancel_queued_run() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();

    let resp = app(state)
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/v1/cancel/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "cancelled");
}

#[tokio::test]
async fn v1_cancel_running_run() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    state
        .registry
        .transition(run_id, RunPhase::Running)
        .await
        .unwrap();

    let resp = app(state)
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/v1/cancel/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn v1_cancel_completed_run_409() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    state
        .registry
        .transition(run_id, RunPhase::Running)
        .await
        .unwrap();
    state
        .registry
        .transition(run_id, RunPhase::Completed)
        .await
        .unwrap();

    let resp = app(state)
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/v1/cancel/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

// ---------------------------------------------------------------------------
// Method checks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_health_post_returns_405() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn v1_run_get_returns_405() {
    let resp = app(test_state())
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/v1/run")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// ---------------------------------------------------------------------------
// End-to-end: submit then check status
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v1_submit_then_status() {
    let state = test_state();
    let wo = WorkOrderBuilder::new("e2e task").build();
    let run_id = wo.id;
    let req_body = RunRequest {
        work_order: wo,
        backend: "mock".into(),
        overrides: BTreeMap::new(),
    };

    // Submit.
    let resp = app(state.clone())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/v1/run")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Check status.
    let resp = app(state)
        .oneshot(
            Request::builder()
                .uri(format!("/v1/status/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "queued");
}

// ---------------------------------------------------------------------------
// DaemonState wrapper
// ---------------------------------------------------------------------------

#[tokio::test]
async fn daemon_state_wrapper_creates_inner() {
    use abp_daemon::state::DaemonState;
    let ds = DaemonState::new(vec!["mock".into()]);
    assert_eq!(ds.uptime_secs(), 0);
    assert_eq!(ds.backends().list().await, vec!["mock".to_string()]);
}

#[tokio::test]
async fn daemon_state_from_arc() {
    use abp_daemon::state::DaemonState;
    let arc = Arc::new(ServerState::new(vec!["a".into()]));
    let ds: DaemonState = arc.into();
    assert_eq!(ds.backends().list().await, vec!["a".to_string()]);
}
