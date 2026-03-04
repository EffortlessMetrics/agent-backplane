#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep workspace staging tests — comprehensive coverage of copy correctness,
//! glob filtering, git initialization, diff detection, cleanup on drop,
//! `WorkspaceSpec` construction, permission preservation, concurrency,
//! binary content, and error handling.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ===========================================================================
// Helpers
// ===========================================================================

fn staged_spec(root: &Path) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

fn staged_spec_globs(root: &Path, inc: &[&str], exc: &[&str]) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: inc.iter().map(|s| s.to_string()).collect(),
        exclude: exc.iter().map(|s| s.to_string()).collect(),
    }
}

/// Sorted relative file paths (no dirs), excluding `.git`.
fn files(root: &Path) -> Vec<String> {
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

/// Run a git command and return stdout.
fn git(path: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

// ===========================================================================
// 1. Basic Staging — file copy correctness
// ===========================================================================

#[test]
fn deep_basic_single_file_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("only.txt"), "sole content").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("only.txt")).unwrap(),
        "sole content"
    );
}

#[test]
fn deep_basic_multiple_files_all_present() {
    let src = tempdir().unwrap();
    for i in 0..20 {
        fs::write(src.path().join(format!("f{i}.txt")), format!("data{i}")).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    for i in 0..20 {
        assert!(ws.path().join(format!("f{i}.txt")).exists());
        assert_eq!(
            fs::read_to_string(ws.path().join(format!("f{i}.txt"))).unwrap(),
            format!("data{i}")
        );
    }
}

#[test]
fn deep_basic_file_size_preserved() {
    let src = tempdir().unwrap();
    let content = "x".repeat(4096);
    fs::write(src.path().join("sized.bin"), &content).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let meta = fs::metadata(ws.path().join("sized.bin")).unwrap();
    assert_eq!(meta.len(), 4096);
}

#[test]
fn deep_basic_empty_file_preserved() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.txt"), "").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("empty.txt").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty.txt")).unwrap(), "");
}

#[test]
fn deep_basic_file_with_newlines_preserved() {
    let src = tempdir().unwrap();
    let content = "line1\nline2\r\nline3\n";
    fs::write(src.path().join("lines.txt"), content).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("lines.txt")).unwrap(),
        content
    );
}

// ===========================================================================
// 2. Glob filtering — include patterns
// ===========================================================================

#[test]
fn deep_include_single_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "rs").unwrap();
    fs::write(src.path().join("b.py"), "py").unwrap();
    fs::write(src.path().join("c.rs"), "rs2").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    let f = files(ws.path());
    assert_eq!(f, vec!["a.rs", "c.rs"]);
}

#[test]
fn deep_include_two_extensions() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.toml"), "").unwrap();
    fs::write(src.path().join("c.md"), "").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs", "*.toml"], &[]))
        .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"a.rs".to_string()));
    assert!(f.contains(&"b.toml".to_string()));
    assert!(!f.contains(&"c.md".to_string()));
}

#[test]
fn deep_include_subdirectory_only() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "").unwrap();
    fs::write(src.path().join("root.rs"), "").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["src/**"], &[])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"src/lib.rs".to_string()));
    assert!(!f.contains(&"root.rs".to_string()));
}

#[test]
fn deep_include_question_mark_wildcard() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a1.txt"), "").unwrap();
    fs::write(src.path().join("a2.txt"), "").unwrap();
    fs::write(src.path().join("ab.txt"), "").unwrap();
    fs::write(src.path().join("abc.txt"), "").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["a?.txt"], &[])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"a1.txt".to_string()));
    assert!(f.contains(&"a2.txt".to_string()));
    assert!(f.contains(&"ab.txt".to_string()));
    // abc.txt has 3 chars before .txt, so a?.txt shouldn't match via globset
    // (globset * matches across path separators but ? matches single char)
    assert!(!f.contains(&"abc.txt".to_string()));
}

#[test]
fn deep_include_brace_expansion() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.py"), "").unwrap();
    fs::write(src.path().join("c.go"), "").unwrap();
    fs::write(src.path().join("d.txt"), "").unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.{rs,py,go}"], &[])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"a.rs".to_string()));
    assert!(f.contains(&"b.py".to_string()));
    assert!(f.contains(&"c.go".to_string()));
    assert!(!f.contains(&"d.txt".to_string()));
}

// ===========================================================================
// 3. Glob filtering — exclude patterns
// ===========================================================================

#[test]
fn deep_exclude_single_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "").unwrap();
    fs::write(src.path().join("debug.log"), "").unwrap();
    fs::write(src.path().join("info.log"), "").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["*.log"])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"code.rs".to_string()));
    assert!(!f.contains(&"debug.log".to_string()));
    assert!(!f.contains(&"info.log".to_string()));
}

#[test]
fn deep_exclude_multiple_directories() {
    let src = tempdir().unwrap();
    for d in &["node_modules", "target", "dist", "src"] {
        fs::create_dir_all(src.path().join(d)).unwrap();
        fs::write(src.path().join(d).join("file.txt"), d).unwrap();
    }
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        &[],
        &["node_modules/**", "target/**", "dist/**"],
    ))
    .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"src/file.txt".to_string()));
    assert!(!f.iter().any(|p| p.starts_with("node_modules/")));
    assert!(!f.iter().any(|p| p.starts_with("target/")));
    assert!(!f.iter().any(|p| p.starts_with("dist/")));
}

