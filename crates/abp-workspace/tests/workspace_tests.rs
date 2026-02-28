use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::WorkspaceManager;
use std::fs;
use tempfile::tempdir;

fn make_spec(root: &str, mode: WorkspaceMode) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string(),
        mode,
        include: vec![],
        exclude: vec![],
    }
}

// 1. PassThrough mode returns original path
#[test]
fn passthrough_returns_original_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "hello").unwrap();

    let spec = make_spec(&src.path().to_string_lossy(), WorkspaceMode::PassThrough);
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert_eq!(prepared.path(), src.path());
}

// 2. Staged mode creates temp directory with copied files
#[test]
fn staged_creates_temp_directory_with_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "world").unwrap();

    let spec = make_spec(&src.path().to_string_lossy(), WorkspaceMode::Staged);
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert_ne!(prepared.path(), src.path());
    assert!(prepared.path().join("hello.txt").exists());
    assert_eq!(fs::read_to_string(prepared.path().join("hello.txt")).unwrap(), "world");
}

// 3. Staged mode excludes .git directory
#[test]
fn staged_excludes_dot_git() {
    let src = tempdir().unwrap();
    let git_dir = src.path().join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("config"), "git stuff").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    let spec = make_spec(&src.path().to_string_lossy(), WorkspaceMode::Staged);
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    // .git should exist (created by ensure_git_repo), but the original .git/config content should not
    assert!(prepared.path().join("code.rs").exists());
    // The original .git/config with "git stuff" must NOT be present
    let config_path = prepared.path().join(".git").join("config");
    if config_path.exists() {
        let content = fs::read_to_string(&config_path).unwrap();
        assert_ne!(content, "git stuff", ".git content from source should not be copied");
    }
}

// 4. Staged mode respects include globs
#[test]
fn staged_respects_include_globs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "keep me").unwrap();
    fs::write(src.path().join("skip.txt"), "skip me").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".to_string()],
        exclude: vec![],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert!(prepared.path().join("keep.rs").exists());
    assert!(!prepared.path().join("skip.txt").exists());
}

// 5. Staged mode respects exclude globs
#[test]
fn staged_respects_exclude_globs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "keep me").unwrap();
    fs::write(src.path().join("secret.env"), "SECRET=123").unwrap();

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["*.env".to_string()],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert!(prepared.path().join("keep.rs").exists());
    assert!(!prepared.path().join("secret.env").exists());
}

// 6. Staged mode initializes git repo
#[test]
fn staged_initializes_git_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("main.rs"), "fn main() {}").unwrap();

    let spec = make_spec(&src.path().to_string_lossy(), WorkspaceMode::Staged);
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert!(prepared.path().join(".git").exists(), "staged workspace should have .git");
}

// 7. git_status on staged workspace shows clean state
#[test]
fn git_status_on_staged_workspace_is_clean() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("app.rs"), "fn main() {}").unwrap();

    let spec = make_spec(&src.path().to_string_lossy(), WorkspaceMode::Staged);
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    let status = WorkspaceManager::git_status(prepared.path());
    match status {
        Some(s) => assert!(s.trim().is_empty(), "expected clean status, got: {s}"),
        None => {} // also acceptable if git not available
    }
}

// 8. git_diff on clean workspace returns empty
#[test]
fn git_diff_on_clean_workspace_is_empty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("lib.rs"), "pub fn hello() {}").unwrap();

    let spec = make_spec(&src.path().to_string_lossy(), WorkspaceMode::Staged);
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    let diff = WorkspaceManager::git_diff(prepared.path());
    match diff {
        Some(d) => assert!(d.trim().is_empty(), "expected empty diff, got: {d}"),
        None => {}
    }
}

// 9. Nested directory structure is preserved
#[test]
fn nested_directory_structure_preserved() {
    let src = tempdir().unwrap();
    let deep = src.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep.txt"), "deep content").unwrap();
    fs::write(src.path().join("top.txt"), "top content").unwrap();

    let spec = make_spec(&src.path().to_string_lossy(), WorkspaceMode::Staged);
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert!(prepared.path().join("top.txt").exists());
    assert!(prepared.path().join("a").join("b").join("c").join("deep.txt").exists());
    assert_eq!(
        fs::read_to_string(prepared.path().join("a").join("b").join("c").join("deep.txt")).unwrap(),
        "deep content"
    );
}

// 10. Empty source directory handled gracefully
#[test]
fn empty_source_directory() {
    let src = tempdir().unwrap();
    // No files created — source is empty

    let spec = make_spec(&src.path().to_string_lossy(), WorkspaceMode::Staged);
    let prepared = WorkspaceManager::prepare(&spec).unwrap();

    assert_ne!(prepared.path(), src.path());
    // Should still have .git from ensure_git_repo
    assert!(prepared.path().join(".git").exists());
}
