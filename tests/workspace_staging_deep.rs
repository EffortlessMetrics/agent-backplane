// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-workspace crate's staging logic.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_glob::IncludeExcludeGlobs;
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// Create a source tree in a temp directory and return the `TempDir` handle.
fn make_source(files: &[(&str, &str)]) -> TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir");
    for (rel, content) in files {
        let p = tmp.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(&p, content).expect("write file");
    }
    tmp
}

fn make_spec(root: &Path, mode: WorkspaceMode, inc: &[&str], exc: &[&str]) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().into_owned(),
        mode,
        include: patterns(inc),
        exclude: patterns(exc),
    }
}

fn file_exists(base: &Path, rel: &str) -> bool {
    base.join(rel).exists()
}

fn read_file(base: &Path, rel: &str) -> String {
    fs::read_to_string(base.join(rel)).expect("read file")
}

/// Collect all file relative paths under `base` (excluding `.git`).
fn collect_files(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(base)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
    {
        let entry = entry.expect("walkdir entry");
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(base).unwrap().to_path_buf();
            out.push(rel);
        }
    }
    out.sort();
    out
}

// ===========================================================================
// 1. WorkspaceSpec construction
// ===========================================================================

#[test]
fn spec_passthrough_preserves_root() {
    let spec = make_spec(
        Path::new("/some/path"),
        WorkspaceMode::PassThrough,
        &[],
        &[],
    );
    assert_eq!(spec.root, "/some/path");
    assert!(matches!(spec.mode, WorkspaceMode::PassThrough));
}

#[test]
fn spec_staged_preserves_root() {
    let spec = make_spec(Path::new("/other"), WorkspaceMode::Staged, &[], &[]);
    assert_eq!(spec.root, "/other");
    assert!(matches!(spec.mode, WorkspaceMode::Staged));
}

#[test]
fn spec_includes_excludes_stored() {
    let spec = make_spec(
        Path::new("."),
        WorkspaceMode::Staged,
        &["src/**"],
        &["*.log"],
    );
    assert_eq!(spec.include, vec!["src/**"]);
    assert_eq!(spec.exclude, vec!["*.log"]);
}

#[test]
fn spec_empty_globs() {
    let spec = make_spec(Path::new("."), WorkspaceMode::Staged, &[], &[]);
    assert!(spec.include.is_empty());
    assert!(spec.exclude.is_empty());
}

#[test]
fn spec_multiple_globs() {
    let spec = make_spec(
        Path::new("."),
        WorkspaceMode::Staged,
        &["src/**", "tests/**", "*.toml"],
        &["target/**", "*.tmp"],
    );
    assert_eq!(spec.include.len(), 3);
    assert_eq!(spec.exclude.len(), 2);
}

// ===========================================================================
// 2. Staged copy with glob filtering
// ===========================================================================