#[test]
fn deep_exclude_overrides_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src").join("gen")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "").unwrap();
    fs::write(src.path().join("src").join("gen").join("out.rs"), "").unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["src/**"], &["src/gen/**"]))
            .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"src/lib.rs".to_string()));
    assert!(!f.iter().any(|p| p.starts_with("src/gen/")));
}

#[test]
fn deep_exclude_brace_expansion() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.tmp"), "").unwrap();
    fs::write(src.path().join("b.bak"), "").unwrap();
    fs::write(src.path().join("c.rs"), "").unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["*.{tmp,bak}"])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"c.rs".to_string()));
    assert!(!f.contains(&"a.tmp".to_string()));
    assert!(!f.contains(&"b.bak".to_string()));
}

// ===========================================================================
// 4. .git exclusion — always excluded
// ===========================================================================

#[test]
fn deep_dotgit_not_copied_even_with_star_include() {
    let src = tempdir().unwrap();
    let gitdir = src.path().join(".git");
    fs::create_dir_all(gitdir.join("objects")).unwrap();
    fs::write(gitdir.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("file.rs"), "").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("file.rs").exists());
}

#[test]
fn deep_nested_dotgit_excluded() {
    let src = tempdir().unwrap();
    // Simulate a submodule-like nested .git
    let nested_git = src.path().join("vendor").join("dep").join(".git");
    fs::create_dir_all(&nested_git).unwrap();
    fs::write(nested_git.join("HEAD"), "abc123").unwrap();
    fs::write(
        src.path().join("vendor").join("dep").join("lib.rs"),
        "pub fn dep() {}",
    )
    .unwrap();
    fs::write(src.path().join("main.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("main.rs").exists());
    assert!(
        ws.path().join("vendor").join("dep").join("lib.rs").exists(),
        "non-.git files in nested dirs should be copied"
    );
    assert!(
        !ws.path().join("vendor").join("dep").join(".git").exists(),
        "nested .git must be excluded"
    );
}

#[test]
fn deep_dotgit_excluded_source_gets_fresh_git() {
    let src = tempdir().unwrap();
    let gitdir = src.path().join(".git");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(gitdir.join("marker"), "original_git").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Fresh .git exists (from ensure_git_repo) but original marker must not
    assert!(ws.path().join(".git").exists());
    assert!(!ws.path().join(".git").join("marker").exists());
}

// ===========================================================================
// 5. Git initialization
// ===========================================================================

#[test]
fn deep_git_single_commit_exists() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.as_deref().map(str::trim), Some("1"));
}

#[test]
fn deep_git_baseline_message() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let msg = git(ws.path(), &["log", "--format=%s"]).unwrap();
    assert!(msg.trim().contains("baseline"));
}

#[test]
fn deep_git_all_files_tracked_after_init() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("sub").join("b.txt"), "b").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(
        status.trim().is_empty(),
        "all files should be tracked (clean status), got: {status}"
    );
}

#[test]
fn deep_git_author_is_abp() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let author = git(ws.path(), &["log", "--format=%an"]).unwrap();
    assert_eq!(author.trim(), "abp");
}

#[test]
fn deep_stager_git_init_false_no_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(!ws.path().join(".git").exists());
}

// ===========================================================================
// 6. Diff detection — modifications produce meaningful diffs
// ===========================================================================

#[test]
fn deep_diff_modified_tracked_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original line").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "changed line").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("-original line"));
    assert!(diff.contains("+changed line"));
}

#[test]
fn deep_diff_added_file_shows_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("orig.txt"), "orig").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("new.txt"), "brand new").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("?? new.txt"));
}

#[test]
fn deep_diff_deleted_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("victim.txt"), "bye").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("victim.txt")).unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains(" D victim.txt"));
}

#[test]
fn deep_diff_multiple_changes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "aaa").unwrap();
    fs::write(src.path().join("b.txt"), "bbb").unwrap();
    fs::write(src.path().join("c.txt"), "ccc").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("a.txt"), "modified_a").unwrap();
    fs::remove_file(ws.path().join("b.txt")).unwrap();
    fs::write(ws.path().join("d.txt"), "new_d").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("a.txt"), "modified file in status");
    assert!(status.contains("b.txt"), "deleted file in status");
    assert!(status.contains("d.txt"), "new file in status");
}

#[test]
fn deep_diff_clean_workspace_empty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "content").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.trim().is_empty());
}

// ===========================================================================
// 7. Nested directory structures
// ===========================================================================

#[test]
fn deep_nested_three_levels() {
    let src = tempdir().unwrap();
    let p = src.path().join("a").join("b").join("c");
    fs::create_dir_all(&p).unwrap();
    fs::write(p.join("leaf.txt"), "leaf").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("a").join("b").join("c").join("leaf.txt")).unwrap(),
        "leaf"
    );
}

