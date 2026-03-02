// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive deep tests for workspace staging.
//!
//! 80+ tests covering copy correctness, glob filtering, .git exclusion,
//! git init/baseline, nested dirs, symlinks, empty workspaces, many-files
//! scenarios, glob edge cases, cleanup, and concurrent staging.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

fn spec_globs(root: &Path, inc: &[&str], exc: &[&str]) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: inc.iter().map(|s| (*s).to_string()).collect(),
        exclude: exc.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn passthrough_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    }
}

/// Collect sorted relative file paths (no dirs), excluding `.git`.
fn ls(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = WalkDir::new(root)
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
    v.sort();
    v
}

/// Collect sorted relative directory paths, excluding `.git`.
fn ls_dirs(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == ".git"))
        .filter(|e| e.file_type().is_dir())
        .filter_map(|e| {
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel.is_empty() {
                None
            } else {
                Some(rel)
            }
        })
        .collect();
    v.sort();
    v
}

fn git_cmd(path: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

/// Populate a standard project tree.
fn seed(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
    fs::write(root.join("README.md"), "# readme").unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn lib(){}").unwrap();
    fs::write(root.join("src/main.rs"), "fn main(){}").unwrap();
    fs::write(root.join("tests/it.rs"), "#[test] fn t(){}").unwrap();
    fs::write(root.join("docs/guide.md"), "guide").unwrap();
}

// ===========================================================================
// 1. Basic copy correctness (10 tests)
// ===========================================================================

#[test]
fn sd_copy_single_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "world").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("hello.txt")).unwrap(),
        "world"
    );
}

#[test]
fn sd_copy_preserves_content_exactly() {
    let src = tempdir().unwrap();
    let content = "line1\nline2\r\nline3\ttab";
    fs::write(src.path().join("mixed.txt"), content).unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("mixed.txt")).unwrap(),
        content
    );
}

#[test]
fn sd_copy_multiple_files() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let src_files = ls(src.path());
    let staged_files = ls(ws.path());
    assert_eq!(staged_files, src_files);
}

#[test]
fn sd_copy_empty_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert!(ws.path().join("empty").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty")).unwrap(), "");
}

#[test]
fn sd_copy_binary_data() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("bin.dat"), &data).unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("bin.dat")).unwrap(), data);
}

#[test]
fn sd_staged_path_differs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn sd_source_unchanged_after_staging() {
    let src = tempdir().unwrap();
    seed(src.path());
    let before = ls(src.path());
    let _ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert_eq!(ls(src.path()), before);
}

#[test]
fn sd_staged_mutation_does_not_affect_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "changed").unwrap();
    assert_eq!(
        fs::read_to_string(src.path().join("f.txt")).unwrap(),
        "original"
    );
}

#[test]
fn sd_copy_preserves_subdirectory_structure() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b/c")).unwrap();
    fs::write(src.path().join("a/b/c/deep.txt"), "deep").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert!(ws.path().join("a/b/c/deep.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("a/b/c/deep.txt")).unwrap(),
        "deep"
    );
}

#[test]
fn sd_copy_file_sizes_match() {
    let src = tempdir().unwrap();
    let big = "A".repeat(50_000);
    fs::write(src.path().join("big.txt"), &big).unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let meta = fs::metadata(ws.path().join("big.txt")).unwrap();
    assert_eq!(meta.len(), 50_000);
}

// ===========================================================================
// 2. Glob include patterns (12 tests)
// ===========================================================================

#[test]
fn sd_include_single_extension() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    for f in ls(ws.path()) {
        assert!(f.ends_with(".rs"), "unexpected: {f}");
    }
}

#[test]
fn sd_include_recursive_star_star() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["**/*.rs"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(
        files.iter().any(|f| f.contains('/')),
        "should include nested .rs"
    );
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected: {f}");
    }
}

#[test]
fn sd_include_two_extensions() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs", "*.md"], &[])).unwrap();
    for f in ls(ws.path()) {
        assert!(f.ends_with(".rs") || f.ends_with(".md"), "unexpected: {f}");
    }
}

#[test]
fn sd_include_specific_dir() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["src/**"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(!files.is_empty());
    for f in &files {
        assert!(f.starts_with("src/"), "unexpected outside src: {f}");
    }
}

