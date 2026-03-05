// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
use abp_claude_sdk as claude_sdk;
use abp_cli::cli::{
    Cli, Commands, ConfigAction, LaneArg, ReceiptAction, SchemaArg, WorkspaceModeArg,
};
use abp_cli::commands::{self, SchemaKind};
use abp_cli::health as health_cmd;
use abp_cli::schema as schema_cmd;
use abp_cli::status as status_cmd;
use abp_cli::translate as translate_cmd;
use abp_cli::validate as validate_cmd;
use abp_codex_sdk as codex_sdk;
use abp_copilot_sdk as copilot_sdk;
use abp_core::{
    CapabilityRequirements, ContextPacket, PolicyProfile, RuntimeConfig, WorkOrder, WorkspaceSpec,
};
use abp_gemini_sdk as gemini_sdk;
use abp_host::SidecarSpec;
use abp_integrations::SidecarBackend;
use abp_kimi_sdk as kimi_sdk;
use abp_runtime::Runtime;
use anyhow::{Context, Result};
use clap::Parser;
use serde_json::{Map as JsonMap, Value as JsonValue};
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let filter = if cli.debug {
        EnvFilter::new("abp=debug,abp.sidecar=debug")
    } else {
        EnvFilter::new("abp=info")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Load configuration from --config path, backplane.toml fallback, or defaults.
    let config_path = cli.config.clone().or_else(|| {
        let p = PathBuf::from("backplane.toml");
        if p.exists() {
            Some(p)
        } else {
            None
        }
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

    let result = match cli.command {
        Commands::Backends {
            capabilities,
            health,
            json,
        } => cmd_backends(capabilities, health, json, &config).await,
        Commands::Validate { file, config_file } => {
            cmd_validate(file.as_deref(), config_file.as_deref())
        }
        Commands::Schema { kind, output } => cmd_schema(kind, output),
        Commands::Inspect { file } => cmd_inspect(&file),
        Commands::Translate { from, to, file } => cmd_translate(&from, &to, file),
        Commands::Health { json } => cmd_health(&config, json),
        Commands::ConfigCmd { action } => cmd_config(action, config_path),
        Commands::ReceiptCmd { action } => cmd_receipt(action),
        Commands::Status { json } => cmd_status(&config, json),
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
            stream,
            timeout,
            retry,
            fallback,
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
                stream,
                timeout,
                retry,
                fallback,
                &config,
            )
            .await
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(EXIT_RUNTIME_ERROR);
    }
}

async fn cmd_backends(
    capabilities: bool,
    health: bool,
    json: bool,
    config: &abp_config::BackplaneConfig,
) -> Result<()> {
    let rt = Runtime::with_default_backends();

    // Register config backends.
    let mut rt = rt;
    for (name, entry) in &config.backends {
        match entry {
            abp_config::BackendEntry::Mock {} => {
                rt.register_backend(name, abp_integrations::MockBackend);
            }
            abp_config::BackendEntry::Sidecar { command, args, .. } => {
                let mut spec = SidecarSpec::new(command);
                spec.args = args.clone();
                rt.register_backend(name, SidecarBackend::new(spec));
            }
        }
    }

    if json {
        let mut entries = Vec::new();
        for name in rt.backend_names() {
            let mut entry = serde_json::json!({"name": name});
            if capabilities || health {
                if let Some(b) = rt.backend(&name) {
                    if capabilities {
                        let caps = b.capabilities();
                        let caps_json: serde_json::Map<String, JsonValue> = caps
                            .iter()
                            .map(|(k, v)| {
                                (
                                    format!("{k:?}"),
                                    serde_json::to_value(v).unwrap_or_default(),
                                )
                            })
                            .collect();
                        entry["capabilities"] = JsonValue::Object(caps_json);
                    }
                    if health {
                        entry["health"] = JsonValue::String("ok".into());
                    }
                }
            }
            entries.push(entry);
        }
        // Add well-known sidecars.
        for s in SIDECAR_NAMES {
            if !entries.iter().any(|e| e["name"] == *s) {
                entries.push(serde_json::json!({"name": s}));
            }
        }
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        for name in rt.backend_names() {
            if capabilities || health {
                let mut parts = vec![name.clone()];
                if let Some(b) = rt.backend(&name) {
                    if capabilities {
                        let caps = b.capabilities();
                        parts.push(format!("capabilities={}", caps.len()));
                    }
                    if health {
                        parts.push("health=ok".into());
                    }
                }
                println!("{}", parts.join("  "));
            } else {
                println!("{name}");
            }
        }
        for s in SIDECAR_NAMES {
            if !rt.backend_names().contains(&s.to_string()) {
                println!("{s}");
            }
        }
    }
    Ok(())
}

const SIDECAR_NAMES: &[&str] = &[
    "sidecar:node",
    "sidecar:python",
    "sidecar:claude",
    "sidecar:copilot",
    "sidecar:kimi",
    "sidecar:gemini",
    "sidecar:codex",
    "node",
    "python",
    "claude",
    "copilot",
    "kimi",
    "gemini",
    "codex",
];

fn cmd_validate(
    file: Option<&std::path::Path>,
    config_file: Option<&std::path::Path>,
) -> Result<()> {
    // If --config-file is given, validate that config file.
    if let Some(cfg_path) = config_file {
        let result = validate_cmd::validate_config(Some(cfg_path))?;
        for e in &result.errors {
            eprintln!("error: {e}");
        }
        for w in &result.warnings {
            eprintln!("warning: {w}");
        }
        if result.valid {
            println!("config: valid");
        } else {
            std::process::exit(EXIT_RUNTIME_ERROR);
        }
        return Ok(());
    }

    // If a positional file is given, validate it as a WorkOrder or Receipt.
    if let Some(path) = file {
        let detected = commands::validate_file(path)?;
        match detected {
            commands::ValidatedType::WorkOrder => println!("valid work_order"),
            commands::ValidatedType::Receipt => println!("valid receipt"),
        }
        return Ok(());
    }

    // No arguments: validate the current config.
    let result = validate_cmd::validate_config(None)?;
    for e in &result.errors {
        eprintln!("error: {e}");
    }
    for w in &result.warnings {
        eprintln!("warning: {w}");
    }
    if result.valid {
        println!("config: valid");
    } else {
        std::process::exit(EXIT_RUNTIME_ERROR);
    }
    Ok(())
}

fn cmd_schema(kind: SchemaArg, output: Option<PathBuf>) -> Result<()> {
    let sk = match kind {
        SchemaArg::WorkOrder => SchemaKind::WorkOrder,
        SchemaArg::Receipt => SchemaKind::Receipt,
        SchemaArg::Config => SchemaKind::Config,
    };
    schema_cmd::output_schema(sk, output.as_deref())
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

fn cmd_translate(from: &str, to: &str, file: Option<PathBuf>) -> Result<()> {
    let from_dialect = translate_cmd::parse_dialect(from)?;
    let to_dialect = translate_cmd::parse_dialect(to)?;

    match file {
        Some(path) => {
            let result = translate_cmd::translate_file(from_dialect, to_dialect, &path)?;
            println!("{result}");
        }
        None => {
            let mut input = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
                .context("read stdin")?;
            let result = translate_cmd::translate_json_str(from_dialect, to_dialect, &input)?;
            println!("{result}");
        }
    }
    Ok(())
}

fn cmd_health(config: &abp_config::BackplaneConfig, json: bool) -> Result<()> {
    let report = health_cmd::check_health(config)?;
    if json {
        health_cmd::print_health_json(&report)
    } else {
        health_cmd::print_health(&report)
    }
}

fn cmd_config(action: ConfigAction, global_config_path: Option<PathBuf>) -> Result<()> {
    match action {
        ConfigAction::Check { config } | ConfigAction::Validate { config } => {
            let path = config.or(global_config_path);
            let diagnostics = commands::config_check(path.as_deref())?;
            for d in &diagnostics {
                println!("{d}");
            }
            if diagnostics.iter().any(|d| d.starts_with("error:")) {
                std::process::exit(EXIT_RUNTIME_ERROR);
            }
            Ok(())
        }
        ConfigAction::Show { format } => {
            let path = global_config_path;
            let cfg = abp_config::load_config(path.as_deref()).unwrap_or_else(|e| {
                tracing::warn!("failed to load config: {e}");
                abp_config::BackplaneConfig::default()
            });
            match format.as_str() {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&cfg)?);
                }
                _ => {
                    println!("{}", toml::to_string_pretty(&cfg)?);
                }
            }
            Ok(())
        }
        ConfigAction::Diff { file1, file2 } => {
            let cfg1 = abp_config::load_from_file(&file1)
                .with_context(|| format!("load config '{}'", file1.display()))?;
            let cfg2 = abp_config::load_from_file(&file2)
                .with_context(|| format!("load config '{}'", file2.display()))?;
            let diff = abp_config::diff::diff(&cfg1, &cfg2);
            if diff.is_empty() {
                println!("no differences");
            } else {
                println!("{diff}");
            }
            Ok(())
        }
    }
}