#[test]
fn deep_nested_sibling_dirs() {
    let src = tempdir().unwrap();
    for d in &["alpha", "beta", "gamma"] {
        fs::create_dir_all(src.path().join(d)).unwrap();
        fs::write(src.path().join(d).join("file.txt"), *d).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    for d in &["alpha", "beta", "gamma"] {
        assert_eq!(
            fs::read_to_string(ws.path().join(d).join("file.txt")).unwrap(),
            *d
        );
    }
}

#[test]
fn deep_nested_files_at_every_level() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("root.txt"), "0").unwrap();
    fs::create_dir_all(src.path().join("l1")).unwrap();
    fs::write(src.path().join("l1").join("l1.txt"), "1").unwrap();
    fs::create_dir_all(src.path().join("l1").join("l2")).unwrap();
    fs::write(src.path().join("l1").join("l2").join("l2.txt"), "2").unwrap();
    fs::create_dir_all(src.path().join("l1").join("l2").join("l3")).unwrap();
    fs::write(
        src.path().join("l1").join("l2").join("l3").join("l3.txt"),
        "3",
    )
    .unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let f = files(ws.path());
    assert!(f.contains(&"root.txt".to_string()));
    assert!(f.contains(&"l1/l1.txt".to_string()));
    assert!(f.contains(&"l1/l2/l2.txt".to_string()));
    assert!(f.contains(&"l1/l2/l3/l3.txt".to_string()));
}

#[test]
fn deep_nested_20_levels() {
    let src = tempdir().unwrap();
    let mut p = src.path().to_path_buf();
    for i in 0..20 {
        p = p.join(format!("d{i}"));
    }
    fs::create_dir_all(&p).unwrap();
    fs::write(p.join("bottom.txt"), "deep").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let mut expected = ws.path().to_path_buf();
    for i in 0..20 {
        expected = expected.join(format!("d{i}"));
    }
    assert!(expected.join("bottom.txt").exists());
    assert_eq!(
        fs::read_to_string(expected.join("bottom.txt")).unwrap(),
        "deep"
    );
}

// ===========================================================================
// 8. Symlink handling (follow_links is false)
// ===========================================================================

#[cfg(unix)]
#[test]
fn deep_symlink_not_followed() {
    use std::os::unix::fs as unix_fs;

    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real content").unwrap();
    unix_fs::symlink(src.path().join("real.txt"), src.path().join("link.txt")).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // With follow_links(false) the symlink is not a regular file, so it's skipped
    assert!(ws.path().join("real.txt").exists());
    assert!(
        !ws.path().join("link.txt").exists(),
        "symlinks should not be followed/copied"
    );
}

// ===========================================================================
// 9. Empty directories
// ===========================================================================

#[test]
fn deep_empty_subdir_created() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_sub")).unwrap();
    fs::write(src.path().join("file.txt"), "x").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // walkdir visits dirs so copy_workspace creates them
    assert!(ws.path().join("empty_sub").is_dir());
    assert!(ws.path().join("file.txt").exists());
}

#[test]
fn deep_nested_empty_dirs() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("c")).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("a").join("b").join("c").is_dir());
}

#[test]
fn deep_empty_source_produces_empty_staging() {
    let src = tempdir().unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let f = files(ws.path());
    assert!(f.is_empty());
}

// ===========================================================================
// 10. Binary files preserved correctly
// ===========================================================================

#[test]
fn deep_binary_null_bytes() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = vec![0x00; 512];
    fs::write(src.path().join("nulls.bin"), &data).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(fs::read(ws.path().join("nulls.bin")).unwrap(), data);
}

#[test]
fn deep_binary_all_byte_values() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("allbytes.bin"), &data).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(fs::read(ws.path().join("allbytes.bin")).unwrap(), data);
}

#[test]
fn deep_binary_png_header() {
    let src = tempdir().unwrap();
    let png: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    fs::write(src.path().join("image.png"), &png).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(fs::read(ws.path().join("image.png")).unwrap(), png);
}

#[test]
fn deep_binary_mixed_text_and_binary() {
    let src = tempdir().unwrap();
    let mut data = b"text header\n".to_vec();
    data.extend(vec![0x00, 0xFF, 0xFE, 0xFD]);
    data.extend(b"\ntext footer\n");
    fs::write(src.path().join("mixed.dat"), &data).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(fs::read(ws.path().join("mixed.dat")).unwrap(), data);
}

// ===========================================================================
// 11. Large files
// ===========================================================================

#[test]
fn deep_large_1mb_file() {
    let src = tempdir().unwrap();
    let data = vec![0xABu8; 1024 * 1024];
    fs::write(src.path().join("big.bin"), &data).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = fs::read(ws.path().join("big.bin")).unwrap();
    assert_eq!(staged.len(), 1024 * 1024);
    assert_eq!(staged, data);
}

#[test]
fn deep_large_many_small_files() {
    let src = tempdir().unwrap();
    let count = 200;
    for i in 0..count {
        fs::write(src.path().join(format!("f{i:04}.txt")), format!("{i}")).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let f = files(ws.path());
    assert_eq!(f.len(), count);
}

// ===========================================================================
// 12. Unicode filenames
// ===========================================================================

#[test]
fn deep_unicode_cjk_filename() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("文件.txt"), "Chinese").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("文件.txt")).unwrap(),
        "Chinese"
    );
}

#[test]
fn deep_unicode_accented_chars() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("café.txt"), "latte").unwrap();
    fs::write(src.path().join("naïve.rs"), "fn naïve() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("café.txt")).unwrap(),
        "latte"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("naïve.rs")).unwrap(),
        "fn naïve() {}"
    );
}

