// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep stress and edge-case tests for the ABP daemon HTTP API.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
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

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "stress test task".into(),
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

/// Helper: POST /run and return the RunResponse.
async fn do_run(state: &Arc<AppState>) -> RunResponse {
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
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

/// Helper: extract JSON body from a response.
async fn body_json(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ---------------------------------------------------------------------------
// 1. Submit 10 work orders simultaneously, all complete successfully
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_runs_10_all_complete() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = state.clone();
        handles.push(tokio::spawn(async move { do_run(&s).await }));
    }

    let mut run_ids = Vec::new();
    for h in handles {
        let resp = h.await.unwrap();
        assert!(resp.receipt.receipt_sha256.is_some(), "receipt must be hashed");
        assert!(!resp.events.is_empty(), "mock backend must produce events");
        run_ids.push(resp.run_id);
    }

    // All 10 run IDs must be unique.
    let unique: std::collections::HashSet<_> = run_ids.iter().collect();
    assert_eq!(unique.len(), 10, "all 10 run IDs must be unique");

    // All receipts should be stored.
    let receipt_count = state.receipts.read().await.len();
    assert_eq!(receipt_count, 10, "all 10 receipts must be stored");
}

// ---------------------------------------------------------------------------
// 2. Hit /health 100 times rapidly — all must return 200 OK
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rapid_health_checks_100() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let mut handles = Vec::new();
    for _ in 0..100 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let app = build_app(s);
            app.oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        }));
    }

    for h in handles {
        let resp = h.await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

// ---------------------------------------------------------------------------
// 3. Submit work order with very large task string (~100 KB)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn large_payload_100kb_task() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // 100_000 chars is exactly at the MAX_TASK_LENGTH boundary.
    let large_task = "x".repeat(100_000);
    let mut wo = test_work_order();
    wo.task = large_task;
    let req_body = RunRequest {
        backend: "mock".into(),
        work_order: wo,
    };

    let app = build_app(state);
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
    let run_resp: RunResponse = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert!(run_resp.receipt.receipt_sha256.is_some());
}

// ---------------------------------------------------------------------------
// 4. POST /run with completely empty body → proper error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_body_post_run_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        resp.status().is_client_error(),
        "expected 4xx for empty body, got {}",
        resp.status()
    );
}

// ---------------------------------------------------------------------------
// 5. POST malformed JSON → proper error message
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalid_json_returns_error_with_message() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from("{invalid json!!!}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn truncated_json_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"backend":"mock","work_order":{"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn wrong_type_json_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    // Valid JSON but wrong shape (array instead of object).
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/run")
                .header("content-type", "application/json")
                .body(Body::from("[1, 2, 3]"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(resp.status().is_client_error());
}

// ---------------------------------------------------------------------------
// 6. Full run lifecycle: create → check status → get receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_run_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Step 1: Create a run.
    let run_resp = do_run(&state).await;
    let run_id = run_resp.run_id;

    // Step 2: Check status via GET /runs/{id}.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["run_id"], run_id.to_string());
    // Status should indicate completed — the format depends on whether the
    // run was tracked via RunTracker (tagged enum object) or the receipts
    // fallback (plain string).
    let status_val = &json["status"];
    let is_completed = status_val == "completed"
        || status_val.get("status").and_then(|s| s.as_str()) == Some("completed");
    assert!(is_completed, "expected completed status, got: {status_val}");

    // Step 3: Get receipt via /runs/{id}/receipt.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{run_id}/receipt"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let receipt: abp_core::Receipt = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
    assert!(receipt.receipt_sha256.is_some());

    // Step 4: Verify run appears in /runs list.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/runs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let ids: Vec<Uuid> = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert!(ids.contains(&run_id));

    // Step 5: Verify run appears in /receipts list.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/receipts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let ids: Vec<Uuid> = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert!(ids.contains(&run_id));
}

// ---------------------------------------------------------------------------
// 7. DELETE /runs/{random_uuid} → 404
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_many_nonexistent_runs_all_404() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    for _ in 0..10 {
        let app = build_app(state.clone());
        let fake_id = Uuid::new_v4();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/runs/{fake_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}

// ---------------------------------------------------------------------------
// 8. SSE stream: verify event-stream format
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sse_stream_format_verification() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));
    let run_id = Uuid::new_v4();

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
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("text/event-stream"));

    let body = String::from_utf8_lossy(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .to_string();
    // SSE format: each event has "data: " prefix.
    assert!(body.contains("data: "), "SSE body must contain data field: {body}");
    // Each event ends with double newline.
    assert!(body.contains("\n\n"), "SSE events separated by blank line");
}

