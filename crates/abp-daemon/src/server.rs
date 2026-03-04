// SPDX-License-Identifier: MIT OR Apache-2.0
//! Axum-based HTTP server scaffolding for the ABP daemon.
//!
//! Provides [`DaemonServer`](crate::server::DaemonServer) — the top-level
//! entry point for starting the HTTP control-plane — and
//! [`router()`](crate::server::router) for building the Axum router used in
//! tests and production.

use crate::api::{
    BackendInfo as ApiBackendInfo, ErrorResponse, HealthResponse, ListBackendsResponse, RunRequest,
    RunResponse, RunStatus as ApiRunStatus,
};
use crate::state::ServerState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// VersionResponse
// ---------------------------------------------------------------------------

/// Response body for `GET /version`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResponse {
    /// Crate version (from `Cargo.toml`).
    pub version: String,
    /// ABP contract version.
    pub contract_version: String,
}

// ---------------------------------------------------------------------------
// DaemonServer
// ---------------------------------------------------------------------------

/// High-level HTTP server for the ABP daemon control-plane.
///
/// Wraps shared [`ServerState`] and provides methods for building the router
/// and starting the server.
pub struct DaemonServer {
    state: Arc<ServerState>,
}

impl DaemonServer {
    /// Create a new daemon server wrapping the given shared state.
    pub fn new(state: Arc<ServerState>) -> Self {
        Self { state }
    }

    /// Build the Axum [`Router`] for this server instance.
    pub fn router(&self) -> Router {
        router(self.state.clone())
    }

    /// Bind to `addr` (e.g. `"127.0.0.1:8088"`) and serve until shutdown.
    pub async fn start(self, addr: &str) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(bind = %addr, "DaemonServer listening");
        axum::serve(listener, self.router()).await?;
        Ok(())
    }

    /// Return a reference to the inner [`ServerState`].
    pub fn state(&self) -> &Arc<ServerState> {
        &self.state
    }
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

/// Build an Axum [`Router`] with the standard daemon endpoints.
///
/// Routes:
/// - `GET  /health`   — health check ([`HealthResponse`])
/// - `GET  /backends` — list registered backends ([`ListBackendsResponse`])
/// - `POST /run`      — submit a work order ([`RunResponse`])
/// - `GET  /version`  — version information ([`VersionResponse`])
pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/health", get(handle_health))
        .route("/backends", get(handle_backends))
        .route("/run", post(handle_run))
        .route("/version", get(handle_version))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /health` handler.
async fn handle_health(State(state): State<Arc<ServerState>>) -> Json<HealthResponse> {
    let backends = state.backends.list().await;
    Json(HealthResponse {
        status: "ok".into(),
        version: abp_core::CONTRACT_VERSION.into(),
        uptime_secs: state.uptime_secs(),
        backends,
    })
}

/// `GET /backends` handler.
async fn handle_backends(State(state): State<Arc<ServerState>>) -> Json<ListBackendsResponse> {
    let names = state.backends.list().await;
    let backends = names
        .into_iter()
        .map(|name| ApiBackendInfo {
            name,
            dialect: "unknown".into(),
            status: "available".into(),
        })
        .collect();
    Json(ListBackendsResponse { backends })
}

/// `POST /run` handler.
async fn handle_run(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<RunRequest>,
) -> Result<(StatusCode, Json<RunResponse>), (StatusCode, Json<ErrorResponse>)> {
    if req.task.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "task must not be empty".into(),
                code: Some("invalid_request".into()),
            }),
        ));
    }

    let run_id = Uuid::new_v4();
    let backend = req.backend.unwrap_or_else(|| "default".into());

    if backend != "default" && !state.backends.contains(&backend).await {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("unknown backend: {backend}"),
                code: Some("unknown_backend".into()),
            }),
        ));
    }

    let _ = state.registry.create_run(run_id, backend).await;

    Ok((
        StatusCode::CREATED,
        Json(RunResponse {
            run_id: run_id.to_string(),
            status: ApiRunStatus::Queued,
        }),
    ))
}

/// `GET /version` handler.
async fn handle_version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION").into(),
        contract_version: abp_core::CONTRACT_VERSION.into(),
    })
}
