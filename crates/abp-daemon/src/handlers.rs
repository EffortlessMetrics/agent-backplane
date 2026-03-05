// SPDX-License-Identifier: MIT OR Apache-2.0
//! Request handlers for the daemon HTTP control-plane `/v1` endpoints.
//!
//! Each handler validates its input and returns a typed JSON response.
//! Handlers are framework-agnostic functions that accept shared state and
//! return `Result<Json<_>, (StatusCode, Json<ErrorBody>)>`.

use crate::models::{
    BackendInfo, BackendsListResponse, CancelResponse, ErrorBody, HealthResponse, ReceiptResponse,
    ReceiptSummary, ReceiptsListResponse, RunRequest, RunResponse, RunStatusKind, StatusResponse,
    TranslateRequest, TranslateResponse,
};
use crate::sse;
use crate::state::{RunPhase, ServerState};
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, Sse};
use axum::Json;
use std::convert::Infallible;
use std::sync::Arc;
use uuid::Uuid;

type HandlerError = (StatusCode, Json<ErrorBody>);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn phase_to_kind(phase: RunPhase) -> RunStatusKind {
    match phase {
        RunPhase::Queued => RunStatusKind::Queued,
        RunPhase::Running => RunStatusKind::Running,
        RunPhase::Completed => RunStatusKind::Completed,
        RunPhase::Failed => RunStatusKind::Failed,
        RunPhase::Cancelled => RunStatusKind::Cancelled,
    }
}

fn backend_type_for(name: &str) -> String {
    if name.starts_with("sidecar:") {
        "sidecar".into()
    } else {
        name.to_string()
    }
}

// ---------------------------------------------------------------------------
// POST /v1/run — submit a work order
// ---------------------------------------------------------------------------

