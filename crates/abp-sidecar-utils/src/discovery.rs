// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar discovery and registration.
//!
//! [`SidecarRegistry`] maintains a mapping of backend names to their
//! executable configurations, supporting both static registration and
//! directory-based discovery.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::debug;

/// Errors from sidecar discovery and registration.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// A sidecar with the given name is already registered.
    #[error("sidecar already registered: {0}")]
    AlreadyRegistered(String),
    /// The specified sidecar was not found in the registry.
    #[error("sidecar not found: {0}")]
    NotFound(String),
    /// I/O error during directory scanning.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Entry point configuration for a sidecar.
#[derive(Debug, Clone)]
pub struct SidecarEntry {
    /// Human-readable name / identifier for this sidecar.
    pub name: String,
    /// Path to the entry-point script or executable.
    pub program: PathBuf,
    /// Default arguments.
    pub args: Vec<String>,
    /// Optional working directory override.
    pub working_dir: Option<PathBuf>,
    /// Optional description.
    pub description: Option<String>,
}

impl SidecarEntry {
    /// Create a new entry with the given name and program path.
    #[must_use]
    pub fn new(name: impl Into<String>, program: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: Vec::new(),
            working_dir: None,
            description: None,
        }
    }

    /// Set default arguments.
    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Set the working directory.
    #[must_use]
    pub fn working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Set a description.
    #[must_use]
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Registry of known sidecar backends.
///
/// # Examples
///
/// ```
/// use abp_sidecar_utils::discovery::SidecarRegistry;
/// use abp_sidecar_utils::discovery::SidecarEntry;
///
/// let mut registry = SidecarRegistry::new();
/// registry.register(SidecarEntry::new("node", "hosts/node/index.js")).unwrap();
/// assert!(registry.get("node").is_some());
/// assert_eq!(registry.list().len(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct SidecarRegistry {
    entries: BTreeMap<String, SidecarEntry>,
}