// ---------------------------------------------------------------------------
// 9. WebSocket: connect, send message, verify echo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn websocket_echo_roundtrip() {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite;

    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("ws://127.0.0.1:{}/ws", addr.port());
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

    // Send a text message.
    let msg = "hello backplane";
    ws.send(tungstenite::Message::Text(msg.into())).await.unwrap();

    // Receive the echo.
    let echoed = ws.next().await.unwrap().unwrap();
    match echoed {
        tungstenite::Message::Text(t) => assert_eq!(t.as_str(), msg),
        other => panic!("expected Text echo, got {other:?}"),
    }

    // Send close.
    ws.send(tungstenite::Message::Close(None)).await.unwrap();
}

// ---------------------------------------------------------------------------
// 10. Metrics endpoint: verify structure and values
// ---------------------------------------------------------------------------

#[tokio::test]
async fn metrics_structure_after_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // No runs yet → all zeros.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let m: RunMetrics = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert_eq!(m.total_runs, 0);
    assert_eq!(m.completed, 0);
    assert_eq!(m.failed, 0);

    // Do 3 runs.
    for _ in 0..3 {
        do_run(&state).await;
    }

    let app = build_app(state.clone());
    let resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let m: RunMetrics = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    // Receipts map is the source of truth for completed count.
    assert!(m.completed >= 3, "expected ≥3 completed, got {}", m.completed);
    assert!(m.total_runs >= 3, "expected ≥3 total, got {}", m.total_runs);
}

// ---------------------------------------------------------------------------
// 11. Config endpoint: verify structure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn config_structure_deep_check() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let app = build_app(state);
    let resp = app
        .oneshot(Request::builder().uri("/config").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_json(resp).await;
    // Must have backends array.
    let backends = json["backends"].as_array().expect("backends must be array");
    assert!(backends.iter().any(|b| b == "mock"));
    // Contract version.
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
    // Receipts dir.
    assert!(json.get("receipts_dir").is_some());
    assert!(json["receipts_dir"].is_string());
}

// ---------------------------------------------------------------------------
// 12. Schema endpoint: verify valid JSON Schema document
// ---------------------------------------------------------------------------

#[tokio::test]
async fn schema_work_order_is_valid_json_schema() {
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
    assert_eq!(resp.status(), StatusCode::OK);

    let schema = body_json(resp).await;
    // Must have title or $schema or definitions — hallmarks of JSON Schema.
    assert!(
        schema.get("title").is_some()
            || schema.get("$schema").is_some()
            || schema.get("definitions").is_some()
            || schema.get("$defs").is_some(),
        "not a JSON Schema document: {schema}"
    );
    // The schema type should reference WorkOrder.
    let schema_str = serde_json::to_string(&schema).unwrap();
    assert!(
        schema_str.contains("WorkOrder") || schema_str.contains("work_order"),
        "schema should reference WorkOrder type"
    );
}

// ---------------------------------------------------------------------------
// 13. Backends listing: verify format is array of strings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn backends_listing_format() {
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

    let json = body_json(resp).await;
    let arr = json.as_array().expect("backends must be array");
    assert!(!arr.is_empty(), "at least one backend must be registered");
    for item in arr {
        assert!(item.is_string(), "each backend must be a string, got: {item}");
    }
    // Must contain "mock".
    let names: Vec<&str> = arr.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains(&"mock"));
}

