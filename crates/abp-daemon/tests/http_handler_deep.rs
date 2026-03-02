// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep HTTP handler tests for the ABP daemon.
//!
//! Covers: health, backends, run lifecycle, error responses, concurrent
//! requests, timeout behavior, HTTP status codes, CORS, content-type
//! negotiation, and large payloads.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, Receipt, RuntimeConfig,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::{AppState, RunMetrics, RunRequest, RunResponse, RunTracker, build_app};
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
        task: "deep handler test task".into(),
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

/// State with no backends registered at all.
fn empty_state(receipts_dir: &std::path::Path) -> Arc<AppState> {
    let runtime = Runtime::new();
    Arc::new(AppState {
        runtime: Arc::new(runtime),
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: receipts_dir.to_path_buf(),
        run_tracker: RunTracker::new(),
    })
}

async fn body_json(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

async fn get(app: axum::Router, uri: &str) -> axum::http::Response<Body> {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn post_raw(
    app: axum::Router,
    uri: &str,
    content_type: &str,
    body: Body,
) -> axum::http::Response<Body> {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", content_type)
            .body(body)
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn post_json_body(
    app: axum::Router,
    uri: &str,
    body: &impl serde::Serialize,
) -> axum::http::Response<Body> {
    post_raw(
        app,
        uri,
        "application/json",
        Body::from(serde_json::to_vec(body).unwrap()),
    )
    .await
}

async fn do_run(state: &Arc<AppState>) -> RunResponse {
    let app = build_app(state.clone());
    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = post_json_body(app, "/run", &req_body).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ===========================================================================
// 1. Health endpoint
// ===========================================================================

#[tokio::test]
async fn health_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_response_is_json_object() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/health").await).await;
    assert!(json.is_object());
}

#[tokio::test]
async fn health_status_field_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/health").await).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_contract_version_matches_core() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/health").await).await;
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn health_time_is_rfc3339() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/health").await).await;
    let t = json["time"].as_str().unwrap();
    chrono::DateTime::parse_from_rfc3339(t).expect("time must be RFC 3339");
}

#[tokio::test]
async fn health_has_exactly_three_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/health").await).await;
    assert_eq!(
        json.as_object().unwrap().len(),
        3,
        "expected status, contract_version, time"
    );
}

#[tokio::test]
async fn health_with_empty_state() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(empty_state(tmp.path())), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
}

// ===========================================================================
// 2. Backends endpoint returns JSON array
// ===========================================================================

#[tokio::test]
async fn backends_returns_json_array_of_strings() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/backends").await).await;
    let arr = json.as_array().unwrap();
    for v in arr {
        assert!(v.is_string(), "each backend entry must be a string");
    }
}

#[tokio::test]
async fn backends_contains_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/backends").await).await;
    let names: Vec<String> = serde_json::from_value(json).unwrap();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn backends_empty_runtime_returns_empty_array() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(empty_state(tmp.path())), "/backends").await).await;
    let arr = json.as_array().unwrap();
    assert!(arr.is_empty());
}

// ===========================================================================
// 3. Run work order – valid JSON, returns results
// ===========================================================================

#[tokio::test]
async fn run_valid_returns_200_with_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let run_resp = do_run(&state).await;
    assert!(run_resp.receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn run_response_has_all_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state.clone());
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let json = body_json(post_json_body(app, "/run", &req).await).await;
    assert!(json.get("run_id").is_some());
    assert!(json.get("backend").is_some());
    assert!(json.get("events").is_some());
    assert!(json.get("receipt").is_some());
}

#[tokio::test]
async fn run_events_array_is_nonempty() {
    let tmp = tempfile::tempdir().unwrap();
    let r = do_run(&test_state(tmp.path())).await;
    assert!(!r.events.is_empty());
}

#[tokio::test]
async fn run_backend_echoed_in_response() {
    let tmp = tempfile::tempdir().unwrap();
    let r = do_run(&test_state(tmp.path())).await;
    assert_eq!(r.backend, "mock");
}

#[tokio::test]
async fn run_receipt_run_id_matches_response() {
    let tmp = tempfile::tempdir().unwrap();
    let r = do_run(&test_state(tmp.path())).await;
    assert_eq!(r.receipt.meta.run_id, r.run_id);
}

