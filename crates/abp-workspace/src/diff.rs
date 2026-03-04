// SPDX-License-Identifier: MIT OR Apache-2.0
//! Workspace diff analysis utilities.
//!
//! Provides [`DiffSummary`] and [`diff_workspace`] for analysing changes in a
//! [`PreparedWorkspace`] against its baseline git commit.
//!
//! Higher-level utilities [`WorkspaceDiff`], [`DiffAnalyzer`], and
//! [`DiffPolicy`] build on the raw summary to support per-file change
//! tracking, path-based querying, and policy enforcement.

use crate::PreparedWorkspace;
use abp_glob::IncludeExcludeGlobs;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
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

// ---------------------------------------------------------------------------
// Per-file change types
// ---------------------------------------------------------------------------

/// Classification of a single file change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    /// File was newly created.
    Added,
    /// File was modified.
    Modified,
    /// File was deleted.
    Deleted,
}

impl fmt::Display for ChangeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added => write!(f, "added"),
            Self::Modified => write!(f, "modified"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

/// Detailed information about a single changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Relative path of the changed file.
    pub path: PathBuf,
    /// Whether the file was added, modified, or deleted.
    pub change_type: ChangeType,
    /// Number of lines added in this file.
    pub additions: usize,
    /// Number of lines deleted in this file.
    pub deletions: usize,
    /// Whether the file is binary (line counts will be zero).
    pub is_binary: bool,
}

// ---------------------------------------------------------------------------
// WorkspaceDiff — rich per-file diff result
// ---------------------------------------------------------------------------

/// Rich diff result with per-file change details.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDiff {
    /// Files that were added.
    pub files_added: Vec<FileChange>,
    /// Files that were modified.
    pub files_modified: Vec<FileChange>,
    /// Files that were deleted.
    pub files_deleted: Vec<FileChange>,
    /// Total lines added across all files.
    pub total_additions: usize,
    /// Total lines deleted across all files.
    pub total_deletions: usize,
}

impl WorkspaceDiff {
    /// Human-readable summary of the diff.
    #[must_use]
    pub fn summary(&self) -> String {
        let added = self.files_added.len();
        let modified = self.files_modified.len();
        let deleted = self.files_deleted.len();
        let total = added + modified + deleted;
        if total == 0 {
            return "No changes detected.".to_string();
        }
        format!(
            "{total} file(s) changed: {added} added, {modified} modified, {deleted} deleted (+{} -{})",
            self.total_additions, self.total_deletions,
        )
    }

    /// Returns `true` when no changes were detected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files_added.is_empty()
            && self.files_modified.is_empty()
            && self.files_deleted.is_empty()
    }

    /// Total number of files changed.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files_added.len() + self.files_modified.len() + self.files_deleted.len()
    }
}

// ---------------------------------------------------------------------------
// DiffAnalyzer — workspace-oriented query API
// ---------------------------------------------------------------------------

/// Analyses changes in a workspace directory against its baseline git commit.
///
/// The workspace must contain a `.git` directory with at least one commit
/// (created automatically by [`WorkspaceStager`](crate::WorkspaceStager)).
#[derive(Debug, Clone)]
pub struct DiffAnalyzer {
    workspace_path: PathBuf,
}

