// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar registry — tracks available sidecars by name.

use crate::SidecarSpec;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Configuration for a registered sidecar.
///
/// Unlike [`SidecarSpec`] (which is a low-level spawn descriptor),
/// `SidecarConfig` carries a unique `name` and uses [`PathBuf`] for the
/// working directory, making it suitable for persistent storage in a
/// registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarConfig {
    /// Unique name identifying this sidecar.
    pub name: String,
    /// Executable command to run.
    pub command: String,
    /// Arguments passed to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables set for the process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Working directory for the sidecar process.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

impl SidecarConfig {
    /// Create a config with the given name and command (empty args/env).
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            working_dir: None,
        }
    }

    /// Validate that required fields are non-empty.
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            bail!("sidecar name must not be empty");
        }
        if self.command.is_empty() {
            bail!("sidecar command must not be empty");
        }
        Ok(())
    }

    /// Convert to a [`SidecarSpec`] suitable for spawning.
    pub fn to_spec(&self) -> SidecarSpec {
        SidecarSpec {
            command: self.command.clone(),
            args: self.args.clone(),
            env: self.env.clone(),
            cwd: self
                .working_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        }
    }
}

/// A registry of named [`SidecarConfig`]s.
///
/// Sidecars can be registered manually or discovered from a directory
/// that follows the ABP hosts layout (each subdirectory contains a
/// `host.js`, `host.py`, or similar entry-point).
#[derive(Debug, Clone, Default)]
pub struct SidecarRegistry {
    sidecars: BTreeMap<String, SidecarConfig>,
}

/// Recognised sidecar entry-point filenames and their interpreters.
const KNOWN_HOSTS: &[(&str, &str)] = &[
    ("host.js", "node"),
    ("host.py", "python"),
    ("host.sh", "bash"),
];

impl SidecarRegistry {
    /// Register a sidecar configuration.
    ///
    /// Returns an error if a sidecar with the same name is already
    /// registered or if the config fails validation.
    pub fn register(&mut self, config: SidecarConfig) -> Result<()> {
        config.validate()?;
        if self.sidecars.contains_key(&config.name) {
            bail!("sidecar '{}' is already registered", config.name);
        }
        self.sidecars.insert(config.name.clone(), config);
        Ok(())
    }

    /// Look up a sidecar by name.
    pub fn get(&self, name: &str) -> Option<&SidecarConfig> {
        self.sidecars.get(name)
    }

    /// Return the names of all registered sidecars in sorted order.
    pub fn list(&self) -> Vec<&str> {
        self.sidecars.keys().map(String::as_str).collect()
    }

    /// Remove a sidecar by name. Returns `true` if it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.sidecars.remove(name).is_some()
    }

    /// Scan `dir` for subdirectories that contain a recognised host
    /// script and build a registry from them.
    ///
    /// For each child directory, the first matching file from
    /// the known host scripts list becomes the sidecar's command, with the
    /// directory name used as the sidecar name.
    pub fn from_config_dir(dir: &Path) -> Result<SidecarRegistry> {
        let mut registry = SidecarRegistry::default();

        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("failed to read directory: {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            for &(filename, interpreter) in KNOWN_HOSTS {
                let script = path.join(filename);
                if script.is_file() {
                    let mut config = SidecarConfig::new(name.clone(), interpreter);
                    config.args = vec![script.to_string_lossy().into_owned()];
                    // Discovery never produces duplicates (directory names are unique).
                    registry.sidecars.insert(name.clone(), config);
                    break;
                }
            }
        }

        Ok(registry)
    }

    /// Alias for [`from_config_dir`](Self::from_config_dir).
    pub fn discover_from_dir(dir: &Path) -> Result<SidecarRegistry> {
        Self::from_config_dir(dir)
    }
}
