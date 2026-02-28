use abp_claude_sdk as claude_sdk;
use abp_codex_sdk as codex_sdk;
use abp_copilot_sdk as copilot_sdk;
use abp_core::{AgentEvent, CapabilityManifest, Receipt, WorkOrder};
use abp_gemini_sdk as gemini_sdk;
use abp_host::SidecarSpec;
use abp_integrations::{MockBackend, SidecarBackend};
use abp_kimi_sdk as kimi_sdk;
use abp_runtime::Runtime;
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path as AxPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "abp-daemon", version, about = "Agent Backplane daemon")]
struct Args {
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1:8088")]
    bind: String,

    /// Root folder containing the sidecar hosts under hosts/*/host.js|host.py.
    #[arg(long, default_value = ".")]
    host_root: PathBuf,

    /// Directory to persist receipts.
    #[arg(long, default_value = ".agent-backplane/receipts")]
    receipts_dir: PathBuf,

    /// Enable request/response debug logging.
    #[arg(long)]
    debug: bool,
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<Runtime>,
    receipts: Arc<RwLock<HashMap<Uuid, Receipt>>>,
    receipts_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RunRequest {
    backend: String,
    work_order: WorkOrder,
}

#[derive(Debug, Serialize)]
struct RunResponse {
    run_id: Uuid,
    backend: String,
    events: Vec<AgentEvent>,
    receipt: Receipt,
}

#[derive(Debug, Serialize)]
struct BackendInfo {
    id: String,
    capabilities: CapabilityManifest,
}

#[derive(Debug, Deserialize)]
struct ReceiptListQuery {
    limit: Option<usize>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let filter = if args.debug {
        EnvFilter::new("abp=debug,abp.runtime=debug,abp.daemon=info")
    } else {
        EnvFilter::new("abp=info")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    fs::create_dir_all(&args.receipts_dir)
        .await
        .with_context(|| format!("create receipts dir {}", args.receipts_dir.display()))?;

    let runtime = Arc::new(build_runtime(&args.host_root)?);
    let state = Arc::new(AppState {
        runtime,
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: args.receipts_dir.clone(),
    });

    // Warm cache with any existing receipt files to support immediate GET /receipts/:id.
    hydrate_receipts_from_disk(&state.receipts, &state.receipts_dir).await?;

    let app = Router::new()
        .route("/health", get(cmd_health))
        .route("/backends", get(cmd_backends))
        .route("/capabilities", get(cmd_capabilities))
        .route("/run", post(cmd_run))
        .route("/receipts", get(cmd_list_receipts))
        .route("/receipts/:run_id", get(cmd_get_receipt))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .with_context(|| format!("bind {}", args.bind))?;
    info!(
        bind = %args.bind,
        hosts = %args.host_root.display(),
        "abp-daemon listening"
    );

    axum::serve(listener, app).await.context("serve")
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

    // Keep an in-memory copy and persist to disk for replay/debug.
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

fn build_runtime(host_root: &Path) -> Result<Runtime> {
    let mut runtime = Runtime::new();
    runtime.register_backend("mock", MockBackend);

    register_sidecar_backend(
        &mut runtime,
        "sidecar:node",
        "node",
        &host_root.join("hosts/node/host.js"),
    )?;

    register_sidecar_backend(
        &mut runtime,
        "sidecar:python",
        if which("python3").is_some() {
            "python3"
        } else {
            "python"
        },
        &host_root.join("hosts/python/host.py"),
    )?;

    if !claude_sdk::register_default(&mut runtime, host_root, None)? {
        // Silently skip if Claude host/Node is unavailable in this environment.
        // Keep startup resilient for deployments that omit that optional backend.
    }

    copilot_sdk::register_default(&mut runtime, host_root, None)?;

    kimi_sdk::register_default(&mut runtime, host_root, None)?;

    gemini_sdk::register_default(&mut runtime, host_root, None)?;

    codex_sdk::register_default(&mut runtime, host_root, None)?;

    Ok(runtime)
}

fn register_sidecar_backend(
    runtime: &mut Runtime,
    name: &str,
    command: &str,
    script: &Path,
) -> Result<()> {
    if which(command).is_none() {
        return Ok(());
    }
    if !script.is_file() {
        return Ok(());
    }

    let mut spec = SidecarSpec::new(command);
    spec.args = vec![script.to_string_lossy().to_string()];
    runtime.register_backend(name, SidecarBackend::new(spec));
    Ok(())
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for p in std::env::split_paths(&path) {
        let candidate = p.join(bin);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

async fn hydrate_receipts_from_disk(
    receipts: &Arc<RwLock<HashMap<Uuid, Receipt>>>,
    dir: &Path,
) -> Result<()> {
    let mut entries = fs::read_dir(dir).await.context("read receipts dir")?;
    while let Some(entry) = entries.next_entry().await.context("iterate receipts dir")? {
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

fn receipt_path(root: &Path, run_id: Uuid) -> PathBuf {
    let mut p = root.to_path_buf();
    p.push(format!("{run_id}.json"));
    p
}

async fn persist_receipt(root: &Path, receipt: &Receipt) -> Result<()> {
    let path = receipt_path(root, receipt.meta.run_id);
    let bytes = serde_json::to_vec_pretty(receipt)?;
    fs::write(path, bytes).await.context("write receipt")
}
