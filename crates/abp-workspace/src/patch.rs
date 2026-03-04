#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Patch creation and application for workspace changes.
//!
//! Provides utilities for creating git-compatible patches from workspace
//! changes and applying patches to workspace directories.

use abp_git::git_create_patch;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Header metadata for a patch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchHeader {
    /// Source (from) reference.
    pub from: String,
    /// Target (to) reference.
    pub to: String,
    /// Author of the patch.
    pub author: String,
    /// Timestamp when the patch was created.
    pub date: DateTime<Utc>,
    /// Description of the patch.
    pub description: String,
}

/// A complete patch with header and diff content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Patch {
    /// Patch metadata.
    pub header: PatchHeader,
    /// The raw unified diff content.
    pub diff_content: String,
}

impl Patch {
    /// Format the patch as a git-compatible patch string.
    #[must_use]
    pub fn to_patch_string(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("From: {}\n", self.header.author));
        out.push_str(&format!("Date: {}\n", self.header.date.to_rfc3339()));
        out.push_str(&format!("Subject: {}\n", self.header.description));
        out.push_str(&format!("Ref-From: {}\n", self.header.from));
        out.push_str(&format!("Ref-To: {}\n", self.header.to));
        out.push('\n');
        out.push_str("---\n");
        out.push('\n');
        out.push_str(&self.diff_content);
        out
    }

    /// Returns `true` when the patch has no diff content.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diff_content.trim().is_empty()
    }
}

/// Create a unified patch from all workspace changes relative to HEAD.
///
/// Uses `git add -A` + `git diff --cached` under the hood via
/// [`abp_git::git_create_patch`].
///
/// # Errors
///
/// Returns an error if git commands fail or the workspace has no git repo.
pub fn create_patch(workspace_path: &Path) -> Result<String> {
    git_create_patch(workspace_path).context("failed to create patch from workspace changes")
}

/// Create a [`Patch`] with full metadata from workspace changes.
///
/// # Errors
///
/// Returns an error if git commands fail.
pub fn create_patch_with_header(workspace_path: &Path, description: &str) -> Result<Patch> {
    let diff_content = create_patch(workspace_path)?;

    Ok(Patch {
        header: PatchHeader {
            from: "HEAD".to_string(),
            to: "working-tree".to_string(),
            author: "abp".to_string(),
            date: Utc::now(),
            description: description.to_string(),
        },
        diff_content,
    })
}

/// Validate that a patch can be applied cleanly to the workspace.
///
/// Runs `git apply --check` to verify the patch without actually applying it.
///
/// # Errors
///
/// Returns an error if the git command cannot be executed.
pub fn validate_patch(workspace_path: &Path, patch: &str) -> Result<bool> {
    if patch.trim().is_empty() {
        return Ok(true);
    }

    let tmp = tempfile::NamedTempFile::new().context("create temp patch file")?;
    fs::write(tmp.path(), patch).context("write patch to temp file")?;

    let output = Command::new("git")
        .args(["apply", "--check"])
        .arg(tmp.path())
        .current_dir(workspace_path)
        .output()
        .context("run git apply --check")?;

    Ok(output.status.success())
}

/// Apply a unified diff patch to the workspace.
///
/// Validates the patch first, then applies it with `git apply`.
///
/// # Errors
///
/// Returns an error if the patch does not apply cleanly or git commands fail.
pub fn apply_patch(workspace_path: &Path, patch: &str) -> Result<()> {
    if patch.trim().is_empty() {
        return Ok(());
    }

    let valid = validate_patch(workspace_path, patch)?;
    if !valid {
        anyhow::bail!("patch does not apply cleanly");
    }

    let tmp = tempfile::NamedTempFile::new().context("create temp patch file")?;
    fs::write(tmp.path(), patch).context("write patch to temp file")?;

    let output = Command::new("git")
        .args(["apply"])
        .arg(tmp.path())
        .current_dir(workspace_path)
        .output()
        .context("run git apply")?;

    if !output.status.success() {
        anyhow::bail!(
            "git apply failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}
