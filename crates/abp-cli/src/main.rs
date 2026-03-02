// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
use abp_claude_sdk as claude_sdk;
use abp_cli::commands::{self, SchemaKind};
use abp_cli::config::BackendConfig;
use abp_cli_params::{insert_vendor_path, parse_key_value_flag, parse_param_value};
use abp_codex_sdk as codex_sdk;
use abp_copilot_sdk as copilot_sdk;
use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_gemini_sdk as gemini_sdk;
use abp_host::SidecarSpec;
use abp_integrations::SidecarBackend;
use abp_kimi_sdk as kimi_sdk;
use abp_runtime::Runtime;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio_stream::StreamExt;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

/// Exit code for runtime errors.
const EXIT_RUNTIME_ERROR: i32 = 1;
/// Exit code for usage / argument errors (clap exits with 2 automatically).
#[allow(dead_code)]
const EXIT_USAGE_ERROR: i32 = 2;

#[derive(Parser, Debug)]
#[command(name = "abp", version, about = "Agent Backplane CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable debug logging.
    #[arg(long)]
    debug: bool,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
enum Commands {
    /// List available backends.
    Backends,

    /// Run a work order.
    Run {
        /// Backend name: mock | sidecar:node | sidecar:python | sidecar:claude | sidecar:copilot | sidecar:kimi | sidecar:gemini | sidecar:codex.
        /// Aliases are also supported: node, python, claude, copilot, kimi, gemini, codex.
        #[arg(long, default_value = "mock")]
        backend: String,

        /// Task to execute.
        #[arg(long)]
        task: String,

        /// Preferred model (sets work_order.config.model).
        #[arg(long)]
        model: Option<String>,

        /// Workspace root.
        #[arg(long, default_value = ".")]
        root: String,

        /// Workspace mode (pass-through or staged).
        #[arg(long, value_enum, default_value_t = WorkspaceModeArg::Staged)]
        workspace_mode: WorkspaceModeArg,

        /// Execution lane.
        #[arg(long, value_enum, default_value_t = LaneArg::PatchFirst)]
        lane: LaneArg,

        /// Include glob(s) (relative to root). Can be repeated.
        #[arg(long)]
        include: Vec<String>,

        /// Exclude glob(s) (relative to root). Can be repeated.
        #[arg(long)]
        exclude: Vec<String>,

        /// Vendor params as key=value. Repeated values are merged.
        ///
        /// Examples:
        /// --param model=gemini-2.5-flash
        /// --param abp.mode=passthrough
        /// --param stream=true
        #[arg(long = "param")]
        params: Vec<String>,

        /// Environment variables passed through to the runtime as KEY=VALUE.
        #[arg(long = "env")]
        env_vars: Vec<String>,

        /// Optional hard cap on run budget in USD (best-effort).
        #[arg(long)]
        max_budget_usd: Option<f64>,

        /// Optional hard cap on run turns/iterations (best-effort).
        #[arg(long)]
        max_turns: Option<u32>,

        /// Where to write the receipt (defaults to .agent-backplane/receipts/<run_id>.json).
        #[arg(long)]
        out: Option<PathBuf>,

        /// Print JSON instead of pretty output.
        #[arg(long)]
        json: bool,

        /// Path to a policy profile JSON file to load.
        #[arg(long)]
        policy: Option<PathBuf>,

        /// Write the receipt to this file path.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Write streamed events as JSONL to this file.
        #[arg(long)]
        events: Option<PathBuf>,
    },

    /// Validate a work order JSON file against the schema.
    Validate {
        /// Path to the work order JSON file.
        #[arg()]
        file: PathBuf,
    },

    /// Print a JSON schema to stdout.
    Schema {
        /// Which schema to print.
        #[arg(value_enum)]
        kind: SchemaArg,
    },

    /// Inspect a receipt file and verify its hash.
    Inspect {
        /// Path to the receipt JSON file.
        #[arg()]
        file: PathBuf,
    },
}

