// SPDX-License-Identifier: MIT OR Apache-2.0
//! Merge changes from multiple workspaces into one.
//!
//! [`WorkspaceMerge`] compares a set of *branch* workspace snapshots against a
//! common *base* snapshot and produces a merged result directory.

use crate::snapshot::{self, FileSnapshot, SnapshotContents, WorkspaceSnapshot};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Outcome of merging a single file across branches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Only one branch touched the file — auto-resolved.
    Clean,
    /// Multiple branches modified the same file with different content.
    Conflict,
    /// File was deleted in one branch but modified in another.
    DeleteModifyConflict,
}

/// Per-file merge result.
#[derive(Clone, Debug)]
pub struct FileMergeResult {
    /// Relative path.
    pub path: PathBuf,
    /// Resolution outcome.
    pub outcome: MergeOutcome,
    /// Which branch index (0-based) was chosen for the content, if any.
    pub chosen_branch: Option<usize>,
}

/// Summary returned by [`WorkspaceMerge::merge`].
#[derive(Clone, Debug)]
pub struct MergeReport {
    /// Per-file merge results.
    pub files: Vec<FileMergeResult>,
    /// `true` when there are zero conflicts.
    pub clean: bool,
    /// Number of conflicts encountered.
    pub conflict_count: usize,
}

/// Strategy for resolving conflicts during merge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Use the content from the first branch that modified the file.
    #[default]
    FirstWins,
    /// Use the content from the last branch that modified the file.
    LastWins,
    /// Mark the file as conflicted and skip writing.
    Skip,
}

/// Merges changes from multiple workspace snapshots.
///
/// The merge is performed against a shared *base* snapshot. Each branch
/// snapshot is diffed against the base and non-conflicting changes are
/// applied to the output directory.
#[derive(Debug)]
pub struct WorkspaceMerge {
    strategy: ConflictStrategy,
}

