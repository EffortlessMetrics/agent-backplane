// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep integration tests for the daemon HTTP control-plane API.
//!
//! Tests cover both the `server::router` (lightweight `ServerState`) and the
//! full `build_app` router (with `Runtime` + `MockBackend`).

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::api::{
    HealthResponse as ApiHealthResponse, ListBackendsResponse, RunResponse as ApiRunResponse,
    RunStatus as ApiRunStatus,
};
use abp_daemon::server::{VersionResponse, router as server_router};
use abp_daemon::state::{BackendList, RunPhase, RunRegistry, ServerState};
use abp_daemon::{AppState, RunRequest, RunTracker, build_app, build_versioned_app};
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
        task: "deep test task".into(),
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

async fn body_json(resp: http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ===========================================================================
// Section 1: server::router — Health endpoint (GET /health)
// ===========================================================================

#[tokio::test]
async fn health_returns_200_ok() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_status_field_is_ok() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/health").await;
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_version_matches_contract() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/health").await;
    let json = body_json(resp).await;
    assert_eq!(json["version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_includes_uptime_secs() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/health").await;
    let json = body_json(resp).await;
    assert!(json["uptime_secs"].is_number());
}

#[tokio::test]
async fn health_includes_backend_list() {
    let resp = send_get(
        server_router(server_state(vec!["mock", "sidecar:node"])),
        "/health",
    )
    .await;
    let json = body_json(resp).await;
    let backends = json["backends"].as_array().unwrap();
    assert_eq!(backends.len(), 2);
}

#[tokio::test]
async fn health_returns_json_content_type() {
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
async fn health_deserializes_into_typed_response() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/health").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let health: ApiHealthResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(health.status, "ok");
    assert_eq!(health.version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_empty_backends_when_none() {
    let resp = send_get(server_router(server_state(vec![])), "/health").await;
    let json = body_json(resp).await;
    assert!(json["backends"].as_array().unwrap().is_empty());
}

// ===========================================================================
// Section 2: server::router — Backend list endpoint (GET /backends)
// ===========================================================================

#[tokio::test]
async fn backends_returns_200() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn backends_lists_registered_names() {
    let resp = send_get(
        server_router(server_state(vec!["mock", "sidecar:node"])),
        "/backends",
    )
    .await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let list: ListBackendsResponse = serde_json::from_slice(&bytes).unwrap();
    let names: Vec<&str> = list.backends.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"mock"));
    assert!(names.contains(&"sidecar:node"));
}

#[tokio::test]
async fn backends_returns_empty_list() {
    let resp = send_get(server_router(server_state(vec![])), "/backends").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let list: ListBackendsResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(list.backends.is_empty());
}

#[tokio::test]
async fn backends_returns_json_content_type() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/backends").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));
}

#[tokio::test]
async fn backends_each_entry_has_name_and_status() {
    let resp = send_get(server_router(server_state(vec!["mock"])), "/backends").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let list: ListBackendsResponse = serde_json::from_slice(&bytes).unwrap();
    for b in &list.backends {
        assert!(!b.name.is_empty());
        assert!(!b.status.is_empty());
    }
}

// ===========================================================================
// Section 3: server::router — Run endpoint (POST /run)
// ===========================================================================

