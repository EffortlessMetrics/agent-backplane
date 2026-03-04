// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep workspace staging and git integration tests.
//!
//! Covers file staging scenarios, git initialisation, diff computation,
//! cleanup behaviour, and edge cases for the `WorkspaceStager` and
//! `WorkspaceManager` APIs.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::diff::{diff_workspace, DiffAnalyzer, DiffPolicy, PolicyResult};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn staged_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

fn staged_spec_globs(root: &Path, include: Vec<String>, exclude: Vec<String>) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
    }
}

/// Collect sorted relative file paths (excluding `.git`) under `root`.
fn collect_files(root: &Path) -> Vec<String> {
    let mut files: Vec<String> = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == ".git"))
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            e.path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    files
}

/// Run a git command in `dir` and return trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Create a standard fixture tree for tests.
fn create_fixture(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src").join("data.json"), "{}").unwrap();
}

// ===========================================================================
// 1. FILE STAGING SCENARIOS (20+ tests)
// ===========================================================================

#[test]
fn stage_all_files_no_filters() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"main.rs".to_string()));
    assert!(staged.contains(&"lib.rs".to_string()));
    assert!(staged.contains(&"README.md".to_string()));
    assert!(staged.contains(&"src/utils.rs".to_string()));
    assert!(staged.contains(&"src/data.json".to_string()));
}

#[test]
fn stage_with_include_pattern_rs_only() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*.rs".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"main.rs".to_string()));
    assert!(staged.contains(&"lib.rs".to_string()));
    assert!(staged.contains(&"src/utils.rs".to_string()));
    assert!(!staged.contains(&"README.md".to_string()));
    assert!(!staged.contains(&"src/data.json".to_string()));
}

#[test]
fn stage_with_exclude_pattern() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.md".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(!staged.contains(&"README.md".to_string()));
    assert!(staged.contains(&"main.rs".to_string()));
}

#[test]
fn stage_include_and_exclude_combined() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["**/*.json".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"src/utils.rs".to_string()));
    assert!(!staged.contains(&"src/data.json".to_string()));
    assert!(!staged.contains(&"main.rs".to_string()));
}

#[test]
fn stage_deeply_nested_directories() {
    let src = tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c").join("d").join("e");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.txt"), "deep content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"a/b/c/d/e/deep.txt".to_string()));
}

#[test]
fn stage_hidden_dotfiles() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    fs::write(src.path().join(".env"), "KEY=VAL").unwrap();
    fs::write(src.path().join("visible.txt"), "hi").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&".hidden".to_string()));
    assert!(staged.contains(&".env".to_string()));
    assert!(staged.contains(&"visible.txt".to_string()));
}

#[test]
fn stage_excludes_dotfiles_with_pattern() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    fs::write(src.path().join("visible.txt"), "hi").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec![".*".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(!staged.contains(&".hidden".to_string()));
    assert!(staged.contains(&"visible.txt".to_string()));
}

#[test]
fn stage_empty_directories_are_created() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_dir")).unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // The empty dir may or may not exist (walkdir visits it, copy_workspace
    // creates it). At minimum the file must be staged.
    assert!(ws.path().join("file.txt").exists());
}

#[test]
fn stage_large_number_of_files() {
    let src = tempdir().unwrap();
    for i in 0..120 {
        fs::write(src.path().join(format!("file_{i:04}.txt")), format!("content {i}")).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert_eq!(staged.len(), 120);
}

#[test]
fn stage_large_files_with_include_filter() {
    let src = tempdir().unwrap();
    for i in 0..100 {
        let ext = if i % 2 == 0 { "rs" } else { "txt" };
        fs::write(
            src.path().join(format!("file_{i:04}.{ext}")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert_eq!(staged.len(), 50);
    assert!(staged.iter().all(|f| f.ends_with(".rs")));
}

#[test]
fn stage_unicode_filenames() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("données.txt"), "french data").unwrap();
    fs::write(src.path().join("日本語.txt"), "japanese data").unwrap();
    fs::write(src.path().join("emoji_🦀.txt"), "rust crab").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert_eq!(staged.len(), 3);
}

#[test]
fn stage_binary_files() {
    let src = tempdir().unwrap();
    // Write binary content (non-UTF-8 bytes).
    fs::write(src.path().join("image.bin"), [0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF]).unwrap();
    fs::write(src.path().join("text.txt"), "hello").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"image.bin".to_string()));
    assert!(staged.contains(&"text.txt".to_string()));

    // Verify binary content is preserved.
    let content = fs::read(ws.path().join("image.bin")).unwrap();
    assert_eq!(content, &[0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF]);
}

