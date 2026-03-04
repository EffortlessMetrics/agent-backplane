// SPDX-License-Identifier: MIT OR Apache-2.0
//! Workspace diff analysis utilities.
//!
//! Provides [`DiffSummary`] and [`diff_workspace`] for analysing changes in a
//! [`PreparedWorkspace`] against its baseline git commit.
//!
//! Higher-level utilities [`WorkspaceDiff`], [`DiffAnalyzer`], and
//! [`DiffPolicy`] build on the raw summary to support per-file change
//! tracking, path-based querying, and policy enforcement.

use crate::PreparedWorkspace;
use abp_glob::IncludeExcludeGlobs;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Summary of changes in a workspace compared to its baseline commit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Files that were added (new, previously untracked).
    pub added: Vec<PathBuf>,
    /// Files that were modified.
    pub modified: Vec<PathBuf>,
    /// Files that were deleted.
    pub deleted: Vec<PathBuf>,
    /// Total number of lines added across all files.
    pub total_additions: usize,
    /// Total number of lines removed across all files.
    pub total_deletions: usize,
}

impl DiffSummary {
    /// Returns `true` when no changes were detected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    /// Total number of files changed (added + modified + deleted).
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }

    /// Total line-level changes (additions + deletions).
    #[must_use]
    pub fn total_changes(&self) -> usize {
        self.total_additions + self.total_deletions
    }
}

/// Analyse the workspace diff by running `git add -A` followed by
/// `git diff --cached --numstat` and `git diff --cached --name-status`.
///
/// The workspace must have been staged with git initialisation enabled
/// (the default for [`WorkspaceStager`](crate::WorkspaceStager) and
/// [`WorkspaceManager::prepare`](crate::WorkspaceManager::prepare) in
/// [`Staged`](abp_core::WorkspaceMode::Staged) mode).
///
/// # Errors
///
/// Returns an error if git commands fail (e.g. no git repo in the workspace).
pub fn diff_workspace(workspace: &PreparedWorkspace) -> Result<DiffSummary> {
    let path = workspace.path();

    // Stage everything so new/deleted files are visible in the diff.
    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .output()
        .context("run git add -A")?;
    if !status.status.success() {
        anyhow::bail!(
            "git add -A failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    // --name-status gives us the classification (A/M/D) per file.
    let name_status_out = Command::new("git")
        .args(["diff", "--cached", "--name-status"])
        .current_dir(path)
        .output()
        .context("run git diff --cached --name-status")?;
    if !name_status_out.status.success() {
        anyhow::bail!(
            "git diff --name-status failed: {}",
            String::from_utf8_lossy(&name_status_out.stderr)
        );
    }

    // --numstat gives us line counts per file (binary files show `-\t-`).
    let numstat_out = Command::new("git")
        .args(["diff", "--cached", "--numstat"])
        .current_dir(path)
        .output()
        .context("run git diff --cached --numstat")?;
    if !numstat_out.status.success() {
        anyhow::bail!(
            "git diff --numstat failed: {}",
            String::from_utf8_lossy(&numstat_out.stderr)
        );
    }

    let name_status = String::from_utf8_lossy(&name_status_out.stdout);
    let numstat = String::from_utf8_lossy(&numstat_out.stdout);

    let mut summary = DiffSummary::default();

    // Parse name-status lines: "<status>\t<path>"
    for line in name_status.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Split on first tab
        let (status_code, file_path) = match line.split_once('\t') {
            Some(pair) => pair,
            None => continue,
        };
        let path = PathBuf::from(file_path);
        match status_code.chars().next() {
            Some('A') => summary.added.push(path),
            Some('M') => summary.modified.push(path),
            Some('D') => summary.deleted.push(path),
            // Treat renames/copies/etc. as modifications for simplicity.
            _ => summary.modified.push(path),
        }
    }

    // Parse numstat lines: "<added>\t<deleted>\t<path>"
    // Binary files show "-\t-\t<path>".
    for line in numstat.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        // Skip binary entries (shown as "-")
        if parts[0] == "-" || parts[1] == "-" {
            continue;
        }
        if let (Ok(added), Ok(deleted)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
            summary.total_additions += added;
            summary.total_deletions += deleted;
        }
    }

    // Sort paths for deterministic output.
    summary.added.sort();
    summary.modified.sort();
    summary.deleted.sort();

    Ok(summary)
}

