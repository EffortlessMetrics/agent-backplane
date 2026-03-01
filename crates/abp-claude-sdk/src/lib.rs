// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! Microcrate for wiring the Claude sidecar into ABP runtimes.
//!
//! Registers the Claude sidecar backend and exposes the
//! [`dialect`] module for translating between ABP contract types and
//! the Anthropic Messages API format.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod dialect;
pub mod lowering;

use abp_runtime::Runtime;
use abp_sidecar_sdk::{register_sidecar_backend, sidecar_script as resolve_sidecar_script};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Canonical backend name used by CLI, daemon, and integrations.
pub const BACKEND_NAME: &str = "sidecar:claude";

/// Relative path to the JS Claude host inside a workspace checkout.
pub const HOST_SCRIPT_RELATIVE: &str = "hosts/claude/host.js";

/// Preferred executable for the default Claude sidecar host.
pub const DEFAULT_NODE_COMMAND: &str = "node";

/// Register the Claude sidecar backend if available.
pub fn register_default(
    runtime: &mut Runtime,
    host_root: &Path,
    command_override: Option<&str>,
) -> Result<bool> {
    register_backend(runtime, BACKEND_NAME, host_root, command_override)
}

/// Register a Claude backend under a custom name.
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
        "Claude",
    )
    .with_context(|| format!("resolve Claude command for {backend_name}"))
}

/// Resolve the host script path for a given runtime root.
pub fn sidecar_script(host_root: &Path) -> PathBuf {
    resolve_sidecar_script(host_root, HOST_SCRIPT_RELATIVE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn backend_name_is_correct() {
        assert_eq!(BACKEND_NAME, "sidecar:claude");
    }

    #[test]
    fn sidecar_script_returns_correct_path() {
        let root = Path::new("/fake/root");
        let script = sidecar_script(root);
        assert_eq!(script, root.join("hosts/claude/host.js"));
    }

    #[test]
    fn register_default_with_nonexistent_root_returns_false() {
        let mut runtime = Runtime::new();
        let bogus = Path::new("/nonexistent/path/that/does/not/exist");
        let result = register_default(&mut runtime, bogus, None).unwrap_or(false);
        assert!(!result);
    }
}
