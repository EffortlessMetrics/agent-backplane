// SPDX-License-Identifier: MIT OR Apache-2.0
//! Workspace diff analysis utilities.
//!
//! Provides [`DiffSummary`] and [`diff_workspace`] for analysing changes in a
//! [`PreparedWorkspace`] against its baseline git commit.

use crate::PreparedWorkspace;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