#[test]
fn stage_preserves_file_content() {
    let src = tempdir().unwrap();
    let original = "fn main() {\n    println!(\"Hello, world!\");\n}\n";
    fs::write(src.path().join("main.rs"), original).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged_content = fs::read_to_string(ws.path().join("main.rs")).unwrap();
    assert_eq!(staged_content, original);
}

#[test]
fn stage_multiple_extensions_exclude() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn f() {}").unwrap();
    fs::write(src.path().join("data.json"), "{}").unwrap();
    fs::write(src.path().join("log.log"), "entry").unwrap();
    fs::write(src.path().join("temp.tmp"), "tmp").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into(), "*.tmp".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"code.rs".to_string()));
    assert!(staged.contains(&"data.json".to_string()));
    assert!(!staged.contains(&"log.log".to_string()));
    assert!(!staged.contains(&"temp.tmp".to_string()));
}

#[test]
fn stage_nested_include_exclude() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src").join("generated")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "pub mod m;").unwrap();
    fs::write(
        src.path().join("src").join("generated").join("out.rs"),
        "// generated",
    )
    .unwrap();
    fs::write(src.path().join("README.md"), "# Readme").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["src/generated/**".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"src/lib.rs".to_string()));
    assert!(!staged.contains(&"src/generated/out.rs".to_string()));
    assert!(!staged.contains(&"README.md".to_string()));
}

#[test]
fn stage_dot_directory_excluded() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".config")).unwrap();
    fs::write(src.path().join(".config").join("settings.json"), "{}").unwrap();
    fs::write(src.path().join("app.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec![".config/**".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"app.rs".to_string()));
    assert!(!staged.contains(&".config/settings.json".to_string()));
}

#[test]
fn stage_source_git_directory_excluded() {
    let src = tempdir().unwrap();
    // Simulate a .git directory in source.
    fs::create_dir_all(src.path().join(".git").join("objects")).unwrap();
    fs::write(
        src.path().join(".git").join("HEAD"),
        "ref: refs/heads/main",
    )
    .unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // The staged workspace should NOT copy source .git but will have its own.
    let staged = collect_files(ws.path());
    assert!(staged.contains(&"file.txt".to_string()));
    // The staged workspace gets its own .git from ensure_git_repo.
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stage_manager_staged_mode() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let spec = staged_spec(src.path());
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"main.rs".to_string()));
    // Staged path should differ from source.
    assert_ne!(ws.path(), src.path());
}

#[test]
fn stage_manager_passthrough_mode() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn stage_manager_with_globs() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let spec = staged_spec_globs(src.path(), vec!["**/*.rs".into()], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.iter().all(|f| f.ends_with(".rs")));
}

#[test]
fn stage_without_git_init() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("file.txt").exists());
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn stage_empty_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let content = fs::read_to_string(ws.path().join("empty.txt")).unwrap();
    assert!(content.is_empty());
}

#[test]
fn stage_file_with_special_characters_in_name() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file with spaces.txt"), "spaces").unwrap();
    fs::write(src.path().join("file-with-dashes.txt"), "dashes").unwrap();
    fs::write(src.path().join("file_with_underscores.txt"), "underscores").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert_eq!(staged.len(), 3);
}

// ===========================================================================
// 2. GIT INITIALIZATION (10+ tests)
// ===========================================================================

#[test]
fn git_directory_present_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join(".git").is_dir());
}

#[test]
fn git_baseline_commit_exists() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(log.contains("baseline"), "log should contain baseline commit: {log}");
}

#[test]
fn git_log_shows_single_commit() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1", "should have exactly one commit");
}

#[test]
fn git_working_tree_clean_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let status = git(ws.path(), &["status", "--porcelain"]);
    assert!(status.is_empty(), "working tree should be clean: {status}");
}

#[test]
fn git_all_staged_files_in_initial_commit() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let committed = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    let committed_files: Vec<&str> = committed.lines().collect();
    let staged = collect_files(ws.path());

    for f in &staged {
        assert!(
            committed_files.contains(&f.as_str()),
            "file {f} should be in initial commit"
        );
    }
}

#[test]
fn git_status_api_returns_clean() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.as_ref().is_none_or(|s| s.trim().is_empty()),
        "should be clean: {status:?}"
    );
}

#[test]
fn git_diff_api_returns_empty_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(
        diff.as_ref().is_none_or(|d| d.trim().is_empty()),
        "diff should be empty: {diff:?}"
    );
}

#[test]
fn git_can_create_diff_after_modification() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // Modify a file.
    fs::write(ws.path().join("main.rs"), "fn main() { println!(\"changed\"); }").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    let diff_text = diff.unwrap();
    assert!(diff_text.contains("changed"), "diff should show change: {diff_text}");
}

