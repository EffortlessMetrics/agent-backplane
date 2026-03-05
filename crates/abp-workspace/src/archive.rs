// SPDX-License-Identifier: MIT OR Apache-2.0
//! Workspace archive creation and restoration.
//!
//! Provides [`WorkspaceArchive`] for creating compressed tarball snapshots
//! of workspace state and restoring them for rollback.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Metadata about a created archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveMetadata {
    /// Number of files in the archive.
    pub file_count: usize,
    /// Total uncompressed size in bytes.
    pub uncompressed_size: u64,
    /// Compressed archive size in bytes.
    pub compressed_size: u64,
    /// Timestamp when the archive was created.
    pub created_at: DateTime<Utc>,
}

/// An entry in an archive listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveEntry {
    /// Relative file path.
    pub path: String,
    /// File size in bytes.
    pub size: u64,
}

/// Workspace archive utility for creating and restoring tarball snapshots.
#[derive(Debug)]
pub struct WorkspaceArchive;

impl WorkspaceArchive {
    /// Create a compressed tar.gz archive of the workspace at `workspace_path`
    /// and write it to `output_path`.
    ///
    /// Excludes the `.git` directory.
    ///
    /// # Errors
    ///
    /// Returns an error if I/O operations fail.
    pub fn create(workspace_path: &Path, output_path: &Path) -> Result<ArchiveMetadata> {
        let data = Self::create_bytes(workspace_path)?;
        let metadata = ArchiveMetadata {
            file_count: count_files(workspace_path)?,
            uncompressed_size: total_file_size(workspace_path)?,
            compressed_size: data.len() as u64,
            created_at: Utc::now(),
        };
        fs::write(output_path, &data)
            .with_context(|| format!("write archive to {}", output_path.display()))?;
        Ok(metadata)
    }

    /// Create a compressed tar.gz archive in memory.
    ///
    /// # Errors
    ///
    /// Returns an error if I/O operations fail.
    pub fn create_bytes(workspace_path: &Path) -> Result<Vec<u8>> {
        let buf = Vec::new();
        let enc = GzEncoder::new(buf, Compression::default());
        let mut tar_builder = tar::Builder::new(enc);

        let walker = WalkDir::new(workspace_path)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

        for entry in walker {
            let entry = entry.context("walk workspace")?;
            if !entry.file_type().is_file() {
                continue;
            }
            let abs = entry.path();
            let rel = abs.strip_prefix(workspace_path).unwrap_or(abs);
            // Normalize to forward slashes for cross-platform archives.
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            tar_builder
                .append_path_with_name(abs, &rel_str)
                .with_context(|| format!("archive {rel_str}"))?;
        }

        let enc = tar_builder.into_inner().context("finalize tar")?;
        let data = enc.finish().context("finalize gzip")?;
        Ok(data)
    }

    /// Restore an archive from `archive_path` into `target_path`.
    ///
    /// # Errors
    ///
    /// Returns an error if I/O operations fail or the archive is invalid.
    pub fn restore(archive_path: &Path, target_path: &Path) -> Result<()> {
        let data = fs::read(archive_path)
            .with_context(|| format!("read archive {}", archive_path.display()))?;
        Self::restore_bytes(&data, target_path)
    }

    /// Restore an archive from in-memory bytes into `target_path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive is invalid or I/O fails.
    pub fn restore_bytes(data: &[u8], target_path: &Path) -> Result<()> {
        fs::create_dir_all(target_path)
            .with_context(|| format!("create target dir {}", target_path.display()))?;

        let decoder = GzDecoder::new(data);
        let mut archive = tar::Archive::new(decoder);

        archive
            .unpack(target_path)
            .context("unpack tar.gz archive")?;

        Ok(())
    }

    /// List the files in an archive without extracting.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive is invalid.
    pub fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        let data = fs::read(archive_path)
            .with_context(|| format!("read archive {}", archive_path.display()))?;
        Self::list_bytes(&data)
    }

    /// List files from in-memory archive bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive is invalid.
    pub fn list_bytes(data: &[u8]) -> Result<Vec<ArchiveEntry>> {
        let decoder = GzDecoder::new(data);
        let mut archive = tar::Archive::new(decoder);
        let mut entries = Vec::new();

        for entry in archive.entries().context("read archive entries")? {
            let entry = entry.context("read archive entry")?;
            let path = entry
                .path()
                .context("entry path")?
                .to_string_lossy()
                .to_string();
            let size = entry.size();
            entries.push(ArchiveEntry { path, size });
        }

        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }
}

fn count_files(root: &Path) -> Result<usize> {
    let mut count = 0usize;
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));
    for entry in walker {
        let entry = entry.context("walk")?;
        if entry.file_type().is_file() {
            count += 1;
        }
    }
    Ok(count)
}

fn total_file_size(root: &Path) -> Result<u64> {
    let mut total = 0u64;
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));
    for entry in walker {
        let entry = entry.context("walk")?;
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok(total)
}
