// SPDX-License-Identifier: MIT OR Apache-2.0
//! Workspace snapshot and comparison utilities.
//!
//! Provides [`WorkspaceSnapshot`] for capturing the state of a directory tree
//! and [`compare`] for computing the difference between two snapshots.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Metadata snapshot of a single file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileSnapshot {
    /// File size in bytes.
    pub size: u64,
    /// Hex-encoded SHA-256 hash of the file contents.
    pub sha256: String,
    /// Whether the file appears to be binary.
    pub is_binary: bool,
}

/// Point-in-time snapshot of an entire workspace directory tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    /// All files keyed by their path relative to the snapshot root.
    pub files: BTreeMap<PathBuf, FileSnapshot>,
    /// Timestamp when the snapshot was created.
    pub created_at: DateTime<Utc>,
    /// Root directory that was snapshotted.
    pub root: PathBuf,
}

/// In-memory file contents captured alongside a [`WorkspaceSnapshot`].
///
/// Used by [`restore_snapshot`] to restore file content without requiring the
/// original snapshot root to still be present on disk.
#[derive(Clone, Debug, Default)]
pub struct SnapshotContents {
    /// File contents keyed by relative path.
    pub contents: BTreeMap<PathBuf, Vec<u8>>,
}

impl WorkspaceSnapshot {
    /// Number of files in the snapshot.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Sum of all file sizes in bytes.
    #[must_use]
    pub fn total_size(&self) -> u64 {
        self.files.values().map(|f| f.size).sum()
    }

    /// Check whether the snapshot contains a file at `path`.
    #[must_use]
    pub fn has_file(&self, path: impl AsRef<Path>) -> bool {
        self.files.contains_key(path.as_ref())
    }

    /// Look up a file snapshot by relative path.
    #[must_use]
    pub fn get_file(&self, path: impl AsRef<Path>) -> Option<&FileSnapshot> {
        self.files.get(path.as_ref())
    }
}

/// Capture a [`WorkspaceSnapshot`] of the directory at `path`.
///
/// Walks the directory tree (excluding `.git`), hashes every regular file with
/// SHA-256, and records size and binary detection.
///
/// # Errors
///
/// Returns an error if the directory cannot be read or a file cannot be hashed.
pub fn capture(path: &Path) -> Result<WorkspaceSnapshot> {
    let (snapshot, _contents) = capture_with_contents(path)?;
    Ok(snapshot)
}

/// Capture a [`WorkspaceSnapshot`] together with the raw file contents.
///
/// The returned [`SnapshotContents`] can be passed to [`restore_snapshot`]
/// to restore the workspace even if files have been modified or deleted
/// since the snapshot was taken.
///
/// # Errors
///
/// Returns an error if the directory cannot be read or a file cannot be hashed.
pub fn capture_with_contents(path: &Path) -> Result<(WorkspaceSnapshot, SnapshotContents)> {
    let root = path
        .canonicalize()
        .with_context(|| format!("canonicalize {}", path.display()))?;

    let mut files = BTreeMap::new();
    let mut contents_map = BTreeMap::new();

    let walker = WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

    for entry in walker {
        let entry = entry.with_context(|| "walk directory")?;
        if !entry.file_type().is_file() {
            continue;
        }

        let abs = entry.path();
        let rel = abs.strip_prefix(&root).unwrap_or(abs);

        let content = fs::read(abs).with_context(|| format!("read {}", abs.display()))?;

        let mut hasher = Sha256::new();
        hasher.update(&content);
        let sha256 = format!("{:x}", hasher.finalize());

        let is_binary = content.iter().take(8192).any(|&b| b == 0);

        let rel_buf = rel.to_path_buf();
        files.insert(
            rel_buf.clone(),
            FileSnapshot {
                size: content.len() as u64,
                sha256,
                is_binary,
            },
        );
        contents_map.insert(rel_buf, content);
    }

    Ok((
        WorkspaceSnapshot {
            files,
            created_at: Utc::now(),
            root: root.clone(),
        },
        SnapshotContents {
            contents: contents_map,
        },
    ))
}

/// Result of comparing two workspace snapshots.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SnapshotDiff {
    /// Files present only in the second snapshot.
    pub added: Vec<PathBuf>,
    /// Files present only in the first snapshot.
    pub removed: Vec<PathBuf>,
    /// Files present in both but with different content hashes.
    pub modified: Vec<PathBuf>,
    /// Files present in both with identical content hashes.
    pub unchanged: Vec<PathBuf>,
}

/// Compare two snapshots and return a [`SnapshotDiff`].
#[must_use]
pub fn compare(a: &WorkspaceSnapshot, b: &WorkspaceSnapshot) -> SnapshotDiff {
    let keys_a: BTreeSet<&PathBuf> = a.files.keys().collect();
    let keys_b: BTreeSet<&PathBuf> = b.files.keys().collect();

    let mut diff = SnapshotDiff::default();

    for path in keys_a.difference(&keys_b) {
        diff.removed.push((*path).clone());
    }
    for path in keys_b.difference(&keys_a) {
        diff.added.push((*path).clone());
    }
    for path in keys_a.intersection(&keys_b) {
        let fa = &a.files[*path];
        let fb = &b.files[*path];
        if fa.sha256 == fb.sha256 {
            diff.unchanged.push((*path).clone());
        } else {
            diff.modified.push((*path).clone());
        }
    }

    diff.added.sort();
    diff.removed.sort();
    diff.modified.sort();
    diff.unchanged.sort();

    diff
}

/// Alias for [`compare`] — compare two snapshots and return a
/// [`SnapshotDiff`].
#[must_use]
pub fn compare_snapshots(before: &WorkspaceSnapshot, after: &WorkspaceSnapshot) -> SnapshotDiff {
    compare(before, after)
}

/// Restore a workspace directory to the state captured in a snapshot.
///
/// When `contents` is provided, file data is read from the in-memory store.
/// Otherwise the function falls back to reading from the snapshot's original
/// root path.  Files present on disk but absent from the snapshot are removed.
/// The `.git` directory is left untouched.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn restore_snapshot(
    workspace_path: &Path,
    snapshot: &WorkspaceSnapshot,
    contents: Option<&SnapshotContents>,
) -> Result<()> {
    // Collect current files on disk (excluding .git).
    let mut current_files: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();

    let walker = WalkDir::new(workspace_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

    for entry in walker {
        let entry = entry.with_context(|| "walk workspace")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(workspace_path)
            .unwrap_or(entry.path());
        current_files.insert(rel.to_path_buf());
    }

    let snapshot_files: std::collections::BTreeSet<PathBuf> =
        snapshot.files.keys().cloned().collect();

    // Delete files not in snapshot.
    for rel in current_files.difference(&snapshot_files) {
        let abs = workspace_path.join(rel);
        if abs.exists() {
            fs::remove_file(&abs).with_context(|| format!("remove {}", abs.display()))?;
        }
    }

    // Restore / create files from snapshot.
    for rel in snapshot.files.keys() {
        let dest = workspace_path.join(rel);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }

        // Prefer in-memory content, fall back to reading from snapshot root.
        let content = if let Some(c) = contents.and_then(|sc| sc.contents.get(rel)) {
            c.clone()
        } else {
            let src = snapshot.root.join(rel);
            if src.exists() {
                fs::read(&src).with_context(|| format!("read {}", src.display()))?
            } else {
                Vec::new()
            }
        };

        fs::write(&dest, &content).with_context(|| format!("write {}", dest.display()))?;
    }

    Ok(())
}