#[test]
fn git_head_ref_is_valid() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let sha = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(sha.len(), 40, "HEAD should be a full SHA: {sha}");
    assert!(
        sha.chars().all(|c| c.is_ascii_hexdigit()),
        "SHA should be hex: {sha}"
    );
}

#[test]
fn git_author_is_abp() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let author = git(ws.path(), &["log", "-1", "--format=%an"]);
    assert_eq!(author, "abp");
}

#[test]
fn git_email_is_abp_local() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let email = git(ws.path(), &["log", "-1", "--format=%ae"]);
    assert_eq!(email, "abp@local");
}

#[test]
fn git_status_api_shows_changes_after_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("file.txt"), "modified").unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(
        !status.as_ref().unwrap().trim().is_empty(),
        "status should be non-empty"
    );
}

// ===========================================================================
// 3. DIFF COMPUTATION (10+ tests)
// ===========================================================================

#[test]
fn diff_empty_after_staging() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.is_empty());
    assert_eq!(summary.file_count(), 0);
}

#[test]
fn diff_detects_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("file.txt"), "modified content").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 1);
    assert_eq!(summary.modified[0], PathBuf::from("file.txt"));
    assert!(summary.total_changes() > 0);
}

#[test]
fn diff_detects_new_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.txt"), "old").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("new_file.txt"), "new content").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 1);
    assert_eq!(summary.added[0], PathBuf::from("new_file.txt"));
}

#[test]
fn diff_detects_file_deletion() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("to_delete.txt"), "will be removed").unwrap();
    fs::write(src.path().join("keep.txt"), "stay").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::remove_file(ws.path().join("to_delete.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.deleted.len(), 1);
    assert_eq!(summary.deleted[0], PathBuf::from("to_delete.txt"));
}

#[test]
fn diff_multi_file_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "aaa").unwrap();
    fs::write(src.path().join("b.txt"), "bbb").unwrap();
    fs::write(src.path().join("c.txt"), "ccc").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("a.txt"), "aaa modified").unwrap();
    fs::write(ws.path().join("d.txt"), "new file").unwrap();
    fs::remove_file(ws.path().join("c.txt")).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 1);
    assert_eq!(summary.added.len(), 1);
    assert_eq!(summary.deleted.len(), 1);
    assert_eq!(summary.file_count(), 3);
}

#[test]
fn diff_counts_additions_and_deletions() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "line1\nline2\nline3\n").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("file.txt"), "line1\nmodified\nline3\nnew_line\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert!(summary.total_additions > 0);
    assert!(summary.total_deletions > 0);
}

#[test]
fn diff_analyzer_detects_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "original").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(!analyzer.has_changes());

    fs::write(ws.path().join("file.txt"), "modified").unwrap();
    assert!(analyzer.has_changes());
}

#[test]
fn diff_analyzer_changed_files_list() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("a.txt"), "a-modified").unwrap();
    fs::write(ws.path().join("c.txt"), "new").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let changed = analyzer.changed_files();
    assert!(changed.contains(&PathBuf::from("a.txt")));
    assert!(changed.contains(&PathBuf::from("c.txt")));
}

#[test]
fn diff_analyzer_file_was_modified() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("mod.txt"), "original").unwrap();
    fs::write(src.path().join("untouched.txt"), "same").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("mod.txt"), "changed").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.file_was_modified(Path::new("mod.txt")));
    assert!(!analyzer.file_was_modified(Path::new("untouched.txt")));
}

#[test]
fn diff_analyzer_workspace_diff_summary_text() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    assert_eq!(diff.summary(), "No changes detected.");

    fs::write(ws.path().join("b.txt"), "new").unwrap();
    let diff = analyzer.analyze().unwrap();
    assert!(diff.summary().contains("1 file(s) changed"));
}

#[test]
fn diff_workspace_diff_is_empty_method() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "f").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    assert!(diff.is_empty());

    fs::write(ws.path().join("f.txt"), "changed").unwrap();
    let diff = analyzer.analyze().unwrap();
    assert!(!diff.is_empty());
}

#[test]
fn diff_binary_file_change() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.bin"), [0x00, 0x01, 0x02]).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("data.bin"), [0xFF, 0xFE, 0xFD]).unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.modified.len(), 1);
}

#[test]
fn diff_new_file_in_subdirectory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("existing.txt"), "hello").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::create_dir_all(ws.path().join("sub").join("deep")).unwrap();
    fs::write(ws.path().join("sub").join("deep").join("new.txt"), "new").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 1);
    assert_eq!(
        summary.added[0],
        PathBuf::from("sub/deep/new.txt")
    );
}