#[test]
fn deep_unicode_emoji_filename() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("🦀.rs"), "fn crab() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("🦀.rs")).unwrap(),
        "fn crab() {}"
    );
}

#[test]
fn deep_unicode_content_preserved() {
    let src = tempdir().unwrap();
    let content = "こんにちは世界 🌍\nЗдравствуй мир\n";
    fs::write(src.path().join("intl.txt"), content).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("intl.txt")).unwrap(),
        content
    );
}

// ===========================================================================
// 13. Deep nesting
// ===========================================================================

#[test]
fn deep_nesting_15_levels_with_files_at_each() {
    let src = tempdir().unwrap();
    let mut cur = src.path().to_path_buf();
    for i in 0..15 {
        cur = cur.join(format!("l{i}"));
        fs::create_dir_all(&cur).unwrap();
        fs::write(cur.join("file.txt"), format!("level {i}")).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let f = files(ws.path());
    assert_eq!(f.len(), 15);
    // Verify deepest
    let mut check = ws.path().to_path_buf();
    for i in 0..15 {
        check = check.join(format!("l{i}"));
    }
    assert_eq!(
        fs::read_to_string(check.join("file.txt")).unwrap(),
        "level 14"
    );
}

// ===========================================================================
// 14. Glob edge cases
// ===========================================================================

#[test]
fn deep_glob_star_matches_across_separators() {
    // globset default: literal_separator is false
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::write(src.path().join("top.rs"), "").unwrap();
    fs::write(src.path().join("a").join("mid.rs"), "").unwrap();
    fs::write(src.path().join("a").join("b").join("deep.rs"), "").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    let f = files(ws.path());
    // globset * crosses path separators by default
    assert!(f.contains(&"top.rs".to_string()));
    assert!(f.contains(&"a/mid.rs".to_string()));
    assert!(f.contains(&"a/b/deep.rs".to_string()));
}

#[test]
fn deep_glob_overlapping_include_exclude() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src")).unwrap();
    fs::write(src.path().join("src").join("main.rs"), "").unwrap();
    fs::write(src.path().join("src").join("test.rs"), "").unwrap();

    // Include and exclude both match src/**
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["src/**"], &["src/**"]))
        .unwrap();
    let f = files(ws.path());
    // Exclude takes precedence
    assert!(f.is_empty(), "exclude should win over include: {f:?}");
}

#[test]
fn deep_glob_character_class() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a1.txt"), "").unwrap();
    fs::write(src.path().join("b2.txt"), "").unwrap();
    fs::write(src.path().join("c3.txt"), "").unwrap();
    fs::write(src.path().join("x9.txt"), "").unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["[abc]*.txt"], &[])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"a1.txt".to_string()));
    assert!(f.contains(&"b2.txt".to_string()));
    assert!(f.contains(&"c3.txt".to_string()));
    assert!(!f.contains(&"x9.txt".to_string()));
}

#[test]
fn deep_glob_no_patterns_allows_everything() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.txt"), "").unwrap();
    fs::write(src.path().join("c.log"), "").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &[])).unwrap();
    let f = files(ws.path());
    assert_eq!(f.len(), 3);
}

#[test]
fn deep_glob_exclude_specific_filename() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Cargo.lock"), "").unwrap();
    fs::write(src.path().join("Cargo.toml"), "").unwrap();
    fs::write(src.path().join("main.rs"), "").unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["Cargo.lock"])).unwrap();
    let f = files(ws.path());
    assert!(!f.contains(&"Cargo.lock".to_string()));
    assert!(f.contains(&"Cargo.toml".to_string()));
    assert!(f.contains(&"main.rs".to_string()));
}

#[test]
fn deep_glob_include_only_top_level_via_pattern() {
    // Test that a pattern like "*.rs" matches nested files too (globset behavior)
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("top.rs"), "").unwrap();
    fs::write(src.path().join("top.txt"), "").unwrap();
    fs::write(src.path().join("sub").join("nested.rs"), "").unwrap();
    fs::write(src.path().join("sub").join("nested.txt"), "").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    let f = files(ws.path());
    // All .rs files should match (globset * crosses path separators)
    for file in &f {
        assert!(file.ends_with(".rs"), "unexpected file: {file}");
    }
}

#[test]
fn deep_glob_double_star_slash_pattern() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b")).unwrap();
    fs::write(src.path().join("root.json"), "").unwrap();
    fs::write(src.path().join("a").join("mid.json"), "").unwrap();
    fs::write(src.path().join("a").join("b").join("deep.json"), "").unwrap();
    fs::write(src.path().join("a").join("skip.txt"), "").unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["**/*.json"], &[])).unwrap();
    let f = files(ws.path());
    assert_eq!(f.len(), 3);
    for file in &f {
        assert!(file.ends_with(".json"));
    }
}

// ===========================================================================
// 15. Error handling
// ===========================================================================

#[test]
fn deep_error_nonexistent_source() {
    let result = WorkspaceStager::new()
        .source_root("/this/path/does/not/exist/at/all")
        .stage();
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("does not exist"), "error: {msg}");
}

#[test]
fn deep_error_missing_source_root() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("source_root"), "error: {msg}");
}

#[test]
fn deep_error_invalid_glob_pattern() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let result = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["[".into()])
        .stage();
    assert!(result.is_err());
}

