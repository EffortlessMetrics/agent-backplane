// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
#![warn(missing_docs)]
//! File change tracking primitives.
//!
//! Provides [`ChangeTracker`] for recording file-level changes and producing
//! a [`ChangeSummary`] with aggregate statistics.

use serde::{Deserialize, Serialize};

/// The kind of change observed for a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ChangeKind {
    /// A new file was created.
    Created,
    /// An existing file was modified.
    Modified,
    /// A file was deleted.
    Deleted,
    /// A file was renamed from a previous path.
    Renamed {
        /// The original path before the rename.
        from: String,
    },
}

/// A single recorded file change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Relative path of the affected file.
    pub path: String,
    /// What kind of change occurred.
    pub kind: ChangeKind,
    /// File size in bytes before the change, if known.
    pub size_before: Option<u64>,
    /// File size in bytes after the change, if known.
    pub size_after: Option<u64>,
    /// Hex-encoded SHA-256 content hash after the change, if known.
    pub content_hash: Option<String>,
}

/// Aggregate statistics derived from a set of [`FileChange`]s.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSummary {
    /// Number of created files.
    pub created: usize,
    /// Number of modified files.
    pub modified: usize,
    /// Number of deleted files.
    pub deleted: usize,
    /// Number of renamed files.
    pub renamed: usize,
    /// Net change in total file size (may be negative).
    pub total_size_delta: i64,
}

/// Records [`FileChange`]s and provides query/summary helpers.
#[derive(Clone, Debug, Default)]
pub struct ChangeTracker {
    changes: Vec<FileChange>,
}

impl ChangeTracker {
    /// Create a new, empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a file change.
    pub fn record(&mut self, change: FileChange) {
        self.changes.push(change);
    }

    /// All recorded changes in insertion order.
    #[must_use]
    pub fn changes(&self) -> &[FileChange] {
        &self.changes
    }

    /// Produce an aggregate [`ChangeSummary`].
    #[must_use]
    pub fn summary(&self) -> ChangeSummary {
        let mut s = ChangeSummary::default();
        for c in &self.changes {
            match &c.kind {
                ChangeKind::Created => s.created += 1,
                ChangeKind::Modified => s.modified += 1,
                ChangeKind::Deleted => s.deleted += 1,
                ChangeKind::Renamed { .. } => s.renamed += 1,
            }
            let before = c.size_before.unwrap_or(0) as i64;
            let after = c.size_after.unwrap_or(0) as i64;
            s.total_size_delta += after - before;
        }
        s
    }

    /// Return changes matching a specific [`ChangeKind`].
    #[must_use]
    pub fn by_kind(&self, kind: &ChangeKind) -> Vec<&FileChange> {
        self.changes.iter().filter(|c| &c.kind == kind).collect()
    }

    /// Unique affected file paths in insertion order.
    #[must_use]
    pub fn affected_paths(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for c in &self.changes {
            if !seen.contains(&c.path.as_str()) {
                seen.push(c.path.as_str());
            }
        }
        seen
    }

    /// Whether any changes have been recorded.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Remove all recorded changes.
    pub fn clear(&mut self) {
        self.changes.clear();
    }
}
