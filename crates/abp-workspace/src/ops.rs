// SPDX-License-Identifier: MIT OR Apache-2.0
//! File operation tracking and filtering.
//!
//! Provides [`FileOperation`] for recording individual file-system operations,
//! [`OperationLog`] for collecting them with query helpers, and
//! [`OperationFilter`] for glob-based path filtering.

use abp_glob::IncludeExcludeGlobs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A single file-system operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum FileOperation {
    /// A file was read.
    Read {
        /// Path of the file that was read.
        path: String,
    },
    /// A file was written.
    Write {
        /// Path of the file that was written.
        path: String,
        /// Size in bytes of the written content.
        size: u64,
    },
    /// A file was deleted.
    Delete {
        /// Path of the deleted file.
        path: String,
    },
    /// A file was moved/renamed.
    Move {
        /// Original path.
        from: String,
        /// Destination path.
        to: String,
    },
    /// A file was copied.
    Copy {
        /// Source path.
        from: String,
        /// Destination path.
        to: String,
    },
    /// A directory was created.
    CreateDir {
        /// Path of the created directory.
        path: String,
    },
}

impl FileOperation {
    /// All paths referenced by this operation.
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Self::Read { path }
            | Self::Write { path, .. }
            | Self::Delete { path }
            | Self::CreateDir { path } => vec![path.as_str()],
            Self::Move { from, to } | Self::Copy { from, to } => {
                vec![from.as_str(), to.as_str()]
            }
        }
    }
}

/// Aggregate counts per operation type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSummary {
    /// Number of read operations.
    pub reads: usize,
    /// Number of write operations.
    pub writes: usize,
    /// Number of delete operations.
    pub deletes: usize,
    /// Number of move operations.
    pub moves: usize,
    /// Number of copy operations.
    pub copies: usize,
    /// Number of create-directory operations.
    pub create_dirs: usize,
    /// Total bytes written across all write operations.
    pub total_writes_bytes: u64,
}

/// Ordered log of [`FileOperation`]s with query helpers.
#[derive(Clone, Debug, Default)]
pub struct OperationLog {
    ops: Vec<FileOperation>,
}

impl OperationLog {
    /// Create an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an operation.
    pub fn record(&mut self, op: FileOperation) {
        self.ops.push(op);
    }

    /// All recorded operations in insertion order.
    #[must_use]
    pub fn operations(&self) -> &[FileOperation] {
        &self.ops
    }

    /// Paths from all [`FileOperation::Read`] entries.
    #[must_use]
    pub fn reads(&self) -> Vec<&str> {
        self.ops
            .iter()
            .filter_map(|op| match op {
                FileOperation::Read { path } => Some(path.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Paths from all [`FileOperation::Write`] entries.
    #[must_use]
    pub fn writes(&self) -> Vec<&str> {
        self.ops
            .iter()
            .filter_map(|op| match op {
                FileOperation::Write { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Paths from all [`FileOperation::Delete`] entries.
    #[must_use]
    pub fn deletes(&self) -> Vec<&str> {
        self.ops
            .iter()
            .filter_map(|op| match op {
                FileOperation::Delete { path } => Some(path.as_str()),
                _ => None,
            })
            .collect()
    }

    /// All unique paths referenced by any operation.
    #[must_use]
    pub fn affected_paths(&self) -> BTreeSet<String> {
        self.ops
            .iter()
            .flat_map(|op| op.paths().into_iter().map(String::from))
            .collect()
    }

    /// Produce an aggregate [`OperationSummary`].
    #[must_use]
    pub fn summary(&self) -> OperationSummary {
        let mut s = OperationSummary::default();
        for op in &self.ops {
            match op {
                FileOperation::Read { .. } => s.reads += 1,
                FileOperation::Write { size, .. } => {
                    s.writes += 1;
                    s.total_writes_bytes += size;
                }
                FileOperation::Delete { .. } => s.deletes += 1,
                FileOperation::Move { .. } => s.moves += 1,
                FileOperation::Copy { .. } => s.copies += 1,
                FileOperation::CreateDir { .. } => s.create_dirs += 1,
            }
        }
        s
    }

    /// Remove all recorded operations.
    pub fn clear(&mut self) {
        self.ops.clear();
    }
}

/// Glob-based filter for file operations.
///
/// Allowed patterns act as an include-list; denied patterns act as an
/// exclude-list.  Denied patterns take precedence, matching the semantics of
/// [`IncludeExcludeGlobs`].
#[derive(Clone, Debug, Default)]
pub struct OperationFilter {
    allowed: Vec<String>,
    denied: Vec<String>,
}

impl OperationFilter {
    /// Create a permissive filter (no constraints).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a glob pattern to the allow-list.
    pub fn add_allowed_path(&mut self, pattern: &str) {
        self.allowed.push(pattern.to_string());
    }

    /// Add a glob pattern to the deny-list.
    pub fn add_denied_path(&mut self, pattern: &str) {
        self.denied.push(pattern.to_string());
    }

    /// Check whether `path` is allowed by the current rules.
    ///
    /// Returns `true` when glob compilation fails (fail-open) so callers can
    /// still rely on this in non-critical paths.
    #[must_use]
    pub fn is_allowed(&self, path: &str) -> bool {
        let Ok(globs) = IncludeExcludeGlobs::new(&self.allowed, &self.denied) else {
            return true;
        };
        globs.decide_str(path).is_allowed()
    }

    /// Return only the operations whose *every* referenced path is allowed.
    #[must_use]
    pub fn filter_operations<'a>(&self, ops: &'a [FileOperation]) -> Vec<&'a FileOperation> {
        let Ok(globs) = IncludeExcludeGlobs::new(&self.allowed, &self.denied) else {
            return ops.iter().collect();
        };
        ops.iter()
            .filter(|op| op.paths().iter().all(|p| globs.decide_str(p).is_allowed()))
            .collect()
    }
}