// ---------------------------------------------------------------------------
// Per-file change types
// ---------------------------------------------------------------------------

/// Classification of a single file change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    /// File was newly created.
    Added,
    /// File was modified.
    Modified,
    /// File was deleted.
    Deleted,
}

impl fmt::Display for ChangeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added => write!(f, "added"),
            Self::Modified => write!(f, "modified"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

/// Detailed information about a single changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Relative path of the changed file.
    pub path: PathBuf,
    /// Whether the file was added, modified, or deleted.
    pub change_type: ChangeType,
    /// Number of lines added in this file.
    pub additions: usize,
    /// Number of lines deleted in this file.
    pub deletions: usize,
    /// Whether the file is binary (line counts will be zero).
    pub is_binary: bool,
}

// ---------------------------------------------------------------------------
// WorkspaceDiff — rich per-file diff result
// ---------------------------------------------------------------------------

/// Rich diff result with per-file change details.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDiff {
    /// Files that were added.
    pub files_added: Vec<FileChange>,
    /// Files that were modified.
    pub files_modified: Vec<FileChange>,
    /// Files that were deleted.
    pub files_deleted: Vec<FileChange>,
    /// Total lines added across all files.
    pub total_additions: usize,
    /// Total lines deleted across all files.
    pub total_deletions: usize,
}

impl WorkspaceDiff {
    /// Human-readable summary of the diff.
    #[must_use]
    pub fn summary(&self) -> String {
        let added = self.files_added.len();
        let modified = self.files_modified.len();
        let deleted = self.files_deleted.len();
        let total = added + modified + deleted;
        if total == 0 {
            return "No changes detected.".to_string();
        }
        format!(
            "{total} file(s) changed: {added} added, {modified} modified, {deleted} deleted (+{} -{})",
            self.total_additions, self.total_deletions,
        )
    }

    /// Returns `true` when no changes were detected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files_added.is_empty()
            && self.files_modified.is_empty()
            && self.files_deleted.is_empty()
    }

    /// Total number of files changed.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files_added.len() + self.files_modified.len() + self.files_deleted.len()
    }
}

// ---------------------------------------------------------------------------
// DiffAnalyzer — workspace-oriented query API
// ---------------------------------------------------------------------------

/// Analyses changes in a workspace directory against its baseline git commit.
///
/// The workspace must contain a `.git` directory with at least one commit
/// (created automatically by [`WorkspaceStager`](crate::WorkspaceStager)).
#[derive(Debug, Clone)]
pub struct DiffAnalyzer {
    workspace_path: PathBuf,
}