impl Default for WorkspaceMerge {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceMerge {
    /// Create a merge with the default [`ConflictStrategy::FirstWins`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            strategy: ConflictStrategy::default(),
        }
    }

    /// Set the conflict resolution strategy.
    #[must_use]
    pub fn with_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Merge multiple branch snapshots against a common base into `output_dir`.
    ///
    /// The base content is written first, then each branch's changes are
    /// layered on top. Conflicts are resolved according to the configured
    /// [`ConflictStrategy`].
    ///
    /// # Errors
    ///
    /// Returns an error on I/O failure.
    pub fn merge(
        &self,
        base: &WorkspaceSnapshot,
        base_contents: &SnapshotContents,
        branches: &[(&WorkspaceSnapshot, &SnapshotContents)],
        output_dir: &Path,
    ) -> Result<MergeReport> {
        fs::create_dir_all(output_dir)
            .with_context(|| format!("create output dir {}", output_dir.display()))?;

        // Start with base content.
        let mut merged: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
        for (rel, data) in &base_contents.contents {
            merged.insert(rel.clone(), data.clone());
        }

        // Track which branches touched which files, and what content.
        let mut file_branches: BTreeMap<PathBuf, Vec<(usize, Option<Vec<u8>>)>> = BTreeMap::new();

        for (idx, (snap, contents)) in branches.iter().enumerate() {
            let diff = snapshot::compare(base, snap);

            for path in &diff.added {
                file_branches
                    .entry(path.clone())
                    .or_default()
                    .push((idx, contents.contents.get(path).cloned()));
            }
            for path in &diff.modified {
                file_branches
                    .entry(path.clone())
                    .or_default()
                    .push((idx, contents.contents.get(path).cloned()));
            }
            for path in &diff.removed {
                file_branches
                    .entry(path.clone())
                    .or_default()
                    .push((idx, None)); // None signals deletion.
            }
        }

        let mut report_files = Vec::new();
        let mut conflict_count: usize = 0;

        for (path, touches) in &file_branches {
            if touches.len() == 1 {
                // Single branch touched this file — clean.
                let (branch_idx, ref data) = touches[0];
                match data {
                    Some(bytes) => {
                        merged.insert(path.clone(), bytes.clone());
                    }
                    None => {
                        merged.remove(path);
                    }
                }
                report_files.push(FileMergeResult {
                    path: path.clone(),
                    outcome: MergeOutcome::Clean,
                    chosen_branch: Some(branch_idx),
                });
            } else {
                // Multiple branches touched the same file.
                let has_delete = touches.iter().any(|(_, d)| d.is_none());
                let has_modify = touches.iter().any(|(_, d)| d.is_some());

                if has_delete && has_modify {
                    conflict_count += 1;
                    let outcome = MergeOutcome::DeleteModifyConflict;
                    let chosen = self.resolve_conflict(&touches, &mut merged, path);
                    report_files.push(FileMergeResult {
                        path: path.clone(),
                        outcome,
                        chosen_branch: chosen,
                    });
                } else {
                    // Check if all modifications agree.
                    let contents_set: BTreeSet<Option<&[u8]>> =
                        touches.iter().map(|(_, d)| d.as_deref()).collect();

                    if contents_set.len() == 1 {
                        // All branches agree.
                        let (branch_idx, ref data) = touches[0];
                        match data {
                            Some(bytes) => {
                                merged.insert(path.clone(), bytes.clone());
                            }
                            None => {
                                merged.remove(path);
                            }
                        }
                        report_files.push(FileMergeResult {
                            path: path.clone(),
                            outcome: MergeOutcome::Clean,
                            chosen_branch: Some(branch_idx),
                        });
                    } else {
                        conflict_count += 1;
                        let chosen = self.resolve_conflict(&touches, &mut merged, path);
                        report_files.push(FileMergeResult {
                            path: path.clone(),
                            outcome: MergeOutcome::Conflict,
                            chosen_branch: chosen,
                        });
                    }
                }
            }
        }

        // Write merged state to output_dir.
        for (rel, data) in &merged {
            let dest = output_dir.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create dir {}", parent.display()))?;
            }
            fs::write(&dest, data).with_context(|| format!("write {}", dest.display()))?;
        }

        report_files.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(MergeReport {
            files: report_files,
            clean: conflict_count == 0,
            conflict_count,
        })
    }

    fn resolve_conflict(
        &self,
        touches: &[(usize, Option<Vec<u8>>)],
        merged: &mut BTreeMap<PathBuf, Vec<u8>>,
        path: &Path,
    ) -> Option<usize> {
        match self.strategy {
            ConflictStrategy::FirstWins => {
                // Pick the first branch with content.
                for (idx, data) in touches {
                    if let Some(bytes) = data {
                        merged.insert(path.to_path_buf(), bytes.clone());
                        return Some(*idx);
                    }
                }
                merged.remove(path);
                None
            }
            ConflictStrategy::LastWins => {
                // Pick the last branch with content.
                for (idx, data) in touches.iter().rev() {
                    if let Some(bytes) = data {
                        merged.insert(path.to_path_buf(), bytes.clone());
                        return Some(*idx);
                    }
                }
                merged.remove(path);
                None
            }
            ConflictStrategy::Skip => {
                // Leave the base content (or remove if base didn't have it).
                None
            }
        }
    }
}

/// Convenience: merge two workspaces against a common base and write into
/// `output_dir`.
///
/// Uses [`ConflictStrategy::FirstWins`] by default.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub fn merge_two(
    base: &WorkspaceSnapshot,
    base_contents: &SnapshotContents,
    a: &WorkspaceSnapshot,
    a_contents: &SnapshotContents,
    b: &WorkspaceSnapshot,
    b_contents: &SnapshotContents,
    output_dir: &Path,
) -> Result<MergeReport> {
    WorkspaceMerge::new().merge(
        base,
        base_contents,
        &[(a, a_contents), (b, b_contents)],
        output_dir,
    )
}