// ---------------------------------------------------------------------------
// 14. Validate endpoint: valid vs. invalid work orders
// ---------------------------------------------------------------------------

#[tokio::test]
async fn validate_valid_work_order_accepted() {
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
    let json = body_json(resp).await;
    assert_eq!(json["valid"], true);
}

#[tokio::test]
async fn validate_empty_workspace_root_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.workspace.root = String::new();
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
}

#[tokio::test]
async fn validate_whitespace_only_task_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = "   \t\n  ".into();
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
}

#[tokio::test]
async fn validate_task_exceeding_max_length_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let mut wo = test_work_order();
    wo.task = "x".repeat(100_001); // 1 byte over MAX_TASK_LENGTH
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
}

// ---------------------------------------------------------------------------
// 15. Create 5 runs, delete all, verify all gone
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_5_runs_delete_all_verify_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Create 5 runs.
    let mut run_ids = Vec::new();
    for _ in 0..5 {
        let resp = do_run(&state).await;
        run_ids.push(resp.run_id);
    }
    assert_eq!(run_ids.len(), 5);

    // Delete each run.
    for run_id in &run_ids {
        let app = build_app(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/runs/{run_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "failed to delete {run_id}");
    }

    // Verify each run returns 404.
    for run_id in &run_ids {
        let app = build_app(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "run {run_id} should be gone after delete"
        );
    }
}

// ---------------------------------------------------------------------------
// 16. Double-delete same run returns 404 on second attempt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn double_delete_returns_404_on_second() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let run_resp = do_run(&state).await;
    let run_id = run_resp.run_id;

    // First delete succeeds.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Second delete returns 404.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{run_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 17. Receipts ?limit=N pagination parameter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn receipts_limit_parameter() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Create 5 runs.
    for _ in 0..5 {
        do_run(&state).await;
    }

    // Limit to 2.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/receipts?limit=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ids: Vec<Uuid> = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert_eq!(ids.len(), 2, "limit=2 should return exactly 2 receipts");

    // Limit to 0 returns empty.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/receipts?limit=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let ids: Vec<Uuid> = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert!(ids.is_empty(), "limit=0 should return empty list");
}

// ---------------------------------------------------------------------------
// 18. Concurrent health + metrics + config reads under load
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_mixed_reads_under_load() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Seed some runs.
    for _ in 0..3 {
        do_run(&state).await;
    }

    let endpoints = ["/health", "/metrics", "/config", "/backends", "/runs", "/receipts"];
    let mut handles = Vec::new();
    for _ in 0..5 {
        for ep in &endpoints {
            let s = state.clone();
            let uri = ep.to_string();
            handles.push(tokio::spawn(async move {
                let app = build_app(s);
                let resp = app
                    .oneshot(
                        Request::builder()
                            .uri(&uri)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "endpoint {uri} failed under load"
                );
            }));
        }
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// 19. Metrics counter increments correctly with mixed runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn metrics_track_running_state() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Manually insert a "running" entry into the tracker.
    let running_id = Uuid::new_v4();
    state.run_tracker.start_run(running_id).await.unwrap();

    // Also complete one run through the API.
    do_run(&state).await;

    let app = build_app(state.clone());
    let resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let m: RunMetrics = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();

    assert!(m.running >= 1, "should have at least 1 running: got {}", m.running);
    assert!(m.completed >= 1, "should have at least 1 completed: got {}", m.completed);
    assert!(m.total_runs >= 2, "total should be >= 2: got {}", m.total_runs);
}

// ---------------------------------------------------------------------------
// 20. Schema receipt has expected JSON Schema structure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn schema_receipt_deep_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/schema/receipt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let schema = body_json(resp).await;
    let schema_str = serde_json::to_string(&schema).unwrap();
    assert!(
        schema_str.contains("Receipt") || schema_str.contains("receipt"),
        "schema should reference Receipt type"
    );
    // Must be a valid JSON object.
    assert!(schema.is_object());
}
