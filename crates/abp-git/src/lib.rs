#![deny(unsafe_code)]
#![warn(missing_docs)]
//! abp-git
//!
//! Git repository helpers used by workspace staging and verification.

use anyhow::{Context, Result};
use std::path::Path;
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