impl DiffAnalyzer {
    /// Create a new analyser for the given workspace path.
    #[must_use]
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
        }
    }

    /// Run a full diff analysis and return a [`WorkspaceDiff`].
    ///
    /// # Errors
    ///
    /// Returns an error if git commands fail.
    pub fn analyze(&self) -> Result<WorkspaceDiff> {
        let path = &self.workspace_path;

        // Stage everything.
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

        let name_status = run_git_output(path, &["diff", "--cached", "--name-status"])?;
        let numstat = run_git_output(path, &["diff", "--cached", "--numstat"])?;

        // Build per-file stat map: path -> (additions, deletions, is_binary)
        let mut stat_map: std::collections::HashMap<String, (usize, usize, bool)> =
            std::collections::HashMap::new();
        for line in numstat.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let file = parts[2].to_string();
            if parts[0] == "-" || parts[1] == "-" {
                stat_map.insert(file, (0, 0, true));
            } else if let (Ok(a), Ok(d)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                stat_map.insert(file, (a, d, false));
            }
        }

        let mut diff = WorkspaceDiff::default();

        for line in name_status.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (status_code, file_path) = match line.split_once('\t') {
                Some(pair) => pair,
                None => continue,
            };
            let (additions, deletions, is_binary) =
                stat_map.get(file_path).copied().unwrap_or((0, 0, false));

            let change_type = match status_code.chars().next() {
                Some('A') => ChangeType::Added,
                Some('D') => ChangeType::Deleted,
                _ => ChangeType::Modified,
            };

            let fc = FileChange {
                path: PathBuf::from(file_path),
                change_type,
                additions,
                deletions,
                is_binary,
            };

            diff.total_additions += additions;
            diff.total_deletions += deletions;

            match change_type {
                ChangeType::Added => diff.files_added.push(fc),
                ChangeType::Modified => diff.files_modified.push(fc),
                ChangeType::Deleted => diff.files_deleted.push(fc),
            }
        }

        // Deterministic ordering.
        diff.files_added.sort_by(|a, b| a.path.cmp(&b.path));
        diff.files_modified.sort_by(|a, b| a.path.cmp(&b.path));
        diff.files_deleted.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(diff)
    }

    /// Returns `true` if there are any uncommitted changes in the workspace.
    pub fn has_changes(&self) -> bool {
        let Ok(status) = run_git_output(&self.workspace_path, &["status", "--porcelain=v1"]) else {
            return false;
        };
        !status.trim().is_empty()
    }

    /// List all changed file paths (added, modified, and deleted).
    pub fn changed_files(&self) -> Vec<PathBuf> {
        let Ok(diff) = self.analyze() else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = diff
            .files_added
            .iter()
            .chain(diff.files_modified.iter())
            .chain(diff.files_deleted.iter())
            .map(|fc| fc.path.clone())
            .collect();
        files.sort();
        files
    }

    /// Check whether a specific path was modified (added, changed, or deleted).
    pub fn file_was_modified(&self, path: &Path) -> bool {
        let Ok(diff) = self.analyze() else {
            return false;
        };
        diff.files_added
            .iter()
            .chain(diff.files_modified.iter())
            .chain(diff.files_deleted.iter())
            .any(|fc| fc.path == path)
    }
}

/// Run a git command and return stdout as a `String`.
fn run_git_output(path: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(path)
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

// ---------------------------------------------------------------------------
// DiffPolicy — enforce constraints on workspace changes
// ---------------------------------------------------------------------------

/// Outcome of a policy check against a [`WorkspaceDiff`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum PolicyResult {
    /// The diff satisfies all policy constraints.
    Pass,
    /// One or more policy constraints were violated.
    Fail {
        /// Human-readable descriptions of each violation.
        violations: Vec<String>,
    },
}

impl PolicyResult {
    /// Returns `true` when the policy passed.
    #[must_use]
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// Constraints that a workspace diff must satisfy.
///
/// All fields are optional — omitted fields impose no limit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffPolicy {
    /// Maximum number of changed files allowed.
    pub max_files: Option<usize>,
    /// Maximum number of added lines allowed.
    pub max_additions: Option<usize>,
    /// Glob patterns for paths that must not be changed.
    pub denied_paths: Vec<String>,
}

impl DiffPolicy {
    /// Evaluate the policy against a [`WorkspaceDiff`].
    ///
    /// # Errors
    ///
    /// Returns an error if the `denied_paths` globs fail to compile.
    pub fn check(&self, diff: &WorkspaceDiff) -> Result<PolicyResult> {
        let mut violations: Vec<String> = Vec::new();

        if let Some(max) = self.max_files {
            let count = diff.file_count();
            if count > max {
                violations.push(format!("too many files changed: {count} (max {max})"));
            }
        }

        if let Some(max) = self.max_additions {
            if diff.total_additions > max {
                violations.push(format!(
                    "too many additions: {} (max {max})",
                    diff.total_additions
                ));
            }
        }

        if !self.denied_paths.is_empty() {
            let globs = IncludeExcludeGlobs::new(&self.denied_paths, &[])
                .context("compile denied_paths globs")?;

            let all_files = diff
                .files_added
                .iter()
                .chain(diff.files_modified.iter())
                .chain(diff.files_deleted.iter());

            for fc in all_files {
                if globs.decide_path(&fc.path).is_allowed() {
                    violations.push(format!("change to denied path: {}", fc.path.display()));
                }
            }
        }

        if violations.is_empty() {
            Ok(PolicyResult::Pass)
        } else {
            Ok(PolicyResult::Fail { violations })
        }
    }
}

