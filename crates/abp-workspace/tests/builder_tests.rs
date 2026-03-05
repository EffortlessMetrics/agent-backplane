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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
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
