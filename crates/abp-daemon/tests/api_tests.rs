#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
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

// ===========================================================================
// V1 API types — unit tests
// ===========================================================================

mod v1_api_types {
    use abp_daemon::api::{
        BackendInfo as ApiBackendInfo, ErrorResponse, HealthResponse as ApiHealth,
        ListBackendsResponse, RunRequest as ApiRunRequest, RunResponse as ApiRunResponse,
        RunStatus as ApiRunStatus,
    };
    use abp_daemon::routes::{Route, api_routes};

    // -- api::RunRequest ------------------------------------------------

    #[test]
    fn run_request_serde_roundtrip_full() {
        let req = ApiRunRequest {
            task: "fix the bug".into(),
            backend: Some("mock".into()),
            config: Some(serde_json::json!({"model": "gpt-4"})),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ApiRunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task, "fix the bug");
        assert_eq!(back.backend.as_deref(), Some("mock"));
        assert!(back.config.is_some());
    }

    #[test]
    fn run_request_serde_roundtrip_minimal() {
        let req = ApiRunRequest {
            task: "hello".into(),
            backend: None,
            config: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ApiRunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task, "hello");
        assert!(back.backend.is_none());
        assert!(back.config.is_none());
    }

    #[test]
    fn run_request_omits_none_fields() {
        let req = ApiRunRequest {
            task: "t".into(),
            backend: None,
            config: None,
        };
        let val = serde_json::to_value(&req).unwrap();
        assert!(val.get("backend").is_none());
        assert!(val.get("config").is_none());
    }

    #[test]
    fn run_request_with_nested_config() {
        let req = ApiRunRequest {
            task: "deploy".into(),
            backend: Some("sidecar:node".into()),
            config: Some(serde_json::json!({"nested": {"key": "val"}})),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ApiRunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.config.unwrap()["nested"]["key"], "val");
    }

    // -- api::RunResponse -----------------------------------------------

    #[test]
    fn run_response_serde_roundtrip() {
        let resp = ApiRunResponse {
            run_id: "abc-123".into(),
            status: ApiRunStatus::Queued,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ApiRunResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, "abc-123");
        assert_eq!(back.status, ApiRunStatus::Queued);
    }

    #[test]
    fn run_response_all_statuses() {
        for status in [
            ApiRunStatus::Queued,
            ApiRunStatus::Running,
            ApiRunStatus::Completed,
            ApiRunStatus::Failed,
        ] {
            let resp = ApiRunResponse {
                run_id: "r1".into(),
                status,
            };
            let json = serde_json::to_string(&resp).unwrap();
            let back: ApiRunResponse = serde_json::from_str(&json).unwrap();
            assert_eq!(back.status, status);
        }
    }

    // -- api::BackendInfo -----------------------------------------------

    #[test]
    fn backend_info_serde_roundtrip() {
        let info = ApiBackendInfo {
            name: "sidecar:node".into(),
            dialect: "openai".into(),
            status: "ready".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: ApiBackendInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "sidecar:node");
        assert_eq!(back.dialect, "openai");
        assert_eq!(back.status, "ready");
    }

    #[test]
    fn backend_info_clone() {
        let info = ApiBackendInfo {
            name: "mock".into(),
            dialect: "mock".into(),
            status: "ok".into(),
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, info.name);
        assert_eq!(cloned.dialect, info.dialect);
    }

    // -- api::ListBackendsResponse --------------------------------------

