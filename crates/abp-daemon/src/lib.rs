// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
use abp_core::{AgentEvent, CapabilityManifest, Receipt, WorkOrder};
use abp_runtime::Runtime;
use axum::{
    Json, Router,
    extract::{Path as AxPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    response::sse::{Event as SseEvent, Sse},
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

#[derive(Clone)]
pub struct AppState {
    pub runtime: Arc<Runtime>,
    pub receipts: Arc<RwLock<HashMap<Uuid, Receipt>>>,
    pub receipts_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunRequest {
    pub backend: String,
    pub work_order: WorkOrder,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunResponse {
    pub run_id: Uuid,
    pub backend: String,
    pub events: Vec<AgentEvent>,
    pub receipt: Receipt,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackendInfo {
    pub id: String,
    pub capabilities: CapabilityManifest,
}

#[derive(Debug, Deserialize)]
pub struct ReceiptListQuery {
    pub limit: Option<usize>,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

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
        .route("/backends", get(cmd_backends))
        .route("/capabilities", get(cmd_capabilities))
        .route("/run", post(cmd_run))
        .route("/receipts", get(cmd_list_receipts))
        .route("/receipts/{run_id}", get(cmd_get_receipt))
        .route("/runs/{run_id}/events", get(cmd_run_events))
        .with_state(state)
}

async fn cmd_health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "contract_version": abp_core::CONTRACT_VERSION,
        "time": Utc::now().to_rfc3339(),
    }))
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
    let handle = state
        .runtime
        .run_streaming(&req.backend, req.work_order)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;

    let mut events: Vec<AgentEvent> = Vec::new();
    let mut event_stream = handle.events;
    while let Some(event) = event_stream.next().await {
        events.push(event);
    }

    let receipt = handle
        .receipt
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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

async fn cmd_run_events(
    AxPath(_run_id): AxPath<Uuid>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let stream = tokio_stream::iter(vec![Ok(SseEvent::default().data("ping"))]);
    Sse::new(stream)
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

pub async fn persist_receipt(root: &Path, receipt: &Receipt) -> anyhow::Result<()> {
    let path = receipt_path(root, receipt.meta.run_id);
    let bytes = serde_json::to_vec_pretty(receipt)?;
    fs::write(path, bytes).await.context("write receipt")
}