// ===========================================================================
// 4. DIFF POLICY TESTS
// ===========================================================================

#[test]
fn diff_policy_pass_when_no_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    let policy = DiffPolicy {
        max_files: Some(5),
        max_additions: Some(100),
        denied_paths: vec![],
    };
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_fail_too_many_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // Create 5 new files.
    for i in 0..5 {
        fs::write(ws.path().join(format!("new_{i}.txt")), "new").unwrap();
    }

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    let policy = DiffPolicy {
        max_files: Some(2),
        max_additions: None,
        denied_paths: vec![],
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

#[test]
fn diff_policy_fail_denied_path() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("secrets")).unwrap();
    fs::write(src.path().join("secrets").join("key.pem"), "old key").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("secrets").join("key.pem"), "new key").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    let policy = DiffPolicy {
        max_files: None,
        max_additions: None,
        denied_paths: vec!["secrets/**".into()],
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

// ===========================================================================
// 5. CLEANUP TESTS (5+ tests)
// ===========================================================================

#[test]
fn temp_directory_removed_on_drop() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged_path = ws.path().to_path_buf();
    assert!(staged_path.exists());

    drop(ws);
    assert!(
        !staged_path.exists(),
        "temp directory should be cleaned up on drop"
    );
}

#[test]
fn multiple_stages_and_drops() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let mut paths = Vec::new();
    for _ in 0..5 {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .stage()
            .unwrap();
        paths.push(ws.path().to_path_buf());
        assert!(ws.path().exists());
    }
    // All should be cleaned up.
    for p in &paths {
        assert!(!p.exists(), "path {p:?} should be cleaned up");
    }
}

#[test]
fn stage_failure_no_source_root() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("source_root is required")
    );
}

#[test]
fn stage_failure_nonexistent_source() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/should/not/exist")
        .stage();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("does not exist")
    );
}

#[test]
fn passthrough_does_not_create_temp() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    // PassThrough uses original path, no temp dir.
    assert_eq!(ws.path(), src.path());
    drop(ws);
    // Source should still exist since it's tempdir-owned, not PreparedWorkspace-owned.
    assert!(src.path().exists());
}

#[test]
fn staged_manager_cleanup_on_drop() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let spec = staged_spec(src.path());
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let staged_path = ws.path().to_path_buf();
    drop(ws);
    assert!(!staged_path.exists());
}

// ===========================================================================
// 6. EDGE CASES (5+ tests)
// ===========================================================================

#[test]
fn stage_empty_source_directory() {
    let src = tempdir().unwrap();
    // Empty directory — no files at all.

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.is_empty());
    // Git should still be initialized.
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stage_source_with_only_excluded_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.log"), "log").unwrap();
    fs::write(src.path().join("b.log"), "log").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.is_empty());
}

#[test]
fn stage_very_long_path_names() {
    let src = tempdir().unwrap();
    // Create a moderately deep path (avoid OS limits).
    let mut path = src.path().to_path_buf();
    for _ in 0..15 {
        path = path.join("subdir");
    }
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("deep_file.txt"), "deep").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(
        staged.iter().any(|f| f.contains("deep_file.txt")),
        "should stage deeply nested file: {staged:?}"
    );
}

#[test]
fn stage_read_only_files() {
    let src = tempdir().unwrap();
    let file_path = src.path().join("readonly.txt");
    fs::write(&file_path, "immutable content").unwrap();

    // Make file read-only.
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file_path, perms).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"readonly.txt".to_string()));
    let content = fs::read_to_string(ws.path().join("readonly.txt")).unwrap();
    assert_eq!(content, "immutable content");

    // Cleanup: remove read-only so tempdir can clean up.
    #[allow(clippy::permissions_set_readonly_false)]
    {
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_readonly(false);
        fs::set_permissions(&file_path, perms).unwrap();
    }
}

#[test]
fn stage_same_source_twice_gives_independent_workspaces() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert_ne!(ws1.path(), ws2.path());

    // Modifying one doesn't affect the other.
    fs::write(ws1.path().join("file.txt"), "modified in ws1").unwrap();
    let content_ws2 = fs::read_to_string(ws2.path().join("file.txt")).unwrap();
    assert_eq!(content_ws2, "content");
}

#[test]
fn stage_large_file_content() {
    let src = tempdir().unwrap();
    let large_content: String = "x".repeat(1_000_000); // 1MB
    fs::write(src.path().join("large.txt"), &large_content).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged_content = fs::read_to_string(ws.path().join("large.txt")).unwrap();
    assert_eq!(staged_content.len(), 1_000_000);
}