#[test]
fn sd_include_question_mark_wildcard() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a1.txt"), "").unwrap();
    fs::write(src.path().join("a2.txt"), "").unwrap();
    fs::write(src.path().join("ab.txt"), "").unwrap();
    fs::write(src.path().join("abc.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["a?.txt"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"a1.txt".to_string()));
    assert!(files.contains(&"a2.txt".to_string()));
    assert!(files.contains(&"ab.txt".to_string()));
    // abc.txt has two chars after 'a' before '.txt' — ? matches exactly one char
    assert!(!files.contains(&"abc.txt".to_string()));
}

#[test]
fn sd_include_brace_expansion() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "").unwrap();
    fs::write(src.path().join("config.toml"), "").unwrap();
    fs::write(src.path().join("data.json"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.{rs,toml}"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(files.contains(&"config.toml".to_string()));
    assert!(!files.contains(&"data.json".to_string()));
}

#[test]
fn sd_include_no_patterns_allows_all() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &[])).unwrap();
    assert_eq!(ls(ws.path()), ls(src.path()));
}

#[test]
fn sd_include_char_class() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file1.txt"), "").unwrap();
    fs::write(src.path().join("file2.txt"), "").unwrap();
    fs::write(src.path().join("fileA.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["file[12].txt"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"file1.txt".to_string()));
    assert!(files.contains(&"file2.txt".to_string()));
    assert!(!files.contains(&"fileA.txt".to_string()));
}

#[test]
fn sd_include_deeply_nested_glob() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b/c/d")).unwrap();
    fs::write(src.path().join("a/b/c/d/target.rs"), "fn t(){}").unwrap();
    fs::write(src.path().join("a/b/skip.txt"), "skip").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["**/*.rs"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"a/b/c/d/target.rs".to_string()));
    assert!(!files.contains(&"a/b/skip.txt".to_string()));
}

#[test]
fn sd_include_multiple_dirs() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("tests")).unwrap();
    fs::create_dir_all(src.path().join("bench")).unwrap();
    fs::write(src.path().join("src/a.rs"), "").unwrap();
    fs::write(src.path().join("tests/b.rs"), "").unwrap();
    fs::write(src.path().join("bench/c.rs"), "").unwrap();
    let ws =
        WorkspaceManager::prepare(&spec_globs(src.path(), &["src/**", "tests/**"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"src/a.rs".to_string()));
    assert!(files.contains(&"tests/b.rs".to_string()));
    assert!(!files.contains(&"bench/c.rs".to_string()));
}

#[test]
fn sd_include_root_file_with_specific_name() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Cargo.toml"), "").unwrap();
    fs::write(src.path().join("Cargo.lock"), "").unwrap();
    fs::write(src.path().join("README.md"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["Cargo.*"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"Cargo.toml".to_string()));
    assert!(files.contains(&"Cargo.lock".to_string()));
    assert!(!files.contains(&"README.md".to_string()));
}

#[test]
fn sd_include_star_matches_across_separators() {
    // globset default: * crosses / when literal_separator is false
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b")).unwrap();
    fs::write(src.path().join("a/b/deep.rs"), "").unwrap();
    fs::write(src.path().join("top.rs"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"top.rs".to_string()));
    // globset * matches across / by default
    assert!(files.contains(&"a/b/deep.rs".to_string()));
}

// ===========================================================================
// 3. Glob exclude patterns (10 tests)
// ===========================================================================

#[test]
fn sd_exclude_single_extension() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["*.md"])).unwrap();
    for f in ls(ws.path()) {
        assert!(!f.ends_with(".md"), "excluded: {f}");
    }
}

#[test]
fn sd_exclude_directory() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["tests/**"])).unwrap();
    for f in ls(ws.path()) {
        assert!(!f.starts_with("tests/"), "excluded: {f}");
    }
}

#[test]
fn sd_exclude_multiple_patterns() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["*.md", "*.toml"])).unwrap();
    for f in ls(ws.path()) {
        assert!(
            !f.ends_with(".md") && !f.ends_with(".toml"),
            "excluded: {f}"
        );
    }
}

#[test]
fn sd_exclude_specific_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("secret.key"), "secret").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["secret.key"])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"keep.txt".to_string()));
    assert!(!files.contains(&"secret.key".to_string()));
}

#[test]
fn sd_exclude_brace_expansion() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.log"), "").unwrap();
    fs::write(src.path().join("b.tmp"), "").unwrap();
    fs::write(src.path().join("c.rs"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["*.{log,tmp}"])).unwrap();
    let files = ls(ws.path());
    assert!(!files.contains(&"a.log".to_string()));
    assert!(!files.contains(&"b.tmp".to_string()));
    assert!(files.contains(&"c.rs".to_string()));
}

