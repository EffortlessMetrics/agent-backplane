//! Microcrate for wiring the Kimi sidecar into ABP runtimes.

use abp_runtime::Runtime;
use abp_sidecar_sdk as sidecar;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Canonical backend name used by CLI, daemon, and integrations.
pub const BACKEND_NAME: &str = "sidecar:kimi";

/// Relative path to the JS Kimi host inside a workspace checkout.
pub const HOST_SCRIPT_RELATIVE: &str = "hosts/kimi/host.js";

/// Preferred executable for the default Kimi sidecar host.
pub const DEFAULT_NODE_COMMAND: &str = "node";

/// Register the Kimi sidecar backend if available.
pub fn register_default(
    runtime: &mut Runtime,
    host_root: &Path,
    command_override: Option<&str>,
) -> Result<bool> {
    register_backend(runtime, BACKEND_NAME, host_root, command_override)
}

/// Register a Kimi backend under a custom name.
pub fn register_backend(
    runtime: &mut Runtime,
    backend_name: &str,
    host_root: &Path,
    command_override: Option<&str>,
) -> Result<bool> {
    sidecar::register_node_sidecar_backend(
        runtime,
        backend_name,
        host_root,
        HOST_SCRIPT_RELATIVE,
        command_override,
        DEFAULT_NODE_COMMAND,
        "Kimi",
    )
}

/// Resolve the host script path for a given runtime root.
pub fn sidecar_script(host_root: &Path) -> PathBuf {
    sidecar::sidecar_script(host_root, HOST_SCRIPT_RELATIVE)
}