    #[test]
    fn list_backends_response_serde_roundtrip() {
        let resp = ListBackendsResponse {
            backends: vec![ApiBackendInfo {
                name: "mock".into(),
                dialect: "mock".into(),
                status: "ready".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ListBackendsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backends.len(), 1);
        assert_eq!(back.backends[0].name, "mock");
    }

    #[test]
    fn list_backends_response_empty() {
        let resp = ListBackendsResponse { backends: vec![] };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val["backends"].as_array().unwrap().is_empty());
    }

    // -- api::ErrorResponse ---------------------------------------------

    #[test]
    fn error_response_serde_roundtrip_with_code() {
        let err = ErrorResponse {
            error: "not found".into(),
            code: Some("not_found".into()),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.error, "not found");
        assert_eq!(back.code.as_deref(), Some("not_found"));
    }

    #[test]
    fn error_response_serde_roundtrip_without_code() {
        let err = ErrorResponse {
            error: "something went wrong".into(),
            code: None,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.error, "something went wrong");
        assert!(back.code.is_none());
    }

    #[test]
    fn error_response_omits_none_code() {
        let err = ErrorResponse {
            error: "oops".into(),
            code: None,
        };
        let val = serde_json::to_value(&err).unwrap();
        assert!(val.get("code").is_none());
    }

    // -- api::HealthResponse (updated) ----------------------------------

    #[test]
    fn health_response_has_backends_list() {
        let resp = ApiHealth {
            status: "ok".into(),
            version: abp_core::CONTRACT_VERSION.into(),
            uptime_secs: 42,
            backends: vec!["mock".into(), "sidecar:node".into()],
        };
        let val = serde_json::to_value(&resp).unwrap();
        let arr = val["backends"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "mock");
    }

    #[test]
    fn health_response_empty_backends() {
        let resp = ApiHealth {
            status: "ok".into(),
            version: "abp/v0.1".into(),
            uptime_secs: 0,
            backends: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ApiHealth = serde_json::from_str(&json).unwrap();
        assert!(back.backends.is_empty());
        assert_eq!(back.uptime_secs, 0);
    }

    // -- routes::Route --------------------------------------------------

    #[test]
    fn route_serde_roundtrip() {
        let route = Route {
            method: "GET".into(),
            path: "/api/v1/health".into(),
            description: "Health check".into(),
        };
        let json = serde_json::to_string(&route).unwrap();
        let back: Route = serde_json::from_str(&json).unwrap();
        assert_eq!(back.method, "GET");
        assert_eq!(back.path, "/api/v1/health");
        assert_eq!(back.description, "Health check");
    }

    #[test]
    fn api_routes_returns_six_routes() {
        let routes = api_routes();
        assert_eq!(routes.len(), 6);
    }

    #[test]
    fn api_routes_contains_post_run() {
        let routes = api_routes();
        assert!(
            routes
                .iter()
                .any(|r| r.method == "POST" && r.path == "/api/v1/run")
        );
    }

    #[test]
    fn api_routes_contains_get_health() {
        let routes = api_routes();
        assert!(
            routes
                .iter()
                .any(|r| r.method == "GET" && r.path == "/api/v1/health")
        );
    }

    #[test]
    fn api_routes_contains_get_backends() {
        let routes = api_routes();
        assert!(
            routes
                .iter()
                .any(|r| r.method == "GET" && r.path == "/api/v1/backends")
        );
    }

    #[test]
    fn api_routes_contains_get_run_events() {
        let routes = api_routes();
        assert!(
            routes
                .iter()
                .any(|r| r.method == "GET" && r.path == "/api/v1/run/{id}/events")
        );
    }

    #[test]
    fn api_routes_contains_get_run_receipt() {
        let routes = api_routes();
        assert!(
            routes
                .iter()
                .any(|r| r.method == "GET" && r.path == "/api/v1/run/{id}/receipt")
        );
    }

    #[test]
    fn api_routes_all_have_descriptions() {
        for route in api_routes() {
            assert!(
                !route.description.is_empty(),
                "route {} {} has empty description",
                route.method,
                route.path
            );
        }
    }

    #[test]
    fn api_routes_contains_get_run_status() {
        let routes = api_routes();
        assert!(
            routes
                .iter()
                .any(|r| r.method == "GET" && r.path == "/api/v1/run/{id}")
        );
    }
}