#[test]
fn deep_error_invalid_exclude_glob() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let result = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["[invalid".into()])
        .stage();
    assert!(result.is_err());
}

// ===========================================================================
// 16. Concurrent staging — independent temp dirs
// ===========================================================================

#[test]
fn deep_concurrent_three_stages_independent() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "shared").unwrap();

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
    let ws3 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_ne!(ws1.path(), ws2.path());
    assert_ne!(ws2.path(), ws3.path());
    assert_ne!(ws1.path(), ws3.path());

    // Mutate ws1
    fs::write(ws1.path().join("shared.txt"), "mutated1").unwrap();

    // ws2 and ws3 unaffected
    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "shared"
    );
    assert_eq!(
        fs::read_to_string(ws3.path().join("shared.txt")).unwrap(),
        "shared"
    );
}

#[test]
fn deep_concurrent_threaded_staging() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "thread_data").unwrap();
    let src_path = src.path().to_path_buf();

    let handles: Vec<_> = (0..5)
        .map(|_| {
            let p = src_path.clone();
            std::thread::spawn(move || {
                WorkspaceStager::new()
                    .source_root(&p)
                    .with_git_init(false)
                    .stage()
                    .unwrap()
            })
        })
        .collect();

    let workspaces: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // All should have unique paths with identical content
    for i in 0..workspaces.len() {
        for j in (i + 1)..workspaces.len() {
            assert_ne!(workspaces[i].path(), workspaces[j].path());
        }
        assert_eq!(
            fs::read_to_string(workspaces[i].path().join("data.txt")).unwrap(),
            "thread_data"
        );
    }
}

// ===========================================================================
// 17. PassThrough mode
// ===========================================================================

#[test]
fn deep_passthrough_returns_original_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

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
fn deep_passthrough_no_temp_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "data").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // In passthrough mode, the workspace IS the source
    fs::write(ws.path().join("new.txt"), "new").unwrap();
    assert!(src.path().join("new.txt").exists());
}

// ===========================================================================
// 18. WorkspaceStager builder API
// ===========================================================================

#[test]
fn deep_stager_default_is_equivalent_to_new() {
    let _stager = WorkspaceStager::default();
    // Should compile and not panic
}

#[test]
fn deep_stager_chain_order_irrelevant() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.log"), "").unwrap();

    // Order 1: source → include → exclude → git
    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .exclude(vec![])
        .with_git_init(false)
        .stage()
        .unwrap();

    // Order 2: git → exclude → include → source
    let ws2 = WorkspaceStager::new()
        .with_git_init(false)
        .exclude(vec![])
        .include(vec!["*.rs".into()])
        .source_root(src.path())
        .stage()
        .unwrap();

    assert_eq!(files(ws1.path()), files(ws2.path()));
}

#[test]
fn deep_stager_restaging_excludes_prior_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    // First stage with git
    let ws1 = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    // Re-stage from ws1 without git
    let ws2 = WorkspaceStager::new()
        .source_root(ws1.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws2.path().join("code.rs").exists());
    assert!(!ws2.path().join(".git").exists());
}

// ===========================================================================
// 19. File types and special files
// ===========================================================================

#[test]
fn deep_dotfiles_except_git_are_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    fs::write(src.path().join(".env"), "KEY=val").unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    fs::create_dir_all(src.path().join(".config")).unwrap();
    fs::write(src.path().join(".config").join("settings.json"), "{}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(".gitignore").exists());
    assert!(ws.path().join(".env").exists());
    assert!(ws.path().join(".hidden").exists());
    assert!(ws.path().join(".config").join("settings.json").exists());
}

#[test]
fn deep_files_without_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Makefile"), "all:").unwrap();
    fs::write(src.path().join("Dockerfile"), "FROM rust").unwrap();
    fs::write(src.path().join("LICENSE"), "MIT").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("Makefile").exists());
    assert!(ws.path().join("Dockerfile").exists());
    assert!(ws.path().join("LICENSE").exists());
}

#[test]
fn deep_readonly_file_content_preserved() {
    let src = tempdir().unwrap();
    let file = src.path().join("ro.txt");
    fs::write(&file, "read only data").unwrap();

    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("ro.txt")).unwrap(),
        "read only data"
    );

    // Cleanup: restore permissions
    #[allow(clippy::permissions_set_readonly_false)]
    {
        let mut p = fs::metadata(&file).unwrap().permissions();
        p.set_readonly(false);
        fs::set_permissions(&file, p).unwrap();
    }
}

// ===========================================================================
// 20. Additional diff and status scenarios
// ===========================================================================

#[test]
fn deep_diff_append_to_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("log.txt"), "line1\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Append a line
    let mut content = fs::read_to_string(ws.path().join("log.txt")).unwrap();
    content.push_str("line2\n");
    fs::write(ws.path().join("log.txt"), &content).unwrap();

    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("+line2"), "appended line in diff: {diff}");
}

#[test]
fn deep_diff_replace_file_content() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "AAAA").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "BBBB").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("-AAAA"));
    assert!(diff.contains("+BBBB"));
}

#[test]
fn deep_status_after_staged_add_and_delete() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("remove.txt"), "remove").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("remove.txt")).unwrap();
    fs::write(ws.path().join("added.txt"), "new").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("remove.txt"));
    assert!(status.contains("added.txt"));
    // keep.txt should NOT appear (unchanged)
    assert!(!status.contains("keep.txt"));
}