#[tokio::test]
async fn run_persists_receipt_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    let path = tmp.path().join(format!("{}.json", r.run_id));
    assert!(path.exists());
    let receipt: Receipt = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(receipt.meta.run_id, r.run_id);
}

#[tokio::test]
async fn run_tracked_in_tracker() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    // The run should be trackable via the runs list (tracker + receipts).
    let app = build_app(state.clone());
    let json = body_json(get(app, "/runs").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&r.run_id), "run should appear in /runs list");
}

#[tokio::test]
async fn run_appears_in_receipts_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    assert!(state.receipts.read().await.contains_key(&r.run_id));
}

// ===========================================================================
// 4. Error responses – bad JSON, missing fields, wrong content types
// ===========================================================================

#[tokio::test]
async fn run_invalid_json_returns_422_or_400() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = post_raw(
        build_app(test_state(tmp.path())),
        "/run",
        "application/json",
        Body::from("not json at all"),
    )
    .await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_empty_body_returns_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = post_raw(
        build_app(test_state(tmp.path())),
        "/run",
        "application/json",
        Body::empty(),
    )
    .await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_array_body_returns_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = post_raw(
        build_app(test_state(tmp.path())),
        "/run",
        "application/json",
        Body::from("[1,2,3]"),
    )
    .await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_truncated_json_returns_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = post_raw(
        build_app(test_state(tmp.path())),
        "/run",
        "application/json",
        Body::from(r#"{"backend":"mock","work_order":{"#),
    )
    .await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_missing_backend_field_returns_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = post_raw(
        build_app(test_state(tmp.path())),
        "/run",
        "application/json",
        Body::from(r#"{"work_order":{"id":"00000000-0000-0000-0000-000000000000","task":"hi","lane":"patch_first","workspace":{"root":".","mode":"pass_through","include":[],"exclude":[]},"context":{},"policy":{},"requirements":{},"config":{}}}"#),
    )
    .await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_missing_work_order_returns_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = post_raw(
        build_app(test_state(tmp.path())),
        "/run",
        "application/json",
        Body::from(r#"{"backend":"mock"}"#),
    )
    .await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn run_empty_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let mut wo = test_work_order();
    wo.task = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_whitespace_task_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let mut wo = test_work_order();
    wo.task = "  \t\n ".into();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_unknown_backend_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_error_response_has_error_field() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "nonexistent".into(),
        work_order: test_work_order(),
    };
    let json =
        body_json(post_json_body(build_app(test_state(tmp.path())), "/run", &req).await).await;
    assert!(
        json.get("error").is_some(),
        "error responses must have 'error' field"
    );
}

#[tokio::test]
async fn run_empty_workspace_root_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let mut wo = test_work_order();
    wo.workspace.root = String::new();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn validate_invalid_json_returns_client_error() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = post_raw(
        build_app(test_state(tmp.path())),
        "/validate",
        "application/json",
        Body::from("garbage"),
    )
    .await;
    assert!(resp.status().is_client_error());
}

// ===========================================================================
// 5. Concurrent request handling
// ===========================================================================

#[tokio::test]
async fn concurrent_20_runs_all_unique() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let mut handles = Vec::new();
    for _ in 0..20 {
        let s = state.clone();
        handles.push(tokio::spawn(async move { do_run(&s).await }));
    }
    let mut ids = Vec::new();
    for h in handles {
        ids.push(h.await.unwrap().run_id);
    }
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 20);
}

#[tokio::test]
async fn concurrent_health_and_run_interleaved() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let mut handles = Vec::new();
    for i in 0..20 {
        let s = state.clone();
        if i % 2 == 0 {
            handles.push(tokio::spawn(async move {
                let resp = get(build_app(s), "/health").await;
                assert_eq!(resp.status(), StatusCode::OK);
            }));
        } else {
            handles.push(tokio::spawn(async move {
                do_run(&s).await;
            }));
        }
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn concurrent_reads_on_multiple_endpoints() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    // Seed a run first.
    do_run(&state).await;

    let uris = vec![
        "/health",
        "/metrics",
        "/config",
        "/backends",
        "/runs",
        "/receipts",
    ];
    let mut handles = Vec::new();
    for uri in &uris {
        for _ in 0..5 {
            let s = state.clone();
            let u = uri.to_string();
            handles.push(tokio::spawn(async move {
                let resp = get(build_app(s), &u).await;
                assert_eq!(resp.status(), StatusCode::OK, "failed on {u}");
            }));
        }
    }
    for h in handles {
        h.await.unwrap();
    }
}

