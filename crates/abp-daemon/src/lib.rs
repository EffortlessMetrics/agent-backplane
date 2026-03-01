// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]
/// Middleware stack for the daemon HTTP API.
pub mod middleware;
/// Priority-based run queue.
pub mod queue;
/// Request validation for the daemon API.
pub mod validation;
/// API versioning support.
pub mod versioning;

use abp_core::{AgentEvent, CapabilityManifest, Receipt, WorkOrder};
use abp_runtime::Runtime;
use axum::{
    Json, Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path as AxPath, Query, State},
    http::StatusCode,
    response::sse::{Event as SseEvent, Sse},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{error, info};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Run lifecycle tracking
// ---------------------------------------------------------------------------

/// Status of a tracked run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunStatus {
    /// The run is queued but not yet started.
    Pending,
    /// The run is currently executing.
    Running,
    /// The run completed successfully with a receipt.
    Completed {
        /// The final receipt.
        receipt: Box<Receipt>,
    },
    /// The run failed with an error.
    Failed {
        /// Error description.
        error: String,
    },
}

/// Tracks active and finished runs with their current status.
#[derive(Clone, Default)]
pub struct RunTracker {
    runs: Arc<RwLock<HashMap<Uuid, RunStatus>>>,
}

impl RunTracker {
    /// Create an empty run tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a run as running. Errors if the run is already tracked.
    pub async fn start_run(&self, run_id: Uuid) -> anyhow::Result<()> {
        let mut guard = self.runs.write().await;
        if guard.contains_key(&run_id) {
            anyhow::bail!("run {run_id} is already tracked");
        }
        guard.insert(run_id, RunStatus::Running);
        Ok(())
    }

    /// Transition a run to completed with its receipt. Errors if the run is
    /// not currently tracked.
    pub async fn complete_run(&self, run_id: Uuid, receipt: Receipt) -> anyhow::Result<()> {
        let mut guard = self.runs.write().await;
        if !guard.contains_key(&run_id) {
            anyhow::bail!("run {run_id} is not tracked");
        }
        guard.insert(
            run_id,
            RunStatus::Completed {
                receipt: Box::new(receipt),
            },
        );
        Ok(())
    }

    /// Transition a run to failed with an error message. Errors if the run is
    /// not currently tracked.
    pub async fn fail_run(&self, run_id: Uuid, error: String) -> anyhow::Result<()> {
        let mut guard = self.runs.write().await;
        if !guard.contains_key(&run_id) {
            anyhow::bail!("run {run_id} is not tracked");
        }
        guard.insert(run_id, RunStatus::Failed { error });
        Ok(())
    }

    /// Return the current status of a run, or `None` if not tracked.
    pub async fn get_run_status(&self, run_id: Uuid) -> Option<RunStatus> {
        self.runs.read().await.get(&run_id).cloned()
    }

    /// Remove a completed or failed run from the tracker. Returns the removed
    /// status, or an error if the run is still running or not found.
    pub async fn remove_run(&self, run_id: Uuid) -> Result<RunStatus, &'static str> {
        let mut guard = self.runs.write().await;
        match guard.get(&run_id) {
            None => Err("not found"),
            Some(RunStatus::Running) | Some(RunStatus::Pending) => Err("conflict"),
            Some(_) => Ok(guard.remove(&run_id).unwrap()),
        }
    }

    /// Return all tracked runs and their statuses.
    pub async fn list_runs(&self) -> Vec<(Uuid, RunStatus)> {
        self.runs
            .read()
            .await
            .iter()
            .map(|(id, s)| (*id, s.clone()))
            .collect()
    }
}

/// Shared application state for the daemon HTTP server.
#[derive(Clone)]
pub struct AppState {
    /// The ABP runtime with registered backends.
    pub runtime: Arc<Runtime>,
    /// In-memory receipt cache keyed by run ID.
    pub receipts: Arc<RwLock<HashMap<Uuid, Receipt>>>,
    /// Directory for persisting receipts to disk.
    pub receipts_dir: PathBuf,
    /// Tracks active and completed runs.
    pub run_tracker: RunTracker,
}

