//! Shared helpers for SDK-specific sidecar registration crates.

use abp_host::SidecarSpec;
use abp_integrations::SidecarBackend;
use abp_runtime::Runtime;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Registration metadata for sidecars that run through a local command.
#[derive(Debug, Clone, Copy)]
pub struct SidecarRegistration<'a> {
    pub display_name: &'a str,
    pub backend_name: &'a str,
    pub host_script_relative: &'a str,
    pub default_command: &'a str,
}

/// Register a sidecar backend if both command and script are available.
pub fn register_sidecar(
    runtime: &mut Runtime,
    host_root: &Path,
    command_override: Option<&str>,
    registration: SidecarRegistration<'_>,
) -> Result<bool> {
    let command = resolve_command(command_override, registration).with_context(|| {
        format!(
            "resolve {} command for {}",
            registration.display_name, registration.backend_name
        )
    })?;
    let command = match command {
        Some(c) => c,
        None => return Ok(false),
    };

    let host_script = sidecar_script(host_root, registration.host_script_relative);
    if !host_script.is_file() {
        return Ok(false);
    }

    let mut spec = SidecarSpec::new(command);
    spec.args = vec![host_script.to_string_lossy().into_owned()];
    runtime.register_backend(registration.backend_name, SidecarBackend::new(spec));
    Ok(true)
}

/// Resolve the host script path for a given runtime root.
pub fn sidecar_script(host_root: &Path, host_script_relative: &str) -> PathBuf {
    host_root.join(host_script_relative)
}

fn resolve_command(
    command_override: Option<&str>,
    registration: SidecarRegistration<'_>,
) -> Result<Option<String>> {
    if let Some(command) = command_override {
        let command = command.trim();
        if !command.is_empty() {
            if command_exists(command) {
                return Ok(Some(command.to_string()));
            }

            anyhow::bail!(
                "explicit {} command '{command}' is not available",
                registration.display_name
            );
        }
    }

    if command_exists(registration.default_command) {
        return Ok(Some(registration.default_command.to_string()));
    }

    Ok(None)
}

fn command_exists(command: &str) -> bool {
    let candidate = Path::new(command);
    let has_path = candidate.components().count() > 1;

    if has_path {
        return candidate.exists();
    }

    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| path_has_command(&dir, command)))
}

fn path_has_command(dir: &Path, command: &str) -> bool {
    if dir.join(command).exists() {
        return true;
    }

    if !cfg!(windows) {
        return false;
    }

    for ext in ["", ".exe", ".cmd", ".bat", ".com"] {
        if dir.join(format!("{command}{ext}")).exists() {
            return true;
        }
    }

    false
}
