use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_codex_sdk as codex_sdk;
use abp_claude_sdk as claude_sdk;
use abp_gemini_sdk as gemini_sdk;
use abp_host::SidecarSpec;
use abp_integrations::SidecarBackend;
use abp_runtime::Runtime;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio_stream::StreamExt;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "abp", version, about = "Agent Backplane CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable debug logging.
    #[arg(long)]
    debug: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List available backends.
    Backends,

    /// Run a work order.
    Run {
        /// Backend name: mock | sidecar:node | sidecar:python | sidecar:claude | sidecar:copilot | sidecar:kimi | sidecar:gemini | sidecar:codex
        #[arg(long, default_value = "mock")]
        backend: String,

        /// Task to execute.
        #[arg(long)]
        task: String,

        /// Workspace root.
        #[arg(long, default_value = ".")]
        root: String,

        #[arg(long, value_enum, default_value_t = WorkspaceModeArg::Staged)]
        workspace_mode: WorkspaceModeArg,

        #[arg(long, value_enum, default_value_t = LaneArg::PatchFirst)]
        lane: LaneArg,

        /// Include glob(s) (relative to root). Can be repeated.
        #[arg(long)]
        include: Vec<String>,

        /// Exclude glob(s) (relative to root). Can be repeated.
        #[arg(long)]
        exclude: Vec<String>,

        /// Where to write the receipt (defaults to .agent-backplane/receipts/<run_id>.json).
        #[arg(long)]
        out: Option<PathBuf>,

        /// Print JSON instead of pretty output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum WorkspaceModeArg {
    PassThrough,
    Staged,
}