/// Request body for the `/run` endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct RunRequest {
    /// Backend name to execute the work order against.
    pub backend: String,
    /// The work order to execute.
    pub work_order: WorkOrder,
}

/// Response body from the `/run` endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct RunResponse {
    /// Unique run identifier.
    pub run_id: Uuid,
    /// Backend that executed the run.
    pub backend: String,
    /// Agent events emitted during the run.
    pub events: Vec<AgentEvent>,
    /// Final receipt with verification metadata.
    pub receipt: Receipt,
}

/// Backend identity and capability information.
#[derive(Debug, Serialize, Deserialize)]
pub struct BackendInfo {
    /// Backend identifier.
    pub id: String,
    /// Capability manifest reported by this backend.
    pub capabilities: CapabilityManifest,
}

/// Query parameters for the receipt list endpoint.
#[derive(Debug, Deserialize)]
pub struct ReceiptListQuery {
    /// Maximum number of receipts to return.
    pub limit: Option<usize>,
}

/// Aggregate run metrics exposed via GET /metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetrics {
    /// Total number of runs tracked.
    pub total_runs: usize,
    /// Number of currently running tasks.
    pub running: usize,
    /// Number of completed runs.
    pub completed: usize,
    /// Number of failed runs.
    pub failed: usize,
}

/// An API error with HTTP status code and message.
#[derive(Debug)]
pub struct ApiError {
    /// HTTP status code.
    pub status: StatusCode,
    /// Human-readable error message.
    pub message: String,
}

impl ApiError {
    /// Create a new `ApiError` with the given status and message.
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status.as_u16(), self.message)
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}

/// Build the Axum router with all daemon routes.
pub fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(cmd_health))
        .route("/metrics", get(cmd_metrics))
        .route("/backends", get(cmd_backends))
        .route("/capabilities", get(cmd_capabilities))
        .route("/config", get(cmd_config))
        .route("/validate", post(cmd_validate))
        .route("/schema/{schema_type}", get(cmd_schema))
        .route("/run", post(cmd_run))
        .route("/runs", get(cmd_list_runs).post(cmd_run))
        .route("/runs/{run_id}", get(cmd_get_run).delete(cmd_delete_run))
        .route("/runs/{run_id}/receipt", get(cmd_get_run_receipt))
        .route("/receipts", get(cmd_list_receipts))
        .route("/receipts/{run_id}", get(cmd_get_receipt))
        .route("/runs/{run_id}/events", get(cmd_run_events))
        .route("/ws", get(cmd_ws))
        .with_state(state)
}

async fn cmd_health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "contract_version": abp_core::CONTRACT_VERSION,
        "time": Utc::now().to_rfc3339(),
    }))
}

async fn cmd_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let runs = state.run_tracker.list_runs().await;
    let mut running = 0usize;
    let mut completed = 0usize;
    let mut failed = 0usize;
    for (_, status) in &runs {
        match status {
            RunStatus::Pending | RunStatus::Running => running += 1,
            RunStatus::Completed { .. } => completed += 1,
            RunStatus::Failed { .. } => failed += 1,
        }
    }
    // Include receipts that may not be in the tracker (legacy runs).
    let receipt_count = state.receipts.read().await.len();
    if receipt_count > completed {
        completed = receipt_count;
    }
    let total = running + completed + failed;
    Json(RunMetrics {
        total_runs: total,
        running,
        completed,
        failed,
    })
}

async fn cmd_backends(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(state.runtime.backend_names())
}