impl SidecarRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a sidecar entry.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::AlreadyRegistered`] if the name is taken.
    pub fn register(&mut self, entry: SidecarEntry) -> Result<(), RegistryError> {
        if self.entries.contains_key(&entry.name) {
            return Err(RegistryError::AlreadyRegistered(entry.name.clone()));
        }
        debug!(name = %entry.name, program = %entry.program.display(), "registering sidecar");
        self.entries.insert(entry.name.clone(), entry);
        Ok(())
    }

    /// Register or replace a sidecar entry.
    pub fn register_or_replace(&mut self, entry: SidecarEntry) {
        debug!(name = %entry.name, program = %entry.program.display(), "registering sidecar (replace)");
        self.entries.insert(entry.name.clone(), entry);
    }

    /// Look up a sidecar by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SidecarEntry> {
        self.entries.get(name)
    }

    /// Remove a sidecar entry by name.
    pub fn remove(&mut self, name: &str) -> Option<SidecarEntry> {
        self.entries.remove(name)
    }

    /// List all registered sidecar names (sorted).
    #[must_use]
    pub fn list(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }

    /// Total number of registered sidecars.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Scan a directory for sidecar entry-point scripts.
    ///
    /// Each immediate subdirectory that contains a file named
    /// `entry_point_name` (e.g. `"index.js"`) is registered using the
    /// subdirectory name as the sidecar name.
    ///
    /// Existing entries with the same name are **not** overwritten.
    pub fn discover_from_dir(
        &mut self,
        hosts_dir: &Path,
        entry_point_name: &str,
    ) -> Result<Vec<String>, RegistryError> {
        let mut discovered = Vec::new();

        let read_dir = std::fs::read_dir(hosts_dir)?;
        for dir_entry in read_dir {
            let dir_entry = dir_entry?;
            let path = dir_entry.path();
            if !path.is_dir() {
                continue;
            }

            let entry_point = path.join(entry_point_name);
            if !entry_point.exists() {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();

            if name.is_empty() || self.entries.contains_key(&name) {
                continue;
            }

            let entry = SidecarEntry {
                name: name.clone(),
                program: entry_point,
                args: Vec::new(),
                working_dir: Some(path),
                description: None,
            };
            self.entries.insert(name.clone(), entry);
            discovered.push(name);
        }

        Ok(discovered)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry() {
        let reg = SidecarRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.list().is_empty());
    }

    #[test]
    fn register_and_get() {
        let mut reg = SidecarRegistry::new();
        reg.register(SidecarEntry::new("node", "hosts/node/index.js"))
            .unwrap();
        assert_eq!(reg.len(), 1);
        let entry = reg.get("node").unwrap();
        assert_eq!(entry.name, "node");
        assert_eq!(entry.program, PathBuf::from("hosts/node/index.js"));
    }

    #[test]
    fn register_duplicate_errors() {
        let mut reg = SidecarRegistry::new();
        reg.register(SidecarEntry::new("node", "path1")).unwrap();
        let err = reg
            .register(SidecarEntry::new("node", "path2"))
            .unwrap_err();
        assert!(matches!(err, RegistryError::AlreadyRegistered(_)));
        assert!(err.to_string().contains("node"));
    }

    #[test]
    fn register_or_replace_overwrites() {
        let mut reg = SidecarRegistry::new();
        reg.register(SidecarEntry::new("node", "path1")).unwrap();
        reg.register_or_replace(SidecarEntry::new("node", "path2"));
        assert_eq!(reg.get("node").unwrap().program, PathBuf::from("path2"));
    }

    #[test]
    fn remove_entry() {
        let mut reg = SidecarRegistry::new();
        reg.register(SidecarEntry::new("node", "path")).unwrap();
        let removed = reg.remove("node");
        assert!(removed.is_some());
        assert!(reg.is_empty());
        assert!(reg.remove("node").is_none());
    }

    #[test]
    fn list_sorted() {
        let mut reg = SidecarRegistry::new();
        reg.register(SidecarEntry::new("python", "py")).unwrap();
        reg.register(SidecarEntry::new("node", "js")).unwrap();
        reg.register(SidecarEntry::new("claude", "cl")).unwrap();
        let names = reg.list();
        assert_eq!(names, vec!["claude", "node", "python"]);
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let reg = SidecarRegistry::new();
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn entry_builder() {
        let entry = SidecarEntry::new("test", "bin/test")
            .args(["--flag"])
            .working_dir("/tmp")
            .description("A test sidecar");
        assert_eq!(entry.name, "test");
        assert_eq!(entry.args, vec!["--flag"]);
        assert_eq!(entry.working_dir, Some(PathBuf::from("/tmp")));
        assert_eq!(entry.description.as_deref(), Some("A test sidecar"));
    }

    #[test]
    fn discover_from_nonexistent_dir() {
        let mut reg = SidecarRegistry::new();
        let result = reg.discover_from_dir(Path::new("__nonexistent_dir_12345__"), "index.js");
        assert!(result.is_err());
    }

    #[test]
    fn discover_from_empty_dir() {
        let dir = std::env::temp_dir().join("abp_test_discover_empty");
        let _ = std::fs::create_dir_all(&dir);
        let mut reg = SidecarRegistry::new();
        let discovered = reg.discover_from_dir(&dir, "index.js").unwrap();
        assert!(discovered.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_finds_entry_points() {
        let base = std::env::temp_dir().join("abp_test_discover_find");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("my-sidecar")).unwrap();
        std::fs::write(base.join("my-sidecar").join("main.py"), "# entry").unwrap();

        // Also create a dir without the entry point
        std::fs::create_dir_all(base.join("no-entry")).unwrap();

        let mut reg = SidecarRegistry::new();
        let discovered = reg.discover_from_dir(&base, "main.py").unwrap();
        assert_eq!(discovered, vec!["my-sidecar"]);
        assert!(reg.get("my-sidecar").is_some());
        assert!(reg.get("no-entry").is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn discover_does_not_overwrite_existing() {
        let base = std::env::temp_dir().join("abp_test_discover_nooverwrite");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("node")).unwrap();
        std::fs::write(base.join("node").join("index.js"), "//").unwrap();

        let mut reg = SidecarRegistry::new();
        reg.register(SidecarEntry::new("node", "custom/path"))
            .unwrap();
        let discovered = reg.discover_from_dir(&base, "index.js").unwrap();
        assert!(discovered.is_empty());
        assert_eq!(
            reg.get("node").unwrap().program,
            PathBuf::from("custom/path")
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn error_display_messages() {
        let e = RegistryError::AlreadyRegistered("node".into());
        assert!(e.to_string().contains("already registered"));

        let e = RegistryError::NotFound("missing".into());
        assert!(e.to_string().contains("not found"));
    }
}