impl From<WorkspaceModeArg> for WorkspaceMode {
    fn from(v: WorkspaceModeArg) -> Self {
        match v {
            WorkspaceModeArg::PassThrough => WorkspaceMode::PassThrough,
            WorkspaceModeArg::Staged => WorkspaceMode::Staged,
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum LaneArg {
    PatchFirst,
    WorkspaceFirst,
}

impl From<LaneArg> for ExecutionLane {
    fn from(v: LaneArg) -> Self {
        match v {
            LaneArg::PatchFirst => ExecutionLane::PatchFirst,
            LaneArg::WorkspaceFirst => ExecutionLane::WorkspaceFirst,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.debug {
        EnvFilter::new("abp=debug,abp.sidecar=debug")
    } else {
        EnvFilter::new("abp=info")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    match cli.command {
        Commands::Backends => cmd_backends().await,
        Commands::Run {
            backend,
            task,
            root,
            workspace_mode,
            lane,
            include,
            exclude,
            out,
            json,
        } => {
            cmd_run(
                backend,
                task,
                root,
                workspace_mode,
                lane,
                include,
                exclude,
                out,
                json,
            )
            .await
        }
    }
}

async fn cmd_backends() -> Result<()> {
    let rt = Runtime::with_default_backends();
    for b in rt.backend_names() {
        println!("{b}");
    }
    println!("sidecar:node");
    println!("sidecar:python");
    println!("sidecar:claude");
    println!("sidecar:copilot");
    println!("sidecar:kimi");
    println!("sidecar:gemini");
    println!("sidecar:codex");
    Ok(())
}

async fn cmd_run(
    backend: String,
    task: String,
    root: String,
    workspace_mode: WorkspaceModeArg,
    lane: LaneArg,
    include: Vec<String>,
    exclude: Vec<String>,
    out: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let mut rt = Runtime::with_default_backends();

    // Register built-in sidecars.
    if backend == "sidecar:node" {
        // These example sidecars are checked in under `hosts/` and are meant for local dev.
        // If you're running `abp` outside the repo root, pass a real backend config instead.
        let script = PathBuf::from("hosts/node/host.js");
        if !script.is_file() {
            anyhow::bail!(
                "node sidecar host script not found at {} (run from repo root)",
                script.display()
            );
        }

        let mut spec = SidecarSpec::new("node");
        spec.args = vec![script.to_string_lossy().into_owned()];
        rt.register_backend("sidecar:node", SidecarBackend::new(spec));
    }
    if backend == "sidecar:python" {
        // Prefer python3, but fall back to python.
        let cmd = if which("python3").is_some() {
            "python3"
        } else {
            "python"
        };

        // These example sidecars are checked in under `hosts/` and are meant for local dev.
        let script = PathBuf::from("hosts/python/host.py");
        if !script.is_file() {
            anyhow::bail!(
                "python sidecar host script not found at {} (run from repo root)",
                script.display()
            );
        }

        let mut spec = SidecarSpec::new(cmd);
        spec.args = vec![script.to_string_lossy().into_owned()];
        rt.register_backend("sidecar:python", SidecarBackend::new(spec));
    }
    if backend == claude_sdk::BACKEND_NAME {
        if !claude_sdk::register_default(&mut rt, &PathBuf::from("."), None)? {
            anyhow::bail!(
                "claude sidecar not available at {} (node not found or script missing)",
                claude_sdk::sidecar_script(&PathBuf::from(".")).display()
            );
        }
    }
    if backend == "sidecar:copilot" {
        let cmd = if which("node").is_some() {
            "node"
        } else {
            anyhow::bail!("node executable not found in PATH");
        };

        let script = PathBuf::from("hosts/copilot/host.js");
        if !script.is_file() {
            anyhow::bail!(
                "copilot sidecar host script not found at {} (run from repo root)",
                script.display()
            );
        }

        let mut spec = SidecarSpec::new(cmd);
        spec.args = vec![script.to_string_lossy().into_owned()];
        rt.register_backend("sidecar:copilot", SidecarBackend::new(spec));
    }
    if backend == codex_sdk::BACKEND_NAME {
        if !codex_sdk::register_default(&mut rt, &PathBuf::from("."), None)? {
            anyhow::bail!(
                "codex sidecar not available at {} (node not found or script missing)",
                codex_sdk::sidecar_script(&PathBuf::from(".")).display()
            );
        }
    }
    if backend == gemini_sdk::BACKEND_NAME {
        if !gemini_sdk::register_default(&mut rt, &PathBuf::from("."), None)? {
            anyhow::bail!(
                "gemini sidecar not available at {} (node not found or script missing)",
                gemini_sdk::sidecar_script(&PathBuf::from(".")).display()
            );
        }
    }
    if backend == "sidecar:kimi" {
        let cmd = if which("node").is_some() {
            "node"
        } else {
            anyhow::bail!("node executable not found in PATH");
        };

        let script = PathBuf::from("hosts/kimi/host.js");
        if !script.is_file() {
            anyhow::bail!(
                "kimi sidecar host script not found at {} (run from repo root)",
                script.display()
            );
        }

        let mut spec = SidecarSpec::new(cmd);
        spec.args = vec![script.to_string_lossy().into_owned()];
        rt.register_backend("sidecar:kimi", SidecarBackend::new(spec));
    }

    let work_order_id = Uuid::new_v4();
    let wo = WorkOrder {
        id: work_order_id,
        task,
        lane: lane.into(),
        workspace: WorkspaceSpec {
            root,
            mode: workspace_mode.into(),
            include,
            exclude,
        },
        context: ContextPacket::default(),
        policy: default_policy(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: None,
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        },
    };

    let handle = rt
        .run_streaming(&backend, wo)
        .await
        .with_context(|| format!("run backend={backend}"))?;

    let run_id = handle.run_id;

    if !json {
        eprintln!("run_id: {run_id}");
        eprintln!("backend: {backend}");
        eprintln!("---");
    }

    let mut events = handle.events;
    while let Some(ev) = events.next().await {
        if json {
            println!("{}", serde_json::to_string(&ev)?);
        } else {
            print_event(&ev);
        }
    }

    let receipt = handle.receipt.await.context("join receipt task")??;

    let out_path = out.unwrap_or_else(|| {
        let mut p = PathBuf::from(".agent-backplane/receipts");
        p.push(format!("{run_id}.json"));
        p
    });

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create receipt directory {}", parent.display()))?;
    }

    std::fs::write(&out_path, serde_json::to_string_pretty(&receipt)?)
        .with_context(|| format!("write receipt to {}", out_path.display()))?;

    if !json {
        eprintln!("---");
        eprintln!("receipt: {}", out_path.display());
        eprintln!(
            "sha256: {}",
            receipt.receipt_sha256.clone().unwrap_or_default()
        );
    }

    Ok(())
}

fn print_event(ev: &abp_core::AgentEvent) {
    use abp_core::AgentEventKind::*;
    match &ev.kind {
        RunStarted { message } => eprintln!("[start] {message}"),
        RunCompleted { message } => eprintln!("[done] {message}"),

        AssistantDelta { text } => {
            print!("{text}");
        }
        AssistantMessage { text } => {
            println!("{text}");
        }

        ToolCall {
            tool_name,
            tool_use_id,
            ..
        } => {
            eprintln!("[tool] call {tool_name} id={:?}", tool_use_id);
        }
        ToolResult {
            tool_name,
            tool_use_id,
            is_error,
            ..
        } => {
            eprintln!(
                "[tool] result {tool_name} id={:?} error={is_error}",
                tool_use_id
            );
        }

        FileChanged { path, summary } => {
            eprintln!("[file] {path} :: {summary}");
        }

        CommandExecuted {
            command, exit_code, ..
        } => {
            eprintln!("[bash] {:?} => {:?}", command, exit_code);
        }

        Warning { message } => eprintln!("[warn] {message}"),
        Error { message } => eprintln!("[error] {message}"),
    }
}

fn default_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec!["KillBash".into(), "NotebookEdit".into(), "mcp__*__*".into()],
        deny_read: vec![
            "**/.env".into(),
            "**/.env.*".into(),
            "**/.git/**".into(),
            "**/id_rsa".into(),
            "**/id_ed25519".into(),
        ],
        deny_write: vec!["**/.git/**".into()],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec!["Bash".into(), "Write".into(), "Edit".into()],
    }
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
