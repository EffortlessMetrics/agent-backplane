// SPDX-License-Identifier: MIT OR Apache-2.0
//! Workspace template system.
//!
//! Provides [`WorkspaceTemplate`] for defining reusable file layouts and
//! [`TemplateRegistry`] for managing a collection of named templates.

use abp_glob::IncludeExcludeGlobs;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A reusable workspace template containing a set of files and optional glob filters.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceTemplate {
    /// Human-readable name for this template.
    pub name: String,
    /// Description of the template's purpose.
    pub description: String,
    /// Template files keyed by relative path, with content as values.
    pub files: BTreeMap<PathBuf, String>,
    /// Optional include/exclude glob filter applied when writing files.
    #[serde(skip)]
    pub globs: Option<IncludeExcludeGlobs>,
}

impl WorkspaceTemplate {
    /// Create a new empty template.
    #[must_use]
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            files: BTreeMap::new(),
            globs: None,
        }
    }

    /// Add a file to the template.
    pub fn add_file(&mut self, path: impl AsRef<Path>, content: &str) {
        self.files
            .insert(path.as_ref().to_path_buf(), content.to_string());
    }

    /// Number of files in the template.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Check whether the template contains the given path.
    #[must_use]
    pub fn has_file(&self, path: impl AsRef<Path>) -> bool {
        self.files.contains_key(path.as_ref())
    }

    /// Write template files into `target`, creating parent directories as needed.
    ///
    /// When [`globs`](Self::globs) is set, only files whose relative path is
    /// allowed by the filter are written. Returns the number of files written.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation or file writing fails.
    pub fn apply(&self, target: &Path) -> Result<usize> {
        let mut written = 0usize;
        for (rel, content) in &self.files {
            if self
                .globs
                .as_ref()
                .is_some_and(|g| !g.decide_path(rel).is_allowed())
            {
                continue;
            }
            let dest = target.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create dir {}", parent.display()))?;
            }
            fs::write(&dest, content).with_context(|| format!("write {}", dest.display()))?;
            written += 1;
        }
        Ok(written)
    }

    /// Validate the template and return a list of problems (empty means valid).
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.name.is_empty() {
            problems.push("template name is empty".to_string());
        }
        if self.description.is_empty() {
            problems.push("template description is empty".to_string());
        }
        for path in self.files.keys() {
            if path.is_absolute() {
                problems.push(format!("absolute path not allowed: {}", path.display()));
            }
        }
        problems
    }
}

/// Registry of named workspace templates.
#[derive(Debug, Clone, Default)]
pub struct TemplateRegistry {
    templates: BTreeMap<String, WorkspaceTemplate>,
}

impl TemplateRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a template. Overwrites any existing template with the same name.
    pub fn register(&mut self, template: WorkspaceTemplate) {
        self.templates.insert(template.name.clone(), template);
    }

    /// Look up a template by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&WorkspaceTemplate> {
        self.templates.get(name)
    }

    /// Return a sorted list of registered template names.
    #[must_use]
    pub fn list(&self) -> Vec<&str> {
        self.templates.keys().map(String::as_str).collect()
    }

    /// Number of registered templates.
    #[must_use]
    pub fn count(&self) -> usize {
        self.templates.len()
    }
}
