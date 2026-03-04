#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Workspace change tracking with git integration.
//!
//! Provides [`ChangeSet`] for collecting file-level changes since workspace
//! creation and [`WorkspaceChangeTracker`] for detecting changes via git
//! status and snapshot comparison.

use crate::snapshot::{self, WorkspaceSnapshot};
use abp_git::{git_change_stats, git_status};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// The kind of change observed for a workspace file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum FileChangeKind {
    /// A new file was created.
    Created,
    /// An existing file was modified.
    Modified,
    /// A file was deleted.
    Deleted,
    /// A file was renamed.
    Renamed {
        /// The original path before the rename.
        old: String,
        /// The new path after the rename.
        new: String,
    },
}

impl fmt::Display for FileChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Modified => write!(f, "modified"),
            Self::Deleted => write!(f, "deleted"),
            Self::Renamed { old, new } => write!(f, "renamed ({old} -> {new})"),
        }
    }
}

/// A single file change entry within a [`ChangeSet`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChangeEntry {
    /// Relative path of the affected file.
    pub path: PathBuf,
    /// What kind of change occurred.
    pub kind: FileChangeKind,
}

/// A collection of file changes since workspace creation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChangeSet {
    /// All detected changes in deterministic order.
    pub changes: Vec<FileChangeEntry>,
    /// Timestamp when the change set was collected.
    pub collected_at: DateTime<Utc>,
    /// Total lines added (if available from git).
    pub total_additions: usize,
    /// Total lines deleted (if available from git).
    pub total_deletions: usize,
}

impl ChangeSet {
    /// Create an empty change set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            changes: Vec::new(),
            collected_at: Utc::now(),
            total_additions: 0,
            total_deletions: 0,
        }
    }

    /// Number of changes in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Whether the change set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Files that were created.
    #[must_use]
    pub fn created(&self) -> Vec<&FileChangeEntry> {
        self.changes
            .iter()
            .filter(|c| matches!(c.kind, FileChangeKind::Created))
            .collect()
    }

    /// Files that were modified.
    #[must_use]
    pub fn modified(&self) -> Vec<&FileChangeEntry> {
        self.changes
            .iter()
            .filter(|c| matches!(c.kind, FileChangeKind::Modified))
            .collect()
    }

    /// Files that were deleted.
    #[must_use]
    pub fn deleted(&self) -> Vec<&FileChangeEntry> {
        self.changes
            .iter()
            .filter(|c| matches!(c.kind, FileChangeKind::Deleted))
            .collect()
    }

    /// Files that were renamed.
    #[must_use]
    pub fn renamed(&self) -> Vec<&FileChangeEntry> {
        self.changes
            .iter()
            .filter(|c| matches!(c.kind, FileChangeKind::Renamed { .. }))
            .collect()
    }

    /// Human-readable summary of the changes.
    #[must_use]
    pub fn change_summary(&self) -> String {
        if self.changes.is_empty() {
            return "No changes detected.".to_string();
        }

        let created = self.created().len();
        let modified = self.modified().len();
        let deleted = self.deleted().len();
        let renamed = self.renamed().len();

        let mut parts = Vec::new();
        if created > 0 {
            parts.push(format!("{created} created"));
        }
        if modified > 0 {
            parts.push(format!("{modified} modified"));
        }
        if deleted > 0 {
            parts.push(format!("{deleted} deleted"));
        }
        if renamed > 0 {
            parts.push(format!("{renamed} renamed"));
        }

        format!(
            "{} file(s) changed: {} (+{} -{})",
            self.changes.len(),
            parts.join(", "),
            self.total_additions,
            self.total_deletions,
        )
    }
}

/// Tracks workspace changes using git status integration.
///
/// Detects created, modified, deleted, and renamed files by querying
/// git porcelain status and change statistics.
#[derive(Debug, Clone)]
pub struct WorkspaceChangeTracker {
    workspace_path: PathBuf,
}

impl WorkspaceChangeTracker {
    /// Create a new tracker for the given workspace path.
    #[must_use]
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
        }
    }

    /// Detect all changes in the workspace since the baseline commit.
    ///
    /// # Errors
    ///
    /// Returns an error if git change stats cannot be retrieved.
    pub fn detect_changes(&self) -> Result<ChangeSet> {
        let stats =
            git_change_stats(&self.workspace_path).context("failed to get git change stats")?;

        let mut changes = Vec::new();

        for path in &stats.added {
            changes.push(FileChangeEntry {
                path: path.clone(),
                kind: FileChangeKind::Created,
            });
        }
        for path in &stats.modified {
            changes.push(FileChangeEntry {
                path: path.clone(),
                kind: FileChangeKind::Modified,
            });
        }
        for path in &stats.deleted {
            changes.push(FileChangeEntry {
                path: path.clone(),
                kind: FileChangeKind::Deleted,
            });
        }

        // Sort for deterministic ordering.
        changes.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(ChangeSet {
            changes,
            collected_at: Utc::now(),
            total_additions: stats.total_additions,
            total_deletions: stats.total_deletions,
        })
    }

    /// Detect changes by comparing two snapshots.
    #[must_use]
    pub fn detect_from_snapshots(
        before: &WorkspaceSnapshot,
        after: &WorkspaceSnapshot,
    ) -> ChangeSet {
        let diff = snapshot::compare(before, after);

        let mut changes = Vec::new();

        for path in &diff.added {
            changes.push(FileChangeEntry {
                path: path.clone(),
                kind: FileChangeKind::Created,
            });
        }
        for path in &diff.modified {
            changes.push(FileChangeEntry {
                path: path.clone(),
                kind: FileChangeKind::Modified,
            });
        }
        for path in &diff.removed {
            changes.push(FileChangeEntry {
                path: path.clone(),
                kind: FileChangeKind::Deleted,
            });
        }

        changes.sort_by(|a, b| a.path.cmp(&b.path));

        ChangeSet {
            changes,
            collected_at: Utc::now(),
            total_additions: 0,
            total_deletions: 0,
        }
    }

    /// Returns `true` if the workspace has any uncommitted changes.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        git_status(&self.workspace_path)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }
}