#[test]
fn sd_exclude_nested_dir() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("target/debug")).unwrap();
    fs::write(src.path().join("target/debug/bin"), "").unwrap();
    fs::write(src.path().join("src.rs"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["target/**"])).unwrap();
    assert!(!ls(ws.path()).iter().any(|f| f.starts_with("target/")));
}

#[test]
fn sd_exclude_dotfiles() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".env"), "SECRET=1").unwrap();
    fs::write(src.path().join(".hidden"), "h").unwrap();
    fs::write(src.path().join("visible.txt"), "v").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &[".*"])).unwrap();
    let files = ls(ws.path());
    assert!(!files.contains(&".env".to_string()));
    assert!(!files.contains(&".hidden".to_string()));
    assert!(files.contains(&"visible.txt".to_string()));
}

#[test]
fn sd_exclude_preserves_other_files() {
    let src = tempdir().unwrap();
    for i in 0..10 {
        fs::write(
            src.path().join(format!("file{i}.txt")),
            format!("content{i}"),
        )
        .unwrap();
    }
    fs::write(src.path().join("remove_me.log"), "log").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["*.log"])).unwrap();
    let files = ls(ws.path());
    assert_eq!(files.len(), 10);
    assert!(!files.iter().any(|f| f.ends_with(".log")));
}

#[test]
fn sd_exclude_deeply_nested() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b/c/generated")).unwrap();
    fs::write(src.path().join("a/b/c/generated/out.rs"), "").unwrap();
    fs::write(src.path().join("a/b/c/keep.rs"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["**/generated/**"])).unwrap();
    let files = ls(ws.path());
    assert!(!files.iter().any(|f| f.contains("generated")));
    assert!(files.contains(&"a/b/c/keep.rs".to_string()));
}

#[test]
fn sd_exclude_with_char_class() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("test1.txt"), "").unwrap();
    fs::write(src.path().join("test2.txt"), "").unwrap();
    fs::write(src.path().join("testA.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["test[0-9].txt"])).unwrap();
    let files = ls(ws.path());
    assert!(!files.contains(&"test1.txt".to_string()));
    assert!(!files.contains(&"test2.txt".to_string()));
    assert!(files.contains(&"testA.txt".to_string()));
}

// ===========================================================================
// 4. Include + exclude interaction (8 tests)
// ===========================================================================

#[test]
fn sd_exclude_overrides_include() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws =
        WorkspaceManager::prepare(&spec_globs(src.path(), &["**/*.rs"], &["tests/**"])).unwrap();
    let files = ls(ws.path());
    assert!(files.iter().any(|f| f.ends_with(".rs")));
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
}

#[test]
fn sd_include_and_exclude_same_pattern_excludes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs"], &["*.rs"])).unwrap();
    assert!(ls(ws.path()).is_empty());
}

#[test]
fn sd_include_dir_exclude_subdir() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src/private")).unwrap();
    fs::write(src.path().join("src/lib.rs"), "pub").unwrap();
    fs::write(src.path().join("src/private/secret.rs"), "sec").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["src/**"], &["src/private/**"]))
        .unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.contains(&"src/private/secret.rs".to_string()));
}

#[test]
fn sd_include_ext_exclude_specific_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("main.rs"), "").unwrap();
    fs::write(src.path().join("generated.rs"), "").unwrap();
    fs::write(src.path().join("notes.txt"), "").unwrap();
    let ws =
        WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs"], &["generated.rs"])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"main.rs".to_string()));
    assert!(!files.contains(&"generated.rs".to_string()));
    assert!(!files.contains(&"notes.txt".to_string()));
}

#[test]
fn sd_include_two_dirs_exclude_one() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::create_dir_all(src.path().join("tests")).unwrap();
    fs::create_dir_all(src.path().join("bench")).unwrap();
    fs::write(src.path().join("src/a.rs"), "").unwrap();
    fs::write(src.path().join("tests/t.rs"), "").unwrap();
    fs::write(src.path().join("bench/b.rs"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(
        src.path(),
        &["src/**", "tests/**", "bench/**"],
        &["bench/**"],
    ))
    .unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"src/a.rs".to_string()));
    assert!(files.contains(&"tests/t.rs".to_string()));
    assert!(!files.contains(&"bench/b.rs".to_string()));
}