// ---------------------------------------------------------------------------
// Structured diff parsing — DiffAnalysis
// ---------------------------------------------------------------------------

/// Classification of a change in a parsed diff, including rename support.
///
/// Extends [`ChangeType`] with a `Renamed` variant for use with full
/// unified-diff parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffChangeKind {
    /// File was newly created.
    Added,
    /// File content was modified.
    Modified,
    /// File was deleted.
    Deleted,
    /// File was renamed (possibly with content changes).
    Renamed,
}

impl fmt::Display for DiffChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added => write!(f, "added"),
            Self::Modified => write!(f, "modified"),
            Self::Deleted => write!(f, "deleted"),
            Self::Renamed => write!(f, "renamed"),
        }
    }
}

/// Detected file type based on extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    /// Rust source file.
    Rust,
    /// JavaScript source file.
    JavaScript,
    /// TypeScript source file.
    TypeScript,
    /// Python source file.
    Python,
    /// Go source file.
    Go,
    /// Java source file.
    Java,
    /// C# source file.
    CSharp,
    /// C++ source file.
    Cpp,
    /// C source file.
    C,
    /// HTML file.
    Html,
    /// CSS/SCSS/LESS file.
    Css,
    /// JSON file.
    Json,
    /// YAML file.
    Yaml,
    /// TOML file.
    Toml,
    /// Markdown file.
    Markdown,
    /// Shell script.
    Shell,
    /// SQL file.
    Sql,
    /// XML file.
    Xml,
    /// Binary file.
    Binary,
    /// Unknown or unrecognised file type.
    Other,
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rust => write!(f, "rust"),
            Self::JavaScript => write!(f, "javascript"),
            Self::TypeScript => write!(f, "typescript"),
            Self::Python => write!(f, "python"),
            Self::Go => write!(f, "go"),
            Self::Java => write!(f, "java"),
            Self::CSharp => write!(f, "csharp"),
            Self::Cpp => write!(f, "cpp"),
            Self::C => write!(f, "c"),
            Self::Html => write!(f, "html"),
            Self::Css => write!(f, "css"),
            Self::Json => write!(f, "json"),
            Self::Yaml => write!(f, "yaml"),
            Self::Toml => write!(f, "toml"),
            Self::Markdown => write!(f, "markdown"),
            Self::Shell => write!(f, "shell"),
            Self::Sql => write!(f, "sql"),
            Self::Xml => write!(f, "xml"),
            Self::Binary => write!(f, "binary"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Identify the [`FileType`] from a file path's extension.
#[must_use]
pub fn identify_file_type(path: &str) -> FileType {
    if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => FileType::Rust,
            "js" | "mjs" | "cjs" | "jsx" => FileType::JavaScript,
            "ts" | "tsx" | "mts" => FileType::TypeScript,
            "py" | "pyi" => FileType::Python,
            "go" => FileType::Go,
            "java" => FileType::Java,
            "cs" => FileType::CSharp,
            "cpp" | "cxx" | "cc" | "hpp" | "hxx" => FileType::Cpp,
            "c" | "h" => FileType::C,
            "html" | "htm" => FileType::Html,
            "css" | "scss" | "less" | "sass" => FileType::Css,
            "json" => FileType::Json,
            "yaml" | "yml" => FileType::Yaml,
            "toml" => FileType::Toml,
            "md" | "markdown" => FileType::Markdown,
            "sh" | "bash" | "zsh" | "fish" | "ps1" => FileType::Shell,
            "sql" => FileType::Sql,
            "xml" | "xsl" | "xslt" => FileType::Xml,
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "svg" | "webp" | "wasm" | "exe"
            | "dll" | "so" | "dylib" | "a" | "o" | "obj" | "zip" | "tar" | "gz" | "bz2" | "xz"
            | "7z" | "rar" | "pdf" | "doc" | "docx" | "xls" | "xlsx" | "mp3" | "mp4" | "wav"
            | "avi" | "mkv" | "ttf" | "otf" | "woff" | "woff2" | "eot" => FileType::Binary,
            _ => FileType::Other,
        }
    } else {
        FileType::Other
    }
}

/// Kind of a single line within a diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    /// Context line (unchanged).
    Context,
    /// Added line.
    Added,
    /// Removed line.
    Removed,
    /// The `\ No newline at end of file` marker.
    NoNewlineMarker,
}

