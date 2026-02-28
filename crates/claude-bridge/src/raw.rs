// SPDX-License-Identifier: MIT OR Apache-2.0
use serde_json::{Value, json};
use sidecar_kit::{ProcessSpec, RawRun, SidecarClient};
use uuid::Uuid;

use crate::{BridgeError, ClaudeBridgeConfig, discovery};

/// Options for a mapped-mode run.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub lane: Option<String>,
    pub workspace_root: Option<String>,
    pub extra_config: Option<Value>,
}

/// Run in passthrough mode: sends a raw vendor request, returns raw vendor events.
/// Constructs a minimal WorkOrder JSON internally (no abp-core dep).
pub async fn run_raw(config: &ClaudeBridgeConfig, request: Value) -> Result<RawRun, BridgeError> {
    let spec = build_process_spec(config)?;
    let client = SidecarClient::spawn(spec).await?;

    let run_id = Uuid::new_v4().to_string();

    let work_order = json!({
        "id": run_id,
        "task": request.get("prompt").and_then(|v| v.as_str()).unwrap_or("passthrough"),
        "lane": "patch_first",
        "workspace": {
            "root": config.cwd.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string()),
            "mode": "pass_through"
        },
        "context": {},
        "policy": {},
        "requirements": { "required": [] },
        "config": {
            "vendor": {
                "abp.mode": "passthrough",
                "abp.request": request
            }
        }
    });

    let raw_run = client.run_raw(run_id, work_order).await?;
    Ok(raw_run)
}

/// Run in mapped mode: task string + options -> raw JSON events.
pub async fn run_mapped_raw(
    config: &ClaudeBridgeConfig,
    task: &str,
    opts: RunOptions,
) -> Result<RawRun, BridgeError> {
    let spec = build_process_spec(config)?;
    let client = SidecarClient::spawn(spec).await?;

    let run_id = Uuid::new_v4().to_string();

    let mut vendor_config = json!({
        "abp.mode": "mapped"
    });
    if let Some(extra) = &opts.extra_config
        && let Some(obj) = extra.as_object()
    {
        for (k, v) in obj {
            vendor_config[k] = v.clone();
        }
    }

    let work_order = json!({
        "id": run_id,
        "task": task,
        "lane": opts.lane.as_deref().unwrap_or("patch_first"),
        "workspace": {
            "root": opts.workspace_root.as_deref()
                .or(config.cwd.as_ref().map(|p| p.to_str().unwrap_or(".")))
                .unwrap_or("."),
            "mode": "staged"
        },
        "context": {},
        "policy": {},
        "requirements": { "required": [] },
        "config": {
            "vendor": vendor_config
        }
    });

    let raw_run = client.run_raw(run_id, work_order).await?;
    Ok(raw_run)
}

/// Build the ProcessSpec from bridge config.
fn build_process_spec(config: &ClaudeBridgeConfig) -> Result<ProcessSpec, BridgeError> {
    let node = discovery::resolve_node(config.node_command.as_deref())?;
    let host_script = discovery::resolve_host_script(config.host_script.as_deref())?;

    let mut spec = ProcessSpec::new(node);
    spec.args = vec![host_script.to_string_lossy().into_owned()];
    spec.env = config.env.clone();

    if let Some(cwd) = &config.cwd {
        spec.cwd = Some(cwd.to_string_lossy().into_owned());
    }

    if let Some(adapter) = &config.adapter_module {
        spec.env.insert(
            "ABP_CLAUDE_ADAPTER_MODULE".into(),
            adapter.to_string_lossy().into_owned(),
        );
    }

    Ok(spec)
}
