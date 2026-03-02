// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the workspace snapshot and comparison utilities.

use abp_workspace::snapshot::{capture, compare, SnapshotDiff, WorkspaceSnapshot};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Helper: create a temp dir with a set of files.
fn setup_dir(files: &[(&str, &[u8])]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    for (name, content) in files {
        let path = tmp.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
    }
    tmp
}

#[test]
fn capture_empty_directory() {
    let tmp = TempDir::new().unwrap();
    let snap = capture(tmp.path()).unwrap();
    assert_eq!(snap.file_count(), 0);
    assert_eq!(snap.total_size(), 0);
}

#[test]
fn capture_single_file() {
    let tmp = setup_dir(&[("hello.txt", b"hello world")]);
    let snap = capture(tmp.path()).unwrap();
    assert_eq!(snap.file_count(), 1);
    assert!(snap.has_file("hello.txt"));
    assert_eq!(snap.total_size(), 11);
}

#[test]
fn capture_nested_files() {
    let tmp = setup_dir(&[
        ("a.txt", b"aaa"),
        ("sub/b.txt", b"bbb"),
        ("sub/deep/c.txt", b"ccc"),
    ]);
    let snap = capture(tmp.path()).unwrap();
    assert_eq!(snap.file_count(), 3);
    assert!(snap.has_file(PathBuf::from("sub").join("b.txt")));
    assert!(snap.has_file(PathBuf::from("sub").join("deep").join("c.txt")));
}

#[test]
fn capture_excludes_git_directory() {
    let tmp = setup_dir(&[("src.txt", b"code"), (".git/HEAD", b"ref: refs/heads/main")]);
    let snap = capture(tmp.path()).unwrap();
    assert_eq!(snap.file_count(), 1);
    assert!(snap.has_file("src.txt"));
    assert!(!snap.has_file(PathBuf::from(".git").join("HEAD")));
}

#[test]
fn file_snapshot_sha256_deterministic() {
    let tmp = setup_dir(&[("data.bin", b"deterministic")]);
    let s1 = capture(tmp.path()).unwrap();
    let s2 = capture(tmp.path()).unwrap();
    let f1 = s1.get_file("data.bin").unwrap();
    let f2 = s2.get_file("data.bin").unwrap();
    assert_eq!(f1.sha256, f2.sha256);
}

#[test]
fn file_snapshot_detects_binary() {
    let mut content = vec![0u8; 100];
    content[50] = 0; // null byte
    let tmp = setup_dir(&[("binary.dat", &content)]);
    let snap = capture(tmp.path()).unwrap();
    let f = snap.get_file("binary.dat").unwrap();
    assert!(f.is_binary);
}

#[test]
fn file_snapshot_text_not_binary() {
    let tmp = setup_dir(&[("text.txt", b"just plain text\nwith newlines\n")]);
    let snap = capture(tmp.path()).unwrap();
    let f = snap.get_file("text.txt").unwrap();
    assert!(!f.is_binary);
}

#[test]
fn get_file_returns_none_for_missing() {
    let tmp = setup_dir(&[("exists.txt", b"yes")]);
    let snap = capture(tmp.path()).unwrap();
    assert!(snap.get_file("missing.txt").is_none());
}

#[test]
fn compare_identical_snapshots() {
    let tmp = setup_dir(&[("a.txt", b"aaa"), ("b.txt", b"bbb")]);
    let snap = capture(tmp.path()).unwrap();
    let diff = compare(&snap, &snap);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged.len(), 2);
}

#[test]
fn compare_detects_added_files() {
    let tmp_a = setup_dir(&[("a.txt", b"aaa")]);
    let tmp_b = setup_dir(&[("a.txt", b"aaa"), ("b.txt", b"bbb")]);
    let sa = capture(tmp_a.path()).unwrap();
    let sb = capture(tmp_b.path()).unwrap();
    let diff = compare(&sa, &sb);
    assert_eq!(diff.added, vec![PathBuf::from("b.txt")]);
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged, vec![PathBuf::from("a.txt")]);
}

#[test]
fn compare_detects_removed_files() {
    let tmp_a = setup_dir(&[("a.txt", b"aaa"), ("b.txt", b"bbb")]);
    let tmp_b = setup_dir(&[("a.txt", b"aaa")]);
    let sa = capture(tmp_a.path()).unwrap();
    let sb = capture(tmp_b.path()).unwrap();
    let diff = compare(&sa, &sb);
    assert!(diff.added.is_empty());
    assert_eq!(diff.removed, vec![PathBuf::from("b.txt")]);
    assert!(diff.modified.is_empty());
    assert_eq!(diff.unchanged, vec![PathBuf::from("a.txt")]);
}

#[test]
fn compare_detects_modified_files() {
    let tmp_a = setup_dir(&[("a.txt", b"original")]);
    let tmp_b = setup_dir(&[("a.txt", b"changed")]);
    let sa = capture(tmp_a.path()).unwrap();
    let sb = capture(tmp_b.path()).unwrap();
    let diff = compare(&sa, &sb);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert_eq!(diff.modified, vec![PathBuf::from("a.txt")]);
    assert!(diff.unchanged.is_empty());
}

#[test]
fn compare_mixed_changes() {
    let tmp_a = setup_dir(&[
        ("keep.txt", b"same"),
        ("modify.txt", b"old"),
        ("remove.txt", b"gone"),
    ]);
    let tmp_b = setup_dir(&[
        ("keep.txt", b"same"),
        ("modify.txt", b"new"),
        ("add.txt", b"fresh"),
    ]);
    let sa = capture(tmp_a.path()).unwrap();
    let sb = capture(tmp_b.path()).unwrap();
    let diff = compare(&sa, &sb);
    assert_eq!(diff.added, vec![PathBuf::from("add.txt")]);
    assert_eq!(diff.removed, vec![PathBuf::from("remove.txt")]);
    assert_eq!(diff.modified, vec![PathBuf::from("modify.txt")]);
    assert_eq!(diff.unchanged, vec![PathBuf::from("keep.txt")]);
}

#[test]
fn snapshot_serialization_roundtrip() {
    let tmp = setup_dir(&[("test.txt", b"content")]);
    let snap = capture(tmp.path()).unwrap();
    let json = serde_json::to_string(&snap).unwrap();
    let deserialized: WorkspaceSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.file_count(), snap.file_count());
    assert_eq!(deserialized.total_size(), snap.total_size());
    let orig = snap.get_file("test.txt").unwrap();
    let deser = deserialized.get_file("test.txt").unwrap();
    assert_eq!(orig.sha256, deser.sha256);
}

#[test]
fn snapshot_diff_default_is_empty() {
    let diff = SnapshotDiff::default();
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert!(diff.unchanged.is_empty());
}