/// Schema kind argument for the `schema` subcommand.
#[derive(Debug, Clone, ValueEnum)]
enum SchemaArg {
    /// WorkOrder schema.
    WorkOrder,
    /// Receipt schema.
    Receipt,
    /// BackplaneConfig schema.
    Config,
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
async fn main() {
    let cli = Cli::parse();

    let filter = if cli.debug {
        EnvFilter::new("abp=debug,abp.sidecar=debug")
    } else {
        EnvFilter::new("abp=info")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    let result = match cli.command {
        Commands::Backends => cmd_backends().await,
        Commands::Validate { file } => cmd_validate(&file),
        Commands::Schema { kind } => cmd_schema(kind),
        Commands::Inspect { file } => cmd_inspect(&file),
        Commands::Run {
            backend,
            task,
            model,
            root,
            workspace_mode,
            lane,
            include,
            exclude,
            params,
            env_vars,
            max_budget_usd,
            max_turns,
            out,
            json,
            policy,
            output,
            events,
        } => {
            cmd_run(
                backend,
                task,
                model,
                root,
                workspace_mode,
                lane,
                include,
                exclude,
                params,
                env_vars,
                max_budget_usd,
                max_turns,
                out,
                json,
                policy,
                output,
                events,
            )
            .await
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(EXIT_RUNTIME_ERROR);
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
    println!("node");
    println!("python");
    println!("claude");
    println!("copilot");
    println!("kimi");
    println!("gemini");
    println!("codex");
    Ok(())
}

fn cmd_validate(file: &std::path::Path) -> Result<()> {
    commands::validate_work_order_file(file)?;
    println!("valid");
    Ok(())
}

fn cmd_schema(kind: SchemaArg) -> Result<()> {
    let sk = match kind {
        SchemaArg::WorkOrder => SchemaKind::WorkOrder,
        SchemaArg::Receipt => SchemaKind::Receipt,
        SchemaArg::Config => SchemaKind::Config,
    };
    let json = commands::schema_json(sk)?;
    println!("{json}");
    Ok(())
}

fn cmd_inspect(file: &std::path::Path) -> Result<()> {
    let (receipt, valid) = commands::inspect_receipt_file(file)?;
    println!("outcome: {}", serde_json::to_value(&receipt.outcome)?);
    println!("backend: {}", receipt.backend.id);
    println!("run_id:  {}", receipt.meta.run_id);
    println!(
        "sha256:  {}",
        receipt.receipt_sha256.as_deref().unwrap_or("<none>")
    );
    if valid {
        println!("hash:    VALID");
    } else {
        println!("hash:    INVALID");
        std::process::exit(EXIT_RUNTIME_ERROR);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_run(
    backend: String,
    task: String,
    model: Option<String>,
    root: String,
    workspace_mode: WorkspaceModeArg,
    lane: LaneArg,
    include: Vec<String>,
    exclude: Vec<String>,
    params: Vec<String>,
    env_vars: Vec<String>,
    max_budget_usd: Option<f64>,
    max_turns: Option<u32>,
    out: Option<PathBuf>,
    json: bool,
    policy_path: Option<PathBuf>,
    output: Option<PathBuf>,
    events_path: Option<PathBuf>,
) -> Result<()> {
    let backend = normalize_backend_name(&backend);
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
    if backend == claude_sdk::BACKEND_NAME
        && !claude_sdk::register_default(&mut rt, &PathBuf::from("."), None)?
    {
        anyhow::bail!(
            "claude sidecar not available at {} (node not found or script missing)",
            claude_sdk::sidecar_script(&PathBuf::from(".")).display()
        );
    }
    if backend == copilot_sdk::BACKEND_NAME
        && !copilot_sdk::register_default(&mut rt, &PathBuf::from("."), None)?
    {
        anyhow::bail!(
            "copilot sidecar not available at {} (node not found or script missing)",
            copilot_sdk::sidecar_script(&PathBuf::from(".")).display()
        );
    }
    if backend == kimi_sdk::BACKEND_NAME
        && !kimi_sdk::register_default(&mut rt, &PathBuf::from("."), None)?
    {
        anyhow::bail!(
            "kimi sidecar not available at {} (node not found or script missing)",
            kimi_sdk::sidecar_script(&PathBuf::from(".")).display()
        );
    }
    if backend == codex_sdk::BACKEND_NAME
        && !codex_sdk::register_default(&mut rt, &PathBuf::from("."), None)?
    {
        anyhow::bail!(
            "codex sidecar not available at {} (node not found or script missing)",
            codex_sdk::sidecar_script(&PathBuf::from(".")).display()
        );
    }
    if backend == gemini_sdk::BACKEND_NAME
        && !gemini_sdk::register_default(&mut rt, &PathBuf::from("."), None)?
    {
        anyhow::bail!(
            "gemini sidecar not available at {} (node not found or script missing)",
            gemini_sdk::sidecar_script(&PathBuf::from(".")).display()
        );
    }

    // Register backends from backplane.toml (if present).
    let config_path = std::path::Path::new("backplane.toml");
    if config_path.exists() {
        let cfg = abp_cli::config::load_config(Some(config_path))?;
        if let Err(errors) = abp_cli::config::validate_config(&cfg) {
            for e in &errors {
                tracing::warn!("config: {e}");
            }
        }
        for (name, bc) in cfg.backends {
            match bc {
                BackendConfig::Mock {} => {
                    rt.register_backend(&name, abp_integrations::MockBackend);
                }
                BackendConfig::Sidecar { command, args, .. } => {
                    let mut spec = SidecarSpec::new(&command);
                    spec.args = args;
                    rt.register_backend(&name, SidecarBackend::new(spec));
                }
            }
        }
    }

    let mut resolved_model = model;
    let mut vendor = BTreeMap::new();
    let mut env = BTreeMap::new();
    let default_vendor_namespace = backend_vendor_namespace(&backend);

    for raw in params {
        let (key, raw_value) = parse_key_value_flag(&raw, "--param")?;

        if key == "model" {
            if resolved_model.is_none() {
                resolved_model = Some(raw_value);
            }
            continue;
        }

        let value = parse_param_value(&raw_value);
        if key.contains('.') {
            insert_vendor_path(&mut vendor, &key, value);
            continue;
        }

        if let Some(namespace) = default_vendor_namespace {
            insert_vendor_path(&mut vendor, &format!("{namespace}.{key}"), value);
        } else {
            vendor.insert(key, value);
        }
    }

    for raw in env_vars {
        let (key, value) = parse_key_value_flag(&raw, "--env")?;
        env.insert(key, value);
    }

    let policy = if let Some(ref pp) = policy_path {
        let content = std::fs::read_to_string(pp)
            .with_context(|| format!("read policy file '{}'", pp.display()))?;
        serde_json::from_str::<PolicyProfile>(&content)
            .with_context(|| format!("parse policy from '{}'", pp.display()))?
    } else {
        default_policy()
    };

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
        policy,
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: resolved_model,
            vendor,
            env,
            max_budget_usd,
            max_turns,
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

    let mut events_file = match events_path {
        Some(ref ep) => {
            if let Some(parent) = ep.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create events directory {}", parent.display()))?;
            }
            Some(
                std::fs::File::create(ep)
                    .with_context(|| format!("create events file {}", ep.display()))?,
            )
        }
        None => None,
    };

    let mut events = handle.events;
    while let Some(ev) = events.next().await {
        if json {
            println!("{}", serde_json::to_string(&ev)?);
        } else {
            print_event(&ev);
        }
        if let Some(ref mut f) = events_file {
            use std::io::Write;
            writeln!(f, "{}", serde_json::to_string(&ev)?)?;
        }
    }

    let receipt = handle.receipt.await.context("join receipt task")??;

    // --output takes precedence over --out for the receipt destination.
    let effective_out = output.or(out);
    let out_path = effective_out.unwrap_or_else(|| {
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

fn backend_vendor_namespace(backend: &str) -> Option<&'static str> {
    match backend {
        "sidecar:gemini" => Some("gemini"),
        "sidecar:codex" => Some("codex"),
        "sidecar:claude" => Some("claude"),
        "sidecar:kimi" => Some("kimi"),
        "sidecar:copilot" => Some("copilot"),
        "sidecar:node" | "sidecar:python" => Some("abp"),
        _ => None,
    }
}

fn normalize_backend_name(raw: &str) -> String {
    match raw.trim() {
        "node" => "sidecar:node".to_string(),
        "python" => "sidecar:python".to_string(),
        "claude" | "sidecar:claude" => claude_sdk::BACKEND_NAME.to_string(),
        "copilot" => "sidecar:copilot".to_string(),
        "kimi" | "sidecar:kimi" => kimi_sdk::BACKEND_NAME.to_string(),
        "gemini" | "sidecar:gemini" => gemini_sdk::BACKEND_NAME.to_string(),
        "codex" | "sidecar:codex" => codex_sdk::BACKEND_NAME.to_string(),
        other => other.to_string(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use abp_cli::config::BackplaneConfig;

    #[test]
    fn normalize_backend_supports_aliases() {
        assert_eq!(normalize_backend_name("gemini"), "sidecar:gemini");
        assert_eq!(normalize_backend_name("kimi"), "sidecar:kimi");
        assert_eq!(normalize_backend_name("codex"), "sidecar:codex");
        assert_eq!(normalize_backend_name("node"), "sidecar:node");
        assert_eq!(normalize_backend_name("mock"), "mock");
    }

    #[test]
    fn backend_vendor_namespace_supports_kimi() {
        assert_eq!(backend_vendor_namespace("sidecar:kimi"), Some("kimi"));
    }

    #[test]
    fn parse_example_config() {
        let content = include_str!("../../../backplane.example.toml");
        let config: BackplaneConfig = toml::from_str(content).expect("parse example config");
        assert!(!config.backends.is_empty());
    }
}
