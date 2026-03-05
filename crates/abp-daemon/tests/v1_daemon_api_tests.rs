// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for the v1 daemon HTTP API.
//!
//! Tests cover all eight endpoint families:
//! 1. POST /v1/run          — submit work order
//! 2. GET  /v1/run/{id}     — get run status
//! 3. GET  /v1/run/{id}/events — SSE event stream
//! 4. GET  /v1/backends     — list backends
//! 5. POST /v1/translate    — translate between dialects
//! 6. GET  /v1/health       — system health
//! 7. GET  /v1/receipts     — query stored receipts
//! 8. GET  /v1/receipts/{id} — get specific receipt

use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, WorkOrderBuilder};
use abp_daemon::models::{RunRequest, TranslateRequest};
use abp_daemon::routes::v1_routes;
use abp_daemon::state::{RunPhase, ServerState};
use abp_dialect::Dialect;
use axum::Router;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use axum::response::Response;
use chrono::Utc;
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

fn test_app(state: Arc<ServerState>) -> Router {
    v1_routes(state)
}

async fn body_json(resp: Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn json_request(method: http::Method, uri: &str, body: &impl serde::Serialize) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

fn get_request(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

// ===========================================================================
// 1. POST /v1/run — submit work order
// ===========================================================================

#[tokio::test]
async fn post_run_returns_201_with_run_id() {
    let state = test_state();
    let app = test_app(state);
    let wo = WorkOrderBuilder::new("hello world").build();
    let req_body = RunRequest {
        work_order: wo,
        backend: "mock".into(),
        overrides: BTreeMap::new(),
    };
    let resp = app
        .oneshot(json_request(http::Method::POST, "/v1/run", &req_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "queued");
    assert!(json["run_id"].is_string());
}

#[tokio::test]
async fn post_run_empty_task_returns_400() {
    let app = test_app(test_state());
    let wo = WorkOrderBuilder::new("").build();
    let req_body = RunRequest {
        work_order: wo,
        backend: "mock".into(),
        overrides: BTreeMap::new(),
    };
    let resp = app
        .oneshot(json_request(http::Method::POST, "/v1/run", &req_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp).await;
    assert_eq!(json["code"], "bad_request");
}

#[tokio::test]
async fn post_run_unknown_backend_returns_400() {
    let app = test_app(test_state());
    let wo = WorkOrderBuilder::new("task").build();
    let req_body = RunRequest {
        work_order: wo,
        backend: "nonexistent".into(),
        overrides: BTreeMap::new(),
    };
    let resp = app
        .oneshot(json_request(http::Method::POST, "/v1/run", &req_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("unknown backend")
    );
}

#[tokio::test]
async fn post_run_duplicate_id_returns_409() {
    let state = test_state();
    let wo = WorkOrderBuilder::new("task").build();
    let run_id = wo.id;
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    let app = test_app(state);
    let req_body = RunRequest {
        work_order: wo,
        backend: "mock".into(),
        overrides: BTreeMap::new(),
    };
    let resp = app
        .oneshot(json_request(http::Method::POST, "/v1/run", &req_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn post_run_message_field_present() {
    let state = test_state();
    let app = test_app(state);
    let wo = WorkOrderBuilder::new("task").build();
    let req_body = RunRequest {
        work_order: wo,
        backend: "mock".into(),
        overrides: BTreeMap::new(),
    };
    let resp = app
        .oneshot(json_request(http::Method::POST, "/v1/run", &req_body))
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert!(json.get("message").is_some());
}

// ===========================================================================
// 2. GET /v1/run/{id} — get run status
// ===========================================================================

#[tokio::test]
async fn get_run_returns_status_for_existing_run() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{run_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["run_id"], run_id.to_string());
    assert_eq!(json["status"], "queued");
    assert_eq!(json["backend"], "mock");
}

#[tokio::test]
async fn get_run_not_found_returns_404() {
    let app = test_app(test_state());
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{}", Uuid::new_v4())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let json = body_json(resp).await;
    assert_eq!(json["code"], "not_found");
}

#[tokio::test]
async fn get_run_shows_running_phase() {
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
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{run_id}")))
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["status"], "running");
}

#[tokio::test]
async fn get_run_shows_events_count() {
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
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    state.registry.push_event(run_id, event).await.unwrap();
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{run_id}")))
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["events_count"], 1);
}

#[tokio::test]
async fn get_run_shows_error_when_failed() {
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
    state.registry.fail(run_id, "timeout".into()).await.unwrap();
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{run_id}")))
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error"], "timeout");
}

// ===========================================================================
// 3. GET /v1/run/{id}/events — SSE event stream
// ===========================================================================

#[tokio::test]
async fn get_events_returns_sse_for_existing_run() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{run_id}/events")))
        .await
        .unwrap();
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
async fn get_events_not_found_returns_404() {
    let app = test_app(test_state());
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{}/events", Uuid::new_v4())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_events_includes_done_sentinel() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{run_id}/events")))
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);
    assert!(body.contains("event: done"));
}

#[tokio::test]
async fn get_events_replays_pushed_events() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    state.registry.push_event(run_id, event).await.unwrap();
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/run/{run_id}/events")))
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);
    assert!(body.contains("event: assistant_message"));
    assert!(body.contains("hello"));
}

