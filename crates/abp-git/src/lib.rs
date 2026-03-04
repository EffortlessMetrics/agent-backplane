#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]
//! abp-git
//!
//! Git repository helpers used by workspace staging and verification.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Initializes a git repo at `path` with a baseline commit if one does not already exist.
pub fn ensure_git_repo(path: &Path) {
    if path.join(".git").exists() {
        return;
    }

    let _ = Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .status();

    // Create an initial commit so diffs are meaningful.
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .status();

    let _ = Command::new("git")
        .args([
            "-c",
            "user.name=abp",
            "-c",
            "user.email=abp@local",
            "commit",
            "-qm",
            "baseline",
        ])
        .current_dir(path)
        .status();
}

/// Returns the porcelain v1 status output for the repo at `path`, if available.
pub fn git_status(path: &Path) -> Option<String> {
    run_git(path, &["status", "--porcelain=v1"]).ok()
}

/// Returns the unified diff output for the repo at `path`, if available.
pub fn git_diff(path: &Path) -> Option<String> {
    run_git(path, &["diff", "--no-color"]).ok()
}

// ── diff between commits ────────────────────────────────────────────

/// Returns the unified diff between two commits (or any git refs).
///
/// Equivalent to `git diff <from> <to> --no-color`.
pub fn git_diff_commits(path: &Path, from: &str, to: &str) -> Option<String> {
    run_git(path, &["diff", from, to, "--no-color"]).ok()
}

/// Returns the staged (cached) diff — changes that have been `git add`-ed.
pub fn git_diff_staged(path: &Path) -> Option<String> {
    run_git(path, &["diff", "--cached", "--no-color"]).ok()
}

// ── patch creation ──────────────────────────────────────────────────

/// Creates a unified patch of all workspace changes (staged + unstaged)
/// relative to HEAD.
///
/// This stages everything with `git add -A`, captures the cached diff,
/// then resets the index so the working tree is left unchanged.
pub fn git_create_patch(path: &Path) -> Option<String> {
    // Stage everything so untracked files are included.
    run_git(path, &["add", "-A"]).ok()?;
    let patch = run_git(path, &["diff", "--cached", "--no-color"]).ok();
    // Reset index back — leave working tree untouched.
    let _ = run_git(path, &["reset", "-q"]);
    patch
}

// ── change statistics ───────────────────────────────────────────────

/// Aggregated change statistics for a workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeStats {
    /// Files that were added (new / previously untracked).
    pub added: Vec<PathBuf>,
    /// Files that were modified.
    pub modified: Vec<PathBuf>,
    /// Files that were deleted.
    pub deleted: Vec<PathBuf>,
    /// Total lines added across all files.
    pub total_additions: usize,
    /// Total lines deleted across all files.
    pub total_deletions: usize,
}

impl ChangeStats {
    /// Returns `true` when no changes were detected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    /// Total number of changed files.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }
}

/// Computes change statistics for all workspace modifications relative to HEAD.
///
/// Stages everything with `git add -A`, reads `--name-status` and `--numstat`,
/// then resets the index.
pub fn git_change_stats(path: &Path) -> Option<ChangeStats> {
    run_git(path, &["add", "-A"]).ok()?;

    let name_status = run_git(path, &["diff", "--cached", "--name-status"]).ok();
    let numstat = run_git(path, &["diff", "--cached", "--numstat"]).ok();

    // Reset index back.
    let _ = run_git(path, &["reset", "-q"]);

    let mut stats = ChangeStats::default();

    if let Some(ns) = name_status {
        for line in ns.lines() {
            let mut parts = line.splitn(2, '\t');
            let kind = parts.next().unwrap_or("").trim();
            let file = parts.next().unwrap_or("").trim();
            if file.is_empty() {
                continue;
            }
            match kind {
                "A" => stats.added.push(PathBuf::from(file)),
                "M" => stats.modified.push(PathBuf::from(file)),
                "D" => stats.deleted.push(PathBuf::from(file)),
                _ => {
                    // Renames, copies, etc. — treat as modified.
                    if kind.starts_with('R') || kind.starts_with('C') {
                        // For renames the second column is "old\tnew".
                        if let Some(new_name) = file.split('\t').last() {
                            stats.modified.push(PathBuf::from(new_name.trim()));
                        }
                    }
                }
            }
        }
    }

    if let Some(ns) = numstat {
        for line in ns.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() >= 2 {
                // Binary files show "-" for additions/deletions.
                stats.total_additions += cols[0].parse::<usize>().unwrap_or(0);
                stats.total_deletions += cols[1].parse::<usize>().unwrap_or(0);
            }
        }
    }

    Some(stats)
}