#[test]
fn sd_overlapping_include_exclude_scopes() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src/gen")).unwrap();
    fs::write(src.path().join("src/lib.rs"), "").unwrap();
    fs::write(src.path().join("src/gen/out.rs"), "").unwrap();
    let ws =
        WorkspaceManager::prepare(&spec_globs(src.path(), &["src/**"], &["**/gen/**"])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.iter().any(|f| f.contains("gen/")));
}

#[test]
fn sd_include_multiple_ext_exclude_one_ext() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.toml"), "").unwrap();
    fs::write(src.path().join("c.json"), "").unwrap();
    fs::write(src.path().join("d.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.{rs,toml,json}"], &["*.json"]))
        .unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
    assert!(files.contains(&"b.toml".to_string()));
    assert!(!files.contains(&"c.json".to_string()));
    assert!(!files.contains(&"d.txt".to_string()));
}

#[test]
fn sd_empty_include_with_exclude_means_exclude_from_all() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "").unwrap();
    fs::write(src.path().join("skip.log"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["*.log"])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"keep.rs".to_string()));
    assert!(!files.contains(&"skip.log".to_string()));
}

// ===========================================================================
// 5. .git directory exclusion (5 tests)
// ===========================================================================

#[test]
fn sd_dotgit_excluded_by_default() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git/objects")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("code.rs"), "fn main(){}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // The fresh .git from init should not exist either since we disabled it
    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn sd_dotgit_excluded_even_with_star_star_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git/refs")).unwrap();
    fs::write(src.path().join(".git/config"), "[core]").unwrap();
    fs::write(src.path().join("a.txt"), "").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("a.txt").exists());
}

#[test]
fn sd_dotgit_files_not_in_staged_listing() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git/marker"), "marker").unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = ls(ws.path());
    assert!(!files.iter().any(|f| f.contains(".git")));
    assert_eq!(files, vec!["real.txt"]);
}

#[test]
fn sd_source_dotgit_replaced_by_fresh_init() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git/stale_marker"), "old").unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    // Fresh .git should exist (from git init), but stale_marker should not
    assert!(ws.path().join(".git").exists());
    assert!(!ws.path().join(".git/stale_marker").exists());
}

#[test]
fn sd_dotgit_in_subdirectory_also_excluded() {
    let src = tempdir().unwrap();
    // Simulate a submodule-like .git in a subdirectory
    fs::create_dir_all(src.path().join("sub/.git")).unwrap();
    fs::write(src.path().join("sub/.git/HEAD"), "ref").unwrap();
    fs::write(src.path().join("sub/code.rs"), "fn sub(){}").unwrap();
    fs::write(src.path().join("root.txt"), "root").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join("sub/.git").exists());
    assert!(ws.path().join("sub/code.rs").exists());
    assert!(ws.path().join("root.txt").exists());
}

// ===========================================================================
// 6. Git initialization & baseline commit (8 tests)
// ===========================================================================

#[test]
fn sd_git_repo_initialized() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn sd_git_baseline_commit_message() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let msgs = git_cmd(ws.path(), &["log", "--format=%s"]).unwrap();
    assert!(msgs.contains("baseline"));
}

#[test]
fn sd_git_exactly_one_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "data").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let count = git_cmd(ws.path(), &["rev-list", "--count", "HEAD"]).unwrap();
    assert_eq!(count.trim(), "1");
}

#[test]
fn sd_git_clean_working_tree_after_staging() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.trim().is_empty(), "should be clean, got: {status}");
}

#[test]
fn sd_git_all_files_tracked() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let tracked = git_cmd(ws.path(), &["ls-files"]).unwrap();
    for f in ls(ws.path()) {
        assert!(tracked.contains(&f), "file {f} not tracked");
    }
}

#[test]
fn sd_git_author_is_abp() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "d").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let author = git_cmd(ws.path(), &["log", "--format=%an"]).unwrap();
    assert!(
        author.trim().contains("abp"),
        "author should be abp, got: {author}"
    );
}

#[test]
fn sd_git_new_file_shows_untracked() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    fs::write(ws.path().join("new_file"), "new").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("new_file"));
    assert!(status.contains("??"));
}

#[test]
fn sd_git_modified_file_shows_in_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "modified").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("data.txt"));
    assert!(diff.contains("modified"));
}

// ===========================================================================
// 7. Large / many files (5 tests)
// ===========================================================================

#[test]
fn sd_large_file_1mb() {
    let src = tempdir().unwrap();
    let data = vec![0xABu8; 1024 * 1024];
    fs::write(src.path().join("big.bin"), &data).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(fs::read(ws.path().join("big.bin")).unwrap(), data);
}