#[test]
fn deep_git_status_none_for_non_git_dir() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("x.txt"), "").unwrap();

    assert!(WorkspaceManager::git_status(tmp.path()).is_none());
    assert!(WorkspaceManager::git_diff(tmp.path()).is_none());
}

// ===========================================================================
// 21. More glob interactions
// ===========================================================================

#[test]
fn deep_glob_exclude_dotfiles_via_pattern() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".env"), "SECRET").unwrap();
    fs::write(src.path().join(".hidden"), "data").unwrap();
    fs::write(src.path().join("visible.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &[".*"])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"visible.rs".to_string()));
    assert!(!f.contains(&".env".to_string()));
    assert!(!f.contains(&".hidden".to_string()));
}

#[test]
fn deep_glob_include_multiple_dirs() {
    let src = tempdir().unwrap();
    for d in &["src", "tests", "benches", "docs"] {
        fs::create_dir_all(src.path().join(d)).unwrap();
        fs::write(src.path().join(d).join("file.txt"), *d).unwrap();
    }
    fs::write(src.path().join("root.txt"), "root").unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["src/**", "tests/**"], &[]))
            .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"src/file.txt".to_string()));
    assert!(f.contains(&"tests/file.txt".to_string()));
    assert!(!f.contains(&"benches/file.txt".to_string()));
    assert!(!f.contains(&"docs/file.txt".to_string()));
    assert!(!f.contains(&"root.txt".to_string()));
}

#[test]
fn deep_glob_exclude_deeply_nested_only() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a").join("b").join("secret")).unwrap();
    fs::write(src.path().join("a").join("keep.txt"), "keep").unwrap();
    fs::write(src.path().join("a").join("b").join("keep2.txt"), "keep2").unwrap();
    fs::write(
        src.path()
            .join("a")
            .join("b")
            .join("secret")
            .join("key.pem"),
        "private",
    )
    .unwrap();

    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["a/b/secret/**"])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"a/keep.txt".to_string()));
    assert!(f.contains(&"a/b/keep2.txt".to_string()));
    assert!(!f.iter().any(|p| p.contains("secret")));
}

// ===========================================================================
// 12. Cleanup — temp directories cleaned up when workspace dropped
// ===========================================================================

#[test]
fn deep_cleanup_staged_temp_dir_removed_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let staged_path = ws.path().to_path_buf();
    assert!(staged_path.exists());
    drop(ws);
    assert!(
        !staged_path.exists(),
        "temp dir should be removed after drop"
    );
}

#[test]
fn deep_cleanup_stager_temp_dir_removed_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "hello").unwrap();

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
        "stager temp dir should be removed after drop"
    );
}

#[test]
fn deep_cleanup_passthrough_path_persists_after_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.txt"), "still here").unwrap();
    let src_path = src.path().to_path_buf();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    drop(ws);
    assert!(
        src_path.exists(),
        "passthrough source should still exist after workspace drop"
    );
}

#[test]
fn deep_cleanup_multiple_staged_workspaces_independent() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let p1 = ws1.path().to_path_buf();
    let p2 = ws2.path().to_path_buf();
    assert_ne!(p1, p2, "different staging dirs");

    drop(ws1);
    assert!(!p1.exists(), "first temp dir cleaned");
    assert!(p2.exists(), "second temp dir still alive");
    drop(ws2);
    assert!(!p2.exists(), "second temp dir now cleaned");
}

#[test]
fn deep_cleanup_staged_files_not_accessible_after_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("secret.txt"), "classified").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let file_path = ws.path().join("secret.txt");
    assert!(file_path.exists());
    drop(ws);
    assert!(fs::read_to_string(&file_path).is_err());
}

// ===========================================================================
// 13. WorkspaceSpec — construction, defaults, globs, paths
// ===========================================================================

#[test]
fn deep_spec_staged_mode_creates_temp_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let spec = staged_spec(src.path());
    assert!(matches!(spec.mode, WorkspaceMode::Staged));
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn deep_spec_passthrough_mode_uses_original() {
    let src = tempdir().unwrap();
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
fn deep_spec_include_filters_applied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("skip.txt"), "skip").unwrap();

    let spec = staged_spec_globs(src.path(), &["*.rs"], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"keep.rs".to_string()));
    assert!(!f.contains(&"skip.txt".to_string()));
}

#[test]
fn deep_spec_exclude_filters_applied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("skip.log"), "log").unwrap();

    let spec = staged_spec_globs(src.path(), &[], &["*.log"]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"keep.rs".to_string()));
    assert!(!f.contains(&"skip.log".to_string()));
}

#[test]
fn deep_spec_empty_include_exclude_copies_all() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.rs"), "b").unwrap();
    fs::create_dir(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub").join("c.md"), "c").unwrap();

    let spec = staged_spec_globs(src.path(), &[], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let f = files(ws.path());
    assert_eq!(f.len(), 3);
}

#[test]
fn deep_spec_root_is_string_path() {
    let src = tempdir().unwrap();
    let spec = staged_spec(src.path());
    assert_eq!(spec.root, src.path().to_string_lossy().to_string());
}

#[test]
fn deep_spec_include_and_exclude_combined() {
    let src = tempdir().unwrap();
    fs::create_dir(src.path().join("src")).unwrap();
    fs::write(src.path().join("src").join("lib.rs"), "lib").unwrap();
    fs::write(src.path().join("src").join("gen.rs"), "gen").unwrap();
    fs::write(src.path().join("README.md"), "readme").unwrap();

    let spec = staged_spec_globs(src.path(), &["src/**"], &["**/gen.rs"]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"src/lib.rs".to_string()));
    assert!(!f.contains(&"src/gen.rs".to_string()));
    assert!(!f.contains(&"README.md".to_string()));
}