// ── git blame ───────────────────────────────────────────────────────

/// A single line of blame output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// Abbreviated commit hash.
    pub commit: String,
    /// Author name.
    pub author: String,
    /// Line number (1-based).
    pub line_no: usize,
    /// Content of the line.
    pub content: String,
}

/// Runs `git blame` on `file` within the repo at `path`.
///
/// Returns parsed blame information per line, or `None` if the command fails
/// (e.g. the file is untracked or the path is not a repo).
pub fn git_blame(path: &Path, file: &Path) -> Option<Vec<BlameLine>> {
    let file_str = file.to_string_lossy();
    let output = run_git(path, &["blame", "--porcelain", &file_str]).ok()?;

    let mut lines = Vec::new();
    let mut current_commit = String::new();
    let mut current_author = String::new();
    let mut current_line_no: usize = 0;

    for raw in output.lines() {
        if raw.starts_with('\t') {
            // Content line — ends the block for this source line.
            lines.push(BlameLine {
                commit: current_commit.clone(),
                author: current_author.clone(),
                line_no: current_line_no,
                content: raw[1..].to_string(),
            });
        } else if raw.starts_with("author ") {
            current_author = raw["author ".len()..].to_string();
        } else if !raw.is_empty() && raw.as_bytes()[0].is_ascii_hexdigit() {
            // Header line: "<sha> <orig-line> <final-line> [<count>]"
            let parts: Vec<&str> = raw.split_whitespace().collect();
            if parts.len() >= 3 {
                current_commit = parts[0][..parts[0].len().min(8)].to_string();
                current_line_no = parts[2].parse().unwrap_or(0);
            }
        }
    }

    Some(lines)
}

// ── commit helpers ──────────────────────────────────────────────────

/// Formats an ABP commit message with a summary and optional body.
///
/// Follows conventional-commit style: the summary is prefixed with
/// `abp:` and the body (if any) is separated by a blank line.
#[must_use]
pub fn format_commit_message(summary: &str, body: Option<&str>) -> String {
    let header = format!("abp: {summary}");
    match body {
        Some(b) if !b.trim().is_empty() => format!("{header}\n\n{b}"),
        _ => header,
    }
}

/// Stage all changes and create a commit using the ABP identity.
///
/// Returns the new commit SHA on success.
pub fn git_commit(path: &Path, message: &str) -> Option<String> {
    run_git(path, &["add", "-A"]).ok()?;
    run_git(
        path,
        &[
            "-c",
            "user.name=abp",
            "-c",
            "user.email=abp@local",
            "commit",
            "-qm",
            message,
        ],
    )
    .ok()?;
    let sha = run_git(path, &["rev-parse", "HEAD"]).ok()?;
    Some(sha.trim().to_string())
}

/// Returns the HEAD commit SHA, or `None` if not in a git repo.
pub fn git_head(path: &Path) -> Option<String> {
    run_git(path, &["rev-parse", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
}

// ── internals ───────────────────────────────────────────────────────

fn run_git(path: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .with_context(|| format!("run git {args:?}"))?;

    if !out.status.success() {
        anyhow::bail!("git {:?} failed (code={:?})", args, out.status.code());
    }

    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