// ===========================================================================
// 4. GET /v1/backends — list backends
// ===========================================================================

#[tokio::test]
async fn get_backends_returns_registered() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/backends")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let backends = json["backends"].as_array().unwrap();
    assert_eq!(backends.len(), 2);
    assert_eq!(backends[0]["name"], "mock");
    assert_eq!(backends[1]["name"], "sidecar:node");
}

#[tokio::test]
async fn get_backends_empty_when_none_registered() {
    let app = test_app(empty_state());
    let resp = app.oneshot(get_request("/v1/backends")).await.unwrap();
    let json = body_json(resp).await;
    assert!(json["backends"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_backends_detects_sidecar_type() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/backends")).await.unwrap();
    let json = body_json(resp).await;
    let backends = json["backends"].as_array().unwrap();
    let sidecar = backends
        .iter()
        .find(|b| b["name"] == "sidecar:node")
        .unwrap();
    assert_eq!(sidecar["backend_type"], "sidecar");
}

#[tokio::test]
async fn get_backends_status_is_available() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/backends")).await.unwrap();
    let json = body_json(resp).await;
    let backends = json["backends"].as_array().unwrap();
    for b in backends {
        assert_eq!(b["status"], "available");
    }
}

// ===========================================================================
// 5. POST /v1/translate — translate between dialects
// ===========================================================================

#[tokio::test]
async fn post_translate_passthrough_same_dialect() {
    let state = test_state();
    let app = test_app(state);
    let conversation = abp_core::ir::IrConversation::new();
    let req_body = TranslateRequest {
        from: Dialect::OpenAi,
        to: Dialect::OpenAi,
        conversation,
    };
    let resp = app
        .oneshot(json_request(http::Method::POST, "/v1/translate", &req_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json["mode"].as_str().unwrap().contains("Passthrough"));
}

#[tokio::test]
async fn post_translate_mapped_different_dialects() {
    let state = test_state();
    let app = test_app(state);
    let conversation = abp_core::ir::IrConversation::new();
    let req_body = TranslateRequest {
        from: Dialect::OpenAi,
        to: Dialect::Claude,
        conversation,
    };
    let resp = app
        .oneshot(json_request(http::Method::POST, "/v1/translate", &req_body))
        .await
        .unwrap();
    // Could be OK (200) if translation is supported, or 400 if not
    let status = resp.status();
    assert!(status == StatusCode::OK || status == StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_translate_returns_from_and_to() {
    let state = test_state();
    let app = test_app(state);
    let conversation = abp_core::ir::IrConversation::new();
    let req_body = TranslateRequest {
        from: Dialect::OpenAi,
        to: Dialect::OpenAi,
        conversation,
    };
    let resp = app
        .oneshot(json_request(http::Method::POST, "/v1/translate", &req_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json.get("from").is_some());
    assert!(json.get("to").is_some());
}

// ===========================================================================
// 6. GET /v1/health — system health
// ===========================================================================

#[tokio::test]
async fn get_health_returns_200() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/health")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_health_contains_contract_version() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/health")).await.unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["version"], abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn get_health_has_uptime() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/health")).await.unwrap();
    let json = body_json(resp).await;
    assert!(json["uptime_secs"].is_number());
}

#[tokio::test]
async fn get_health_ok_even_no_backends() {
    let app = test_app(empty_state());
    let resp = app.oneshot(get_request("/v1/health")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// 7. GET /v1/receipts — query stored receipts
// ===========================================================================

#[tokio::test]
async fn get_receipts_empty_initially() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/receipts")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json["receipts"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_receipts_returns_completed_runs() {
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
    state
        .registry
        .complete(run_id, receipt.clone())
        .await
        .unwrap();
    let app = test_app(state);
    let resp = app.oneshot(get_request("/v1/receipts")).await.unwrap();
    let json = body_json(resp).await;
    let receipts = json["receipts"].as_array().unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["backend"], "mock");
}

#[tokio::test]
async fn get_receipts_excludes_non_completed_runs() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    // Run is still queued — no receipt
    let app = test_app(state);
    let resp = app.oneshot(get_request("/v1/receipts")).await.unwrap();
    let json = body_json(resp).await;
    assert!(json["receipts"].as_array().unwrap().is_empty());
}

// ===========================================================================
// 8. GET /v1/receipts/{id} — get specific receipt
// ===========================================================================

#[tokio::test]
async fn get_receipt_by_id_returns_receipt() {
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
    state
        .registry
        .complete(run_id, receipt.clone())
        .await
        .unwrap();
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/receipts/{run_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["run_id"], run_id.to_string());
    assert!(json.get("receipt").is_some());
}

#[tokio::test]
async fn get_receipt_by_id_not_found() {
    let app = test_app(test_state());
    let resp = app
        .oneshot(get_request(&format!("/v1/receipts/{}", Uuid::new_v4())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_receipt_by_id_no_receipt_yet() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    let app = test_app(state);
    let resp = app
        .oneshot(get_request(&format!("/v1/receipts/{run_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let json = body_json(resp).await;
    assert!(json["message"].as_str().unwrap().contains("no receipt"));
}

// ===========================================================================
// Cancel endpoint (existing route, wired under /v1)
// ===========================================================================

#[tokio::test]
async fn cancel_queued_run_returns_ok() {
    let state = test_state();
    let run_id = Uuid::new_v4();
    state
        .registry
        .create_run(run_id, "mock".into())
        .await
        .unwrap();
    let app = test_app(state);
    let req = Request::builder()
        .method(http::Method::POST)
        .uri(format!("/v1/cancel/{run_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "cancelled");
}

#[tokio::test]
async fn cancel_completed_run_returns_409() {
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
    let app = test_app(state);
    let req = Request::builder()
        .method(http::Method::POST)
        .uri(format!("/v1/cancel/{run_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn cancel_nonexistent_run_returns_404() {
    let app = test_app(test_state());
    let req = Request::builder()
        .method(http::Method::POST)
        .uri(format!("/v1/cancel/{}", Uuid::new_v4()))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// Cross-cutting: route matching, method validation, error format
// ===========================================================================

#[tokio::test]
async fn unknown_route_returns_404() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/nonexistent")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_on_post_route_returns_405() {
    let app = test_app(test_state());
    let resp = app.oneshot(get_request("/v1/run")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn invalid_json_body_returns_error() {
    let app = test_app(test_state());
    let req = Request::builder()
        .method(http::Method::POST)
        .uri("/v1/run")
        .header("content-type", "application/json")
        .body(Body::from("not valid json"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    // Axum returns 422 for deserialization failures
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

// ===========================================================================
// Route table matching tests (Endpoint enum coverage for new variants)
// ===========================================================================

use abp_daemon::routes::{Endpoint, MatchResult, Method, RouteTable};

#[test]
fn route_table_matches_translate() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Post, "/api/v1/translate");
    assert_eq!(result, MatchResult::Matched(Endpoint::Translate));
}

#[test]
fn route_table_translate_get_not_allowed() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Get, "/api/v1/translate");
    assert_eq!(result, MatchResult::MethodNotAllowed);
}

#[test]
fn route_table_matches_list_receipts() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Get, "/api/v1/receipts");
    assert_eq!(result, MatchResult::Matched(Endpoint::ListReceipts));
}

#[test]
fn route_table_receipts_post_not_allowed() {
    let table = RouteTable::new("/api/v1");
    let result = table.match_route(Method::Post, "/api/v1/receipts");
    assert_eq!(result, MatchResult::MethodNotAllowed);
}

#[test]
fn route_table_matches_get_receipt_by_id() {
    let table = RouteTable::new("/api/v1");
    let id = Uuid::new_v4().to_string();
    let result = table.match_route(Method::Get, &format!("/api/v1/receipts/{id}"));
    assert_eq!(
        result,
        MatchResult::Matched(Endpoint::GetReceipt { run_id: id })
    );
}

#[test]
fn route_table_receipt_by_id_post_not_allowed() {
    let table = RouteTable::new("/api/v1");
    let id = Uuid::new_v4().to_string();
    let result = table.match_route(Method::Post, &format!("/api/v1/receipts/{id}"));
    assert_eq!(result, MatchResult::MethodNotAllowed);
}

#[test]
fn api_routes_includes_translate() {
    let routes = abp_daemon::routes::api_routes();
    let translate = routes
        .iter()
        .find(|r| r.path.contains("translate"))
        .expect("translate route missing");
    assert_eq!(translate.method, "POST");
}

#[test]
fn api_routes_includes_receipts() {
    let routes = abp_daemon::routes::api_routes();
    let receipts = routes
        .iter()
        .filter(|r| r.path.contains("receipts"))
        .count();
    assert!(
        receipts >= 2,
        "expected both /receipts and /receipts/{{id}}"
    );
}
