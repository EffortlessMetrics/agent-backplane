// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-workspace
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Workspace preparation and harness utilities.
//!
//! Two modes matter:
//! - PassThrough: run directly in the user's workspace.
//! - Staged: create a sanitized copy (and optionally a synthetic git repo).

pub mod diff;
pub mod ops;
pub mod snapshot;
pub mod template;
pub mod tracker;

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_git::{ensure_git_repo, git_diff as git_diff_impl, git_status as git_status_impl};
use abp_glob::IncludeExcludeGlobs;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::debug;
use walkdir::WalkDir;

// ── Workspace metadata ──────────────────────────────────────────────────

/// Metadata about a prepared workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMetadata {
    /// Root path of the workspace.
    pub root: PathBuf,
    /// Total number of files.
    pub file_count: usize,
    /// Total number of directories (excluding the root itself).
    pub dir_count: usize,
    /// Total size of all files in bytes.
    pub total_size: u64,
    /// Timestamp when the metadata was collected.
    pub collected_at: DateTime<Utc>,
}

// ── Workspace validation ────────────────────────────────────────────────

/// Result of a workspace integrity check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// `true` when the workspace passes all integrity checks.
    pub valid: bool,
    /// Human-readable descriptions of any problems found.
    pub problems: Vec<String>,
}

impl ValidationResult {
    /// Returns `true` when the workspace is valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

// ── PreparedWorkspace ───────────────────────────────────────────────────

/// A workspace ready for use, potentially backed by a temporary directory.
///
/// For [`WorkspaceMode::Staged`] workspaces the temp directory is cleaned up
/// when this value is dropped.
#[derive(Debug)]
pub struct PreparedWorkspace {
    path: PathBuf,
    _temp: Option<TempDir>,
    created_at: DateTime<Utc>,
}

impl PreparedWorkspace {
    /// Returns the root path of the prepared workspace.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the timestamp at which this workspace was prepared.
    #[must_use]
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Returns `true` if the workspace is backed by a temporary directory
    /// (i.e. was prepared in [`WorkspaceMode::Staged`] mode).
    #[must_use]
    pub fn is_staged(&self) -> bool {
        self._temp.is_some()
    }

    // ── Metadata ────────────────────────────────────────────────────────

    /// Collect metadata (file count, directory count, total size) about the
    /// workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace directory cannot be walked.
    pub fn metadata(&self) -> Result<WorkspaceMetadata> {
        collect_metadata(&self.path)
    }

    // ── Validation ──────────────────────────────────────────────────────

    /// Validate the workspace integrity.
    ///
    /// Checks that the root exists, is a directory, is readable, and — for
    /// staged workspaces — that the `.git` directory is present.
    #[must_use]
    pub fn validate(&self) -> ValidationResult {
        validate_workspace(&self.path, self.is_staged())
    }

    // ── Diff helpers ────────────────────────────────────────────────────

    /// Extract a structured diff of the workspace against its baseline commit.
    ///
    /// This is a convenience wrapper around [`diff::diff_workspace`].
    ///
    /// # Errors
    ///
    /// Returns an error if git commands fail.
    pub fn diff_summary(&self) -> Result<diff::DiffSummary> {
        diff::diff_workspace(self)
    }

    /// Return the list of files that changed since baseline, with
    /// classification.
    ///
    /// # Errors
    ///
    /// Returns an error if git commands fail.
    pub fn changed_files(&self) -> Result<Vec<diff::FileChange>> {
        let analyzer = diff::DiffAnalyzer::new(&self.path);
        let wd = analyzer.analyze()?;
        let mut out: Vec<diff::FileChange> = Vec::new();
        for fc in wd
            .files_added
            .into_iter()
            .chain(wd.files_modified)
            .chain(wd.files_deleted)
        {
            out.push(fc);
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    // ── Snapshot / checkpoint ───────────────────────────────────────────

    /// Capture a snapshot of the current workspace state.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be walked or files cannot be
    /// read.
    pub fn snapshot(&self) -> Result<snapshot::WorkspaceSnapshot> {
        snapshot::capture(&self.path)
    }

    // ── Cleanup ─────────────────────────────────────────────────────────

    /// Explicitly clean up the workspace.
    ///
    /// For staged workspaces this removes the temporary directory immediately
    /// rather than waiting for the value to be dropped. For pass-through
    /// workspaces this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if the temporary directory cannot be removed.
    pub fn cleanup(mut self) -> Result<()> {
        if let Some(tmp) = self._temp.take() {
            tmp.close()
                .context("remove temporary workspace directory")?;
        }
        Ok(())
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
                created_at: Utc::now(),
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
                    created_at: Utc::now(),
                })
            }
        }
    }

    /// Run `git status --porcelain=v1` in the workspace, returning `None` on failure.
    #[must_use]
    pub fn git_status(path: &Path) -> Option<String> {
        git_status_impl(path)
    }

    /// Run `git diff --no-color` in the workspace, returning `None` on failure.
    #[must_use]
    pub fn git_diff(path: &Path) -> Option<String> {
        git_diff_impl(path)
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
        anyhow::ensure!(
            root.exists(),
            "source directory does not exist: {}",
            root.display()
        );

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
            created_at: Utc::now(),
        })
    }
}

// ── Private helpers ─────────────────────────────────────────────────────

/// Collect workspace metadata by walking the directory tree.
fn collect_metadata(root: &Path) -> Result<WorkspaceMetadata> {
    let mut file_count: usize = 0;
    let mut dir_count: usize = 0;
    let mut total_size: u64 = 0;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

    for entry in walker {
        let entry = entry.with_context(|| format!("walk {}", root.display()))?;
        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        if rel.as_os_str().is_empty() {
            continue;
        }
        if entry.file_type().is_dir() {
            dir_count += 1;
        } else if entry.file_type().is_file() {
            file_count += 1;
            total_size += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }

    Ok(WorkspaceMetadata {
        root: root.to_path_buf(),
        file_count,
        dir_count,
        total_size,
        collected_at: Utc::now(),
    })
}

/// Validate workspace integrity.
fn validate_workspace(root: &Path, staged: bool) -> ValidationResult {
    let mut problems = Vec::new();

    if !root.exists() {
        problems.push(format!("workspace path does not exist: {}", root.display()));
    } else if !root.is_dir() {
        problems.push(format!(
            "workspace path is not a directory: {}",
            root.display()
        ));
    } else {
        // Check readability by attempting to list entries.
        if fs::read_dir(root).is_err() {
            problems.push(format!(
                "workspace directory is not readable: {}",
                root.display()
            ));
        }

        if staged && !root.join(".git").is_dir() {
            problems.push("staged workspace is missing .git directory".to_string());
        }
    }

    ValidationResult {
        valid: problems.is_empty(),
        problems,
    }
}

/// Compute a SHA-256 hash over all file relative paths and their sizes in a
/// workspace, for integrity fingerprinting.
///
/// The `.git` directory is excluded.
pub fn workspace_content_hash(root: &Path) -> Result<String> {
    let mut entries: Vec<(PathBuf, u64)> = Vec::new();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

    for entry in walker {
        let entry = entry.with_context(|| format!("walk {}", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        entries.push((rel.to_path_buf(), size));
    }

    let mut hasher = Sha256::new();
    for (path, size) in &entries {
        // Use forward-slash normalized paths for cross-platform determinism.
        let normalized = path.to_string_lossy().replace('\\', "/");
        hasher.update(normalized.as_bytes());
        hasher.update(b":");
        hasher.update(size.to_string().as_bytes());
        hasher.update(b"\n");
    }

    Ok(format!("{:x}", hasher.finalize()))
}