/// A single line within a diff hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    /// The kind of diff line.
    pub kind: DiffLineKind,
    /// Content of the line (without the leading `+`/`-`/space marker).
    pub content: String,
}

/// A parsed hunk from a unified diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Starting line in the old file.
    pub old_start: usize,
    /// Number of lines in the old file range.
    pub old_count: usize,
    /// Starting line in the new file.
    pub new_start: usize,
    /// Number of lines in the new file range.
    pub new_count: usize,
    /// Raw hunk header text (e.g. `@@ -1,3 +1,4 @@`).
    pub header: String,
    /// Parsed lines within this hunk.
    pub lines: Vec<DiffLine>,
}

/// Parsed representation of a single file within a unified diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    /// Path of the file (new path for renames, otherwise the file path).
    pub path: String,
    /// Kind of change.
    pub change_kind: DiffChangeKind,
    /// Whether the file is binary.
    pub is_binary: bool,
    /// Diff hunks (empty for binary files and mode-only changes).
    pub hunks: Vec<DiffHunk>,
    /// Lines added in this file.
    pub additions: usize,
    /// Lines removed in this file.
    pub deletions: usize,
    /// Old file mode (e.g. `100644`), if a mode change was detected.
    pub old_mode: Option<String>,
    /// New file mode (e.g. `100755`), if a mode change was detected.
    pub new_mode: Option<String>,
    /// Detected file type.
    pub file_type: FileType,
    /// Original path for renamed files.
    pub renamed_from: Option<String>,
}

/// Per-file statistics extracted from a [`DiffAnalysis`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStats {
    /// File path.
    pub path: String,
    /// Lines added.
    pub additions: usize,
    /// Lines removed.
    pub deletions: usize,
    /// Whether the file is binary.
    pub is_binary: bool,
    /// Detected file type.
    pub file_type: FileType,
    /// Kind of change.
    pub change_kind: DiffChangeKind,
}

/// Structured analysis of a complete `git diff` unified output.
///
/// Use [`DiffAnalysis::parse`] to create an instance from raw diff text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffAnalysis {
    /// Per-file diff entries.
    pub files: Vec<FileDiff>,
    /// Total lines added across all files.
    pub total_additions: usize,
    /// Total lines removed across all files.
    pub total_deletions: usize,
    /// Number of binary files in the diff.
    pub binary_file_count: usize,
}

impl DiffAnalysis {
    /// Parse a raw unified diff string into structured data.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut analysis = Self::default();
        if raw.trim().is_empty() {
            return analysis;
        }

        let lines: Vec<&str> = raw.lines().collect();
        let mut sections: Vec<Vec<&str>> = Vec::new();
        let mut current: Vec<&str> = Vec::new();

        for line in &lines {
            if line.starts_with("diff --git ") && !current.is_empty() {
                sections.push(current);
                current = Vec::new();
            }
            current.push(line);
        }
        if !current.is_empty() {
            sections.push(current);
        }

        for section in &sections {
            if let Some(fd) = parse_file_section(section) {
                analysis.total_additions += fd.additions;
                analysis.total_deletions += fd.deletions;
                if fd.is_binary {
                    analysis.binary_file_count += 1;
                }
                analysis.files.push(fd);
            }
        }

        analysis
    }

    /// Returns `true` when the diff contains no file changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Total number of changed files.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Files filtered by change kind.
    #[must_use]
    pub fn files_by_kind(&self, kind: DiffChangeKind) -> Vec<&FileDiff> {
        self.files
            .iter()
            .filter(|f| f.change_kind == kind)
            .collect()
    }

    /// Per-file statistics.
    #[must_use]
    pub fn file_stats(&self) -> Vec<FileStats> {
        self.files
            .iter()
            .map(|f| FileStats {
                path: f.path.clone(),
                additions: f.additions,
                deletions: f.deletions,
                is_binary: f.is_binary,
                file_type: f.file_type,
                change_kind: f.change_kind,
            })
            .collect()
    }
}

// -- private parsing helpers ------------------------------------------------

