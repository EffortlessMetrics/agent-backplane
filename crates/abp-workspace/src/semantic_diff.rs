// SPDX-License-Identifier: MIT OR Apache-2.0
//! Semantic diff analysis with change classification.
//!
//! Provides [`SemanticDiff`] for classifying workspace changes as
//! Added/Modified/Deleted/Renamed with file-level and line-level detail.

use crate::diff::{DiffAnalysis, DiffChangeKind, DiffLineKind, FileDiff};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Classification of a semantic change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SemanticChangeKind {
    /// A new file was added.
    Added,
    /// An existing file was modified.
    Modified,
    /// A file was deleted.
    Deleted,
    /// A file was renamed.
    Renamed {
        /// Original path before rename.
        from: String,
        /// New path after rename.
        to: String,
    },
}

impl fmt::Display for SemanticChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added => write!(f, "added"),
            Self::Modified => write!(f, "modified"),
            Self::Deleted => write!(f, "deleted"),
            Self::Renamed { from, to } => write!(f, "renamed ({from} -> {to})"),
        }
    }
}

/// Kind of line-level change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineChangeKind {
    /// Line was added.
    Added,
    /// Line was removed.
    Removed,
}

impl fmt::Display for LineChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added => write!(f, "+"),
            Self::Removed => write!(f, "-"),
        }
    }
}

/// A single line-level change within a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineChange {
    /// Line number (1-based; in the new file for additions, old file for removals).
    pub line_number: usize,
    /// Whether this line was added or removed.
    pub kind: LineChangeKind,
    /// Content of the line.
    pub content: String,
}

/// A file-level semantic change with optional line-level detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticFileChange {
    /// File path (new path for renames).
    pub path: String,
    /// Kind of change.
    pub kind: SemanticChangeKind,
    /// Lines added in this file.
    pub additions: usize,
    /// Lines removed in this file.
    pub deletions: usize,
    /// Individual line-level changes (may be empty for binary files).
    pub line_changes: Vec<LineChange>,
}

impl SemanticFileChange {
    /// Total number of changed lines (additions + deletions).
    #[must_use]
    pub fn total_changes(&self) -> usize {
        self.additions + self.deletions
    }
}

/// Structured semantic diff with file and line-level change tracking.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticDiff {
    /// Per-file semantic changes.
    pub files: Vec<SemanticFileChange>,
    /// Total lines added across all files.
    pub total_additions: usize,
    /// Total lines removed across all files.
    pub total_deletions: usize,
}

impl SemanticDiff {
    /// Build a [`SemanticDiff`] from a parsed [`DiffAnalysis`].
    #[must_use]
    pub fn from_analysis(analysis: &DiffAnalysis) -> Self {
        let mut files = Vec::new();

        for fd in &analysis.files {
            let kind = match fd.change_kind {
                DiffChangeKind::Added => SemanticChangeKind::Added,
                DiffChangeKind::Modified => SemanticChangeKind::Modified,
                DiffChangeKind::Deleted => SemanticChangeKind::Deleted,
                DiffChangeKind::Renamed => SemanticChangeKind::Renamed {
                    from: fd.renamed_from.clone().unwrap_or_default(),
                    to: fd.path.clone(),
                },
            };

            let line_changes = extract_line_changes(fd);

            files.push(SemanticFileChange {
                path: fd.path.clone(),
                kind,
                additions: fd.additions,
                deletions: fd.deletions,
                line_changes,
            });
        }

        Self {
            total_additions: analysis.total_additions,
            total_deletions: analysis.total_deletions,
            files,
        }
    }

    /// Returns `true` when no changes are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Number of changed files.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Files classified as added.
    #[must_use]
    pub fn added_files(&self) -> Vec<&SemanticFileChange> {
        self.files
            .iter()
            .filter(|f| matches!(f.kind, SemanticChangeKind::Added))
            .collect()
    }

    /// Files classified as modified.
    #[must_use]
    pub fn modified_files(&self) -> Vec<&SemanticFileChange> {
        self.files
            .iter()
            .filter(|f| matches!(f.kind, SemanticChangeKind::Modified))
            .collect()
    }

    /// Files classified as deleted.
    #[must_use]
    pub fn deleted_files(&self) -> Vec<&SemanticFileChange> {
        self.files
            .iter()
            .filter(|f| matches!(f.kind, SemanticChangeKind::Deleted))
            .collect()
    }

    /// Files classified as renamed.
    #[must_use]
    pub fn renamed_files(&self) -> Vec<&SemanticFileChange> {
        self.files
            .iter()
            .filter(|f| matches!(f.kind, SemanticChangeKind::Renamed { .. }))
            .collect()
    }

    /// Human-readable summary of the diff.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.files.is_empty() {
            return "No changes detected.".to_string();
        }

        let added = self.added_files().len();
        let modified = self.modified_files().len();
        let deleted = self.deleted_files().len();
        let renamed = self.renamed_files().len();

        let mut parts = Vec::new();
        if added > 0 {
            parts.push(format!("{added} added"));
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
            self.files.len(),
            parts.join(", "),
            self.total_additions,
            self.total_deletions,
        )
    }
}

impl fmt::Display for SemanticDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

/// Extract line-level changes from a [`FileDiff`].
fn extract_line_changes(fd: &FileDiff) -> Vec<LineChange> {
    let mut changes = Vec::new();

    for hunk in &fd.hunks {
        let mut old_line = hunk.old_start;
        let mut new_line = hunk.new_start;

        for dl in &hunk.lines {
            match dl.kind {
                DiffLineKind::Added => {
                    changes.push(LineChange {
                        line_number: new_line,
                        kind: LineChangeKind::Added,
                        content: dl.content.clone(),
                    });
                    new_line += 1;
                }
                DiffLineKind::Removed => {
                    changes.push(LineChange {
                        line_number: old_line,
                        kind: LineChangeKind::Removed,
                        content: dl.content.clone(),
                    });
                    old_line += 1;
                }
                DiffLineKind::Context => {
                    old_line += 1;
                    new_line += 1;
                }
                DiffLineKind::NoNewlineMarker => {}
            }
        }
    }

    changes
}
