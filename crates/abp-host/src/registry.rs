// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar registry — tracks available sidecars by name.

use crate::SidecarSpec;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// A registry of named [`SidecarSpec`]s.
///
/// Sidecars can be registered manually or discovered from a directory
/// that follows the ABP hosts layout (each subdirectory contains a
/// `host.js`, `host.py`, or similar entry-point).
#[derive(Debug, Clone, Default)]
pub struct SidecarRegistry {
    sidecars: BTreeMap<String, SidecarSpec>,
}

/// Recognised sidecar entry-point filenames and their interpreters.
const KNOWN_HOSTS: &[(&str, &str)] = &[
    ("host.js", "node"),
    ("host.py", "python"),
    ("host.sh", "bash"),
];

impl SidecarRegistry {
    /// Register a sidecar under the given name, replacing any previous entry.
    pub fn register(&mut self, name: impl Into<String>, spec: SidecarSpec) {
        self.sidecars.insert(name.into(), spec);
    }

    /// Look up a sidecar by name.
    pub fn get(&self, name: &str) -> Option<&SidecarSpec> {
        self.sidecars.get(name)
    }

    /// Return the names of all registered sidecars in sorted order.
    pub fn list(&self) -> Vec<&str> {
        self.sidecars.keys().map(String::as_str).collect()
    }

    /// Remove a sidecar by name, returning its spec if it existed.
    pub fn remove(&mut self, name: &str) -> Option<SidecarSpec> {
        self.sidecars.remove(name)
    }

    /// Scan `dir` for subdirectories that contain a recognised host script
    /// and build a registry from them.
    ///
    /// For each child directory, the first matching file from [`KNOWN_HOSTS`]
    /// becomes the sidecar's command, with the directory name used as the
    /// sidecar name.
    pub fn discover_from_dir(dir: &Path) -> Result<SidecarRegistry> {
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
                    let mut spec = SidecarSpec::new(interpreter);
                    spec.args = vec![script.to_string_lossy().into_owned()];
                    registry.register(name.clone(), spec);
                    break;
                }
            }
        }

        Ok(registry)
    }
}