fn parse_file_section(lines: &[&str]) -> Option<FileDiff> {
    if lines.is_empty() {
        return None;
    }
    let header = lines[0];
    if !header.starts_with("diff --git ") {
        return None;
    }

    let (_old_path, new_path) = parse_diff_git_header(header)?;

    let mut change_kind = DiffChangeKind::Modified;
    let mut is_binary = false;
    let mut old_mode: Option<String> = None;
    let mut new_mode: Option<String> = None;
    let mut renamed_from: Option<String> = None;
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut additions: usize = 0;
    let mut deletions: usize = 0;

    let mut i = 1;
    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("new file mode ") {
            change_kind = DiffChangeKind::Added;
            new_mode = line.strip_prefix("new file mode ").map(String::from);
        } else if line.starts_with("deleted file mode ") {
            change_kind = DiffChangeKind::Deleted;
            old_mode = line.strip_prefix("deleted file mode ").map(String::from);
        } else if line.starts_with("old mode ") {
            old_mode = line.strip_prefix("old mode ").map(String::from);
        } else if line.starts_with("new mode ") {
            new_mode = line.strip_prefix("new mode ").map(String::from);
        } else if line.starts_with("rename from ") {
            change_kind = DiffChangeKind::Renamed;
            renamed_from = line.strip_prefix("rename from ").map(String::from);
        } else if line.starts_with("Binary files ") && line.ends_with(" differ") {
            is_binary = true;
        } else if line.starts_with("@@") {
            let (hunk, consumed) = parse_hunk_from_lines(&lines[i..]);
            if let Some(h) = hunk {
                for dl in &h.lines {
                    match dl.kind {
                        DiffLineKind::Added => additions += 1,
                        DiffLineKind::Removed => deletions += 1,
                        _ => {}
                    }
                }
                hunks.push(h);
            }
            i += consumed;
            continue;
        }
        // Skip index, similarity, ---, +++ and other metadata lines.
        i += 1;
    }

    let path = new_path;
    let file_type = if is_binary {
        FileType::Binary
    } else {
        identify_file_type(&path)
    };

    Some(FileDiff {
        path,
        change_kind,
        is_binary,
        hunks,
        additions,
        deletions,
        old_mode,
        new_mode,
        file_type,
        renamed_from,
    })
}

/// Extract `(old_path, new_path)` from `diff --git a/<old> b/<new>`.
fn parse_diff_git_header(header: &str) -> Option<(String, String)> {
    let rest = header.strip_prefix("diff --git ")?;
    let b_idx = rest.find(" b/")?;
    let a_path = rest.get(2..b_idx)?.to_string();
    let b_path = rest.get(b_idx + 3..)?.to_string();
    Some((a_path, b_path))
}

/// Parse a hunk starting at `lines[0]` (the `@@` line).
///
/// Returns the parsed hunk and the number of lines consumed.
fn parse_hunk_from_lines(lines: &[&str]) -> (Option<DiffHunk>, usize) {
    if lines.is_empty() || !lines[0].starts_with("@@") {
        return (None, 1);
    }

    let header = lines[0];
    let (old_start, old_count, new_start, new_count) = parse_hunk_range(header);

    let mut hunk_lines: Vec<DiffLine> = Vec::new();
    let mut consumed: usize = 1;

    for &line in &lines[1..] {
        if line.starts_with("diff --git ") || line.starts_with("@@") {
            break;
        }

        let (kind, content) = if let Some(rest) = line.strip_prefix('+') {
            (DiffLineKind::Added, rest.to_string())
        } else if let Some(rest) = line.strip_prefix('-') {
            (DiffLineKind::Removed, rest.to_string())
        } else if line.starts_with('\\') {
            (DiffLineKind::NoNewlineMarker, line.to_string())
        } else if let Some(rest) = line.strip_prefix(' ') {
            (DiffLineKind::Context, rest.to_string())
        } else {
            (DiffLineKind::Context, line.to_string())
        };

        hunk_lines.push(DiffLine { kind, content });
        consumed += 1;
    }

    (
        Some(DiffHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            header: header.to_string(),
            lines: hunk_lines,
        }),
        consumed,
    )
}

/// Parse `@@ -old_start,old_count +new_start,new_count @@` ranges.
fn parse_hunk_range(header: &str) -> (usize, usize, usize, usize) {
    let parts: Vec<&str> = header.split_whitespace().collect();
    let (old_start, old_count) = if parts.len() > 1 {
        parse_range_part(parts[1].trim_start_matches('-'))
    } else {
        (0, 0)
    };
    let (new_start, new_count) = if parts.len() > 2 {
        parse_range_part(parts[2].trim_start_matches('+'))
    } else {
        (0, 0)
    };
    (old_start, old_count, new_start, new_count)
}

