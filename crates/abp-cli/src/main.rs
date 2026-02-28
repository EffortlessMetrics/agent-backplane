// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
use abp_claude_sdk as claude_sdk;
use abp_codex_sdk as codex_sdk;
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
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{BTreeMap, HashMap};
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
    println!("node");
    println!("python");
    println!("claude");
    println!("copilot");
    println!("kimi");
    println!("gemini");
    println!("codex");
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
    if let Some(cfg) = load_config() {
        for (name, bc) in cfg.backends {
            match bc {
                BackendConfig::Mock {} => {
                    rt.register_backend(&name, abp_integrations::MockBackend);
                }
                BackendConfig::Sidecar { command, args } => {
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

fn parse_key_value_flag(raw: &str, flag_name: &str) -> Result<(String, String)> {
    let (raw_key, raw_value) = raw
        .split_once('=')
        .with_context(|| format!("{flag_name} expects KEY=VALUE, got '{raw}'"))?;

    let key = raw_key.trim();
    if key.is_empty() {
        anyhow::bail!("{flag_name} key cannot be empty (got '{raw}')");
    }

    Ok((key.to_string(), raw_value.to_string()))
}

fn parse_param_value(raw: &str) -> JsonValue {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return JsonValue::String(String::new());
    }

    serde_json::from_str::<JsonValue>(trimmed)
        .unwrap_or_else(|_| JsonValue::String(raw.to_string()))
}

fn insert_vendor_path(vendor: &mut BTreeMap<String, JsonValue>, key: &str, value: JsonValue) {
    let parts: Vec<&str> = key.split('.').filter(|p| !p.trim().is_empty()).collect();
    if parts.is_empty() {
        return;
    }

    if parts.len() == 1 {
        vendor.insert(parts[0].to_string(), value);
        return;
    }

    let root = parts[0].to_string();
    let root_value = vendor
        .entry(root)
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    if !root_value.is_object() {
        *root_value = JsonValue::Object(JsonMap::new());
    }

    let mut current = root_value;
    for part in &parts[1..parts.len() - 1] {
        let obj = current
            .as_object_mut()
            .expect("insert_vendor_path: current node must be an object");
        let entry = obj
            .entry((*part).to_string())
            .or_insert_with(|| JsonValue::Object(JsonMap::new()));
        if !entry.is_object() {
            *entry = JsonValue::Object(JsonMap::new());
        }
        current = entry;
    }

    if let Some(last) = parts.last() {
        let obj = current
            .as_object_mut()
            .expect("insert_vendor_path: final parent must be an object");
        obj.insert((*last).to_string(), value);
    }
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

// ── Config file support ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BackplaneConfig {
    #[serde(default)]
    backends: HashMap<String, BackendConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum BackendConfig {
    #[serde(rename = "mock")]
    Mock {},
    #[serde(rename = "sidecar")]
    Sidecar {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

fn load_config() -> Option<BackplaneConfig> {
    let path = std::path::Path::new("backplane.toml");
    if path.exists() {
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_backend_supports_aliases() {
        assert_eq!(normalize_backend_name("gemini"), "sidecar:gemini");
        assert_eq!(normalize_backend_name("kimi"), "sidecar:kimi");
        assert_eq!(normalize_backend_name("codex"), "sidecar:codex");
        assert_eq!(normalize_backend_name("node"), "sidecar:node");
        assert_eq!(normalize_backend_name("mock"), "mock");
    }

    #[test]
    fn parse_param_value_parses_jsonish_values() {
        assert_eq!(parse_param_value("true"), json!(true));
        assert_eq!(parse_param_value("1.5"), json!(1.5));
        assert_eq!(parse_param_value("{\"a\":1}"), json!({"a": 1}));
        assert_eq!(
            parse_param_value("gemini-2.0-flash"),
            json!("gemini-2.0-flash")
        );
    }

    #[test]
    fn insert_vendor_path_writes_nested_values() {
        let mut vendor = BTreeMap::new();
        insert_vendor_path(&mut vendor, "gemini.model", json!("gemini-2.5-flash"));
        insert_vendor_path(&mut vendor, "gemini.vertex", json!(true));

        assert_eq!(
            vendor.get("gemini"),
            Some(&json!({
                "model": "gemini-2.5-flash",
                "vertex": true
            }))
        );
    }

    #[test]
    fn parse_key_value_requires_equals() {
        let err = parse_key_value_flag("foo", "--param").unwrap_err();
        assert!(
            err.to_string().contains("KEY=VALUE"),
            "unexpected error: {err}"
        );
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