// ===========================================================================
// 6. Request timeout behavior (verify fast response times)
// ===========================================================================

#[tokio::test]
async fn health_responds_under_500ms() {
    let tmp = tempfile::tempdir().unwrap();
    let start = std::time::Instant::now();
    let resp = get(build_app(test_state(tmp.path())), "/health").await;
    let elapsed = start.elapsed();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "health took {elapsed:?}"
    );
}

#[tokio::test]
async fn backends_responds_under_500ms() {
    let tmp = tempfile::tempdir().unwrap();
    let start = std::time::Instant::now();
    let resp = get(build_app(test_state(tmp.path())), "/backends").await;
    let elapsed = start.elapsed();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "backends took {elapsed:?}"
    );
}

#[tokio::test]
async fn metrics_responds_under_500ms() {
    let tmp = tempfile::tempdir().unwrap();
    let start = std::time::Instant::now();
    let resp = get(build_app(test_state(tmp.path())), "/metrics").await;
    let elapsed = start.elapsed();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "metrics took {elapsed:?}"
    );
}

// ===========================================================================
// 7. HTTP status codes – 400, 404, 405, 409, 500
// ===========================================================================

// 400 – bad request
#[tokio::test]
async fn status_400_for_empty_backend_name() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: String::new(),
        work_order: test_work_order(),
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// 404 – not found
#[tokio::test]
async fn status_404_for_unknown_route() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/nonexistent").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn status_404_for_missing_run() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = get(build_app(test_state(tmp.path())), &format!("/runs/{id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn status_404_for_missing_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = get(
        build_app(test_state(tmp.path())),
        &format!("/receipts/{id}"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn status_404_for_unknown_schema_type() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/schema/bogus").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn status_404_for_run_receipt_when_running() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();
    let resp = get(build_app(state), &format!("/runs/{id}/receipt")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// 405 – method not allowed
#[tokio::test]
async fn status_405_post_to_health() {
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

#[tokio::test]
async fn status_405_get_to_run() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/run").await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn status_405_delete_to_health() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn status_405_put_to_run() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/run")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// 409 – conflict
#[tokio::test]
async fn status_409_cancel_completed_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    let app = build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/runs/{}/cancel", r.run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn status_409_delete_running_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();
    let app = build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

// ===========================================================================
// 8. CORS headers (via middleware module types)
// ===========================================================================

#[tokio::test]
async fn cors_config_to_layer_does_not_panic() {
    use abp_daemon::middleware::CorsConfig;
    let config = CorsConfig {
        allowed_origins: vec!["http://localhost:3000".into()],
        allowed_methods: vec!["GET".into(), "POST".into()],
        allowed_headers: vec!["content-type".into()],
    };
    // Constructing the layer must not panic.
    let _layer = config.to_cors_layer();
}

#[tokio::test]
async fn cors_config_empty_origins() {
    use abp_daemon::middleware::CorsConfig;
    let config = CorsConfig {
        allowed_origins: vec![],
        allowed_methods: vec!["GET".into()],
        allowed_headers: vec![],
    };
    let _layer = config.to_cors_layer();
}

// ===========================================================================
// 9. Content-type negotiation
// ===========================================================================

#[tokio::test]
async fn health_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/health").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

#[tokio::test]
async fn backends_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/backends").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

#[tokio::test]
async fn metrics_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/metrics").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

#[tokio::test]
async fn config_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/config").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

#[tokio::test]
async fn schema_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/schema/work_order").await;
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

#[tokio::test]
async fn sse_events_content_type_is_event_stream() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = get(
        build_app(test_state(tmp.path())),
        &format!("/runs/{id}/events"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/event-stream"));
}

#[tokio::test]
async fn run_response_content_type_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

// ===========================================================================
// 10. Large payload handling
// ===========================================================================

#[tokio::test]
async fn large_task_at_boundary_100k_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let mut wo = test_work_order();
    wo.task = "x".repeat(100_000);
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn large_task_over_100k_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let mut wo = test_work_order();
    wo.task = "x".repeat(100_001);
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/run", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn large_task_over_100k_validate_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let mut wo = test_work_order();
    wo.task = "x".repeat(100_001);
    let req = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Additional: Run lifecycle & state
// ===========================================================================

#[tokio::test]
async fn list_runs_empty_initially() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/runs").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn list_runs_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    let json = body_json(get(build_app(state), "/runs").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&r.run_id));
}

#[tokio::test]
async fn get_run_completed_returns_status() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    let json = body_json(get(build_app(state), &format!("/runs/{}", r.run_id)).await).await;
    assert_eq!(json["run_id"], r.run_id.to_string());
}

#[tokio::test]
async fn cancel_running_run_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let id = Uuid::new_v4();
    state.run_tracker.start_run(id).await.unwrap();
    let app = build_app(state);
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
    let json = body_json(resp).await;
    assert_eq!(json["status"], "cancelled");
}

#[tokio::test]
async fn cancel_nonexistent_run_returns_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let app = build_app(test_state(tmp.path()));
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

#[tokio::test]
async fn delete_completed_run_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    let app = build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{}", r.run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["deleted"], r.run_id.to_string());
}

#[tokio::test]
async fn delete_then_get_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    // Delete
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{}", r.run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // GET should 404
    let resp = get(build_app(state), &format!("/runs/{}", r.run_id)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// Additional: Schema endpoints
// ===========================================================================

#[tokio::test]
async fn schema_all_known_types_return_200() {
    let tmp = tempfile::tempdir().unwrap();
    for schema_type in &[
        "work_order",
        "receipt",
        "capability_requirements",
        "backplane_config",
    ] {
        let resp = get(
            build_app(test_state(tmp.path())),
            &format!("/schema/{schema_type}"),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "schema/{schema_type} failed");
    }
}

#[tokio::test]
async fn schema_returns_json_object() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/schema/work_order").await).await;
    assert!(json.is_object());
}

// ===========================================================================
// Additional: Metrics
// ===========================================================================

#[tokio::test]
async fn metrics_zero_initially() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/metrics").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let m: RunMetrics = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(m.total_runs, 0);
    assert_eq!(m.running, 0);
    assert_eq!(m.completed, 0);
    assert_eq!(m.failed, 0);
}

