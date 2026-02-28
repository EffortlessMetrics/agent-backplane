//! Microcrate for wiring the Gemini CLI sidecar into ABP runtimes.

use abp_host::SidecarSpec;
use abp_integrations::SidecarBackend;
use abp_runtime::Runtime;
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
    let command = resolve_command(command_override)
        .with_context(|| format!("resolve Gemini command for {backend_name}"))?;
    let command = match command {
        Some(c) => c,
        None => return Ok(false),
    };

    let host_script = sidecar_script(host_root);
    if !host_script.is_file() {
        return Ok(false);
    }

    let mut spec = SidecarSpec::new(command);
    spec.args = vec![host_script.to_string_lossy().into_owned()];
    runtime.register_backend(backend_name, SidecarBackend::new(spec));
    Ok(true)
}

/// Resolve the host script path for a given runtime root.
pub fn sidecar_script(host_root: &Path) -> PathBuf {
    host_root.join(HOST_SCRIPT_RELATIVE)
}

fn resolve_command(command_override: Option<&str>) -> Result<Option<String>> {
    if let Some(command) = command_override {
        let command = command.trim();
        if !command.is_empty() {
            if command_exists(command) {
                return Ok(Some(command.to_string()));
            }

            anyhow::bail!("explicit Gemini command '{command}' is not available");
        }
    }

    if command_exists(DEFAULT_NODE_COMMAND) {
        return Ok(Some(DEFAULT_NODE_COMMAND.to_string()));
    }

    Ok(None)
}

fn command_exists(command: &str) -> bool {
    let candidate = Path::new(command);
    let has_path = candidate.components().count() > 1;

    if has_path {
        return candidate.exists();
    }

    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| path_has_command(&dir, command))
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn backend_name_is_correct() {
        assert_eq!(BACKEND_NAME, "sidecar:gemini");
    }

    #[test]
    fn sidecar_script_returns_correct_path() {
        let root = Path::new("/fake/root");
        let script = sidecar_script(root);
        assert_eq!(script, root.join("hosts/gemini/host.js"));
    }

    #[test]
    fn register_default_with_nonexistent_root_returns_false() {
        let mut runtime = Runtime::new();
        let bogus = Path::new("/nonexistent/path/that/does/not/exist");
        let result = register_default(&mut runtime, bogus, None).unwrap_or(false);
        assert!(!result);
    }
}