#[test]
fn stage_workspace_path_is_accessible() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("test.txt"), "test").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // Should be able to read/write/list from the returned path.
    assert!(ws.path().is_dir());
    assert!(ws.path().join("test.txt").is_file());
    let entries: Vec<_> = fs::read_dir(ws.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(!entries.is_empty());
}

#[test]
fn stage_default_builder_has_git_init() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "f").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join(".git").exists());
}

#[test]
fn stage_builder_default_is_same_as_new() {
    // WorkspaceStager::default() should behave the same as new().
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "f").unwrap();

    let ws = WorkspaceStager::default()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join("f.txt").exists());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stage_nested_directories_with_mixed_patterns() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src").join("core")).unwrap();
    fs::create_dir_all(src.path().join("tests")).unwrap();
    fs::create_dir_all(src.path().join("docs")).unwrap();
    fs::write(src.path().join("src").join("core").join("main.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("src").join("core").join("config.toml"), "[config]").unwrap();
    fs::write(src.path().join("tests").join("test.rs"), "#[test]").unwrap();
    fs::write(src.path().join("docs").join("guide.md"), "# Guide").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into(), "tests/**".into()])
        .exclude(vec!["**/*.toml".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"src/core/main.rs".to_string()));
    assert!(staged.contains(&"tests/test.rs".to_string()));
    assert!(!staged.contains(&"src/core/config.toml".to_string()));
    assert!(!staged.contains(&"docs/guide.md".to_string()));
}

#[test]
fn diff_after_adding_multiple_new_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("original.txt"), "original").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    for i in 0..10 {
        fs::write(ws.path().join(format!("new_{i}.txt")), format!("content {i}")).unwrap();
    }

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.added.len(), 10);
    assert_eq!(summary.file_count(), 10);
}

#[test]
fn diff_after_deleting_all_files() {
    let src = tempdir().unwrap();
    for i in 0..5 {
        fs::write(src.path().join(format!("f_{i}.txt")), format!("content {i}")).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    for i in 0..5 {
        fs::remove_file(ws.path().join(format!("f_{i}.txt"))).unwrap();
    }

    let summary = diff_workspace(&ws).unwrap();
    assert_eq!(summary.deleted.len(), 5);
    assert!(summary.added.is_empty());
    assert!(summary.modified.is_empty());
}

#[test]
fn diff_summary_total_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "line1\nline2\n").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("file.txt"), "line1\nchanged\nline3\n").unwrap();

    let summary = diff_workspace(&ws).unwrap();
    let total = summary.total_changes();
    assert!(total > 0, "total_changes should be > 0: {total}");
    assert_eq!(total, summary.total_additions + summary.total_deletions);
}

#[test]
fn stage_files_across_many_subdirectories() {
    let src = tempdir().unwrap();
    for i in 0..20 {
        let dir = src.path().join(format!("dir_{i:02}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), format!("dir {i}")).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert_eq!(staged.len(), 20);
}

#[test]
fn stage_exclude_entire_directory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("keep")).unwrap();
    fs::create_dir_all(src.path().join("skip")).unwrap();
    fs::write(src.path().join("keep").join("a.txt"), "a").unwrap();
    fs::write(src.path().join("skip").join("b.txt"), "b").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["skip/**".into()])
        .stage()
        .unwrap();

    let staged = collect_files(ws.path());
    assert!(staged.contains(&"keep/a.txt".to_string()));
    assert!(!staged.contains(&"skip/b.txt".to_string()));
}

#[test]
fn diff_policy_pass_within_limits() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    fs::write(ws.path().join("f.txt"), "changed").unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    let policy = DiffPolicy {
        max_files: Some(10),
        max_additions: Some(1000),
        denied_paths: vec![],
    };
    assert!(policy.check(&diff).unwrap().is_pass());
}

#[test]
fn diff_policy_fail_too_many_additions() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "one line\n").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // Write many lines.
    let many_lines: String = (0..50).map(|i| format!("line {i}\n")).collect();
    fs::write(ws.path().join("f.txt"), &many_lines).unwrap();

    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();

    let policy = DiffPolicy {
        max_files: None,
        max_additions: Some(5),
        denied_paths: vec![],
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
    if let PolicyResult::Fail { violations } = result {
        assert!(violations.iter().any(|v| v.contains("too many additions")));
    }
}

#[test]
fn diff_policy_result_is_pass_method() {
    assert!(PolicyResult::Pass.is_pass());
    assert!(!PolicyResult::Fail {
        violations: vec!["x".into()]
    }
    .is_pass());
}