#[tokio::test]
async fn metrics_completed_increments_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    do_run(&state).await;
    let resp = get(build_app(state), "/metrics").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let m: RunMetrics = serde_json::from_slice(&bytes).unwrap();
    assert!(m.completed >= 1);
    assert!(m.total_runs >= 1);
}

#[tokio::test]
async fn metrics_running_increments_for_in_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    state.run_tracker.start_run(Uuid::new_v4()).await.unwrap();
    let resp = get(build_app(state), "/metrics").await;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let m: RunMetrics = serde_json::from_slice(&bytes).unwrap();
    assert!(m.running >= 1);
}

// ===========================================================================
// Additional: Config endpoint
// ===========================================================================

#[tokio::test]
async fn config_contains_expected_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/config").await).await;
    assert!(json.get("backends").is_some());
    assert!(json.get("contract_version").is_some());
    assert!(json.get("receipts_dir").is_some());
}

#[tokio::test]
async fn config_backends_matches_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/config").await).await;
    let backends = json["backends"].as_array().unwrap();
    assert!(backends.iter().any(|b| b == "mock"));
}

// ===========================================================================
// Additional: Validate endpoint
// ===========================================================================

#[tokio::test]
async fn validate_valid_returns_true() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "mock".into(),
        work_order: test_work_order(),
    };
    let json =
        body_json(post_json_body(build_app(test_state(tmp.path())), "/validate", &req).await).await;
    assert_eq!(json["valid"], true);
    assert_eq!(json["backend"], "mock");
    assert!(json.get("work_order_id").is_some());
}

