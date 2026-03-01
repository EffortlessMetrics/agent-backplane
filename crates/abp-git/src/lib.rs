#![deny(unsafe_code)]
//! abp-git
//!
//! Git repository helpers used by workspace staging and verification.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

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

pub fn git_status(path: &Path) -> Option<String> {
    run_git(path, &["status", "--porcelain=v1"]).ok()
}

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
