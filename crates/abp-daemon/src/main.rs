// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
use abp_claude_sdk as claude_sdk;
use abp_codex_sdk as codex_sdk;
use abp_copilot_sdk as copilot_sdk;
use abp_daemon::{AppState, RunTracker, build_app, hydrate_receipts_from_disk};
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
    #[arg(long)]
    receipts_dir: Option<PathBuf>,

    /// Enable request/response debug logging.
    #[arg(long)]
    debug: bool,

    /// Path to a TOML configuration file.
    ///
    /// Falls back to `backplane.toml` in the current directory if present.
    #[arg(long)]
    config: Option<PathBuf>,
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

    // Load configuration from --config path, backplane.toml fallback, or defaults.
    let config_path = args.config.clone().or_else(|| {
        let p = PathBuf::from("backplane.toml");
        if p.exists() { Some(p) } else { None }
    });
    let config = abp_config::load_config(config_path.as_deref()).unwrap_or_else(|e| {
        tracing::warn!("failed to load config: {e}");
        abp_config::BackplaneConfig::default()
    });
    if let Ok(warnings) = abp_config::validate_config(&config) {
        for w in &warnings {
            tracing::debug!("config: {w}");
        }
    }

    // Resolve receipts_dir: --receipts-dir flag > config receipts_dir > default.
    let receipts_dir = args
        .receipts_dir
        .or_else(|| config.receipts_dir.as_ref().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(".agent-backplane/receipts"));

    fs::create_dir_all(&receipts_dir)
        .await
        .with_context(|| format!("create receipts dir {}", receipts_dir.display()))?;

    let runtime = Arc::new(build_runtime(&args.host_root, &config)?);
    let state = Arc::new(AppState {
        runtime,
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: receipts_dir.clone(),
        run_tracker: RunTracker::new(),
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

    axum::serve(listener, app).await.context("serve")
}

fn build_runtime(host_root: &Path, config: &abp_config::BackplaneConfig) -> Result<Runtime> {
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

    // Register backends from loaded configuration.
    for (name, entry) in &config.backends {
        match entry {
            abp_config::BackendEntry::Mock {} => {
                runtime.register_backend(name, MockBackend);
            }
            abp_config::BackendEntry::Sidecar { command, args, .. } => {
                let mut spec = SidecarSpec::new(command);
                spec.args = args.clone();
                runtime.register_backend(name, SidecarBackend::new(spec));
            }
        }
    }

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
