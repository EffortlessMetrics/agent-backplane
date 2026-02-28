//! Microcrate for wiring the Gemini CLI sidecar into ABP runtimes.

use abp_runtime::Runtime;
use abp_sidecar_sdk::{register_sidecar_backend, sidecar_script as resolve_sidecar_script};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Canonical backend name used by CLI, daemon, and integrations.
pub const BACKEND_NAME: &str = "sidecar:gemini";

/// Relative path to the JS Gemini host inside a workspace checkout.
pub const HOST_SCRIPT_RELATIVE: &str = "hosts/gemini/host.js";

/// Preferred executable for the default Gemini sidecar host.
pub const DEFAULT_NODE_COMMAND: &str = "node";

/// Register the Gemini sidecar backend if available.
pub fn register_default(
    runtime: &mut Runtime,
    host_root: &Path,
    command_override: Option<&str>,
) -> Result<bool> {
    register_backend(runtime, BACKEND_NAME, host_root, command_override)
}

/// Register a Gemini backend under a custom name.
pub fn register_backend(
    runtime: &mut Runtime,
    backend_name: &str,
    host_root: &Path,
    command_override: Option<&str>,
) -> Result<bool> {
    register_sidecar_backend(
        runtime,
        backend_name,
        host_root,
        HOST_SCRIPT_RELATIVE,
        command_override,
        DEFAULT_NODE_COMMAND,
        "Gemini",
    )
    .with_context(|| format!("resolve Gemini command for {backend_name}"))
}

/// Resolve the host script path for a given runtime root.
pub fn sidecar_script(host_root: &Path) -> PathBuf {
    resolve_sidecar_script(host_root, HOST_SCRIPT_RELATIVE)
}