fn cmd_receipt(action: ReceiptAction) -> Result<()> {
    match action {
        ReceiptAction::Verify { file } => {
            let (receipt, valid) = commands::verify_receipt_file(&file)?;
            println!(
                "sha256: {}",
                receipt.receipt_sha256.as_deref().unwrap_or("<none>")
            );
            if valid {
                println!("hash: VALID");
            } else {
                println!("hash: INVALID");
                std::process::exit(EXIT_RUNTIME_ERROR);
            }
            Ok(())
        }
        ReceiptAction::Diff { file1, file2 } => {
            let diff = commands::receipt_diff(&file1, &file2)?;
            println!("{diff}");
            Ok(())
        }
    }
}

fn cmd_status(config: &abp_config::BackplaneConfig, json: bool) -> Result<()> {
    let info = status_cmd::gather_status(config)?;
    if json {
        status_cmd::print_status_json(&info)
    } else {
        status_cmd::print_status(&info)
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_run(
    backend: Option<String>,
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
    _stream: bool,
    timeout: Option<u64>,
    retry: u32,
    fallback: Option<String>,
    config: &abp_config::BackplaneConfig,
) -> Result<()> {
    // Resolve backend: --backend flag > config default_backend > "mock".
    let backend = normalize_backend_name(
        &backend
            .or_else(|| config.default_backend.clone())
            .unwrap_or_else(|| "mock".to_string()),
    );
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

    // Register backends from loaded configuration.
    for (name, entry) in &config.backends {
        match entry {
            abp_config::BackendEntry::Mock {} => {
                rt.register_backend(name, abp_integrations::MockBackend);
            }
            abp_config::BackendEntry::Sidecar { command, args, .. } => {
                let mut spec = SidecarSpec::new(command);
                spec.args = args.clone();
                rt.register_backend(name, SidecarBackend::new(spec));
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

    // Run with retry and fallback support.
    let backends_to_try: Vec<String> = {
        let mut v = vec![backend.clone()];
        if let Some(ref fb) = fallback {
            v.push(normalize_backend_name(fb));
        }
        v
    };
    let max_attempts = retry + 1;

    let mut last_err: Option<anyhow::Error> = None;
    let mut receipt: Option<abp_core::Receipt> = None;
    let mut run_id = Uuid::nil();

    'outer: for attempt_backend in &backends_to_try {
        for attempt in 0..max_attempts {
            if attempt > 0 && !json {
                eprintln!("retry {attempt}/{retry} on {attempt_backend}...");
            }

            let wo_clone = wo.clone();
            let run_result = if let Some(secs) = timeout {
                let fut = rt.run_streaming(attempt_backend, wo_clone);
                match tokio::time::timeout(std::time::Duration::from_secs(secs), fut).await {
                    Ok(r) => r,
                    Err(_) => {
                        last_err = Some(anyhow::anyhow!(
                            "timeout after {secs}s on {attempt_backend}"
                        ));
                        continue;
                    }
                }
            } else {
                rt.run_streaming(attempt_backend, wo_clone).await
            };

            match run_result {
                Ok(handle) => {
                    run_id = handle.run_id;

                    if !json {
                        eprintln!("run_id: {run_id}");
                        eprintln!("backend: {attempt_backend}");
                        eprintln!("---");
                    }

                    let mut events_file =
                        match events_path {
                            Some(ref ep) => {
                                if let Some(parent) = ep.parent() {
                                    std::fs::create_dir_all(parent).with_context(|| {
                                        format!("create events directory {}", parent.display())
                                    })?;
                                }
                                Some(std::fs::File::create(ep).with_context(|| {
                                    format!("create events file {}", ep.display())
                                })?)
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

                    match handle.receipt.await.context("join receipt task")? {
                        Ok(r) => {
                            receipt = Some(r);
                            break 'outer;
                        }
                        Err(e) => {
                            last_err = Some(anyhow::anyhow!("{e}"));
                        }
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow::anyhow!("{e}"));
                }
            }
        }
    }

    let receipt = match receipt {
        Some(r) => r,
        None => {
            return Err(last_err.unwrap_or_else(|| anyhow::anyhow!("all backends failed")));
        }
    };

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
        Error { message, .. } => eprintln!("[error] {message}"),
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

    #[test]
    fn abp_config_load_none_returns_defaults() {
        let config = abp_config::load_config(None).unwrap();
        assert_eq!(config.log_level.as_deref(), Some("info"));
        assert!(config.backends.is_empty());
    }

    #[test]
    fn abp_config_default_backend_fallback() {
        let config = abp_config::BackplaneConfig::default();
        let backend = config.default_backend.unwrap_or_else(|| "mock".to_string());
        assert_eq!(backend, "mock");
    }

    #[test]
    fn abp_config_load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backplane.toml");
        std::fs::write(
            &path,
            r#"
default_backend = "sidecar:claude"
log_level = "debug"

[backends.mock]
type = "mock"
"#,
        )
        .unwrap();
        let config = abp_config::load_config(Some(&path)).unwrap();
        assert_eq!(config.default_backend.as_deref(), Some("sidecar:claude"));
        assert_eq!(config.log_level.as_deref(), Some("debug"));
        assert_eq!(config.backends.len(), 1);
    }

    #[test]
    fn abp_config_validate_detects_issues() {
        let mut config = abp_config::BackplaneConfig::default();
        config.backends.insert(
            "bad".into(),
            abp_config::BackendEntry::Sidecar {
                command: "  ".into(),
                args: vec![],
                timeout_secs: None,
            },
        );
        let err = abp_config::validate_config(&config).unwrap_err();
        assert!(err.to_string().contains("command must not be empty"));
    }
}
