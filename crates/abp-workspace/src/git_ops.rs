// SPDX-License-Identifier: MIT OR Apache-2.0
//! Git operations wrapper for workspace repositories.
//!
//! Provides [`GitOps`] for running common git commands (status, diff,
//! add, commit, log) against a workspace's git repository.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Status of a file in a git repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitFileStatus {
    /// File is new / untracked.
    Added,
    /// File was modified.
    Modified,
    /// File was deleted.
    Deleted,
    /// File was renamed.
    Renamed,
    /// File was copied.
    Copied,
    /// Status is unknown.
    Unknown,
}

/// A single entry from `git status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStatusEntry {
    /// File path.
    pub path: String,
    /// Git status indicator.
    pub status: GitFileStatus,
    /// Raw two-character status code from porcelain output.
    pub raw_status: String,
}

/// Statistics from `git diff --numstat`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffStats {
    /// Number of files changed.
    pub files_changed: usize,
    /// Total lines added.
    pub additions: usize,
    /// Total lines removed.
    pub deletions: usize,
}

/// A single entry from `git log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Commit SHA (full).
    pub sha: String,
    /// Author name.
    pub author: String,
    /// Commit message (first line).
    pub message: String,
    /// Commit timestamp as ISO-8601 string.
    pub timestamp: String,
}

/// Wrapper for git commands operating on a workspace repository.
#[derive(Debug, Clone)]
pub struct GitOps {
    workspace_path: PathBuf,
}

impl GitOps {
    /// Create a new `GitOps` for the given workspace path.
    #[must_use]
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
        }
    }

    /// Return the workspace path.
    #[must_use]
    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }

    /// Get the current status of the workspace repository.
    ///
    /// # Errors
    ///
    /// Returns an error if the git command fails.
    pub fn status(&self) -> Result<Vec<GitStatusEntry>> {
        let output = self.run_git(&["status", "--porcelain=v1"])?;
        let mut entries = Vec::new();

        for line in output.lines() {
            if line.len() < 3 {
                continue;
            }
            let raw_status = line[..2].to_string();
            let path = line[3..].trim().to_string();
            let status = classify_status(&raw_status);
            entries.push(GitStatusEntry {
                path,
                status,
                raw_status,
            });
        }

        Ok(entries)
    }

    /// Get the unified diff of all uncommitted changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the git command fails.
    pub fn diff(&self) -> Result<String> {
        self.run_git(&["add", "-A"])?;
        let diff = self.run_git(&["diff", "--cached", "--no-color"])?;
        let _ = self.run_git(&["reset", "-q"]);
        Ok(diff)
    }

    /// Get diff statistics (files changed, additions, deletions).
    ///
    /// # Errors
    ///
    /// Returns an error if the git command fails.
    pub fn diff_stats(&self) -> Result<DiffStats> {
        self.run_git(&["add", "-A"])?;
        let numstat = self.run_git(&["diff", "--cached", "--numstat"])?;
        let _ = self.run_git(&["reset", "-q"]);

        let mut stats = DiffStats::default();
        for line in numstat.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                stats.files_changed += 1;
                stats.additions += parts[0].parse::<usize>().unwrap_or(0);
                stats.deletions += parts[1].parse::<usize>().unwrap_or(0);
            }
        }
        Ok(stats)
    }

    /// Stage files matching the given paths (or all with `["."]`).
    ///
    /// # Errors
    ///
    /// Returns an error if the git command fails.
    pub fn add(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["add"];
        args.extend(paths);
        self.run_git(&args)?;
        Ok(())
    }

    /// Create a commit with the given message. Returns the commit SHA.
    ///
    /// # Errors
    ///
    /// Returns an error if the git command fails.
    pub fn commit(&self, message: &str) -> Result<String> {
        self.run_git(&["add", "-A"])?;
        self.run_git(&[
            "-c",
            "user.name=abp",
            "-c",
            "user.email=abp@local",
            "commit",
            "-qm",
            message,
        ])?;
        let sha = self.run_git(&["rev-parse", "HEAD"])?;
        Ok(sha.trim().to_string())
    }

    /// Get the commit log (most recent first).
    ///
    /// # Errors
    ///
    /// Returns an error if the git command fails.
    pub fn log(&self, limit: usize) -> Result<Vec<LogEntry>> {
        let n_arg = format!("-{limit}");
        let output = self.run_git(&["log", &n_arg, "--format=%H%n%an%n%s%n%aI%n---"])?;

        let mut entries = Vec::new();
        let mut lines = output.lines().peekable();

        while lines.peek().is_some() {
            let sha = match lines.next() {
                Some(s) if !s.is_empty() && s != "---" => s.to_string(),
                _ => continue,
            };
            let author = lines.next().unwrap_or("").to_string();
            let message = lines.next().unwrap_or("").to_string();
            let timestamp = lines.next().unwrap_or("").to_string();
            // Skip separator.
            if let Some(&sep) = lines.peek() {
                if sep == "---" {
                    lines.next();
                }
            }
            entries.push(LogEntry {
                sha,
                author,
                message,
                timestamp,
            });
        }

        Ok(entries)
    }

    fn run_git(&self, args: &[&str]) -> Result<String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.workspace_path)
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
}

fn classify_status(raw: &str) -> GitFileStatus {
    let bytes = raw.as_bytes();
    let index = if !bytes.is_empty() { bytes[0] } else { b' ' };
    let worktree = if bytes.len() > 1 { bytes[1] } else { b' ' };

    match (index, worktree) {
        (b'A', _) | (b'?', _) => GitFileStatus::Added,
        (b'M', _) | (_, b'M') => GitFileStatus::Modified,
        (b'D', _) | (_, b'D') => GitFileStatus::Deleted,
        (b'R', _) => GitFileStatus::Renamed,
        (b'C', _) => GitFileStatus::Copied,
        _ => GitFileStatus::Unknown,
    }
}
