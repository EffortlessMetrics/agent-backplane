//! Microcrate for wiring the Gemini CLI sidecar into ABP runtimes.

use abp_runtime::Runtime;
use abp_sidecar_sdk::{
    SidecarRegistration, register_sidecar, sidecar_script as resolve_sidecar_script,
};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Canonical backend name used by CLI, daemon, and integrations.
pub const BACKEND_NAME: &str = "sidecar:gemini";

/// Relative path to the JS Gemini host inside a workspace checkout.
pub const HOST_SCRIPT_RELATIVE: &str = "hosts/gemini/host.js";

/// Preferred executable for the default Gemini sidecar host.
pub const DEFAULT_NODE_COMMAND: &str = "node";

const REGISTRATION: SidecarRegistration<'static> = SidecarRegistration {
    display_name: "Gemini",
    backend_name: BACKEND_NAME,
    host_script_relative: HOST_SCRIPT_RELATIVE,
    default_command: DEFAULT_NODE_COMMAND,
};

/// Register the Gemini sidecar backend if available.
pub fn register_default(
    runtime: &mut Runtime,
    host_root: &Path,
    command_override: Option<&str>,
) -> Result<bool> {
    register_sidecar(runtime, host_root, command_override, REGISTRATION)
}

/// Register a Gemini backend under a custom name.
pub fn register_backend(
    runtime: &mut Runtime,
    backend_name: &str,
    host_root: &Path,
    command_override: Option<&str>,
) -> Result<bool> {
    register_sidecar(
        runtime,
        host_root,
        command_override,
        SidecarRegistration {
            backend_name,
            ..REGISTRATION
        },
    )
}

/// Resolve the host script path for a given runtime root.
pub fn sidecar_script(host_root: &Path) -> PathBuf {
    resolve_sidecar_script(host_root, HOST_SCRIPT_RELATIVE)
}