#[test]
fn deep_spec_paths_with_spaces() {
    let src = tempdir().unwrap();
    let dir = src.path().join("my project");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("file.txt"), "content").unwrap();

    let spec = WorkspaceSpec {
        root: dir.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("file.txt")).unwrap(),
        "content"
    );
}

#[test]
fn deep_spec_globs_multiple_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "a").unwrap();
    fs::write(src.path().join("b.toml"), "b").unwrap();
    fs::write(src.path().join("c.txt"), "c").unwrap();
    fs::write(src.path().join("d.md"), "d").unwrap();

    let spec = staged_spec_globs(src.path(), &["*.rs", "*.toml"], &[]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"a.rs".to_string()));
    assert!(f.contains(&"b.toml".to_string()));
    assert!(!f.contains(&"c.txt".to_string()));
    assert!(!f.contains(&"d.md".to_string()));
}

// ===========================================================================
// Additional coverage — temp directory uniqueness
// ===========================================================================

#[test]
fn deep_temp_dirs_are_unique_across_stages() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws3 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws1.path(), ws2.path());
    assert_ne!(ws2.path(), ws3.path());
    assert_ne!(ws1.path(), ws3.path());
}

#[test]
fn deep_temp_dir_path_is_valid_utf8() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "x").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().to_str().is_some(), "path should be valid UTF-8");
}

// ===========================================================================
// Additional coverage — WorkspaceStager builder
// ===========================================================================

#[test]
fn deep_stager_source_root_required() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("source_root"),
        "error should mention source_root"
    );
}

#[test]
fn deep_stager_nonexistent_source_root_fails() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
}

#[test]
fn deep_stager_include_filters() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn main(){}").unwrap();
    fs::write(src.path().join("skip.txt"), "nope").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"keep.rs".to_string()));
    assert!(!f.contains(&"skip.txt".to_string()));
}

#[test]
fn deep_stager_exclude_filters() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "code").unwrap();
    fs::write(src.path().join("junk.log"), "log").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"keep.rs".to_string()));
    assert!(!f.contains(&"junk.log".to_string()));
}

#[test]
fn deep_stager_with_git_init_true_creates_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn deep_stager_builder_chaining_order() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();

    // Set exclude before include and source_root last
    let ws = WorkspaceStager::new()
        .exclude(vec!["*.txt".into()])
        .include(vec![])
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"a.rs".to_string()));
    assert!(!f.contains(&"b.txt".to_string()));
}

// ===========================================================================
// Additional coverage — file content fidelity
// ===========================================================================

#[test]
fn deep_file_copy_preserves_exact_byte_content() {
    let src = tempdir().unwrap();
    let content: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("bytes.bin"), &content).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let copied = fs::read(ws.path().join("bytes.bin")).unwrap();
    assert_eq!(content, copied);
}

#[test]
fn deep_file_copy_preserves_line_endings() {
    let src = tempdir().unwrap();
    let crlf = "line1\r\nline2\r\n";
    let lf = "line1\nline2\n";
    fs::write(src.path().join("crlf.txt"), crlf).unwrap();
    fs::write(src.path().join("lf.txt"), lf).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("crlf.txt")).unwrap(),
        crlf
    );
    assert_eq!(fs::read_to_string(ws.path().join("lf.txt")).unwrap(), lf);
}

#[test]
fn deep_file_copy_preserves_trailing_whitespace() {
    let src = tempdir().unwrap();
    let content = "line with trailing spaces   \n  \n";
    fs::write(src.path().join("ws.txt"), content).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("ws.txt")).unwrap(),
        content
    );
}

// ===========================================================================
// Additional coverage — nested directory edge cases
// ===========================================================================

#[test]
fn deep_nested_single_file_deep_path() {
    let src = tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "deep").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("a/b/c/d/leaf.txt")).unwrap(),
        "deep"
    );
}

#[test]
fn deep_nested_dirs_with_same_name_at_different_levels() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("data")).unwrap();
    fs::create_dir_all(src.path().join("sub").join("data")).unwrap();
    fs::write(src.path().join("data").join("a.txt"), "top").unwrap();
    fs::write(src.path().join("sub").join("data").join("a.txt"), "nested").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("data/a.txt")).unwrap(),
        "top"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("sub/data/a.txt")).unwrap(),
        "nested"
    );
}

// ===========================================================================
// Additional coverage — git initialization details
// ===========================================================================

#[test]
fn deep_git_init_head_points_to_valid_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let head = git(ws.path(), &["rev-parse", "HEAD"]);
    assert!(head.is_some());
    let sha = head.unwrap().trim().to_string();
    assert_eq!(sha.len(), 40, "SHA should be 40 hex chars");
    assert!(
        sha.chars().all(|c| c.is_ascii_hexdigit()),
        "SHA should be hex"
    );
}