#[test]
fn staged_copies_all_files_no_globs() {
    let src = make_source(&[("a.txt", "aaa"), ("b.txt", "bbb"), ("sub/c.txt", "ccc")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a.txt"));
    assert!(file_exists(ws.path(), "b.txt"));
    assert!(file_exists(ws.path(), "sub/c.txt"));
}

#[test]
fn staged_preserves_file_contents() {
    let src = make_source(&[("hello.txt", "world")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "hello.txt"), "world");
}

#[test]
fn staged_include_filters_files() {
    let src = make_source(&[
        ("src/lib.rs", "fn main() {}"),
        ("README.md", "# readme"),
        ("docs/guide.md", "guide"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(!file_exists(ws.path(), "README.md"));
    assert!(!file_exists(ws.path(), "docs/guide.md"));
}

#[test]
fn staged_exclude_filters_files() {
    let src = make_source(&[
        ("src/lib.rs", "code"),
        ("build.log", "log"),
        ("tmp/cache.tmp", "cache"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["*.log", "*.tmp"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(!file_exists(ws.path(), "build.log"));
    assert!(!file_exists(ws.path(), "tmp/cache.tmp"));
}

#[test]
fn staged_exclude_overrides_include() {
    let src = make_source(&[("src/lib.rs", "code"), ("src/generated/out.rs", "gen")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**"]))
        .exclude(patterns(&["src/generated/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(!file_exists(ws.path(), "src/generated/out.rs"));
}

#[test]
fn staged_preserves_directory_structure() {
    let src = make_source(&[("a/b/c/d.txt", "deep"), ("x/y.txt", "shallow")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("a/b/c").is_dir());
    assert!(ws.path().join("x").is_dir());
}

#[test]
fn staged_multiple_include_patterns() {
    let src = make_source(&[
        ("src/lib.rs", "code"),
        ("tests/test.rs", "test"),
        ("docs/guide.md", "guide"),
        ("build.sh", "build"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**", "tests/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(file_exists(ws.path(), "tests/test.rs"));
    assert!(!file_exists(ws.path(), "docs/guide.md"));
    assert!(!file_exists(ws.path(), "build.sh"));
}

#[test]
fn staged_multiple_exclude_patterns() {
    let src = make_source(&[
        ("src/lib.rs", "code"),
        ("target/debug/bin", "bin"),
        ("data.tmp", "tmp"),
        ("app.log", "log"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["target/**", "*.tmp", "*.log"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(!file_exists(ws.path(), "target/debug/bin"));
    assert!(!file_exists(ws.path(), "data.tmp"));
    assert!(!file_exists(ws.path(), "app.log"));
}

#[test]
fn staged_wildcard_include_allows_all() {
    let src = make_source(&[("a.txt", "a"), ("b/c.txt", "c")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a.txt"));
    assert!(file_exists(ws.path(), "b/c.txt"));
}

#[test]
fn staged_extension_include() {
    let src = make_source(&[("lib.rs", "rs"), ("lib.py", "py"), ("lib.js", "js")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["*.rs"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "lib.rs"));
    assert!(!file_exists(ws.path(), "lib.py"));
    assert!(!file_exists(ws.path(), "lib.js"));
}

#[test]
fn staged_binary_file_preserved() {
    let src_dir = tempfile::tempdir().unwrap();
    let bin_data: Vec<u8> = (0..=255).collect();
    fs::write(src_dir.path().join("data.bin"), &bin_data).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let copied = fs::read(ws.path().join("data.bin")).unwrap();
    assert_eq!(copied, bin_data);
}

#[test]
fn staged_large_file_integrity() {
    let src_dir = tempfile::tempdir().unwrap();
    let large = "x".repeat(1_000_000);
    fs::write(src_dir.path().join("big.txt"), &large).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "big.txt").len(), 1_000_000);
}

// ===========================================================================
// 3. Git initialization in staged workspace
// ===========================================================================

#[test]
fn staged_initialises_git_by_default() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists(), ".git should be created");
}

#[test]
fn staged_git_init_disabled() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn staged_git_status_clean_after_init() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.as_ref().is_none_or(|s| s.trim().is_empty()),
        "git status should be clean, got: {status:?}"
    );
}

#[test]
fn staged_git_diff_empty_after_init() {
    let src = make_source(&[("file.txt", "content")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(
        diff.as_ref().is_none_or(|d| d.trim().is_empty()),
        "git diff should be empty, got: {diff:?}"
    );
}

#[test]
fn staged_git_detects_modification() {
    let src = make_source(&[("file.txt", "original")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("file.txt"), "modified").unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.as_ref().is_some_and(|s| !s.trim().is_empty()),
        "git status should show modifications"
    );
}

#[test]
fn staged_git_detects_new_file() {
    let src = make_source(&[("existing.txt", "exists")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("new_file.txt"), "new").unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.as_ref().is_some_and(|s| s.contains("new_file.txt")),
        "git status should show new file"
    );
}

#[test]
fn staged_git_detects_deletion() {
    let src = make_source(&[("to_delete.txt", "bye")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::remove_file(ws.path().join("to_delete.txt")).unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(
        status.as_ref().is_some_and(|s| s.contains("to_delete.txt")),
        "git status should show deletion"
    );
}

#[test]
fn staged_git_diff_shows_content_change() {
    let src = make_source(&[("data.txt", "line1\nline2\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("data.txt"), "line1\nchanged\n").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(
        diff.as_ref().is_some_and(|d| d.contains("changed")),
        "diff should contain the new content"
    );
}

// ===========================================================================
// 4. .git directory exclusion
// ===========================================================================

#[test]
fn staged_excludes_source_git_directory() {
    let src_dir = tempfile::tempdir().unwrap();
    fs::write(src_dir.path().join("file.txt"), "content").unwrap();
    // Create a fake .git directory in the source
    let git_dir = src_dir.path().join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(git_dir.join("config"), "[core]").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "file.txt"));
    // The source .git contents should not be copied
    assert!(!file_exists(ws.path(), ".git/HEAD"));
    assert!(!file_exists(ws.path(), ".git/config"));
}

#[test]
fn staged_excludes_nested_git_directory() {
    let src = make_source(&[
        ("project/file.rs", "code"),
        ("project/.git/HEAD", "ref: refs/heads/main"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "project/file.rs"));
    assert!(!file_exists(ws.path(), "project/.git/HEAD"));
}

#[test]
fn manager_staged_excludes_git_directory() {
    let src_dir = tempfile::tempdir().unwrap();
    fs::write(src_dir.path().join("file.txt"), "data").unwrap();
    let git_dir = src_dir.path().join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    // Place a marker file in source .git to prove it was NOT copied
    fs::write(git_dir.join("source_marker"), "from source").unwrap();

    let spec = make_spec(src_dir.path(), WorkspaceMode::Staged, &[], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "file.txt"));
    // The source .git's marker file must not be in the staged workspace
    assert!(
        !file_exists(ws.path(), ".git/source_marker"),
        "source .git contents should not be copied"
    );
}

// ===========================================================================
// 5. Include/exclude pattern interactions
// ===========================================================================

#[test]
fn include_only_rs_exclude_tests() {
    let src = make_source(&[
        ("src/lib.rs", "lib"),
        ("src/tests/test.rs", "test"),
        ("readme.md", "md"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["**/*.rs"]))
        .exclude(patterns(&["**/tests/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(!file_exists(ws.path(), "src/tests/test.rs"));
    assert!(!file_exists(ws.path(), "readme.md"));
}

#[test]
fn exclude_all_matches_nothing_copied() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty(), "no files should be copied: {files:?}");
}

#[test]
fn include_all_exclude_none() {
    let src = make_source(&[("a.txt", "a"), ("sub/b.txt", "b")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["**"]))
        .exclude(patterns(&[]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a.txt"));
    assert!(file_exists(ws.path(), "sub/b.txt"));
}

#[test]
fn exclude_specific_file_by_name() {
    let src = make_source(&[("keep.txt", "keep"), ("secret.key", "secret")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["secret.key"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "keep.txt"));
    assert!(!file_exists(ws.path(), "secret.key"));
}

#[test]
fn include_single_extension_nested() {
    let src = make_source(&[
        ("a.rs", "a"),
        ("b/c.rs", "c"),
        ("b/d.py", "d"),
        ("e/f/g.rs", "g"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["**/*.rs"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a.rs"));
    assert!(file_exists(ws.path(), "b/c.rs"));
    assert!(!file_exists(ws.path(), "b/d.py"));
    assert!(file_exists(ws.path(), "e/f/g.rs"));
}

#[test]
fn overlapping_include_exclude_complex() {
    let src = make_source(&[
        ("src/main.rs", "main"),
        ("src/lib.rs", "lib"),
        ("src/gen/out.rs", "gen"),
        ("tests/unit.rs", "unit"),
        ("tests/fixtures/data.json", "data"),
        ("docs/api.md", "api"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**", "tests/**"]))
        .exclude(patterns(&["src/gen/**", "tests/fixtures/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(!file_exists(ws.path(), "src/gen/out.rs"));
    assert!(file_exists(ws.path(), "tests/unit.rs"));
    assert!(!file_exists(ws.path(), "tests/fixtures/data.json"));
    assert!(!file_exists(ws.path(), "docs/api.md"));
}

#[test]
fn glob_brace_expansion_in_exclude() {
    let src = make_source(&[
        ("code.rs", "rs"),
        ("code.py", "py"),
        ("code.js", "js"),
        ("code.toml", "toml"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["*.{py,js}"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "code.rs"));
    assert!(!file_exists(ws.path(), "code.py"));
    assert!(!file_exists(ws.path(), "code.js"));
    assert!(file_exists(ws.path(), "code.toml"));
}

#[test]
fn glob_question_mark_pattern() {
    let src = make_source(&[
        ("a1.txt", "1"),
        ("a2.txt", "2"),
        ("ab.txt", "b"),
        ("abc.txt", "c"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["a?.txt"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a1.txt"));
    assert!(file_exists(ws.path(), "a2.txt"));
    assert!(file_exists(ws.path(), "ab.txt"));
    // abc.txt has 3 chars before .txt, so a?.txt won't match it
    assert!(!file_exists(ws.path(), "abc.txt"));
}

// ===========================================================================
// 6. Symlink handling
// ===========================================================================

#[cfg(unix)]
mod symlink_tests {
    use super::*;
    use std::os::unix::fs as unix_fs;

    #[test]
    fn symlink_files_not_followed() {
        let src_dir = tempfile::tempdir().unwrap();
        fs::write(src_dir.path().join("real.txt"), "real").unwrap();
        unix_fs::symlink(
            src_dir.path().join("real.txt"),
            src_dir.path().join("link.txt"),
        )
        .unwrap();
        let ws = WorkspaceStager::new()
            .source_root(src_dir.path())
            .with_git_init(false)
            .stage()
            .unwrap();
        assert!(file_exists(ws.path(), "real.txt"));
        // walkdir follow_links(false) means symlinks are visited but not followed
        // They are not regular files, so they should not be copied
        assert!(!file_exists(ws.path(), "link.txt"));
    }

    #[test]
    fn symlink_dirs_not_followed() {
        let src_dir = tempfile::tempdir().unwrap();
        let sub = src_dir.path().join("real_dir");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("file.txt"), "data").unwrap();
        unix_fs::symlink(&sub, src_dir.path().join("link_dir")).unwrap();
        let ws = WorkspaceStager::new()
            .source_root(src_dir.path())
            .with_git_init(false)
            .stage()
            .unwrap();
        assert!(file_exists(ws.path(), "real_dir/file.txt"));
        assert!(!file_exists(ws.path(), "link_dir/file.txt"));
    }
}

#[cfg(windows)]
mod symlink_tests_windows {
    // On Windows, symlinks require elevated privileges, so we just verify
    // that normal files are not affected by symlink logic.
    use super::*;

    #[test]
    fn regular_files_copied_on_windows() {
        let src = make_source(&[("normal.txt", "data")]);
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();
        assert!(file_exists(ws.path(), "normal.txt"));
        assert_eq!(read_file(ws.path(), "normal.txt"), "data");
    }
}

// ===========================================================================
// 7. Edge cases
// ===========================================================================

#[test]
fn empty_source_directory() {
    let src_dir = tempfile::tempdir().unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

#[test]
fn deeply_nested_directory_structure() {
    let src_dir = tempfile::tempdir().unwrap();
    let deep = "a/b/c/d/e/f/g/h/i/j";
    let full_path = src_dir.path().join(deep);
    fs::create_dir_all(&full_path).unwrap();
    fs::write(full_path.join("deep.txt"), "deep content").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), &format!("{deep}/deep.txt")));
    assert_eq!(
        read_file(ws.path(), &format!("{deep}/deep.txt")),
        "deep content"
    );
}

#[test]
fn special_characters_in_file_names() {
    let src = make_source(&[
        ("file with spaces.txt", "spaces"),
        ("file-with-dashes.txt", "dashes"),
        ("file_with_underscores.txt", "underscores"),
        ("file.multiple.dots.txt", "dots"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "file with spaces.txt"));
    assert!(file_exists(ws.path(), "file-with-dashes.txt"));
    assert!(file_exists(ws.path(), "file_with_underscores.txt"));
    assert!(file_exists(ws.path(), "file.multiple.dots.txt"));
}

#[test]
fn unicode_file_names() {
    let src = make_source(&[("données.txt", "french"), ("日本語.txt", "japanese")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "données.txt"));
    assert!(file_exists(ws.path(), "日本語.txt"));
}

#[test]
fn empty_file_copied() {
    let src = make_source(&[("empty.txt", "")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "empty.txt"));
    assert_eq!(read_file(ws.path(), "empty.txt"), "");
}

#[test]
fn many_files_in_single_directory() {
    let src_dir = tempfile::tempdir().unwrap();
    for i in 0..100 {
        fs::write(
            src_dir.path().join(format!("file_{i}.txt")),
            format!("content_{i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = collect_files(ws.path());
    assert_eq!(files.len(), 100);
}

#[test]
fn stager_missing_source_root_errors() {
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
fn stager_nonexistent_source_errors() {
    let result = WorkspaceStager::new()
        .source_root("/absolutely/nonexistent/path/12345")
        .stage();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("does not exist"));
}

#[test]
fn stager_invalid_glob_errors() {
    let src_dir = tempfile::tempdir().unwrap();
    let result = WorkspaceStager::new()
        .source_root(src_dir.path())
        .include(patterns(&["["]))
        .stage();
    assert!(result.is_err());
}

#[test]
fn stager_invalid_exclude_glob_errors() {
    let src_dir = tempfile::tempdir().unwrap();
    let result = WorkspaceStager::new()
        .source_root(src_dir.path())
        .exclude(patterns(&["["]))
        .stage();
    assert!(result.is_err());
}

// ===========================================================================
// WorkspaceManager::prepare tests
// ===========================================================================

#[test]
fn manager_passthrough_returns_original_path() {
    let src_dir = tempfile::tempdir().unwrap();
    let spec = make_spec(src_dir.path(), WorkspaceMode::PassThrough, &[], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src_dir.path());
}

#[test]
fn manager_staged_returns_different_path() {
    let src = make_source(&[("f.txt", "data")]);
    let spec = make_spec(src.path(), WorkspaceMode::Staged, &[], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn manager_staged_copies_files() {
    let src = make_source(&[("a.txt", "aaa"), ("dir/b.txt", "bbb")]);
    let spec = make_spec(src.path(), WorkspaceMode::Staged, &[], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "a.txt"));
    assert!(file_exists(ws.path(), "dir/b.txt"));
    assert_eq!(read_file(ws.path(), "a.txt"), "aaa");
}

#[test]
fn manager_staged_with_include() {
    let src = make_source(&[("src/main.rs", "main"), ("readme.md", "readme")]);
    let spec = make_spec(src.path(), WorkspaceMode::Staged, &["src/**"], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(!file_exists(ws.path(), "readme.md"));
}

#[test]
fn manager_staged_with_exclude() {
    let src = make_source(&[("src/main.rs", "main"), ("target/out", "out")]);
    let spec = make_spec(src.path(), WorkspaceMode::Staged, &[], &["target/**"]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(!file_exists(ws.path(), "target/out"));
}

#[test]
fn manager_staged_initialises_git() {
    let src = make_source(&[("f.txt", "data")]);
    let spec = make_spec(src.path(), WorkspaceMode::Staged, &[], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn manager_staged_invalid_glob_errors() {
    let src_dir = tempfile::tempdir().unwrap();
    let spec = make_spec(src_dir.path(), WorkspaceMode::Staged, &["["], &[]);
    assert!(WorkspaceManager::prepare(&spec).is_err());
}

// ===========================================================================
// WorkspaceStager builder chain tests
// ===========================================================================

#[test]
fn stager_default_has_git_init_enabled() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_builder_chain_all_options() {
    let src = make_source(&[
        ("src/lib.rs", "code"),
        ("target/out", "out"),
        ("readme.md", "md"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**"]))
        .exclude(patterns(&["target/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(!file_exists(ws.path(), "target/out"));
    assert!(!file_exists(ws.path(), "readme.md"));
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn stager_default_trait() {
    let stager = WorkspaceStager::default();
    // Should be same as new() - no source root
    let result = stager.stage();
    assert!(result.is_err());
}

// ===========================================================================
// IncludeExcludeGlobs direct tests (used by copy_workspace internally)
// ===========================================================================

#[test]
fn glob_empty_rules_allow_everything() {
    let rules = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert!(rules.decide_str("anything").is_allowed());
    assert!(rules.decide_str("deep/nested/path.rs").is_allowed());
}

#[test]
fn glob_include_only() {
    let rules = IncludeExcludeGlobs::new(&patterns(&["*.rs"]), &[]).unwrap();
    assert!(rules.decide_str("main.rs").is_allowed());
    assert!(!rules.decide_str("readme.md").is_allowed());
}

#[test]
fn glob_exclude_only() {
    let rules = IncludeExcludeGlobs::new(&[], &patterns(&["*.log"])).unwrap();
    assert!(rules.decide_str("main.rs").is_allowed());
    assert!(!rules.decide_str("debug.log").is_allowed());
}

#[test]
fn glob_exclude_takes_precedence() {
    let rules =
        IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["src/secret/**"])).unwrap();
    assert!(rules.decide_str("src/lib.rs").is_allowed());
    assert!(!rules.decide_str("src/secret/key.pem").is_allowed());
}

#[test]
fn glob_decide_path_consistency() {
    let rules = IncludeExcludeGlobs::new(&patterns(&["**/*.rs"]), &[]).unwrap();
    let p = Path::new("src/lib.rs");
    assert_eq!(rules.decide_path(p), rules.decide_str("src/lib.rs"));
}

// ===========================================================================
// PreparedWorkspace behavior
// ===========================================================================

#[test]
fn prepared_workspace_path_accessible() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

#[test]
fn prepared_workspace_passthrough_path_is_source() {
    let src_dir = tempfile::tempdir().unwrap();
    let spec = make_spec(src_dir.path(), WorkspaceMode::PassThrough, &[], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src_dir.path());
}

// ===========================================================================
// Multiple staging from same source
// ===========================================================================

#[test]
fn multiple_stagings_are_independent() {
    let src = make_source(&[("f.txt", "original")]);
    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_ne!(ws1.path(), ws2.path());
    // Modify ws1, ws2 should be unaffected
    fs::write(ws1.path().join("f.txt"), "modified").unwrap();
    assert_eq!(read_file(ws2.path(), "f.txt"), "original");
}

// ===========================================================================
// Comprehensive pattern matching with staged files
// ===========================================================================

#[test]
fn staged_dotfiles_included_by_default() {
    let src = make_source(&[
        (".hidden", "hidden"),
        (".config/settings", "settings"),
        ("visible.txt", "visible"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), ".hidden"));
    assert!(file_exists(ws.path(), ".config/settings"));
    assert!(file_exists(ws.path(), "visible.txt"));
}

#[test]
fn staged_exclude_dotfiles() {
    let src = make_source(&[(".env", "secret"), ("src/main.rs", "code")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&[".env"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!file_exists(ws.path(), ".env"));
    assert!(file_exists(ws.path(), "src/main.rs"));
}

#[test]
fn staged_exclude_node_modules_pattern() {
    let src = make_source(&[
        ("src/index.js", "js"),
        ("node_modules/pkg/index.js", "pkg"),
        ("node_modules/.package-lock.json", "lock"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["node_modules/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/index.js"));
    assert!(!file_exists(ws.path(), "node_modules/pkg/index.js"));
    assert!(!file_exists(ws.path(), "node_modules/.package-lock.json"));
}

#[test]
fn staged_include_only_toml_files() {
    let src = make_source(&[
        ("Cargo.toml", "[package]"),
        ("Cargo.lock", "lock"),
        ("pyproject.toml", "[project]"),
        ("src/lib.rs", "code"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["**/*.toml"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "Cargo.toml"));
    assert!(file_exists(ws.path(), "pyproject.toml"));
    assert!(!file_exists(ws.path(), "Cargo.lock"));
    assert!(!file_exists(ws.path(), "src/lib.rs"));
}

#[test]
fn staged_preserves_readonly_file_content() {
    let src_dir = tempfile::tempdir().unwrap();
    let file_path = src_dir.path().join("readonly.txt");
    fs::write(&file_path, "read only content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "readonly.txt"), "read only content");
}

#[test]
fn staged_handles_file_with_no_extension() {
    let src = make_source(&[
        ("Makefile", "all:"),
        ("Dockerfile", "FROM"),
        ("LICENSE", "MIT"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "Makefile"));
    assert!(file_exists(ws.path(), "Dockerfile"));
    assert!(file_exists(ws.path(), "LICENSE"));
}

#[test]
fn staged_include_specific_directory_only() {
    let src = make_source(&[
        ("src/lib.rs", "lib"),
        ("src/main.rs", "main"),
        ("tests/test.rs", "test"),
        ("benches/bench.rs", "bench"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(!file_exists(ws.path(), "tests/test.rs"));
    assert!(!file_exists(ws.path(), "benches/bench.rs"));
}

// ===========================================================================
// Git operations on non-git directory
// ===========================================================================

#[test]
fn git_status_on_non_git_dir_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let status = WorkspaceManager::git_status(dir.path());
    assert!(status.is_none());
}

#[test]
fn git_diff_on_non_git_dir_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let diff = WorkspaceManager::git_diff(dir.path());
    assert!(diff.is_none());
}

// ===========================================================================
// Determinism tests
// ===========================================================================

#[test]
fn staging_produces_same_file_set() {
    let src = make_source(&[("a.txt", "a"), ("b/c.txt", "c"), ("d/e/f.txt", "f")]);
    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files1 = collect_files(ws1.path());
    let files2 = collect_files(ws2.path());
    assert_eq!(files1, files2);
}

#[test]
fn staging_preserves_all_content() {
    let src = make_source(&[("a.txt", "alpha"), ("b/c.txt", "charlie")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(read_file(ws.path(), "a.txt"), "alpha");
    assert_eq!(read_file(ws.path(), "b/c.txt"), "charlie");
}

// ===========================================================================
// Complex real-world-like scenarios
// ===========================================================================

#[test]
fn realistic_rust_project_staging() {
    let src = make_source(&[
        ("Cargo.toml", "[package]"),
        ("Cargo.lock", "lock"),
        ("src/lib.rs", "pub fn foo() {}"),
        ("src/main.rs", "fn main() {}"),
        ("src/utils/mod.rs", "pub mod helpers;"),
        ("tests/integration.rs", "use lib;"),
        ("target/debug/binary", "ELF"),
        ("target/release/binary", "ELF"),
        (".git/HEAD", "ref: refs/heads/main"),
        (".gitignore", "/target"),
        ("README.md", "# Project"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["target/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "Cargo.toml"));
    assert!(file_exists(ws.path(), "Cargo.lock"));
    assert!(file_exists(ws.path(), "src/lib.rs"));
    assert!(file_exists(ws.path(), "src/main.rs"));
    assert!(file_exists(ws.path(), "src/utils/mod.rs"));
    assert!(file_exists(ws.path(), "tests/integration.rs"));
    assert!(!file_exists(ws.path(), "target/debug/binary"));
    assert!(!file_exists(ws.path(), "target/release/binary"));
    // .git excluded by copy_workspace filter_entry
    assert!(!file_exists(ws.path(), ".git/HEAD"));
    assert!(file_exists(ws.path(), ".gitignore"));
    assert!(file_exists(ws.path(), "README.md"));
}

#[test]
fn realistic_node_project_staging() {
    let src = make_source(&[
        ("package.json", "{}"),
        ("src/index.js", "module.exports"),
        ("src/utils.js", "exports"),
        ("node_modules/left-pad/index.js", "pad"),
        ("dist/bundle.js", "bundle"),
        (".env", "SECRET=123"),
        ("tsconfig.json", "{}"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["node_modules/**", "dist/**", ".env"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "package.json"));
    assert!(file_exists(ws.path(), "src/index.js"));
    assert!(file_exists(ws.path(), "src/utils.js"));
    assert!(!file_exists(ws.path(), "node_modules/left-pad/index.js"));
    assert!(!file_exists(ws.path(), "dist/bundle.js"));
    assert!(!file_exists(ws.path(), ".env"));
    assert!(file_exists(ws.path(), "tsconfig.json"));
}

#[test]
fn staged_with_git_then_modify_and_diff() {
    let src = make_source(&[("file1.txt", "original1\n"), ("file2.txt", "original2\n")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    // Modify file1
    fs::write(ws.path().join("file1.txt"), "modified1\n").unwrap();
    // Add a new file
    fs::write(ws.path().join("file3.txt"), "new\n").unwrap();
    // Delete file2
    fs::remove_file(ws.path().join("file2.txt")).unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    let s = status.expect("git status should work");
    assert!(s.contains("file1.txt"), "modified file should appear");
    assert!(s.contains("file2.txt"), "deleted file should appear");
    assert!(s.contains("file3.txt"), "new file should appear");
}

#[test]
fn collect_files_count_matches_source() {
    let src = make_source(&[
        ("a.txt", "a"),
        ("b.txt", "b"),
        ("c/d.txt", "d"),
        ("c/e.txt", "e"),
        ("f/g/h.txt", "h"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let src_files = collect_files(src.path());
    let dest_files = collect_files(ws.path());
    assert_eq!(src_files.len(), dest_files.len());
    assert_eq!(src_files, dest_files);
}

#[test]
fn parallel_staging_different_filters() {
    let src = make_source(&[
        ("src/lib.rs", "lib"),
        ("src/main.rs", "main"),
        ("tests/test.rs", "test"),
        ("docs/api.md", "api"),
    ]);

    let ws_src = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();

    let ws_tests = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["tests/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(file_exists(ws_src.path(), "src/lib.rs"));
    assert!(!file_exists(ws_src.path(), "tests/test.rs"));
    assert!(file_exists(ws_tests.path(), "tests/test.rs"));
    assert!(!file_exists(ws_tests.path(), "src/lib.rs"));
}

#[test]
fn staging_only_specific_nested_path() {
    let src = make_source(&[
        ("a/b/target.txt", "found"),
        ("a/b/other.txt", "other"),
        ("a/c/nope.txt", "nope"),
        ("x.txt", "x"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["a/b/**"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "a/b/target.txt"));
    assert!(file_exists(ws.path(), "a/b/other.txt"));
    assert!(!file_exists(ws.path(), "a/c/nope.txt"));
    assert!(!file_exists(ws.path(), "x.txt"));
}

#[test]
fn exclude_pattern_with_double_star() {
    let src = make_source(&[
        ("a/test_file.rs", "t1"),
        ("b/c/test_file.rs", "t2"),
        ("a/main.rs", "m1"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(patterns(&["**/test_*"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!file_exists(ws.path(), "a/test_file.rs"));
    assert!(!file_exists(ws.path(), "b/c/test_file.rs"));
    assert!(file_exists(ws.path(), "a/main.rs"));
}

#[test]
fn include_multiple_extensions() {
    let src = make_source(&[
        ("code.rs", "rs"),
        ("code.py", "py"),
        ("code.js", "js"),
        ("code.txt", "txt"),
        ("code.md", "md"),
    ]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["*.rs", "*.py"]))
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "code.rs"));
    assert!(file_exists(ws.path(), "code.py"));
    assert!(!file_exists(ws.path(), "code.js"));
    assert!(!file_exists(ws.path(), "code.txt"));
    assert!(!file_exists(ws.path(), "code.md"));
}

#[test]
fn staged_workspace_is_writable() {
    let src = make_source(&[("f.txt", "original")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Should be able to create new files in the staged workspace
    fs::write(ws.path().join("new.txt"), "new content").unwrap();
    assert!(file_exists(ws.path(), "new.txt"));
    // Should be able to modify existing files
    fs::write(ws.path().join("f.txt"), "modified").unwrap();
    assert_eq!(read_file(ws.path(), "f.txt"), "modified");
}

#[test]
fn staged_source_unchanged_after_modification() {
    let src = make_source(&[("f.txt", "original")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    fs::write(ws.path().join("f.txt"), "modified").unwrap();
    // Source should be unchanged
    assert_eq!(
        fs::read_to_string(src.path().join("f.txt")).unwrap(),
        "original"
    );
}

#[test]
fn stager_source_root_accepts_pathbuf() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path().to_path_buf())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "f.txt"));
}

#[test]
fn stager_source_root_accepts_string() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path().to_string_lossy().to_string())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(file_exists(ws.path(), "f.txt"));
}