impl DiffAnalyzer {
    /// Create a new analyser for the given workspace path.
    #[must_use]
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
        }
    }

    /// Run a full diff analysis and return a [`WorkspaceDiff`].
    ///
    /// # Errors
    ///
    /// Returns an error if git commands fail.
    pub fn analyze(&self) -> Result<WorkspaceDiff> {
        let path = &self.workspace_path;

        // Stage everything.
        let status = Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .context("run git add -A")?;
        if !status.status.success() {
            anyhow::bail!(
                "git add -A failed: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        }

        let name_status = run_git_output(path, &["diff", "--cached", "--name-status"])?;
        let numstat = run_git_output(path, &["diff", "--cached", "--numstat"])?;

        // Build per-file stat map: path -> (additions, deletions, is_binary)
        let mut stat_map: std::collections::HashMap<String, (usize, usize, bool)> =
            std::collections::HashMap::new();
        for line in numstat.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let file = parts[2].to_string();
            if parts[0] == "-" || parts[1] == "-" {
                stat_map.insert(file, (0, 0, true));
            } else if let (Ok(a), Ok(d)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                stat_map.insert(file, (a, d, false));
            }
        }

        let mut diff = WorkspaceDiff::default();

        for line in name_status.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (status_code, file_path) = match line.split_once('\t') {
                Some(pair) => pair,
                None => continue,
            };
            let (additions, deletions, is_binary) =
                stat_map.get(file_path).copied().unwrap_or((0, 0, false));

            let change_type = match status_code.chars().next() {
                Some('A') => ChangeType::Added,
                Some('D') => ChangeType::Deleted,
                _ => ChangeType::Modified,
            };

            let fc = FileChange {
                path: PathBuf::from(file_path),
                change_type,
                additions,
                deletions,
                is_binary,
            };

            diff.total_additions += additions;
            diff.total_deletions += deletions;

            match change_type {
                ChangeType::Added => diff.files_added.push(fc),
                ChangeType::Modified => diff.files_modified.push(fc),
                ChangeType::Deleted => diff.files_deleted.push(fc),
            }
        }

        // Deterministic ordering.
        diff.files_added.sort_by(|a, b| a.path.cmp(&b.path));
        diff.files_modified.sort_by(|a, b| a.path.cmp(&b.path));
        diff.files_deleted.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(diff)
    }

    /// Returns `true` if there are any uncommitted changes in the workspace.
    pub fn has_changes(&self) -> bool {
        let Ok(status) = run_git_output(&self.workspace_path, &["status", "--porcelain=v1"]) else {
            return false;
        };
        !status.trim().is_empty()
    }

    /// List all changed file paths (added, modified, and deleted).
    pub fn changed_files(&self) -> Vec<PathBuf> {
        let Ok(diff) = self.analyze() else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = diff
            .files_added
            .iter()
            .chain(diff.files_modified.iter())
            .chain(diff.files_deleted.iter())
            .map(|fc| fc.path.clone())
            .collect();
        files.sort();
        files
    }

    /// Check whether a specific path was modified (added, changed, or deleted).
    pub fn file_was_modified(&self, path: &Path) -> bool {
        let Ok(diff) = self.analyze() else {
            return false;
        };
        diff.files_added
            .iter()
            .chain(diff.files_modified.iter())
            .chain(diff.files_deleted.iter())
            .any(|fc| fc.path == path)
    }
}

/// Run a git command and return stdout as a `String`.
fn run_git_output(path: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .with_context(|| format!("run git {args:?}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

// ---------------------------------------------------------------------------
// DiffPolicy — enforce constraints on workspace changes
// ---------------------------------------------------------------------------

/// Outcome of a policy check against a [`WorkspaceDiff`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum PolicyResult {
    /// The diff satisfies all policy constraints.
    Pass,
    /// One or more policy constraints were violated.
    Fail {
        /// Human-readable descriptions of each violation.
        violations: Vec<String>,
    },
}

impl PolicyResult {
    /// Returns `true` when the policy passed.
    #[must_use]
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// Constraints that a workspace diff must satisfy.
///
/// All fields are optional — omitted fields impose no limit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffPolicy {
    /// Maximum number of changed files allowed.
    pub max_files: Option<usize>,
    /// Maximum number of added lines allowed.
    pub max_additions: Option<usize>,
    /// Glob patterns for paths that must not be changed.
    pub denied_paths: Vec<String>,
}

impl DiffPolicy {
    /// Evaluate the policy against a [`WorkspaceDiff`].
    ///
    /// # Errors
    ///
    /// Returns an error if the `denied_paths` globs fail to compile.
    pub fn check(&self, diff: &WorkspaceDiff) -> Result<PolicyResult> {
        let mut violations: Vec<String> = Vec::new();

        if let Some(max) = self.max_files {
            let count = diff.file_count();
            if count > max {
                violations.push(format!("too many files changed: {count} (max {max})"));
            }
        }

        if let Some(max) = self.max_additions {
            if diff.total_additions > max {
                violations.push(format!(
                    "too many additions: {} (max {max})",
                    diff.total_additions
                ));
            }
        }

        if !self.denied_paths.is_empty() {
            let globs = IncludeExcludeGlobs::new(&self.denied_paths, &[])
                .context("compile denied_paths globs")?;

            let all_files = diff
                .files_added
                .iter()
                .chain(diff.files_modified.iter())
                .chain(diff.files_deleted.iter());

            for fc in all_files {
                if globs.decide_path(&fc.path).is_allowed() {
                    violations.push(format!("change to denied path: {}", fc.path.display()));
                }
            }
        }

        if violations.is_empty() {
            Ok(PolicyResult::Pass)
        } else {
            Ok(PolicyResult::Fail { violations })
        }
    }
}
