#![deny(unsafe_code)]
use abp_daemon::{build_app, hydrate_receipts_from_disk, AppState};
use abp_codex_sdk as codex_sdk;
use abp_claude_sdk as claude_sdk;
use abp_gemini_sdk as gemini_sdk;
use abp_host::SidecarSpec;
use abp_integrations::{MockBackend, SidecarBackend};
use abp_kimi_sdk as kimi_sdk;
use abp_runtime::Runtime;
use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::info;
use tracing_subscriber::EnvFilter;

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

    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .with_context(|| format!("bind {}", args.bind))?;
    info!(
        bind = %args.bind,
        hosts = %args.host_root.display(),
        "abp-daemon listening"
    );

    axum::serve(listener, app)
        .await
        .context("serve")
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
        if which("python3").is_some() { "python3" } else { "python" },
        &host_root.join("hosts/python/host.py"),
    )?;

    if !claude_sdk::register_default(&mut runtime, host_root, None)? {
        // Silently skip if Claude host/Node is unavailable in this environment.
        // Keep startup resilient for deployments that omit that optional backend.
    }

    register_sidecar_backend(
        &mut runtime,
        "sidecar:copilot",
        "node",
        &host_root.join("hosts/copilot/host.js"),
    )?;

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