fn parse_range_part(s: &str) -> (usize, usize) {
    if let Some((start, count)) = s.split_once(',') {
        (start.parse().unwrap_or(0), count.parse().unwrap_or(0))
    } else {
        (s.parse().unwrap_or(0), 1)
    }
}

// ---------------------------------------------------------------------------
// Change classification
// ---------------------------------------------------------------------------

/// Category of a file based on its path and role in the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    /// Production source code.
    SourceCode,
    /// Configuration files.
    Config,
    /// Documentation (READMEs, guides, etc.).
    Documentation,
    /// Test files and fixtures.
    Tests,
    /// Static assets (images, fonts, media).
    Assets,
    /// Build artifacts and lock files.
    Build,
    /// CI/CD pipeline configuration.
    CiCd,
    /// Anything that does not fit other categories.
    Other,
}

impl fmt::Display for FileCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceCode => write!(f, "source code"),
            Self::Config => write!(f, "config"),
            Self::Documentation => write!(f, "documentation"),
            Self::Tests => write!(f, "tests"),
            Self::Assets => write!(f, "assets"),
            Self::Build => write!(f, "build"),
            Self::CiCd => write!(f, "ci/cd"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Classifies file changes by category, security sensitivity, and size.
///
/// Thresholds and heuristics are configurable via the builder methods.
#[derive(Debug, Clone)]
pub struct ChangeClassifier {
    large_change_threshold: usize,
}

impl Default for ChangeClassifier {
    fn default() -> Self {
        Self {
            large_change_threshold: 500,
        }
    }
}

impl ChangeClassifier {
    /// Create a classifier with default settings (large-change threshold = 500).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the threshold (total lines changed) above which a change is
    /// considered large.
    #[must_use]
    pub fn with_large_threshold(mut self, threshold: usize) -> Self {
        self.large_change_threshold = threshold;
        self
    }

    /// Current large-change threshold.
    #[must_use]
    pub fn large_change_threshold(&self) -> usize {
        self.large_change_threshold
    }

    /// Classify a file path into a [`FileCategory`].
    #[must_use]
    pub fn classify_path(&self, path: &str) -> FileCategory {
        let lower = path.to_ascii_lowercase();
        let file_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Tests
        if lower.contains("/tests/")
            || lower.contains("/test/")
            || lower.starts_with("tests/")
            || lower.starts_with("test/")
            || file_name.ends_with("_test.rs")
            || file_name.ends_with("_test.go")
            || file_name.ends_with("_test.py")
            || file_name.ends_with(".test.js")
            || file_name.ends_with(".test.ts")
            || file_name.ends_with("_spec.rb")
            || file_name.ends_with(".spec.ts")
            || file_name.ends_with(".spec.js")
            || file_name.starts_with("test_")
        {
            return FileCategory::Tests;
        }

        // CI/CD
        if lower.contains(".github/workflows/")
            || lower.contains(".gitlab-ci")
            || lower.contains("jenkinsfile")
            || lower.contains(".circleci/")
            || lower.contains(".travis.yml")
            || lower.contains("azure-pipelines")
        {
            return FileCategory::CiCd;
        }

        // Documentation
        if matches!(
            file_name.as_str(),
            "readme.md"
                | "readme.txt"
                | "readme"
                | "changelog.md"
                | "changelog"
                | "contributing.md"
                | "contributing"
                | "license"
                | "license.md"
                | "license-mit"
                | "license-apache"
                | "code_of_conduct.md"
                | "authors"
                | "authors.md"
                | "todo.md"
                | "todo"
                | "history.md"
        ) || lower.starts_with("docs/")
            || lower.starts_with("doc/")
            || lower.ends_with(".md")
            || lower.ends_with(".rst")
            || lower.ends_with(".adoc")
        {
            return FileCategory::Documentation;
        }

        // Build artifacts / lock files
        if matches!(
            file_name.as_str(),
            "cargo.lock"
                | "package-lock.json"
                | "yarn.lock"
                | "pnpm-lock.yaml"
                | "go.sum"
                | "gemfile.lock"
                | "poetry.lock"
                | "composer.lock"
                | "pipfile.lock"
        ) {
            return FileCategory::Build;
        }

        // Config files
        if matches!(
            file_name.as_str(),
            "cargo.toml"
                | "package.json"
                | "tsconfig.json"
                | "pyproject.toml"
                | "setup.py"
                | "setup.cfg"
                | "go.mod"
                | ".gitignore"
                | ".gitattributes"
                | "dockerfile"
                | "docker-compose.yml"
                | "docker-compose.yaml"
                | "makefile"
                | "cmakelists.txt"
                | ".editorconfig"
                | ".prettierrc"
                | ".eslintrc"
                | ".eslintrc.js"
                | ".eslintrc.json"
                | "rustfmt.toml"
                | "clippy.toml"
                | "deny.toml"
                | ".babelrc"
                | "webpack.config.js"
                | "vite.config.js"
                | "jest.config.js"
                | "tarpaulin.toml"
                | "mutants.toml"
        ) || lower.ends_with(".toml")
            || lower.ends_with(".yaml")
            || lower.ends_with(".yml")
            || lower.ends_with(".ini")
            || lower.ends_with(".cfg")
            || lower.ends_with(".conf")
            || lower.ends_with(".env")
            || lower.ends_with(".properties")
        {
            return FileCategory::Config;
        }

        // Assets (binary file types)
        let ft = identify_file_type(path);
        if ft == FileType::Binary {
            return FileCategory::Assets;
        }

        // Source code
        if matches!(
            ft,
            FileType::Rust
                | FileType::JavaScript
                | FileType::TypeScript
                | FileType::Python
                | FileType::Go
                | FileType::Java
                | FileType::CSharp
                | FileType::Cpp
                | FileType::C
                | FileType::Html
                | FileType::Css
                | FileType::Shell
                | FileType::Sql
        ) {
            return FileCategory::SourceCode;
        }

        if matches!(ft, FileType::Json | FileType::Xml) {
            return FileCategory::Config;
        }

        FileCategory::Other
    }

    /// Returns `true` if the path refers to a security-sensitive file.
    #[must_use]
    pub fn is_security_sensitive(&self, path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        let file_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Sensitive extensions
        if lower.ends_with(".pem")
            || lower.ends_with(".key")
            || lower.ends_with(".cert")
            || lower.ends_with(".p12")
            || lower.ends_with(".pfx")
            || lower.ends_with(".keystore")
            || lower.ends_with(".jks")
        {
            return true;
        }

        // Sensitive filenames
        if matches!(
            file_name.as_str(),
            ".env"
                | ".env.local"
                | ".env.production"
                | ".env.development"
                | ".htpasswd"
                | "shadow"
                | "passwd"
                | "id_rsa"
                | "id_ed25519"
                | "id_ecdsa"
                | "id_dsa"
                | "known_hosts"
        ) {
            return true;
        }

        // Sensitive path components
        if lower.contains("/secrets/")
            || lower.contains("/.ssh/")
            || lower.contains("credentials")
            || lower.contains("private_key")
        {
            return true;
        }

        // Sensitive filename keywords (excluding "author*")
        if file_name.contains("secret")
            || file_name.contains("password")
            || file_name.contains("credential")
            || file_name.contains("token")
            || (file_name.contains("auth") && !file_name.contains("author"))
        {
            return true;
        }

        false
    }

    /// Returns `true` when the total change size exceeds the large-change
    /// threshold.
    #[must_use]
    pub fn is_large_change(&self, additions: usize, deletions: usize) -> bool {
        additions + deletions > self.large_change_threshold
    }
}

// ---------------------------------------------------------------------------
// Diff report with risk assessment
// ---------------------------------------------------------------------------

/// Risk level for a change or change set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Low risk — routine changes.
    Low,
    /// Medium risk — large changes or binary files.
    Medium,
    /// High risk — security-sensitive files touched.
    High,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
        }
    }
}