async fn cmd_capabilities(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<BackendInfo>>, ApiError> {
    if let Some(backend_name) = params.get("backend") {
        let backend = state
            .runtime
            .backend(backend_name)
            .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown backend"))?;

        return Ok(Json(vec![BackendInfo {
            id: backend_name.to_string(),
            capabilities: backend.capabilities(),
        }]));
    }

    let mut out = Vec::new();
    for name in state.runtime.backend_names() {
        if let Some(backend) = state.runtime.backend(&name) {
            out.push(BackendInfo {
                id: name,
                capabilities: backend.capabilities(),
            });
        }
    }

    Ok(Json(out))
}

async fn cmd_run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    // Validate the work order before processing.
    if let Err(errors) = validation::RequestValidator::validate_work_order(&req.work_order) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, errors.join("; ")));
    }

    let backend_names = state.runtime.backend_names();
    if let Err(e) =
        validation::RequestValidator::validate_backend_name(&req.backend, &backend_names)
    {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, e));
    }

    let run_id = req.work_order.id;

    // Track the run as running (ignore duplicate-id errors for passthrough
    // compatibility with the existing receipt-only flow).
    let _ = state.run_tracker.start_run(run_id).await;

    let handle = match state
        .runtime
        .run_streaming(&req.backend, req.work_order)
        .await
    {
        Ok(h) => h,
        Err(e) => {
            let _ = state.run_tracker.fail_run(run_id, e.to_string()).await;
            return Err(ApiError::new(StatusCode::BAD_REQUEST, e.to_string()));
        }
    };

    let mut events: Vec<AgentEvent> = Vec::new();
    let mut event_stream = handle.events;
    while let Some(event) = event_stream.next().await {
        events.push(event);
    }

    let receipt = match handle.receipt.await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            let _ = state.run_tracker.fail_run(run_id, e.to_string()).await;
            return Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            ));
        }
        Err(e) => {
            let _ = state.run_tracker.fail_run(run_id, e.to_string()).await;
            return Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            ));
        }
    };

    // Mark completed in tracker.
    let _ = state
        .run_tracker
        .complete_run(receipt.meta.run_id, receipt.clone())
        .await;

    {
        let mut guard = state.receipts.write().await;
        guard.insert(receipt.meta.run_id, receipt.clone());
    }
    persist_receipt(&state.receipts_dir, &receipt)
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!(run_id = %receipt.meta.run_id, backend = %req.backend, "run complete");

    Ok(Json(RunResponse {
        run_id: receipt.meta.run_id,
        backend: req.backend,
        events,
        receipt,
    }))
}

async fn cmd_list_runs(State(state): State<Arc<AppState>>) -> Json<Vec<Uuid>> {
    // Merge tracker runs with legacy receipt-only runs for backward compat.
    let mut ids: Vec<Uuid> = state.receipts.read().await.keys().cloned().collect();
    for (id, _) in state.run_tracker.list_runs().await {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    Json(ids)
}

async fn cmd_get_run(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Prefer tracker status when available.
    if let Some(status) = state.run_tracker.get_run_status(run_id).await {
        return Ok(Json(json!({
            "run_id": run_id,
            "status": status,
        })));
    }

    // Fall back to receipt-only lookup for backward compat.
    let guard = state.receipts.read().await;
    let receipt = guard
        .get(&run_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "run not found"))?;
    Ok(Json(json!({
        "run_id": run_id,
        "status": "completed",
        "receipt": receipt,
    })))
}

async fn cmd_delete_run(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match state.run_tracker.remove_run(run_id).await {
        Ok(_status) => {
            // Also remove from receipts cache if present.
            state.receipts.write().await.remove(&run_id);
            Ok(Json(json!({ "deleted": run_id })))
        }
        Err("conflict") => Err(ApiError::new(StatusCode::CONFLICT, "run is still active")),
        Err(_) => {
            // Fall back: if a receipt exists (legacy run not in tracker), allow deletion.
            if state.receipts.write().await.remove(&run_id).is_some() {
                Ok(Json(json!({ "deleted": run_id })))
            } else {
                Err(ApiError::new(StatusCode::NOT_FOUND, "run not found"))
            }
        }
    }
}

async fn cmd_get_run_receipt(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Receipt>, ApiError> {
    // Check tracker first.
    if let Some(status) = state.run_tracker.get_run_status(run_id).await {
        if let RunStatus::Completed { receipt } = status {
            return Ok(Json(*receipt));
        }
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "run has no receipt yet",
        ));
    }
    // Fall back to receipts map for backward compat.
    if let Some(receipt) = state.receipts.read().await.get(&run_id).cloned() {
        return Ok(Json(receipt));
    }
    Err(ApiError::new(StatusCode::NOT_FOUND, "run not found"))
}