#[test]
fn sd_many_files_100() {
    let src = tempdir().unwrap();
    for i in 0..100 {
        fs::write(
            src.path().join(format!("f{i:03}.txt")),
            format!("content{i}"),
        )
        .unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(ls(ws.path()).len(), 100);
}

#[test]
fn sd_many_files_with_glob_filter() {
    let src = tempdir().unwrap();
    for i in 0..50 {
        fs::write(src.path().join(format!("keep{i}.rs")), "").unwrap();
        fs::write(src.path().join(format!("skip{i}.log")), "").unwrap();
    }
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    let files = ls(ws.path());
    assert_eq!(files.len(), 50);
    for f in &files {
        assert!(f.ends_with(".rs"));
    }
}

#[test]
fn sd_many_dirs_with_files() {
    let src = tempdir().unwrap();
    for i in 0..20 {
        let dir = src.path().join(format!("dir{i:02}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), format!("in dir{i}")).unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(ls(ws.path()).len(), 20);
}

#[test]
fn sd_many_files_content_integrity() {
    let src = tempdir().unwrap();
    for i in 0..30 {
        fs::write(src.path().join(format!("f{i}.txt")), format!("data-{i}")).unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    for i in 0..30 {
        assert_eq!(
            fs::read_to_string(ws.path().join(format!("f{i}.txt"))).unwrap(),
            format!("data-{i}")
        );
    }
}

// ===========================================================================
// 8. Nested directory handling (8 tests)
// ===========================================================================

#[test]
fn sd_nested_three_levels() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b/c")).unwrap();
    fs::write(src.path().join("a/b/c/leaf.txt"), "leaf").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("a/b/c/leaf.txt")).unwrap(),
        "leaf"
    );
}

#[test]
fn sd_nested_20_levels() {
    let src = tempdir().unwrap();
    let mut path = src.path().to_path_buf();
    for i in 0..20 {
        path = path.join(format!("l{i}"));
    }
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("bottom.txt"), "bottom").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();

    let mut staged = ws.path().to_path_buf();
    for i in 0..20 {
        staged = staged.join(format!("l{i}"));
    }
    assert_eq!(
        fs::read_to_string(staged.join("bottom.txt")).unwrap(),
        "bottom"
    );
}

#[test]
fn sd_nested_files_at_every_level() {
    let src = tempdir().unwrap();
    let mut path = src.path().to_path_buf();
    for i in 0..5 {
        path = path.join(format!("d{i}"));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(format!("f{i}.txt")), format!("level{i}")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let files = ls(ws.path());
    assert_eq!(files.len(), 5);
}

#[test]
fn sd_nested_sibling_dirs() {
    let src = tempdir().unwrap();
    for name in &["alpha", "beta", "gamma"] {
        fs::create_dir_all(src.path().join(name)).unwrap();
        fs::write(src.path().join(name).join("data.txt"), name).unwrap();
    }
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    for name in &["alpha", "beta", "gamma"] {
        assert_eq!(
            fs::read_to_string(ws.path().join(name).join("data.txt")).unwrap(),
            *name
        );
    }
}

#[test]
fn sd_nested_empty_dir_created_when_has_allowed_peer() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("parent/empty_child")).unwrap();
    fs::create_dir_all(src.path().join("parent/has_file")).unwrap();
    fs::write(src.path().join("parent/has_file/f.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    // The empty_child dir should exist since no glob filters were applied
    assert!(ws.path().join("parent/empty_child").is_dir());
    assert!(ws.path().join("parent/has_file/f.txt").exists());
}

#[test]
fn sd_nested_dirs_preserved_in_listing() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("x/y")).unwrap();
    fs::create_dir_all(src.path().join("x/z")).unwrap();
    fs::write(src.path().join("x/y/a.txt"), "").unwrap();
    fs::write(src.path().join("x/z/b.txt"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let dirs = ls_dirs(ws.path());
    assert!(dirs.contains(&"x".to_string()));
    assert!(dirs.contains(&"x/y".to_string()));
    assert!(dirs.contains(&"x/z".to_string()));
}

#[test]
fn sd_nested_with_include_preserves_structure() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b")).unwrap();
    fs::write(src.path().join("a/b/keep.rs"), "").unwrap();
    fs::write(src.path().join("a/b/skip.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["**/*.rs"], &[])).unwrap();
    assert!(ws.path().join("a/b/keep.rs").exists());
    assert!(!ws.path().join("a/b/skip.txt").exists());
}

#[test]
fn sd_nested_mixed_depth_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "r").unwrap();
    fs::create_dir_all(src.path().join("a")).unwrap();
    fs::write(src.path().join("a/mid.txt"), "m").unwrap();
    fs::create_dir_all(src.path().join("a/b/c/d")).unwrap();
    fs::write(src.path().join("a/b/c/d/deep.txt"), "d").unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let files = ls(ws.path());
    assert_eq!(files.len(), 3);
    assert!(files.contains(&"root.txt".to_string()));
    assert!(files.contains(&"a/mid.txt".to_string()));
    assert!(files.contains(&"a/b/c/d/deep.txt".to_string()));
}

// ===========================================================================
// 9. Symlink handling (3 tests)
// ===========================================================================

#[cfg(unix)]
#[test]
fn sd_symlink_not_followed() {
    let src = tempdir().unwrap();
    let target = tempdir().unwrap();
    fs::write(target.path().join("external.txt"), "external").unwrap();
    std::os::unix::fs::symlink(target.path(), src.path().join("link")).unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // follow_links is false in walkdir, so symlink targets should not appear
    assert!(ws.path().join("real.txt").exists());
}

#[cfg(unix)]
#[test]
fn sd_symlink_file_not_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("actual.txt"), "content").unwrap();
    std::os::unix::fs::symlink(
        src.path().join("actual.txt"),
        src.path().join("sym_link.txt"),
    )
    .unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("actual.txt").exists());
    // Symlink itself is not a regular file, so it's skipped
    assert!(!ws.path().join("sym_link.txt").exists());
}

#[test]
fn sd_follow_links_false_documented() {
    // This test verifies the copy_workspace behavior: follow_links(false)
    // means symlinks are not dereferenced. We test indirectly by ensuring
    // only regular files and dirs are staged.
    let src = tempdir().unwrap();
    fs::write(src.path().join("regular.txt"), "ok").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = ls(ws.path());
    assert_eq!(files, vec!["regular.txt"]);
}

// ===========================================================================
// 10. Empty workspace staging (4 tests)
// ===========================================================================

#[test]
fn sd_empty_source_stages_nothing() {
    let src = tempdir().unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ls(ws.path()).is_empty());
}

#[test]
fn sd_empty_source_with_git_init() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert!(ls(ws.path()).is_empty());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn sd_only_empty_dirs_source() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_a")).unwrap();
    fs::create_dir_all(src.path().join("empty_b/nested")).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ls(ws.path()).is_empty());
    assert!(ws.path().join("empty_a").is_dir());
}

#[test]
fn sd_empty_source_with_include_glob() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    assert!(ls(ws.path()).is_empty());
}

// ===========================================================================
// 11. Glob edge cases (10 tests)
// ===========================================================================

#[test]
fn sd_glob_no_match_yields_empty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    assert!(ls(ws.path()).is_empty());
}

#[test]
fn sd_glob_exclude_everything() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "").unwrap();
    fs::write(src.path().join("b.rs"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["*"])).unwrap();
    assert!(ls(ws.path()).is_empty());
}

#[test]
fn sd_glob_negation_char_class() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a1.txt"), "").unwrap();
    fs::write(src.path().join("aB.txt"), "").unwrap();
    fs::write(src.path().join("aC.txt"), "").unwrap();
    // [!1] matches anything that is NOT '1'
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["a[!1].txt"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(!files.contains(&"a1.txt".to_string()));
    assert!(files.contains(&"aB.txt".to_string()));
    assert!(files.contains(&"aC.txt".to_string()));
}

#[test]
fn sd_glob_double_star_slash_catches_all_depths() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b/c")).unwrap();
    fs::write(src.path().join("top.rs"), "").unwrap();
    fs::write(src.path().join("a/mid.rs"), "").unwrap();
    fs::write(src.path().join("a/b/c/deep.rs"), "").unwrap();
    fs::write(src.path().join("a/b/c/skip.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["**/*.rs"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"top.rs".to_string()));
    assert!(files.contains(&"a/mid.rs".to_string()));
    assert!(files.contains(&"a/b/c/deep.rs".to_string()));
    assert!(!files.contains(&"a/b/c/skip.txt".to_string()));
}

#[test]
fn sd_glob_multiple_overlapping_includes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.txt"), "").unwrap();
    // Both patterns match a.rs — should still work fine
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs", "a.*"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
}

#[test]
fn sd_glob_include_only_files_without_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Makefile"), "all:").unwrap();
    fs::write(src.path().join("Dockerfile"), "FROM").unwrap();
    fs::write(src.path().join("config.yaml"), "key: val").unwrap();
    // Files without extension match patterns like "Makefile" literally
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["Makefile", "Dockerfile"], &[]))
        .unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"Makefile".to_string()));
    assert!(files.contains(&"Dockerfile".to_string()));
    assert!(!files.contains(&"config.yaml".to_string()));
}

