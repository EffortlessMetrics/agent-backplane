#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Shared sidecar registration helpers used by vendor SDK microcrates.

use abp_host::SidecarSpec;
use abp_integrations::SidecarBackend;
use abp_runtime::Runtime;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Register a sidecar backend using a host script relative to `host_root`.
pub fn register_sidecar_backend(
    runtime: &mut Runtime,
    backend_name: &str,
    host_root: &Path,
    host_script_relative: &str,
    command_override: Option<&str>,
    default_command: &str,
    provider_label: &str,
) -> Result<bool> {
    let command = resolve_command(command_override, default_command, provider_label)?;
    let command = match command {
        Some(c) => c,
        None => return Ok(false),
    };

    let host_script = sidecar_script(host_root, host_script_relative);
    if !host_script.is_file() {
        return Ok(false);
    }

    let mut spec = SidecarSpec::new(command);
    spec.args = vec![host_script.to_string_lossy().into_owned()];
    runtime.register_backend(backend_name, SidecarBackend::new(spec));
    Ok(true)
}

/// Resolve the host script path for a given runtime root.
pub fn sidecar_script(host_root: &Path, host_script_relative: &str) -> PathBuf {
    host_root.join(host_script_relative)
}

fn resolve_command(
    command_override: Option<&str>,
    default_command: &str,
    provider_label: &str,
) -> Result<Option<String>> {
    if let Some(command) = command_override {
        let command = command.trim();
        if !command.is_empty() {
            if let Some(resolved) = abp_which::which(command) {
                return Ok(Some(resolved.to_string_lossy().into_owned()));
            }

            anyhow::bail!("explicit {provider_label} command '{command}' is not available");
        }
    }

    if let Some(resolved) = abp_which::which(default_command) {
        return Ok(Some(resolved.to_string_lossy().into_owned()));
    }

    Ok(None)
}