async fn cmd_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({
        "backends": state.runtime.backend_names(),
        "contract_version": abp_core::CONTRACT_VERSION,
        "receipts_dir": state.receipts_dir.display().to_string(),
    }))
}

async fn cmd_validate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RunRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Validate the work order fields.
    if let Err(errors) = validation::RequestValidator::validate_work_order(&req.work_order) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, errors.join("; ")));
    }

    // Check that the requested backend exists.
    let backend_names = state.runtime.backend_names();
    if let Err(e) =
        validation::RequestValidator::validate_backend_name(&req.backend, &backend_names)
    {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, e));
    }

    Ok(Json(json!({
        "valid": true,
        "backend": req.backend,
        "work_order_id": req.work_order.id,
    })))
}

async fn cmd_schema(
    AxPath(schema_type): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let schema = match schema_type.as_str() {
        "work_order" => schemars::schema_for!(WorkOrder),
        "receipt" => schemars::schema_for!(Receipt),
        _ => {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                format!("unknown schema type: {schema_type}"),
            ));
        }
    };
    let value = serde_json::to_value(&schema)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(value))
}

async fn cmd_run_events(
    AxPath(_run_id): AxPath<Uuid>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let stream = tokio_stream::iter(vec![Ok(SseEvent::default().data("ping"))]);
    Sse::new(stream)
}

async fn cmd_ws(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_ws)
}

#[allow(clippy::collapsible_match)]
async fn handle_ws(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                if socket.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn cmd_list_receipts(
    Query(q): Query<ReceiptListQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let mut out = state
        .receipts
        .read()
        .await
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    out.sort_unstable();

    if let Some(limit) = q.limit {
        if limit == 0 {
            return Ok(Json(vec![]));
        }
        if out.len() > limit {
            out.truncate(limit);
        }
    }

    Ok(Json(out))
}

async fn cmd_get_receipt(
    AxPath(run_id): AxPath<Uuid>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Receipt>, ApiError> {
    if let Some(receipt) = state.receipts.read().await.get(&run_id).cloned() {
        return Ok(Json(receipt));
    }

    let path = receipt_path(&state.receipts_dir, run_id);
    let raw = fs::read(&path)
        .await
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "receipt not found"))?;
    let receipt: Receipt = serde_json::from_slice(&raw)
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "receipt decode failed"))?;

    {
        let mut guard = state.receipts.write().await;
        guard.insert(run_id, receipt.clone());
    }

    Ok(Json(receipt))
}

/// Load all receipt JSON files from `dir` into the in-memory cache.
pub async fn hydrate_receipts_from_disk(
    receipts: &Arc<RwLock<HashMap<Uuid, Receipt>>>,
    dir: &Path,
) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(dir)
        .await
        .with_context(|| "read receipts dir")?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .with_context(|| "iterate receipts dir")?
    {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let bytes = match fs::read(&p).await {
            Ok(b) => b,
            Err(err) => {
                error!(path = %p.display(), error = ?err, "failed to read receipt file");
                continue;
            }
        };

        let receipt: Receipt = match serde_json::from_slice(&bytes) {
            Ok(r) => r,
            Err(err) => {
                error!(path = %p.display(), error = ?err, "failed to parse receipt file");
                continue;
            }
        };

        let mut guard = receipts.write().await;
        guard.insert(receipt.meta.run_id, receipt);
    }
    Ok(())
}

use anyhow::Context as _;

fn receipt_path(root: &Path, run_id: Uuid) -> PathBuf {
    let mut p = root.to_path_buf();
    p.push(format!("{run_id}.json"));
    p
}

/// Persist a receipt to disk as pretty-printed JSON.
pub async fn persist_receipt(root: &Path, receipt: &Receipt) -> anyhow::Result<()> {
    let path = receipt_path(root, receipt.meta.run_id);
    let bytes = serde_json::to_vec_pretty(receipt)?;
    fs::write(path, bytes).await.context("write receipt")
}