/// Per-file breakdown within a [`DiffReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBreakdown {
    /// File path.
    pub path: String,
    /// Kind of change.
    pub change_kind: DiffChangeKind,
    /// Classified category.
    pub category: FileCategory,
    /// Lines added.
    pub additions: usize,
    /// Lines removed.
    pub deletions: usize,
    /// Whether the file is binary.
    pub is_binary: bool,
    /// Whether the file is security-sensitive.
    pub is_security_sensitive: bool,
    /// Whether the change exceeds the large-change threshold.
    pub is_large: bool,
    /// Per-file risk level.
    pub risk: RiskLevel,
}

/// Comprehensive diff report with risk assessment and category breakdown.
///
/// Produced by [`DiffReport::from_analysis`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffReport {
    /// Per-file breakdown.
    pub files: Vec<FileBreakdown>,
    /// Total lines added.
    pub total_additions: usize,
    /// Total lines removed.
    pub total_deletions: usize,
    /// Total files changed.
    pub total_files: usize,
    /// Overall risk level (maximum of per-file risks).
    pub risk_level: RiskLevel,
    /// Human-readable summary text.
    pub summary_text: String,
    /// Count of files per category.
    pub categories: BTreeMap<FileCategory, usize>,
    /// Whether any security-sensitive file was touched.
    pub has_security_sensitive_changes: bool,
    /// Number of files exceeding the large-change threshold.
    pub large_change_count: usize,
}