#[tokio::test]
async fn validate_unknown_backend_returns_400() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RunRequest {
        backend: "does_not_exist".into(),
        work_order: test_work_order(),
    };
    let resp = post_json_body(build_app(test_state(tmp.path())), "/validate", &req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===========================================================================
// Additional: Receipts endpoints
// ===========================================================================

#[tokio::test]
async fn receipts_empty_initially() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/receipts").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn receipts_populated_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    let json = body_json(get(build_app(state), "/receipts").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&r.run_id));
}

#[tokio::test]
async fn receipts_limit_zero_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    do_run(&state).await;
    let json = body_json(get(build_app(state), "/receipts?limit=0").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn receipts_limit_truncates() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    for _ in 0..5 {
        do_run(&state).await;
    }
    let json = body_json(get(build_app(state), "/receipts?limit=2").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert_eq!(ids.len(), 2);
}

#[tokio::test]
async fn get_receipt_by_id_returns_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r = do_run(&state).await;
    let resp = get(build_app(state), &format!("/receipts/{}", r.run_id)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let receipt: Receipt =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(receipt.meta.run_id, r.run_id);
}

// ===========================================================================
// Additional: Capabilities endpoint
// ===========================================================================

#[tokio::test]
async fn capabilities_returns_json_array() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(get(build_app(test_state(tmp.path())), "/capabilities").await).await;
    assert!(json.is_array());
}

#[tokio::test]
async fn capabilities_filter_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(
        build_app(test_state(tmp.path())),
        "/capabilities?backend=no_such",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn capabilities_filter_mock_returns_one() {
    let tmp = tempfile::tempdir().unwrap();
    let json = body_json(
        get(
            build_app(test_state(tmp.path())),
            "/capabilities?backend=mock",
        )
        .await,
    )
    .await;
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "mock");
}

// ===========================================================================
// Additional: SSE events endpoint
// ===========================================================================

#[tokio::test]
async fn sse_events_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = get(
        build_app(test_state(tmp.path())),
        &format!("/runs/{id}/events"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn sse_events_body_contains_data() {
    let tmp = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let resp = get(
        build_app(test_state(tmp.path())),
        &format!("/runs/{id}/events"),
    )
    .await;
    let text = body_string(resp).await;
    assert!(text.contains("data: "));
}

// ===========================================================================
// Additional: Invalid UUID path parameters
// ===========================================================================

#[tokio::test]
async fn invalid_uuid_in_runs_path_returns_4xx() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/runs/not-a-uuid").await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn invalid_uuid_in_receipts_path_returns_4xx() {
    let tmp = tempfile::tempdir().unwrap();
    let resp = get(build_app(test_state(tmp.path())), "/receipts/not-a-uuid").await;
    assert!(resp.status().is_client_error());
}

// ===========================================================================
// Additional: Idempotency & edge cases
// ===========================================================================

#[tokio::test]
async fn multiple_health_checks_return_same_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    for _ in 0..10 {
        let json = body_json(get(build_app(state.clone()), "/health").await).await;
        assert_eq!(json["status"], "ok");
        assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
    }
}

#[tokio::test]
async fn run_two_sequential_runs_both_tracked() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let r1 = do_run(&state).await;
    let r2 = do_run(&state).await;
    assert_ne!(r1.run_id, r2.run_id);
    let json = body_json(get(build_app(state), "/runs").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    assert!(ids.contains(&r1.run_id));
    assert!(ids.contains(&r2.run_id));
}

#[tokio::test]
async fn runs_list_is_sorted() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    for _ in 0..5 {
        do_run(&state).await;
    }
    let json = body_json(get(build_app(state), "/runs").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "/runs should return sorted UUIDs");
}

#[tokio::test]
async fn receipts_list_is_sorted() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    for _ in 0..5 {
        do_run(&state).await;
    }
    let json = body_json(get(build_app(state), "/receipts").await).await;
    let ids: Vec<Uuid> = serde_json::from_value(json).unwrap();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "/receipts should return sorted UUIDs");
}