/// Submit a work order for execution.
///
/// Validates the request, registers the run with the state registry, and
/// returns the assigned run ID with a `queued` status.
pub async fn run_handler(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<RunRequest>,
) -> Result<(StatusCode, Json<RunResponse>), HandlerError> {
    // Validate task is non-empty.
    if req.work_order.task.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody::bad_request("task must not be empty")),
        ));
    }

    // Validate backend exists.
    if !state.backends.contains(&req.backend).await {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody::bad_request(format!(
                "unknown backend: {}",
                req.backend
            ))),
        ));
    }

    let run_id = req.work_order.id;

    match state.registry.create_run(run_id, req.backend.clone()).await {
        Ok(_) => Ok((
            StatusCode::CREATED,
            Json(RunResponse {
                run_id,
                status: RunStatusKind::Queued,
                message: Some("run queued".into()),
            }),
        )),
        Err(e) => Err((
            StatusCode::CONFLICT,
            Json(ErrorBody::conflict(e.to_string())),
        )),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/status/:id — check run status
// ---------------------------------------------------------------------------

/// Return the current status of a run by ID.
pub async fn status_handler(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<StatusResponse>, HandlerError> {
    match state.registry.get(run_id).await {
        Some(record) => Ok(Json(StatusResponse {
            run_id: record.id,
            status: phase_to_kind(record.phase),
            backend: record.backend.clone(),
            created_at: record.created_at,
            events_count: record.events.len(),
            error: record.error.clone(),
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody::not_found(format!("run {run_id} not found"))),
        )),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/receipt/:id — get receipt
// ---------------------------------------------------------------------------

/// Return the receipt for a completed run.
pub async fn receipt_handler(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<ReceiptResponse>, HandlerError> {
    match state.registry.get(run_id).await {
        Some(record) => match record.receipt {
            Some(receipt) => Ok(Json(ReceiptResponse {
                run_id: record.id,
                receipt,
            })),
            None => Err((
                StatusCode::NOT_FOUND,
                Json(ErrorBody::not_found(format!(
                    "run {run_id} has no receipt yet"
                ))),
            )),
        },
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody::not_found(format!("run {run_id} not found"))),
        )),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/backends — list backends
// ---------------------------------------------------------------------------

/// List all registered backends with type information.
pub async fn backends_handler(State(state): State<Arc<ServerState>>) -> Json<BackendsListResponse> {
    let names = state.backends.list().await;
    let backends = names
        .into_iter()
        .map(|name| {
            let backend_type = backend_type_for(&name);
            BackendInfo {
                name,
                backend_type,
                status: "available".into(),
            }
        })
        .collect();
    Json(BackendsListResponse { backends })
}

// ---------------------------------------------------------------------------
// GET /v1/health — health check
// ---------------------------------------------------------------------------

/// Return server health status, contract version, and uptime.
pub async fn health_handler(State(state): State<Arc<ServerState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        version: abp_core::CONTRACT_VERSION.into(),
        uptime_secs: state.uptime_secs(),
    })
}

// ---------------------------------------------------------------------------
// POST /v1/cancel/:id — cancel a run
// ---------------------------------------------------------------------------

/// Cancel a queued or running run.
///
/// Only runs in `Queued` or `Running` phase may be cancelled; terminal runs
/// return 409 Conflict.
pub async fn cancel_handler(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<CancelResponse>, HandlerError> {
    match state.registry.cancel(run_id).await {
        Ok(()) => Ok(Json(CancelResponse {
            run_id,
            status: RunStatusKind::Cancelled,
            message: Some("run cancelled".into()),
        })),
        Err(crate::state::RegistryError::NotFound(_)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody::not_found(format!("run {run_id} not found"))),
        )),
        Err(crate::state::RegistryError::InvalidTransition { .. }) => Err((
            StatusCode::CONFLICT,
            Json(ErrorBody::conflict(format!(
                "run {run_id} cannot be cancelled in its current state"
            ))),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody::internal(e.to_string())),
        )),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/run/:id/events — SSE event stream
// ---------------------------------------------------------------------------

/// Return an SSE stream of events for a run.
///
/// Replays all collected events for the run as SSE messages. For live runs a
/// production implementation would hold open the connection; this replays the
/// snapshot.
pub async fn events_handler(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<ServerState>>,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>,
    (StatusCode, Json<ErrorBody>),
> {
    match state.registry.events(run_id).await {
        Ok(events) => Ok(sse::replay_event_stream(events)),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody::not_found(format!("run {run_id} not found"))),
        )),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/translate — translate between dialects
// ---------------------------------------------------------------------------

/// Translate an IR conversation between agent dialects.
pub async fn translate_handler(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<TranslateRequest>,
) -> Result<Json<TranslateResponse>, (StatusCode, Json<ErrorBody>)> {
    let engine = state.translation_engine();
    if !engine.supports(req.from, req.to) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody::bad_request(format!(
                "translation from {:?} to {:?} is not supported",
                req.from, req.to
            ))),
        ));
    }

    match engine.translate(req.from, req.to, &req.conversation) {
        Ok(result) => Ok(Json(TranslateResponse {
            conversation: result.conversation,
            from: result.from,
            to: result.to,
            mode: format!("{:?}", result.mode),
            gaps: result.gaps.iter().map(|g| g.description.clone()).collect(),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody::internal(e.to_string())),
        )),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/receipts — list stored receipts
// ---------------------------------------------------------------------------

/// List receipt summaries from the registry.
pub async fn receipts_list_handler(
    State(state): State<Arc<ServerState>>,
) -> Json<ReceiptsListResponse> {
    let all = state.registry.list_all().await;
    let receipts = all
        .iter()
        .filter_map(|r| {
            r.receipt.as_ref().map(|receipt| ReceiptSummary {
                run_id: r.id,
                backend: r.backend.clone(),
                outcome: format!("{:?}", receipt.outcome),
            })
        })
        .collect();
    Json(ReceiptsListResponse { receipts })
}

// ---------------------------------------------------------------------------
// GET /v1/receipts/:id — get specific receipt
// ---------------------------------------------------------------------------

/// Get a specific receipt by run ID.
pub async fn receipt_by_id_handler(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<ReceiptResponse>, (StatusCode, Json<ErrorBody>)> {
    match state.registry.get(run_id).await {
        Some(record) => match record.receipt {
            Some(receipt) => Ok(Json(ReceiptResponse {
                run_id: record.id,
                receipt,
            })),
            None => Err((
                StatusCode::NOT_FOUND,
                Json(ErrorBody::not_found(format!(
                    "run {run_id} has no receipt yet"
                ))),
            )),
        },
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody::not_found(format!("run {run_id} not found"))),
        )),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{RunPhase, ServerState};
    use abp_core::WorkOrderBuilder;
    use std::collections::BTreeMap;

    fn test_state() -> Arc<ServerState> {
        Arc::new(ServerState::new(vec!["mock".into(), "sidecar:node".into()]))
    }

    fn empty_state() -> Arc<ServerState> {
        Arc::new(ServerState::new(vec![]))
    }

    // -- phase_to_kind ------------------------------------------------------

    #[test]
    fn phase_to_kind_maps_all() {
        assert_eq!(phase_to_kind(RunPhase::Queued), RunStatusKind::Queued);
        assert_eq!(phase_to_kind(RunPhase::Running), RunStatusKind::Running);
        assert_eq!(phase_to_kind(RunPhase::Completed), RunStatusKind::Completed);
        assert_eq!(phase_to_kind(RunPhase::Failed), RunStatusKind::Failed);
        assert_eq!(phase_to_kind(RunPhase::Cancelled), RunStatusKind::Cancelled);
    }

    // -- backend_type_for ---------------------------------------------------

    #[test]
    fn backend_type_detects_sidecar() {
        assert_eq!(backend_type_for("sidecar:node"), "sidecar");
        assert_eq!(backend_type_for("sidecar:python"), "sidecar");
        assert_eq!(backend_type_for("mock"), "mock");
    }

    // -- run_handler --------------------------------------------------------

    #[tokio::test]
    async fn run_handler_valid_request() {
        let state = test_state();
        let wo = WorkOrderBuilder::new("test task").build();
        let req = RunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: BTreeMap::new(),
        };
        let result = run_handler(State(state), Json(req)).await;
        let (status, Json(resp)) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(resp.status, RunStatusKind::Queued);
    }

    #[tokio::test]
    async fn run_handler_empty_task_400() {
        let state = test_state();
        let wo = WorkOrderBuilder::new("").build();
        let req = RunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: BTreeMap::new(),
        };
        let err = run_handler(State(state), Json(req)).await.unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert_eq!(err.1.code, "bad_request");
    }

    #[tokio::test]
    async fn run_handler_unknown_backend_400() {
        let state = test_state();
        let wo = WorkOrderBuilder::new("task").build();
        let req = RunRequest {
            work_order: wo,
            backend: "nonexistent".into(),
            overrides: BTreeMap::new(),
        };
        let err = run_handler(State(state), Json(req)).await.unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn run_handler_duplicate_id_409() {
        let state = test_state();
        let wo = WorkOrderBuilder::new("task").build();
        let run_id = wo.id;
        state
            .registry
            .create_run(run_id, "mock".into())
            .await
            .unwrap();
        let req = RunRequest {
            work_order: wo,
            backend: "mock".into(),
            overrides: BTreeMap::new(),
        };
        let err = run_handler(State(state), Json(req)).await.unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
    }

    // -- status_handler -----------------------------------------------------

    #[tokio::test]
    async fn status_handler_found() {
        let state = test_state();
        let run_id = Uuid::new_v4();
        state
            .registry
            .create_run(run_id, "mock".into())
            .await
            .unwrap();
        let result = status_handler(AxPath(run_id), State(state)).await;
        let Json(resp) = result.unwrap();
        assert_eq!(resp.run_id, run_id);
        assert_eq!(resp.status, RunStatusKind::Queued);
        assert_eq!(resp.backend, "mock");
    }

    #[tokio::test]
    async fn status_handler_not_found() {
        let state = test_state();
        let err = status_handler(AxPath(Uuid::new_v4()), State(state))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }

    // -- receipt_handler ----------------------------------------------------

    #[tokio::test]
    async fn receipt_handler_no_receipt_yet() {
        let state = test_state();
        let run_id = Uuid::new_v4();
        state
            .registry
            .create_run(run_id, "mock".into())
            .await
            .unwrap();
        let err = receipt_handler(AxPath(run_id), State(state))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
        assert!(err.1.message.contains("no receipt"));
    }

    #[tokio::test]
    async fn receipt_handler_not_found() {
        let state = test_state();
        let err = receipt_handler(AxPath(Uuid::new_v4()), State(state))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn receipt_handler_completed_run() {
        use abp_core::{Outcome, ReceiptBuilder};
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
        let result = receipt_handler(AxPath(run_id), State(state)).await;
        let Json(resp) = result.unwrap();
        assert_eq!(resp.run_id, run_id);
    }

    // -- backends_handler ---------------------------------------------------

    #[tokio::test]
    async fn backends_handler_returns_registered() {
        let state = test_state();
        let Json(resp) = backends_handler(State(state)).await;
        assert_eq!(resp.backends.len(), 2);
        assert_eq!(resp.backends[0].name, "mock");
        assert_eq!(resp.backends[1].name, "sidecar:node");
        assert_eq!(resp.backends[1].backend_type, "sidecar");
    }

    #[tokio::test]
    async fn backends_handler_empty() {
        let state = empty_state();
        let Json(resp) = backends_handler(State(state)).await;
        assert!(resp.backends.is_empty());
    }

    // -- health_handler -----------------------------------------------------

    #[tokio::test]
    async fn health_handler_returns_ok() {
        let state = test_state();
        let Json(resp) = health_handler(State(state)).await;
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.version, abp_core::CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn health_handler_uptime_non_negative() {
        let state = test_state();
        let Json(resp) = health_handler(State(state)).await;
        // uptime_secs should be a small value (test runs fast).
        assert!(resp.uptime_secs < 60);
    }

    // -- cancel_handler -----------------------------------------------------

    #[tokio::test]
    async fn cancel_handler_queued_run() {
        let state = test_state();
        let run_id = Uuid::new_v4();
        state
            .registry
            .create_run(run_id, "mock".into())
            .await
            .unwrap();
        let result = cancel_handler(AxPath(run_id), State(state)).await;
        let Json(resp) = result.unwrap();
        assert_eq!(resp.run_id, run_id);
        assert_eq!(resp.status, RunStatusKind::Cancelled);
    }

    #[tokio::test]
    async fn cancel_handler_running_run() {
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
        let result = cancel_handler(AxPath(run_id), State(state)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cancel_handler_completed_run_409() {
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
        let err = cancel_handler(AxPath(run_id), State(state))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn cancel_handler_not_found() {
        let state = test_state();
        let err = cancel_handler(AxPath(Uuid::new_v4()), State(state))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }
}
