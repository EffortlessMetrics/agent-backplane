// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_workspace::WorkspaceStager;
use std::fs;
use tempfile::tempdir;

#[test]
fn builder_with_defaults_works() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "world").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();

    assert!(ws.path().join("hello.txt").exists());
    assert_eq!(
        fs::read_to_string(ws.path().join("hello.txt")).unwrap(),
        "world"
    );
    // Git repo should be initialized by default.
    assert!(ws.path().join(".git").exists());
}

#[test]
fn builder_with_custom_include_exclude() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("skip.log"), "log data").unwrap();
    fs::write(src.path().join("also_skip.txt"), "text").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into(), "*.txt".into()])
        .exclude(vec!["also_*".into()])
        .stage()
        .unwrap();

    assert!(ws.path().join("keep.rs").exists());
    assert!(!ws.path().join("skip.log").exists());
    assert!(!ws.path().join("also_skip.txt").exists());
}

#[test]
fn builder_without_git_init() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("file.txt").exists());
    assert!(
        !ws.path().join(".git").exists(),
        "git should not be initialized when with_git_init(false)"
    );
}

#[test]
fn builder_error_on_missing_source() {
    let err = WorkspaceStager::new().stage();
    assert!(err.is_err(), "should error when source_root is not set");

    let err = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(
        err.is_err(),
        "should error when source directory does not exist"
    );
}
