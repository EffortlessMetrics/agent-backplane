// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-workspace
#![deny(unsafe_code)]
//!
//! Workspace preparation and harness utilities.
//!
//! Two modes matter:
//! - PassThrough: run directly in the user's workspace.
//! - Staged: create a sanitized copy (and optionally a synthetic git repo).

pub mod diff;

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_glob::IncludeExcludeGlobs;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use tracing::debug;
use walkdir::WalkDir;

/// A workspace ready for use, potentially backed by a temporary directory.
///
/// For [`WorkspaceMode::Staged`] workspaces the temp directory is cleaned up
/// when this value is dropped.
#[derive(Debug)]
pub struct PreparedWorkspace {
    path: PathBuf,
    _temp: Option<TempDir>,
}

impl PreparedWorkspace {
    /// Returns the root path of the prepared workspace.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Entry point for workspace preparation (pass-through or staged copy).
#[derive(Debug, Clone, Copy)]
pub struct WorkspaceManager;

impl WorkspaceManager {
    /// Prepare a workspace according to `spec`.
    ///
    /// In [`WorkspaceMode::PassThrough`] mode the original path is used directly.
    /// In [`WorkspaceMode::Staged`] mode a filtered copy is created in a temp
    /// directory and a fresh git repo is initialised for meaningful diffs.
    ///
    /// # Errors
    ///
    /// Returns an error if the temp directory cannot be created, glob patterns
    /// are invalid, or the file copy fails.
    pub fn prepare(spec: &WorkspaceSpec) -> Result<PreparedWorkspace> {
        let root = PathBuf::from(&spec.root);
        match spec.mode {
            WorkspaceMode::PassThrough => Ok(PreparedWorkspace {
                path: root,
                _temp: None,
            }),
            WorkspaceMode::Staged => {
                let tmp = tempfile::tempdir().context("create temp dir")?;
                let dest = tmp.path().to_path_buf();

                let path_rules = IncludeExcludeGlobs::new(&spec.include, &spec.exclude)
                    .context("compile workspace include/exclude globs")?;

                copy_workspace(&root, &dest, &path_rules)?;

                // If the staged workspace isn't a git repo, initialize one.
                ensure_git_repo(&dest);

                Ok(PreparedWorkspace {
                    path: dest,
                    _temp: Some(tmp),
                })
            }
        }
    }

    /// Run `git status --porcelain=v1` in the workspace, returning `None` on failure.
    #[must_use]
    pub fn git_status(path: &Path) -> Option<String> {
        run_git(path, &["status", "--porcelain=v1"]).ok()
    }

    /// Run `git diff --no-color` in the workspace, returning `None` on failure.
    #[must_use]
    pub fn git_diff(path: &Path) -> Option<String> {
        run_git(path, &["diff", "--no-color"]).ok()
    }
}

fn copy_workspace(
    src_root: &Path,
    dest_root: &Path,
    path_rules: &IncludeExcludeGlobs,
) -> Result<()> {
    debug!(target: "abp.workspace", "staging workspace from {} to {}", src_root.display(), dest_root.display());

    let walker = WalkDir::new(src_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

    for entry in walker {
        let entry = entry?;
        let path = entry.path();

        let rel = path.strip_prefix(src_root).unwrap_or(path);
        if rel.as_os_str().is_empty() {
            continue;
        }

        if !path_rules.decide_path(rel).is_allowed() {
            continue;
        }

        let dest_path = dest_root.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest_path)
                .with_context(|| format!("create dir {}", dest_path.display()))?;
            continue;
        }

        if entry.file_type().is_file() {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create dir {}", parent.display()))?;
            }
            fs::copy(path, &dest_path).with_context(|| format!("copy {}", rel.display()))?;
        }
    }

    Ok(())
}

fn ensure_git_repo(path: &Path) {
    if path.join(".git").exists() {
        return;
    }

    let _ = Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .status();

    // Create an initial commit so diffs are meaningful.
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .status();

    let _ = Command::new("git")
        .args([
            "-c",
            "user.name=abp",
            "-c",
            "user.email=abp@local",
            "commit",
            "-qm",
            "baseline",
        ])
        .current_dir(path)
        .status();
}

/// Builder for staged workspace creation.
///
/// Provides a fluent API as an alternative to [`WorkspaceManager::prepare`]
/// for staged-only workflows where callers don't need pass-through mode.
///
/// # Examples
///
/// ```no_run
/// # use abp_workspace::WorkspaceStager;
/// let ws = WorkspaceStager::new()
///     .source_root("./my-project")
///     .exclude(vec!["*.log".into()])
///     .stage()
///     .unwrap();
/// println!("staged at: {}", ws.path().display());
/// ```
#[derive(Debug, Clone)]
pub struct WorkspaceStager {
    source_root: Option<PathBuf>,
    include: Vec<String>,
    exclude: Vec<String>,
    git_init: bool,
}

impl Default for WorkspaceStager {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceStager {
    /// Create a new builder with default settings (git init enabled, no glob filters).
    #[must_use]
    pub fn new() -> Self {
        Self {
            source_root: None,
            include: Vec::new(),
            exclude: Vec::new(),
            git_init: true,
        }
    }

    /// Set the source directory to stage from.
    #[must_use]
    pub fn source_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.source_root = Some(path.into());
        self
    }

    /// Set include glob patterns.
    #[must_use]
    pub fn include(mut self, patterns: Vec<String>) -> Self {
        self.include = patterns;
        self
    }

    /// Set exclude glob patterns.
    #[must_use]
    pub fn exclude(mut self, patterns: Vec<String>) -> Self {
        self.exclude = patterns;
        self
    }

    /// Whether to initialize a git repository in the staged workspace (default: `true`).
    #[must_use]
    pub fn with_git_init(mut self, init: bool) -> Self {
        self.git_init = init;
        self
    }

    /// Execute staging and return a [`PreparedWorkspace`].
    ///
    /// # Errors
    ///
    /// Returns an error if `source_root` was not set, the source directory does
    /// not exist, glob patterns are invalid, or file copy fails.
    pub fn stage(self) -> Result<PreparedWorkspace> {
        let root = self
            .source_root
            .context("WorkspaceStager: source_root is required")?;
        anyhow::ensure!(root.exists(), "source directory does not exist: {}", root.display());

        let tmp = tempfile::tempdir().context("create temp dir")?;
        let dest = tmp.path().to_path_buf();

        let path_rules = IncludeExcludeGlobs::new(&self.include, &self.exclude)
            .context("compile workspace include/exclude globs")?;

        copy_workspace(&root, &dest, &path_rules)?;

        if self.git_init {
            ensure_git_repo(&dest);
        }

        Ok(PreparedWorkspace {
            path: dest,
            _temp: Some(tmp),
        })
    }
}

fn run_git(path: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .with_context(|| format!("run git {args:?}"))?;

    if !out.status.success() {
        anyhow::bail!("git {:?} failed (code={:?})", args, out.status.code());
    }

    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
