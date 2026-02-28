// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended API tests for the new daemon endpoints.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile,
    RuntimeConfig, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::{AppState, RunRequest, RunResponse, RunTracker, build_app};
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
        task: "extended test task".into(),
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

/// Helper: perform a run via POST /run and return the RunResponse.
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

// ---------------------------------------------------------------------------
// 1. DELETE completed run returns 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_completed_run_returns_200() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let run_resp = do_run(&state).await;

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{}", run_resp.run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["deleted"], run_resp.run_id.to_string());
}

// ---------------------------------------------------------------------------
// 2. DELETE running run returns 409 conflict
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_running_run_returns_409() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Manually insert a running run into the tracker.
    let run_id = Uuid::new_v4();
    state.run_tracker.start_run(run_id).await.unwrap();

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{}", run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

// ---------------------------------------------------------------------------
// 3. DELETE nonexistent run returns 404
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_nonexistent_run_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let fake_id = Uuid::new_v4();
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{}", fake_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 4. GET receipt for completed run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_run_receipt_for_completed_run() {
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
    let receipt: abp_core::Receipt = serde_json::from_slice(&body).unwrap();
    assert_eq!(receipt.meta.run_id, run_resp.run_id);
}

// ---------------------------------------------------------------------------
// 5. GET receipt for running run returns 404
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_run_receipt_for_running_run_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let run_id = Uuid::new_v4();
    state.run_tracker.start_run(run_id).await.unwrap();

    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{}/receipt", run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 6. GET config returns valid JSON
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_config_returns_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("backends").is_some());
    assert!(json.get("contract_version").is_some());
    assert_eq!(json["contract_version"], abp_core::CONTRACT_VERSION);
}

// ---------------------------------------------------------------------------
// 7. POST validate with valid work order returns 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn validate_valid_work_order_returns_200() {
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
    assert_eq!(json["backend"], "mock");
}

// ---------------------------------------------------------------------------
// 8. POST validate with invalid work order returns 400
// ---------------------------------------------------------------------------

#[tokio::test]
async fn validate_unknown_backend_returns_400() {
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
}

// ---------------------------------------------------------------------------
// 9. GET schema/work_order returns valid JSON schema
// ---------------------------------------------------------------------------

#[tokio::test]
async fn schema_work_order_returns_valid_json() {
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

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // JSON Schema documents should have a $schema or type key
    assert!(
        json.get("$schema").is_some() || json.get("type").is_some(),
        "expected a JSON schema document, got: {json}"
    );
}

// ---------------------------------------------------------------------------
// 10. GET schema/receipt returns valid JSON schema
// ---------------------------------------------------------------------------

#[tokio::test]
async fn schema_receipt_returns_valid_json() {
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

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json.get("$schema").is_some() || json.get("type").is_some(),
        "expected a JSON schema document, got: {json}"
    );
}

// ---------------------------------------------------------------------------
// 11. Concurrent API requests don't corrupt state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_requests_dont_corrupt_state() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    // Spawn 10 concurrent runs.
    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            do_run(&s).await
        }));
    }

    let mut run_ids = Vec::new();
    for h in handles {
        let resp = h.await.unwrap();
        run_ids.push(resp.run_id);
    }

    // All run_ids should be unique.
    let unique: std::collections::HashSet<_> = run_ids.iter().collect();
    assert_eq!(unique.len(), run_ids.len(), "expected all run_ids to be unique");

    // List runs should contain all of them.
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
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let ids: Vec<Uuid> = serde_json::from_slice(&body).unwrap();
    for run_id in &run_ids {
        assert!(ids.contains(run_id), "missing run_id {run_id} in /runs list");
    }
}

// ---------------------------------------------------------------------------
// 12. API responses have correct content-type headers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_endpoints_have_json_content_type() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let check = |uri: &str| {
        let s = state.clone();
        let u = uri.to_string();
        async move {
            let app = build_app(s);
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri(&u)
                        .body(Body::empty())
                        .unwrap(),
                )
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

    check("/config").await;
    check("/schema/work_order").await;
    check("/schema/receipt").await;
}

// ---------------------------------------------------------------------------
// Bonus: GET schema for unknown type returns 404
// ---------------------------------------------------------------------------

#[tokio::test]
async fn schema_unknown_type_returns_404() {
    let tmp = tempfile::tempdir().unwrap();
    let app = build_app(test_state(tmp.path()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/schema/foobar")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Bonus: DELETE removes run from /runs list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_run_removes_from_list() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());

    let run_resp = do_run(&state).await;
    let run_id = run_resp.run_id;

    // Delete the run.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/runs/{}", run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // GET on the deleted run should return 404.
    let app = build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/runs/{}", run_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Bonus: validate with empty task returns 400
// ---------------------------------------------------------------------------

#[tokio::test]
async fn validate_empty_task_returns_400() {
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
}