#[tokio::test]
async fn run_valid_returns_201() {
    let body = serde_json::json!({"task": "hello world"});
    let resp = send_post_json(server_router(server_state(vec!["mock"])), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_returns_valid_uuid_run_id() {
    let body = serde_json::json!({"task": "hello"});
    let resp = send_post_json(server_router(server_state(vec!["mock"])), "/run", &body).await;
    let json = body_json(resp).await;
    let run_id = json["run_id"].as_str().unwrap();
    assert!(run_id.parse::<Uuid>().is_ok());
}

#[tokio::test]
async fn run_status_is_queued() {
    let body = serde_json::json!({"task": "hello"});
    let resp = send_post_json(server_router(server_state(vec!["mock"])), "/run", &body).await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let run_resp: ApiRunResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(run_resp.status, ApiRunStatus::Queued);
}

#[tokio::test]
async fn run_rejects_empty_task() {
    let body = serde_json::json!({"task": ""});
    let resp = send_post_json(server_router(server_state(vec!["mock"])), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_rejects_unknown_backend() {
    let body = serde_json::json!({"task": "hi", "backend": "nonexistent"});
    let resp = send_post_json(server_router(server_state(vec!["mock"])), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_error_body_contains_error_field() {
    let body = serde_json::json!({"task": ""});
    let resp = send_post_json(server_router(server_state(vec!["mock"])), "/run", &body).await;
    let json = body_json(resp).await;
    assert!(json["error"].is_string());
}

#[tokio::test]
async fn run_accepts_known_backend() {
    let body = serde_json::json!({"task": "hello", "backend": "mock"});
    let resp = send_post_json(server_router(server_state(vec!["mock"])), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_without_backend_uses_default() {
    let body = serde_json::json!({"task": "hello"});
    let resp = send_post_json(server_router(server_state(vec!["mock"])), "/run", &body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn run_rejects_malformed_json() {
    let router = server_router(server_state(vec!["mock"]));
    let resp = router
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/run")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from("{invalid json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_rejects_missing_content_type() {
    let router = server_router(server_state(vec!["mock"]));
    let resp = router
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

// ===========================================================================
// Section 4: server::router — Version endpoint
// ===========================================================================

#[tokio::test]
async fn version_returns_200() {
    let resp = send_get(server_router(server_state(vec![])), "/version").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn version_includes_contract_version() {
    let resp = send_get(server_router(server_state(vec![])), "/version").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let ver: VersionResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(ver.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn version_includes_crate_version() {
    let resp = send_get(server_router(server_state(vec![])), "/version").await;
    let json = body_json(resp).await;
    assert!(json["version"].is_string());
    assert!(!json["version"].as_str().unwrap().is_empty());
}

// ===========================================================================
// Section 5: server::router — Routing errors
// ===========================================================================

#[tokio::test]
async fn unknown_route_returns_404() {
    let resp = send_get(server_router(server_state(vec![])), "/does_not_exist").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_to_health_returns_405() {
    let body = serde_json::json!({});
    let resp = send_post_json(server_router(server_state(vec![])), "/health", &body).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn get_to_run_returns_405() {
    let resp = send_get(server_router(server_state(vec![])), "/run").await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn post_to_backends_returns_405() {
    let body = serde_json::json!({});
    let resp = send_post_json(server_router(server_state(vec![])), "/backends", &body).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn post_to_version_returns_405() {
    let body = serde_json::json!({});
    let resp = send_post_json(server_router(server_state(vec![])), "/version", &body).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// ===========================================================================
// Section 6: build_app — Health (GET /health)
// ===========================================================================

#[tokio::test]
async fn full_health_returns_status_ok_and_version() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let resp = send_get(app, "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn full_health_includes_time_field() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/health").await).await;
    assert!(json.get("time").is_some());
}

// ===========================================================================
// Section 7: build_app — Backends (GET /backends)
// ===========================================================================

#[tokio::test]
async fn full_backends_contains_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/backends").await).await;
    let arr = json.as_array().unwrap();
    assert!(arr.iter().any(|v| v.as_str() == Some("mock")));
}

// ===========================================================================
// Section 8: build_app — Run endpoint (POST /run) with MockBackend
// ===========================================================================

#[tokio::test]
async fn full_run_with_mock_backend_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let app = build_app(state);
    let resp = send_post_json(app, "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn full_run_response_contains_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let app = build_app(state);
    let json = body_json(send_post_json(app, "/run", &req).await).await;
    assert!(json.get("receipt").is_some());
}

#[tokio::test]
async fn full_run_response_contains_events() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let app = build_app(state);
    let json = body_json(send_post_json(app, "/run", &req).await).await;
    assert!(json["events"].is_array());
}

#[tokio::test]
async fn full_run_response_has_run_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let app = build_app(state);
    let json = body_json(send_post_json(app, "/run", &req).await).await;
    let run_id = json["run_id"].as_str().unwrap();
    assert!(run_id.parse::<Uuid>().is_ok());
}

#[tokio::test]
async fn full_run_response_has_backend_field() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let app = build_app(state);
    let json = body_json(send_post_json(app, "/run", &req).await).await;
    assert_eq!(json["backend"], "mock");
}

#[tokio::test]
async fn full_run_rejects_unknown_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let mut wo = test_work_order();
    wo.task = "test unknown backend".into();
    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: wo,
    };
    let app = build_app(state);
    let resp = send_post_json(app, "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn full_run_stores_receipt_in_state() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let wo = test_work_order();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let app = build_app(state.clone());
    let json = body_json(send_post_json(app, "/run", &req).await).await;
    let run_id: Uuid = json["run_id"].as_str().unwrap().parse().unwrap();
    let receipts = state.receipts.read().await;
    assert!(receipts.contains_key(&run_id));
}

// ===========================================================================
// Section 9: build_app — Status endpoint (GET /status)
// ===========================================================================

#[tokio::test]
async fn full_status_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let resp = send_get(app, "/status").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn full_status_has_status_field() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/status").await).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn full_status_has_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/status").await).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn full_status_has_backends_array() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/status").await).await;
    assert!(json["backends"].is_array());
}

#[tokio::test]
async fn full_status_has_active_runs_array() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/status").await).await;
    assert!(json["active_runs"].is_array());
}

// ===========================================================================
// Section 10: build_app — Run status by ID (GET /runs/:id)
// ===========================================================================

#[tokio::test]
async fn get_run_by_id_after_submit() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());

    let wo = test_work_order();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let app = build_app(state.clone());
    let json = body_json(send_post_json(app, "/run", &req).await).await;
    let run_id = json["run_id"].as_str().unwrap();

    let app2 = build_app(state);
    let resp = send_get(app2, &format!("/runs/{run_id}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_run_by_unknown_id_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let fake_id = Uuid::new_v4();
    let resp = send_get(app, &format!("/runs/{fake_id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// Section 11: build_app — Events SSE endpoint (GET /runs/:id/events)
// ===========================================================================

#[tokio::test]
async fn events_endpoint_returns_sse_content_type() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let fake_id = Uuid::new_v4();
    let resp = send_get(app, &format!("/runs/{fake_id}/events")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/event-stream"));
}

// ===========================================================================
// Section 12: build_app — Metrics endpoint (GET /metrics)
// ===========================================================================

#[tokio::test]
async fn metrics_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let resp = send_get(app, "/metrics").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_has_expected_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/metrics").await).await;
    assert!(json.get("total_runs").is_some());
    assert!(json.get("running").is_some());
    assert!(json.get("completed").is_some());
    assert!(json.get("failed").is_some());
}

#[tokio::test]
async fn metrics_initially_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/metrics").await).await;
    assert_eq!(json["total_runs"], 0);
    assert_eq!(json["running"], 0);
    assert_eq!(json["completed"], 0);
    assert_eq!(json["failed"], 0);
}

// ===========================================================================
// Section 13: build_app — Versioned API (build_versioned_app)
// ===========================================================================

#[tokio::test]
async fn v1_health_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(app_state(tmp.path()));
    let resp = send_get(app, "/api/v1/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn v1_backends_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_versioned_app(app_state(tmp.path()));
    let resp = send_get(app, "/api/v1/backends").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// Section 14: build_app — Validate endpoint (POST /validate)
// ===========================================================================

#[tokio::test]
async fn validate_accepts_valid_request() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let app = build_app(state);
    let resp = send_post_json(app, "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn validate_rejects_empty_task() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let app = build_app(state);
    let resp = send_post_json(app, "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Section 15: build_app — Schema endpoint (GET /schema/:type)
// ===========================================================================

#[tokio::test]
async fn schema_work_order_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let resp = send_get(app, "/schema/work_order").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_receipt_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let resp = send_get(app, "/schema/receipt").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn schema_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let resp = send_get(app, "/schema/nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// Section 16: build_app — Receipts endpoints
// ===========================================================================

#[tokio::test]
async fn receipts_list_initially_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/receipts").await).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn receipt_get_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let fake_id = Uuid::new_v4();
    let resp = send_get(app, &format!("/receipts/{fake_id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// Section 17: build_app — Config endpoint (GET /config)
// ===========================================================================

#[tokio::test]
async fn config_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let resp = send_get(app, "/config").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_includes_contract_version() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(app_state(tmp.path()));
    let json = body_json(send_get(app, "/config").await).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

// ===========================================================================
// Section 18: State layer — RunRegistry integration
// ===========================================================================

#[tokio::test]
async fn registry_full_lifecycle() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Queued);

    reg.transition(id, RunPhase::Running).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Running);

    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    reg.complete(id, receipt).await.unwrap();
    assert_eq!(reg.get(id).await.unwrap().phase, RunPhase::Completed);
}

#[tokio::test]
async fn registry_fail_lifecycle() {
    let reg = RunRegistry::new();
    let id = Uuid::new_v4();
    reg.create_run(id, "mock".into()).await.unwrap();
    reg.transition(id, RunPhase::Running).await.unwrap();
    reg.fail(id, "timeout".into()).await.unwrap();
    let record = reg.get(id).await.unwrap();
    assert_eq!(record.phase, RunPhase::Failed);
    assert_eq!(record.error.as_deref(), Some("timeout"));
}

// ===========================================================================
// Section 19: State layer — BackendList integration
// ===========================================================================

#[tokio::test]
async fn backend_list_from_names() {
    let bl = BackendList::from_names(vec!["a".into(), "b".into()]);
    assert_eq!(bl.len().await, 2);
    assert!(bl.contains("a").await);
    assert!(bl.contains("b").await);
    assert!(!bl.contains("c").await);
}

// ===========================================================================
// Section 20: Concurrent run submissions
// ===========================================================================

#[tokio::test]
async fn concurrent_run_submissions() {
    let tmp = tempfile::tempdir().unwrap();
    let state = app_state(tmp.path());
    let mut handles = vec![];

    for _ in 0..5 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let req = RunRequest {
                backend: "mock".into(),
                work_order: WorkOrder {
                    id: Uuid::new_v4(),
                    task: "concurrent task".into(),
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
                },
            };
            let app = build_app(s);
            let resp = send_post_json(app, "/run", &req).await;
            resp.status()
        }));
    }

    for h in handles {
        let status = h.await.unwrap();
        assert_eq!(status, StatusCode::OK);
    }
}