#[test]
fn deep_git_working_tree_clean_after_init() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    fs::write(src.path().join("b.txt"), "b").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = git(ws.path(), &["status", "--porcelain"]);
    assert!(status.is_some());
    assert!(
        status.unwrap().trim().is_empty(),
        "working tree should be clean"
    );
}

#[test]
fn deep_git_init_only_once_even_if_source_has_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    // Initialize a git repo in the source
    Command::new("git")
        .args(["init"])
        .current_dir(src.path())
        .output()
        .ok();
    Command::new("git")
        .args(["add", "."])
        .current_dir(src.path())
        .output()
        .ok();
    Command::new("git")
        .args(["commit", "-m", "source commit", "--allow-empty"])
        .current_dir(src.path())
        .output()
        .ok();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // .git should not be copied but a new one should be initialized
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(log.is_some());
    let lines: Vec<&str> = log.as_ref().unwrap().trim().lines().collect();
    assert_eq!(lines.len(), 1, "should have exactly one baseline commit");
}

// ===========================================================================
// Additional coverage — WorkspaceManager::git_status / git_diff
// ===========================================================================

#[test]
fn deep_git_status_on_staged_workspace() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    // Clean workspace should have empty status
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(status.unwrap().trim().is_empty());
}

#[test]
fn deep_git_diff_on_staged_workspace_clean() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    assert!(diff.unwrap().trim().is_empty());
}

#[test]
fn deep_git_status_after_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "modified").unwrap();

    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(
        status.unwrap().contains("f.txt"),
        "modified file should appear in status"
    );
}

#[test]
fn deep_git_diff_after_modification() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "original\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "modified\n").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    let d = diff.unwrap();
    assert!(d.contains("-original"));
    assert!(d.contains("+modified"));
}

#[test]
fn deep_git_status_returns_none_for_non_repo() {
    let dir = tempdir().unwrap();
    let status = WorkspaceManager::git_status(dir.path());
    assert!(status.is_none());
}

#[test]
fn deep_git_diff_returns_none_for_non_repo() {
    let dir = tempdir().unwrap();
    let diff = WorkspaceManager::git_diff(dir.path());
    assert!(diff.is_none());
}

// ===========================================================================
// Additional coverage — hidden files, special filenames
// ===========================================================================

#[test]
fn deep_hidden_files_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".hidden"), "secret").unwrap();
    fs::write(src.path().join(".env"), "KEY=VAL").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&".hidden".to_string()));
    assert!(f.contains(&".env".to_string()));
}

#[test]
fn deep_gitignore_file_copied() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".gitignore"), "*.log\ntarget/\n").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join(".gitignore")).unwrap(),
        "*.log\ntarget/\n"
    );
}

#[test]
fn deep_files_with_multiple_extensions() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("archive.tar.gz"), "data").unwrap();
    fs::write(src.path().join("test.spec.ts"), "test").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"archive.tar.gz".to_string()));
    assert!(f.contains(&"test.spec.ts".to_string()));
}

#[test]
fn deep_files_starting_with_dot_and_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".eslintrc.json"), "{}").unwrap();
    fs::write(src.path().join(".prettierrc.yaml"), "---").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&".eslintrc.json".to_string()));
    assert!(f.contains(&".prettierrc.yaml".to_string()));
}

// ===========================================================================
// Additional coverage — permission preservation (Unix-specific)
// ===========================================================================

#[cfg(unix)]
#[test]
fn deep_unix_executable_permission_preserved() {
    use std::os::unix::fs::PermissionsExt;

    let src = tempdir().unwrap();
    let script = src.path().join("run.sh");
    fs::write(&script, "#!/bin/bash\necho hi").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let meta = fs::metadata(ws.path().join("run.sh")).unwrap();
    let mode = meta.permissions().mode();
    // fs::copy on Linux preserves permission bits
    assert!(mode & 0o111 != 0, "executable bits should be preserved");
}

#[cfg(unix)]
#[test]
fn deep_unix_readonly_permission_preserved() {
    use std::os::unix::fs::PermissionsExt;

    let src = tempdir().unwrap();
    let file = src.path().join("readonly.txt");
    fs::write(&file, "locked").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let meta = fs::metadata(ws.path().join("readonly.txt")).unwrap();
    let mode = meta.permissions().mode();
    assert_eq!(mode & 0o777, 0o444, "readonly mode should be preserved");

    // Cleanup: make writable so tempdir cleanup works
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
}

// ===========================================================================
// Additional coverage — large and stress scenarios
// ===========================================================================

#[test]
fn deep_many_nested_dirs_with_files() {
    let src = tempdir().unwrap();
    for i in 0..20 {
        let dir = src.path().join(format!("dir_{i}"));
        fs::create_dir(&dir).unwrap();
        for j in 0..5 {
            fs::write(dir.join(format!("file_{j}.txt")), format!("{i}-{j}")).unwrap();
        }
    }

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let f = files(ws.path());
    assert_eq!(f.len(), 100); // 20 dirs * 5 files
}

#[test]
fn deep_file_exactly_zero_bytes() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty.bin"), b"").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let content = fs::read(ws.path().join("empty.bin")).unwrap();
    assert!(content.is_empty());
}

#[test]
fn deep_file_single_byte() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("one.bin"), &[0x42]).unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("one.bin")).unwrap(), vec![0x42]);
}