#[test]
fn sd_glob_exclude_everything_then_no_files() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["**/*"])).unwrap();
    assert!(ls(ws.path()).is_empty());
}

#[test]
fn sd_glob_question_mark_in_dir_name() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("d1/sub")).unwrap();
    fs::create_dir_all(src.path().join("dx/sub")).unwrap();
    fs::write(src.path().join("d1/sub/f.txt"), "").unwrap();
    fs::write(src.path().join("dx/sub/f.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["d?/sub/**"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"d1/sub/f.txt".to_string()));
    assert!(files.contains(&"dx/sub/f.txt".to_string()));
}

#[test]
fn sd_glob_extension_match_case() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("lower.rs"), "lower").unwrap();
    fs::write(src.path().join("upper.TXT"), "upper").unwrap();
    let ws = WorkspaceManager::prepare(&spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"lower.rs".to_string()));
    // *.rs should not match .TXT regardless of platform
    assert!(!files.contains(&"upper.TXT".to_string()));
}

#[test]
fn sd_glob_invalid_pattern_errors() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "").unwrap();
    let result = WorkspaceManager::prepare(&spec_globs(src.path(), &["["], &[]));
    assert!(result.is_err());
}

// ===========================================================================
// 12. Cleanup / drop behavior (4 tests)
// ===========================================================================

#[test]
fn sd_drop_cleans_up_temp_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "d").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let staged_path = ws.path().to_path_buf();
    assert!(staged_path.exists());
    drop(ws);
    assert!(
        !staged_path.exists(),
        "temp dir should be cleaned up on drop"
    );
}

#[test]
fn sd_drop_multiple_workspaces_independent() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "d").unwrap();
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
    let p1 = ws1.path().to_path_buf();
    let p2 = ws2.path().to_path_buf();
    drop(ws1);
    assert!(!p1.exists());
    assert!(p2.exists(), "ws2 should survive ws1 drop");
    drop(ws2);
    assert!(!p2.exists());
}

#[test]
fn sd_passthrough_does_not_clean_up() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "d").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    let p = ws.path().to_path_buf();
    drop(ws);
    assert!(p.exists(), "passthrough should not delete original dir");
}

#[test]
fn sd_staged_workspace_usable_before_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "hello").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Write and read in staged workspace
    fs::write(ws.path().join("new.txt"), "new").unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("f.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("new.txt")).unwrap(),
        "new"
    );
}

// ===========================================================================
// 13. Concurrent staging (4 tests)
// ===========================================================================

#[test]
fn sd_concurrent_three_stages_different_paths() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws1 = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let ws3 = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert_ne!(ws1.path(), ws2.path());
    assert_ne!(ws2.path(), ws3.path());
    assert_ne!(ws1.path(), ws3.path());
}

#[test]
fn sd_concurrent_stages_identical_content() {
    let src = tempdir().unwrap();
    seed(src.path());
    let ws1 = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    assert_eq!(ls(ws1.path()), ls(ws2.path()));
}

#[test]
fn sd_concurrent_threaded_staging() {
    let src = tempdir().unwrap();
    seed(src.path());
    let src_path = src.path().to_path_buf();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let p = src_path.clone();
            std::thread::spawn(move || {
                let s = WorkspaceSpec {
                    root: p.to_string_lossy().to_string(),
                    mode: WorkspaceMode::Staged,
                    include: vec![],
                    exclude: vec![],
                };
                WorkspaceManager::prepare(&s).unwrap()
            })
        })
        .collect();

    let workspaces: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All paths unique
    for i in 0..workspaces.len() {
        for j in (i + 1)..workspaces.len() {
            assert_ne!(workspaces[i].path(), workspaces[j].path());
        }
    }
    // All have same files
    let baseline = ls(workspaces[0].path());
    for ws in &workspaces[1..] {
        assert_eq!(ls(ws.path()), baseline);
    }
}

#[test]
fn sd_concurrent_mutations_isolated() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "original").unwrap();
    let ws1 = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&spec(src.path())).unwrap();
    fs::write(ws1.path().join("shared.txt"), "changed_in_ws1").unwrap();
    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "original",
        "ws2 should not see ws1 mutation"
    );
}

// ===========================================================================
// 14. WorkspaceStager builder (7 tests)
// ===========================================================================

#[test]
fn sd_stager_default_git_init_on() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn sd_stager_git_init_disabled() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn sd_stager_include_and_exclude() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.log"), "").unwrap();
    fs::create_dir_all(src.path().join("gen")).unwrap();
    fs::write(src.path().join("gen/out.rs"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**/*.rs".into()])
        .exclude(vec!["gen/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let files = ls(ws.path());
    assert!(files.contains(&"a.rs".to_string()));
    assert!(!files.contains(&"b.log".to_string()));
    assert!(!files.iter().any(|f| f.starts_with("gen/")));
}

#[test]
fn sd_stager_no_source_errors() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
}

#[test]
fn sd_stager_nonexistent_source_errors() {
    let result = WorkspaceStager::new()
        .source_root("/no/such/path/abc_xyz_123")
        .stage();
    assert!(result.is_err());
}

#[test]
fn sd_stager_chain_order_irrelevant() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "fn a(){}").unwrap();
    fs::write(src.path().join("b.log"), "log").unwrap();

    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let ws2 = WorkspaceStager::new()
        .with_git_init(false)
        .include(vec!["*.rs".into()])
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_eq!(ls(ws1.path()), ls(ws2.path()));
}