impl DiffReport {
    /// Build a report from a parsed [`DiffAnalysis`] using the given
    /// [`ChangeClassifier`].
    #[must_use]
    pub fn from_analysis(analysis: &DiffAnalysis, classifier: &ChangeClassifier) -> Self {
        let mut files = Vec::new();
        let mut categories: BTreeMap<FileCategory, usize> = BTreeMap::new();
        let mut has_security = false;
        let mut large_count: usize = 0;
        let mut max_risk = RiskLevel::Low;

        for fd in &analysis.files {
            let category = classifier.classify_path(&fd.path);
            let sensitive = classifier.is_security_sensitive(&fd.path);
            let large = classifier.is_large_change(fd.additions, fd.deletions);

            *categories.entry(category).or_default() += 1;
            if sensitive {
                has_security = true;
            }
            if large {
                large_count += 1;
            }

            let risk = compute_file_risk(sensitive, large, fd.is_binary);
            if risk > max_risk {
                max_risk = risk;
            }

            files.push(FileBreakdown {
                path: fd.path.clone(),
                change_kind: fd.change_kind,
                category,
                additions: fd.additions,
                deletions: fd.deletions,
                is_binary: fd.is_binary,
                is_security_sensitive: sensitive,
                is_large: large,
                risk,
            });
        }

        let summary_text = build_summary_text(
            &files,
            analysis.total_additions,
            analysis.total_deletions,
            &categories,
            max_risk,
        );

        Self {
            files,
            total_additions: analysis.total_additions,
            total_deletions: analysis.total_deletions,
            total_files: analysis.files.len(),
            risk_level: max_risk,
            summary_text,
            categories,
            has_security_sensitive_changes: has_security,
            large_change_count: large_count,
        }
    }
}

impl fmt::Display for DiffReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary_text)
    }
}

fn compute_file_risk(sensitive: bool, large: bool, binary: bool) -> RiskLevel {
    if sensitive {
        return RiskLevel::High;
    }
    if large || binary {
        return RiskLevel::Medium;
    }
    RiskLevel::Low
}

fn build_summary_text(
    files: &[FileBreakdown],
    total_add: usize,
    total_del: usize,
    categories: &BTreeMap<FileCategory, usize>,
    risk: RiskLevel,
) -> String {
    if files.is_empty() {
        return "No changes detected.".to_string();
    }

    let added = files
        .iter()
        .filter(|f| f.change_kind == DiffChangeKind::Added)
        .count();
    let modified = files
        .iter()
        .filter(|f| f.change_kind == DiffChangeKind::Modified)
        .count();
    let deleted = files
        .iter()
        .filter(|f| f.change_kind == DiffChangeKind::Deleted)
        .count();
    let renamed = files
        .iter()
        .filter(|f| f.change_kind == DiffChangeKind::Renamed)
        .count();

    let mut change_parts: Vec<String> = Vec::new();
    if added > 0 {
        change_parts.push(format!("{added} added"));
    }
    if modified > 0 {
        change_parts.push(format!("{modified} modified"));
    }
    if deleted > 0 {
        change_parts.push(format!("{deleted} deleted"));
    }
    if renamed > 0 {
        change_parts.push(format!("{renamed} renamed"));
    }

    let header = format!(
        "{} file(s) changed: {}: +{} -{}",
        files.len(),
        change_parts.join(", "),
        total_add,
        total_del,
    );

    let mut lines = vec![header];

    if categories.len() > 1 {
        let cat_parts: Vec<String> = categories
            .iter()
            .map(|(cat, count)| format!("{cat}: {count}"))
            .collect();
        lines.push(format!("Categories: {}", cat_parts.join(", ")));
    }

    lines.push(format!("Risk: {risk}"));
    lines.join("\n")
}
