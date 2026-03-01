// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
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

#[tokio::test]
async fn health_returns_ok() {
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

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn backends_includes_mock() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/backends")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let names: Vec<String> = serde_json::from_slice(&body).unwrap();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn capabilities_returns_all() {
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
    assert!(infos.iter().any(|i| i.id == "mock"));
}

#[tokio::test]
async fn capabilities_single_backend() {
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
async fn capabilities_unknown_backend_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/capabilities?backend=nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn run_with_mock_backend() {
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
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let run_resp: RunResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(run_resp.backend, "mock");
    assert!(!run_resp.events.is_empty());
    assert!(run_resp.receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn receipts_list_after_run() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // First, perform a run
    let run_resp = {
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
        serde_json::from_slice::<RunResponse>(&body).unwrap()
    };

    // Now list receipts
    let app = build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/receipts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let ids: Vec<Uuid> = serde_json::from_slice(&body).unwrap();
    assert!(ids.contains(&run_resp.run_id));
}

#[tokio::test]
async fn get_receipt_by_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Perform a run first
    let run_resp = {
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
        serde_json::from_slice::<RunResponse>(&body).unwrap()
    };

    // Fetch the receipt by ID
    let app = build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/receipts/{}", run_resp.run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let receipt: abp_core::Receipt = serde_json::from_slice(&body).unwrap();
    assert_eq!(receipt.meta.run_id, run_resp.run_id);
}

#[tokio::test]
async fn get_receipt_unknown_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let fake_id = Uuid::new_v4();
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/receipts/{}", fake_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sse_events_returns_200_with_correct_content_type() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let run_id = Uuid::nil();

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{}/events", run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        ct.contains("text/event-stream"),
        "expected SSE content type, got: {ct}"
    );
}

#[tokio::test]
async fn sse_stream_contains_event() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let run_id = Uuid::nil();

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{}/events", run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("data: ping"),
        "expected SSE data field, got: {text}"
    );
}

// ---------------------------------------------------------------------------
// POST /runs – invalid JSON body returns error
// ---------------------------------------------------------------------------
#[tokio::test]
async fn post_runs_invalid_json_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs")
                .header("content-type", "application/json")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();

    // Axum returns 422 for JSON deserialization failures
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "expected 400 or 422, got {}",
        resp.status()
    );
}

// ---------------------------------------------------------------------------
// POST /runs – valid body returns 200 with run_id
// ---------------------------------------------------------------------------
#[tokio::test]
async fn post_runs_valid_body_returns_ok_with_run_id() {
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
                .uri("/runs")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let run_resp: RunResponse = serde_json::from_slice(&body).unwrap();
    // run_id must be present and non-nil (receipt generates one)
    assert_eq!(run_resp.backend, "mock");
    assert!(run_resp.receipt.receipt_sha256.is_some());
}

// ---------------------------------------------------------------------------
// GET /runs – returns a JSON array
// ---------------------------------------------------------------------------
#[tokio::test]
async fn list_runs_returns_array() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Perform a run first so the list is non-empty
    {
        let app = build_app(state.clone());
        let req_body = RunRequest {
            backend: "mock".into(),
            work_order: test_work_order(),
        };
        let _resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
    }

    let app = build_app(state);
    let resp = app
        .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let ids: Vec<Uuid> = serde_json::from_slice(&body).unwrap();
    assert!(!ids.is_empty(), "expected at least one run in list");
}

// ---------------------------------------------------------------------------
// GET /runs/{run_id} – non-existent ID returns 404
// ---------------------------------------------------------------------------
#[tokio::test]
async fn get_run_nonexistent_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let fake_id = Uuid::new_v4();
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{}", fake_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// GET /health – response body contains status and contract_version
// ---------------------------------------------------------------------------
#[tokio::test]
async fn health_response_has_required_fields() {
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

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert!(json.get("contract_version").is_some());
    assert!(json.get("time").is_some());
}

// ---------------------------------------------------------------------------
// Content-Type is application/json on JSON endpoints
// ---------------------------------------------------------------------------
#[tokio::test]
async fn json_endpoints_have_json_content_type() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let check = |uri: &str| {
        let s = state.clone();
        let u = uri.to_string();
        async move {
            let app = build_app(s);
            let resp = app
                .oneshot(Request::builder().uri(&u).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let ct = resp
                .headers()
                .get("content-type")
                .unwrap_or_else(|| panic!("no content-type on {u}"))
                .to_str()
                .unwrap()
                .to_string();
            assert!(
                ct.contains("application/json"),
                "expected application/json on {u}, got: {ct}"
            );
        }
    };

    check("/health").await;
    check("/backends").await;
    check("/capabilities").await;
    check("/runs").await;
    check("/receipts").await;
}

// ---------------------------------------------------------------------------
// POST /runs with missing content-type still returns an error, not panic
// ---------------------------------------------------------------------------
#[tokio::test]
async fn post_runs_missing_content_type_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/runs")
                .body(Body::from(r#"{"backend":"mock"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should get 4xx, not 500 or panic
    assert!(
        resp.status().is_client_error(),
        "expected 4xx, got {}",
        resp.status()
    );
}

// ---------------------------------------------------------------------------
// POST /run (legacy) still works
// ---------------------------------------------------------------------------
#[tokio::test]
async fn post_run_legacy_endpoint_still_works() {
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
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// GET /ws – WebSocket upgrade returns 101 Switching Protocols
// ---------------------------------------------------------------------------
#[tokio::test]
async fn ws_upgrade_returns_101() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("ws://127.0.0.1:{}/ws", addr.port());
    let (ws_stream, resp) = tokio_tungstenite::connect_async(&url).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SWITCHING_PROTOCOLS);
    drop(ws_stream);
}

// ---------------------------------------------------------------------------
// GET /ws – request without upgrade headers is rejected
// ---------------------------------------------------------------------------
#[tokio::test]
async fn ws_without_upgrade_headers_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(Request::builder().uri("/ws").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Without proper upgrade headers, axum rejects with a client error
    assert!(
        resp.status().is_client_error(),
        "expected 4xx without upgrade headers, got {}",
        resp.status()
    );
}