#[test]
fn sd_stager_default_equals_new() {
    let s1 = WorkspaceStager::new();
    let s2 = WorkspaceStager::default();
    // Both should produce equivalent staging with the same source
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let ws1 = s1
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let ws2 = s2
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(ls(ws1.path()), ls(ws2.path()));
}

// ===========================================================================
// 15. PassThrough mode (3 tests)
// ===========================================================================

#[test]
fn sd_passthrough_returns_same_path() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn sd_passthrough_no_git_init() {
    let src = tempdir().unwrap();
    // Ensure no .git before
    assert!(!src.path().join(".git").exists());
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    // PassThrough should not modify source
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn sd_passthrough_sees_source_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "v1").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    fs::write(src.path().join("f.txt"), "v2").unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("f.txt")).unwrap(), "v2");
}

// ===========================================================================
// 16. Special filenames and content (5 tests)
// ===========================================================================

#[test]
fn sd_files_with_spaces() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("my file.txt"), "spaced").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("my file.txt")).unwrap(),
        "spaced"
    );
}

#[test]
fn sd_dotfiles_copied_except_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".env"), "KEY=1").unwrap();
    fs::write(src.path().join(".gitignore"), "*.log").unwrap();
    fs::write(src.path().join(".editorconfig"), "root=true").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(".env").exists());
    assert!(ws.path().join(".gitignore").exists());
    assert!(ws.path().join(".editorconfig").exists());
}

