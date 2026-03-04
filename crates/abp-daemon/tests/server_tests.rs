// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the daemon HTTP server scaffolding.

use abp_daemon::api::{HealthResponse, ListBackendsResponse, RunResponse, RunStatus};
use abp_daemon::server::{router, DaemonServer, VersionResponse};
use abp_daemon::state::ServerState;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_state() -> Arc<ServerState> {
    Arc::new(ServerState::new(vec![
        "mock".into(),
        "sidecar:node".into(),
    ]))
}

fn empty_state() -> Arc<ServerState> {
    Arc::new(ServerState::new(vec![]))
}

async fn send_get(
    state: Arc<ServerState>,
    uri: &str,
) -> http::Response<Body> {
    router(state)
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn send_post_json(
    state: Arc<ServerState>,
    uri: &str,
    body: &impl serde::Serialize,
) -> http::Response<Body> {
    let json = serde_json::to_string(body).unwrap();
    router(state)
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

async fn body_json(resp: http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ===========================================================================
// 1. Router construction
// ===========================================================================

#[test]
fn router_construction_succeeds() {
    let state = test_state();
    let _router = router(state);
}

#[test]
fn daemon_server_construction() {
    let state = test_state();
    let server = DaemonServer::new(state.clone());
    let _router = server.router();
}

#[tokio::test]
async fn server_state_default_has_empty_backends() {
    let state = ServerState::default();
    assert!(state.backends.is_empty().await);
}

#[test]
fn server_state_uptime_zero_initially() {
    let state = ServerState::new(vec![]);
    assert!(state.uptime_secs() < 2);
}

// ===========================================================================
// 2. Health endpoint
// ===========================================================================

#[tokio::test]
async fn health_returns_200() {
    let resp = send_get(test_state(), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_returns_json_content_type() {
    let resp = send_get(test_state(), "/health").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn health_status_is_ok() {
    let resp = send_get(test_state(), "/health").await;
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_includes_contract_version() {
    let resp = send_get(test_state(), "/health").await;
    let json = body_json(resp).await;
    assert_eq!(json["version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_includes_uptime() {
    let resp = send_get(test_state(), "/health").await;
    let json = body_json(resp).await;
    assert!(json["uptime_secs"].is_number());
}

#[tokio::test]
async fn health_lists_registered_backends() {
    let resp = send_get(test_state(), "/health").await;
    let json = body_json(resp).await;
    let backends = json["backends"].as_array().unwrap();
    assert_eq!(backends.len(), 2);
    assert!(backends.contains(&serde_json::json!("mock")));
    assert!(backends.contains(&serde_json::json!("sidecar:node")));
}

#[tokio::test]
async fn health_empty_backends_when_none_registered() {
    let resp = send_get(empty_state(), "/health").await;
    let json = body_json(resp).await;
    let backends = json["backends"].as_array().unwrap();
    assert!(backends.is_empty());
}

#[tokio::test]
async fn health_deserializes_as_health_response() {
    let resp = send_get(test_state(), "/health").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let health: HealthResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(health.status, "ok");
    assert_eq!(health.version, abp_core::CONTRACT_VERSION);
}

// ===========================================================================
// 3. Backends endpoint
// ===========================================================================

#[tokio::test]
async fn backends_returns_200() {
    let resp = send_get(test_state(), "/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn backends_returns_registered_names() {
    let resp = send_get(test_state(), "/backends").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let list: ListBackendsResponse = serde_json::from_slice(&bytes).unwrap();
    let names: Vec<&str> = list.backends.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"mock"));
    assert!(names.contains(&"sidecar:node"));
}

#[tokio::test]
async fn backends_empty_when_none_registered() {
    let resp = send_get(empty_state(), "/backends").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let list: ListBackendsResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(list.backends.is_empty());
}

#[tokio::test]
async fn backends_returns_json_content_type() {
    let resp = send_get(test_state(), "/backends").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

// ===========================================================================
// 4. Run endpoint
// ===========================================================================

#[tokio::test]
async fn run_accepts_valid_request() {
    let body = serde_json::json!({"task": "hello world"});
    let resp = send_post_json(test_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_returns_run_id() {
    let body = serde_json::json!({"task": "hello world"});
    let resp = send_post_json(test_state(), "/run", &body).await;
    let json = body_json(resp).await;
    let run_id = json["run_id"].as_str().unwrap();
    assert!(!run_id.is_empty());
    // Should be a valid UUID.
    assert!(run_id.parse::<uuid::Uuid>().is_ok());
}

#[tokio::test]
async fn run_returns_queued_status() {
    let body = serde_json::json!({"task": "hello world"});
    let resp = send_post_json(test_state(), "/run", &body).await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let run_resp: RunResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(run_resp.status, RunStatus::Queued);
}

#[tokio::test]
async fn run_rejects_empty_task() {
    let body = serde_json::json!({"task": ""});
    let resp = send_post_json(test_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp).await;
    assert!(json["error"].as_str().unwrap().contains("task"));
}

#[tokio::test]
async fn run_rejects_unknown_backend() {
    let body = serde_json::json!({"task": "hello", "backend": "nonexistent"});
    let resp = send_post_json(test_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp).await;
    assert!(json["error"].as_str().unwrap().contains("unknown backend"));
}

#[tokio::test]
async fn run_accepts_request_without_backend() {
    let body = serde_json::json!({"task": "hello"});
    let resp = send_post_json(test_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_accepts_known_backend() {
    let body = serde_json::json!({"task": "hello", "backend": "mock"});
    let resp = send_post_json(test_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_rejects_missing_content_type() {
    let resp = router(test_state())
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
async fn run_rejects_malformed_json() {
    let resp = router(test_state())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/run")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();
    // axum returns 400 for JSON syntax errors.
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===========================================================================
// 5. Version endpoint
// ===========================================================================

#[tokio::test]
async fn version_returns_200() {
    let resp = send_get(test_state(), "/version").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn version_includes_contract_version() {
    let resp = send_get(test_state(), "/version").await;
    let json = body_json(resp).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn version_includes_crate_version() {
    let resp = send_get(test_state(), "/version").await;
    let json = body_json(resp).await;
    let version = json["version"].as_str().unwrap();
    assert!(!version.is_empty());
}

#[tokio::test]
async fn version_returns_json_content_type() {
    let resp = send_get(test_state(), "/version").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn version_deserializes_as_version_response() {
    let resp = send_get(test_state(), "/version").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let ver: VersionResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(ver.contract_version, abp_core::CONTRACT_VERSION);
    assert!(!ver.version.is_empty());
}

// ===========================================================================
// 6. Routing errors
// ===========================================================================

#[tokio::test]
async fn unknown_route_returns_404() {
    let resp = send_get(test_state(), "/nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_to_health_returns_405() {
    let body = serde_json::json!({});
    let resp = send_post_json(test_state(), "/health", &body).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn get_to_run_returns_405() {
    let resp = send_get(test_state(), "/run").await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn run_with_config_field_accepted() {
    let body = serde_json::json!({
        "task": "do something",
        "config": {"model": "gpt-4"}
    });
    let resp = send_post_json(test_state(), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn daemon_server_state_accessor() {
    let state = test_state();
    let server = DaemonServer::new(state.clone());
    assert!(Arc::ptr_eq(server.state(), &state));
}
