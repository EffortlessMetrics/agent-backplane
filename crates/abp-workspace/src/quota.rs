// SPDX-License-Identifier: MIT OR Apache-2.0
//! Disk-space quotas for workspaces.
//!
//! [`WorkspaceQuota`] enforces per-workspace size limits and provides cleanup
//! helpers for reclaiming space.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Result of a quota check.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaStatus {
    /// Current total size in bytes.
    pub used_bytes: u64,
    /// Configured limit in bytes.
    pub limit_bytes: u64,
    /// Remaining bytes before the limit is reached.
    pub remaining_bytes: u64,
    /// `true` when usage exceeds the limit.
    pub exceeded: bool,
    /// Usage as a percentage (0.0–100.0+).
    pub usage_percent: f64,
}

/// Per-workspace disk-space quota.
///
/// # Examples
///
/// ```no_run
/// # use abp_workspace::quota::WorkspaceQuota;
/// let quota = WorkspaceQuota::new(10 * 1024 * 1024); // 10 MiB
/// let status = quota.check("/tmp/ws").unwrap();
/// assert!(!status.exceeded);
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceQuota {
    limit_bytes: u64,
}

impl WorkspaceQuota {
    /// Create a quota with the given byte limit.
    #[must_use]
    pub fn new(limit_bytes: u64) -> Self {
        Self { limit_bytes }
    }

    /// Create a quota from a megabyte value.
    #[must_use]
    pub fn from_mb(mb: u64) -> Self {
        Self {
            limit_bytes: mb * 1024 * 1024,
        }
    }

    /// Configured limit in bytes.
    #[must_use]
    pub fn limit_bytes(&self) -> u64 {
        self.limit_bytes
    }

    /// Check current usage of the workspace at `root` against the quota.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be walked.
    pub fn check(&self, root: impl AsRef<Path>) -> Result<QuotaStatus> {
        let used = dir_size(root.as_ref())?;
        let exceeded = used > self.limit_bytes;
        let remaining = self.limit_bytes.saturating_sub(used);
        let usage_percent = if self.limit_bytes == 0 {
            if used > 0 { f64::INFINITY } else { 0.0 }
        } else {
            (used as f64 / self.limit_bytes as f64) * 100.0
        };

        Ok(QuotaStatus {
            used_bytes: used,
            limit_bytes: self.limit_bytes,
            remaining_bytes: remaining,
            exceeded,
            usage_percent,
        })
    }

    /// Return `true` when the workspace at `root` exceeds the quota.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be walked.
    pub fn is_exceeded(&self, root: impl AsRef<Path>) -> Result<bool> {
        Ok(self.check(root)?.exceeded)
    }

    /// Delete the largest files in the workspace until usage is under the
    /// quota limit, or until there are no more deletable files.
    ///
    /// The `.git` directory is never touched.
    ///
    /// Returns the number of files deleted and the bytes reclaimed.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O failure.
    pub fn cleanup(&self, root: impl AsRef<Path>) -> Result<CleanupResult> {
        let root = root.as_ref();
        let mut used = dir_size(root)?;
        if used <= self.limit_bytes {
            return Ok(CleanupResult {
                files_deleted: 0,
                bytes_reclaimed: 0,
            });
        }

        // Collect files sorted by size descending.
        let mut files = collect_files(root)?;
        files.sort_by_key(|f| std::cmp::Reverse(f.1));

        let mut deleted = 0u64;
        let mut count = 0usize;

        for (path, size) in files {
            if used <= self.limit_bytes {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                used = used.saturating_sub(size);
                deleted += size;
                count += 1;
            }
        }

        Ok(CleanupResult {
            files_deleted: count,
            bytes_reclaimed: deleted,
        })
    }
}

/// Result of a quota [`cleanup`](WorkspaceQuota::cleanup) operation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CleanupResult {
    /// Number of files removed.
    pub files_deleted: usize,
    /// Total bytes freed.
    pub bytes_reclaimed: u64,
}

/// Compute the total size of all regular files under `root` (excluding `.git`).
fn dir_size(root: &Path) -> Result<u64> {
    let mut total: u64 = 0;
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

    for entry in walker {
        let entry = entry.with_context(|| format!("walk {}", root.display()))?;
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok(total)
}

/// Collect `(absolute_path, size)` for every regular file under `root`,
/// excluding `.git`.
fn collect_files(root: &Path) -> Result<Vec<(PathBuf, u64)>> {
    let mut out = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

    for entry in walker {
        let entry = entry.with_context(|| format!("walk {}", root.display()))?;
        if entry.file_type().is_file() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((entry.path().to_path_buf(), size));
        }
    }
    Ok(out)
}