#[test]
fn sd_unicode_filename() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("données.txt"), "unicode content").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("données.txt")).unwrap(),
        "unicode content"
    );
}

#[test]
fn sd_unicode_content_preserved() {
    let src = tempdir().unwrap();
    let content = "こんにちは世界 🌍 café résumé";
    fs::write(src.path().join("uni.txt"), content).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("uni.txt")).unwrap(),
        content
    );
}

#[test]
fn sd_files_without_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Makefile"), "all:").unwrap();
    fs::write(src.path().join("LICENSE"), "MIT").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("Makefile").exists());
    assert!(ws.path().join("LICENSE").exists());
}

// ===========================================================================
// 17. Error handling (3 tests)
// ===========================================================================

#[test]
fn sd_error_invalid_include_glob() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a"), "").unwrap();
    let result = WorkspaceManager::prepare(&spec_globs(src.path(), &["[invalid"], &[]));
    assert!(result.is_err());
}

#[test]
fn sd_error_invalid_exclude_glob() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a"), "").unwrap();
    let result = WorkspaceManager::prepare(&spec_globs(src.path(), &[], &["[invalid"]));
    assert!(result.is_err());
}

#[test]
fn sd_error_both_globs_invalid() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a"), "").unwrap();
    let result = WorkspaceManager::prepare(&spec_globs(src.path(), &["[bad"], &["[worse"]));
    assert!(result.is_err());
}
