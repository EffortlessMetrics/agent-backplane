//! abp-workspace
//!
//! Workspace preparation and harness utilities.
//!
//! Two modes matter:
//! - PassThrough: run directly in the user's workspace.
//! - Staged: create a sanitized copy (and optionally a synthetic git repo).

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_git::{ensure_git_repo, git_diff as git_diff_impl, git_status as git_status_impl};
use abp_glob::IncludeExcludeGlobs;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::debug;
use walkdir::WalkDir;

pub struct PreparedWorkspace {
    path: PathBuf,
    _temp: Option<TempDir>,
}

impl PreparedWorkspace {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub struct WorkspaceManager;

impl WorkspaceManager {
    pub fn prepare(spec: &WorkspaceSpec) -> Result<PreparedWorkspace> {
        let root = PathBuf::from(&spec.root);
        match spec.mode {
            WorkspaceMode::PassThrough => Ok(PreparedWorkspace {
                path: root,
                _temp: None,
            }),
            WorkspaceMode::Staged => {
                let tmp = tempfile::tempdir().context("create temp dir")?;
                let dest = tmp.path().to_path_buf();

                let path_rules = IncludeExcludeGlobs::new(&spec.include, &spec.exclude)
                    .context("compile workspace include/exclude globs")?;

                copy_workspace(&root, &dest, &path_rules)?;

                // If the staged workspace isn't a git repo, initialize one.
                ensure_git_repo(&dest);

                Ok(PreparedWorkspace {
                    path: dest,
                    _temp: Some(tmp),
                })
            }
        }
    }

    pub fn git_status(path: &Path) -> Option<String> {
        git_status_impl(path)
    }

    pub fn git_diff(path: &Path) -> Option<String> {
        git_diff_impl(path)
    }
}

fn copy_workspace(
    src_root: &Path,
    dest_root: &Path,
    path_rules: &IncludeExcludeGlobs,
) -> Result<()> {
    debug!(target: "abp.workspace", "staging workspace from {} to {}", src_root.display(), dest_root.display());

    let walker = WalkDir::new(src_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != std::ffi::OsStr::new(".git"));

    for entry in walker {
        let entry = entry?;
        let path = entry.path();

        let rel = path.strip_prefix(src_root).unwrap_or(path);
        if rel.as_os_str().is_empty() {
            continue;
        }

        if !path_rules.decide_path(rel).is_allowed() {
            continue;
        }

        let dest_path = dest_root.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest_path)
                .with_context(|| format!("create dir {}", dest_path.display()))?;
            continue;
        }

        if entry.file_type().is_file() {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create dir {}", parent.display()))?;
            }
            fs::copy(path, &dest_path).with_context(|| format!("copy {}", rel.display()))?;
        }
    }

    Ok(())
}
