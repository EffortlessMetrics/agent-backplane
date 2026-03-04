#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Axum router for the `/api/v1` HTTP endpoints.
//!
//! Provides `api_v1_router` which builds an Axum `Router` with the
//! following endpoints:
//!
//! - `POST   /api/v1/runs`            — submit a work order
//! - `GET    /api/v1/runs/:id`        — get run status / receipt
//! - `GET    /api/v1/runs/:id/events` — SSE event stream for a run
//! - `DELETE /api/v1/runs/:id`        — cancel a run
//! - `GET    /api/v1/backends`        — list available backends
//! - `GET    /api/v1/health`          — health check

use crate::api_types::{
    BackendInfoEntry, BackendListResponse, CancelRunResponse, ErrorResponse, HealthResponse,
    RunStatusKind, RunStatusResponse, SseEventData, SubmitRunRequest, SubmitRunResponse,
};
use crate::sse;
use crate::state::{RunPhase, RunRecord, ServerState};
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

/// Build an Axum [`Router`] containing all `/api/v1` endpoints.
///
/// The returned router is nested under `/api/v1` so callers should mount it
/// directly on their application root.
///
/// # Example
///
/// ```ignore
/// let app = api_v1_router(state.clone());
/// ```
pub fn api_v1_router(state: Arc<ServerState>) -> Router {
    let runs = Router::new()
        .route("/", post(handle_submit_run))
        .route("/{run_id}", get(handle_get_run).delete(handle_cancel_run))
        .route("/{run_id}/events", get(handle_run_events));

    Router::new()
        .route("/api/v1/health", get(handle_health))
        .route("/api/v1/backends", get(handle_backends))
        .nest("/api/v1/runs", runs)
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a [`RunPhase`] to the API-facing [`RunStatusKind`].
fn phase_to_status(phase: RunPhase) -> RunStatusKind {
    match phase {
        RunPhase::Queued => RunStatusKind::Queued,
        RunPhase::Running => RunStatusKind::Running,
        RunPhase::Completed => RunStatusKind::Completed,
        RunPhase::Failed => RunStatusKind::Failed,
        RunPhase::Cancelled => RunStatusKind::Cancelled,
    }
}

/// Map a [`RunRecord`] to a [`RunStatusResponse`].
fn record_to_response(record: &RunRecord) -> RunStatusResponse {
    RunStatusResponse {
        run_id: record.id,
        status: phase_to_status(record.phase),
        receipt: record.receipt.clone().map(Box::new),
        events_count: record.events.len(),
        backend: record.backend.clone(),
        created_at: record.created_at,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/health` — always returns 200 with server status info.
async fn handle_health(State(state): State<Arc<ServerState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        version: abp_core::CONTRACT_VERSION.into(),
        uptime_secs: state.uptime_secs(),
    })
}

/// `GET /api/v1/backends` — list registered backends.
async fn handle_backends(State(state): State<Arc<ServerState>>) -> Json<BackendListResponse> {
    let names = state.backends.list().await;
    let backends = names
        .into_iter()
        .map(|name| {
            let backend_type = if name.starts_with("sidecar:") {
                "sidecar".into()
            } else {
                name.clone()
            };
            BackendInfoEntry {
                name,
                backend_type,
                status: "available".into(),
            }
        })
        .collect();
    Json(BackendListResponse { backends })
}

/// `POST /api/v1/runs` — submit a work order, return the assigned run ID.
async fn handle_submit_run(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<SubmitRunRequest>,
) -> Result<(StatusCode, Json<SubmitRunResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Validate task is non-empty.
    if req.work_order.task.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::invalid_request("task must not be empty")),
        ));
    }

    // Validate backend exists.
    if !state.backends.contains(&req.backend).await {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::invalid_request(format!(
                "unknown backend: {}",
                req.backend
            ))),
        ));
    }

    let run_id = req.work_order.id;

    match state.registry.create_run(run_id, req.backend.clone()).await {
        Ok(_) => Ok((
            StatusCode::CREATED,
            Json(SubmitRunResponse {
                run_id,
                status: RunStatusKind::Queued,
            }),
        )),
        Err(e) => Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse::conflict(e.to_string())),
        )),
    }
}

/// `GET /api/v1/runs/:id` — get run status / receipt.
async fn handle_get_run(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<RunStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.registry.get(run_id).await {
        Some(record) => Ok(Json(record_to_response(&record))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::not_found(format!("run {run_id} not found"))),
        )),
    }
}

/// `GET /api/v1/runs/:id/events` — SSE event stream for a run.
///
/// If the run exists, replays all collected events as SSE messages. For live
/// runs a production implementation would hold open the connection and push
/// new events as they arrive; this implementation replays the snapshot.
async fn handle_run_events(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<ServerState>>,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    match state.registry.events(run_id).await {
        Ok(events) => Ok(sse::replay_event_stream(events)),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::not_found(format!("run {run_id} not found"))),
        )),
    }
}

/// `DELETE /api/v1/runs/:id` — cancel a run.
///
/// Only runs in `Queued` or `Running` phase may be cancelled; attempting to
/// cancel a terminal run returns 409 Conflict.
async fn handle_cancel_run(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<CancelRunResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.registry.cancel(run_id).await {
        Ok(()) => Ok(Json(CancelRunResponse {
            run_id,
            status: RunStatusKind::Cancelled,
        })),
        Err(crate::state::RegistryError::NotFound(_)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::not_found(format!("run {run_id} not found"))),
        )),
        Err(crate::state::RegistryError::InvalidTransition { .. }) => Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse::conflict(format!(
                "run {run_id} cannot be cancelled in its current state"
            ))),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::internal(e.to_string())),
        )),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::WorkOrderBuilder;
    use axum::body::Body;
    use axum::http::{self, Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    // -- Helpers ------------------------------------------------------------

    fn test_state() -> Arc<ServerState> {
        Arc::new(ServerState::new(vec!["mock".into(), "sidecar:node".into()]))
    }

    fn empty_state() -> Arc<ServerState> {
        Arc::new(ServerState::new(vec![]))
    }

    fn test_router(state: Arc<ServerState>) -> Router {
        api_v1_router(state)
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    // -- Health endpoint ----------------------------------------------------

    #[tokio::test]
    async fn health_returns_200() {
        let app = test_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_contains_version() {
        let app = test_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let json = body_json(resp).await;
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], abp_core::CONTRACT_VERSION);
        assert!(json["uptime_secs"].is_number());
    }

    #[tokio::test]
    async fn health_always_200_even_no_backends() {
        let app = test_router(empty_state());
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -- Backends endpoint --------------------------------------------------

    #[tokio::test]
    async fn backends_returns_registered() {
        let app = test_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/backends")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let backends = json["backends"].as_array().unwrap();
        assert_eq!(backends.len(), 2);
        assert_eq!(backends[0]["name"], "mock");
    }

    #[tokio::test]
    async fn backends_empty_when_none_registered() {
        let app = test_router(empty_state());
        let req = Request::builder()
            .uri("/api/v1/backends")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let json = body_json(resp).await;
        assert!(json["backends"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn backends_sidecar_type_detection() {
        let app = test_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/backends")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let json = body_json(resp).await;
        let backends = json["backends"].as_array().unwrap();
        // "sidecar:node" should have backend_type "sidecar".
        let sidecar = backends
            .iter()
            .find(|b| b["name"] == "sidecar:node")
            .unwrap();
        assert_eq!(sidecar["backend_type"], "sidecar");
    }

    // -- Submit run endpoint ------------------------------------------------

    #[tokio::test]
    async fn submit_run_returns_201() {
        let state = test_state();
        let app = test_router(state.clone());
        let wo = WorkOrderBuilder::new("test task").build();
        let req_body = SubmitRunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: Default::default(),
        };
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/api/v1/runs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "queued");
        assert!(json["run_id"].is_string());
    }

    #[tokio::test]
    async fn submit_run_empty_task_returns_400() {
        let app = test_router(test_state());
        let wo = WorkOrderBuilder::new("").build();
        let req_body = SubmitRunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: Default::default(),
        };
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/api/v1/runs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_run_unknown_backend_returns_400() {
        let app = test_router(test_state());
        let wo = WorkOrderBuilder::new("task").build();
        let req_body = SubmitRunRequest {
            work_order: wo,
            backend: "nonexistent".into(),
            overrides: Default::default(),
        };
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/api/v1/runs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["code"], "invalid_request");
    }

    // -- Get run endpoint ---------------------------------------------------

    #[tokio::test]
    async fn get_run_not_found() {
        let app = test_router(test_state());
        let id = Uuid::new_v4();
        let req = Request::builder()
            .uri(format!("/api/v1/runs/{id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_run_after_submit() {
        let state = test_state();
        let wo = WorkOrderBuilder::new("task").build();
        let run_id = wo.id;
        state
            .registry
            .create_run(run_id, "mock".into())
            .await
            .unwrap();

        let app = test_router(state);
        let req = Request::builder()
            .uri(format!("/api/v1/runs/{run_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "queued");
        assert_eq!(json["run_id"], run_id.to_string());
        assert_eq!(json["backend"], "mock");
    }

    // -- Cancel run endpoint ------------------------------------------------

    #[tokio::test]
    async fn cancel_run_not_found() {
        let app = test_router(test_state());
        let id = Uuid::new_v4();
        let req = Request::builder()
            .method(http::Method::DELETE)
            .uri(format!("/api/v1/runs/{id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn cancel_queued_run_succeeds() {
        let state = test_state();
        let run_id = Uuid::new_v4();
        state
            .registry
            .create_run(run_id, "mock".into())
            .await
            .unwrap();

        let app = test_router(state);
        let req = Request::builder()
            .method(http::Method::DELETE)
            .uri(format!("/api/v1/runs/{run_id}"))
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

        let app = test_router(state);
        let req = Request::builder()
            .method(http::Method::DELETE)
            .uri(format!("/api/v1/runs/{run_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    // -- Events endpoint ----------------------------------------------------

    #[tokio::test]
    async fn events_not_found() {
        let app = test_router(test_state());
        let id = Uuid::new_v4();
        let req = Request::builder()
            .uri(format!("/api/v1/runs/{id}/events"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn events_returns_sse_for_existing_run() {
        let state = test_state();
        let run_id = Uuid::new_v4();
        state
            .registry
            .create_run(run_id, "mock".into())
            .await
            .unwrap();

        let app = test_router(state);
        let req = Request::builder()
            .uri(format!("/api/v1/runs/{run_id}/events"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // SSE responses have text/event-stream content type.
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("text/event-stream"));
    }

    // -- Error response formatting ------------------------------------------

    #[tokio::test]
    async fn error_response_has_code_and_message() {
        let app = test_router(test_state());
        let id = Uuid::new_v4();
        let req = Request::builder()
            .uri(format!("/api/v1/runs/{id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let json = body_json(resp).await;
        assert_eq!(json["code"], "not_found");
        assert!(json["message"].is_string());
    }

    // -- Auth middleware integration (structural) ----------------------------

    #[tokio::test]
    async fn router_integrates_with_middleware_layer() {
        use crate::middleware::request_id_middleware;
        use axum::middleware;

        let state = test_state();
        let app = api_v1_router(state).layer(middleware::from_fn(request_id_middleware));

        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // The request-id middleware should add the header.
        assert!(resp.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn router_integrates_with_bearer_auth() {
        use crate::middleware::BearerAuth;
        use axum::middleware;

        let state = test_state();
        let auth = BearerAuth::new(Some("secret-token".into()));
        let app = api_v1_router(state)
            .layer(axum::Extension(auth.clone()))
            .layer(middleware::from_fn(BearerAuth::layer));

        // Health should still be accessible (middleware allows /api/v1/health).
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
