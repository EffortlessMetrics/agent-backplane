// SPDX-License-Identifier: MIT OR Apache-2.0
//! Insta snapshot tests for workspace staging behavior.
//!
//! These tests snapshot file listings, git status output, and the effects
//! of various include/exclude glob patterns on staged workspaces.

use abp_workspace::WorkspaceStager;
use std::fs;
use tempfile::tempdir;
use walkdir::WalkDir;

/// Collect a sorted, normalized file listing from `root`, excluding `.git`.
fn file_listing(root: &std::path::Path) -> Vec<String> {
    let mut files: Vec<String> = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == ".git"))
        .filter(|e| e.path() != root)
        .map(|e| {
            let rel = e.path().strip_prefix(root).unwrap();
            let mut s = rel.to_string_lossy().replace('\\', "/");
            if e.file_type().is_dir() {
                s.push('/');
            }
            s
        })
        .collect();
    files.sort();
    files
}

// ---------------------------------------------------------------------------
// 1. Complex directory structure snapshot
// ---------------------------------------------------------------------------
#[test]
fn snapshot_complex_directory_staging() {
    let src = tempdir().unwrap();
    let p = src.path();

    // Create a realistic project layout
    fs::create_dir_all(p.join("src").join("utils")).unwrap();
    fs::create_dir_all(p.join("tests")).unwrap();
    fs::create_dir_all(p.join("docs")).unwrap();
    fs::create_dir_all(p.join("assets").join("images")).unwrap();

    fs::write(p.join("Cargo.toml"), "[package]\nname = \"demo\"").unwrap();
    fs::write(p.join("README.md"), "# Demo").unwrap();
    fs::write(p.join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(p.join("src").join("lib.rs"), "pub mod utils;").unwrap();
    fs::write(
        p.join("src").join("utils").join("mod.rs"),
        "pub fn helper() {}",
    )
    .unwrap();
    fs::write(
        p.join("tests").join("integration.rs"),
        "#[test] fn it_works() {}",
    )
    .unwrap();
    fs::write(p.join("docs").join("guide.md"), "# Guide").unwrap();
    fs::write(
        p.join("assets").join("images").join("logo.png"),
        [0x89, 0x50, 0x4E, 0x47],
    )
    .unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("complex_directory_staging", listing);
}

// ---------------------------------------------------------------------------
// 2. Git status after staging
// ---------------------------------------------------------------------------
#[test]
fn snapshot_git_status_after_staging() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::write(p.join("main.rs"), "fn main() {}").unwrap();
    fs::write(p.join("lib.rs"), "pub fn hello() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(true)
        .stage()
        .unwrap();

    // Fresh staged workspace should have clean git status
    let status = abp_workspace::WorkspaceManager::git_status(ws.path()).unwrap_or_default();

    insta::assert_snapshot!("git_status_clean_after_staging", status.trim());
}

// ---------------------------------------------------------------------------
// 3. Include/exclude pattern variants
// ---------------------------------------------------------------------------
#[test]
fn snapshot_include_only_rs_files() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::create_dir_all(p.join("src")).unwrap();
    fs::write(p.join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(p.join("src").join("lib.rs"), "pub mod a;").unwrap();
    fs::write(p.join("README.md"), "# Readme").unwrap();
    fs::write(p.join("Cargo.toml"), "[package]").unwrap();
    fs::write(p.join("data.json"), "{}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .include(vec!["**/*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("include_only_rs_files", listing);
}

#[test]
fn snapshot_exclude_logs_and_tmp() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::create_dir_all(p.join("logs")).unwrap();
    fs::write(p.join("app.rs"), "fn main() {}").unwrap();
    fs::write(p.join("server.log"), "log data").unwrap();
    fs::write(p.join("cache.tmp"), "temp data").unwrap();
    fs::write(p.join("logs").join("debug.log"), "debug log").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .exclude(vec!["*.log".into(), "*.tmp".into(), "logs/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("exclude_logs_and_tmp", listing);
}

#[test]
fn snapshot_include_src_exclude_generated() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::create_dir_all(p.join("src").join("generated")).unwrap();
    fs::create_dir_all(p.join("src").join("core")).unwrap();
    fs::write(p.join("src").join("lib.rs"), "pub mod core;").unwrap();
    fs::write(p.join("src").join("core").join("mod.rs"), "pub fn run() {}").unwrap();
    fs::write(
        p.join("src").join("generated").join("out.rs"),
        "// generated",
    )
    .unwrap();
    fs::write(p.join("README.md"), "# Readme").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .include(vec!["src/**".into()])
        .exclude(vec!["src/generated/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("include_src_exclude_generated", listing);
}

#[test]
fn snapshot_multiple_include_patterns() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::create_dir_all(p.join("src")).unwrap();
    fs::create_dir_all(p.join("tests")).unwrap();
    fs::write(p.join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(p.join("tests").join("test.rs"), "#[test] fn t() {}").unwrap();
    fs::write(p.join("Cargo.toml"), "[package]").unwrap();
    fs::write(p.join("README.md"), "# Readme").unwrap();
    fs::write(p.join("build.sh"), "#!/bin/sh").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .include(vec![
            "src/**".into(),
            "tests/**".into(),
            "Cargo.toml".into(),
        ])
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("multiple_include_patterns", listing);
}

// ---------------------------------------------------------------------------
// 4. Nested directories
// ---------------------------------------------------------------------------
#[test]
fn snapshot_nested_directories() {
    let src = tempdir().unwrap();
    let p = src.path();

    let deep = p.join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep).unwrap();
    fs::write(p.join("root.txt"), "root").unwrap();
    fs::write(p.join("a").join("a.txt"), "a").unwrap();
    fs::write(p.join("a").join("b").join("b.txt"), "b").unwrap();
    fs::write(p.join("a").join("b").join("c").join("c.txt"), "c").unwrap();
    fs::write(deep.join("d.txt"), "d").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("nested_directories", listing);
}

// ---------------------------------------------------------------------------
// 5. Symlink handling — skip on platforms/configs that don't support them
// ---------------------------------------------------------------------------
#[test]
fn snapshot_symlinks_not_followed() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::write(p.join("real.txt"), "real content").unwrap();
    fs::create_dir_all(p.join("sub")).unwrap();
    fs::write(p.join("sub").join("other.txt"), "other").unwrap();

    // Attempt to create a symlink; skip test gracefully if unsupported
    #[cfg(unix)]
    let link_ok = std::os::unix::fs::symlink(p.join("real.txt"), p.join("link.txt")).is_ok();
    #[cfg(windows)]
    let link_ok =
        std::os::windows::fs::symlink_file(p.join("real.txt"), p.join("link.txt")).is_ok();
    #[cfg(not(any(unix, windows)))]
    let link_ok = false;

    if !link_ok {
        // Symlinks not available — snapshot without symlinks
        let ws = WorkspaceStager::new()
            .source_root(p)
            .with_git_init(false)
            .stage()
            .unwrap();

        let listing = file_listing(ws.path());
        insta::assert_json_snapshot!("symlinks_not_followed", listing);
        return;
    }

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    // WalkDir follow_links(false) means symlinks are NOT followed/copied
    // The staging copies only regular files, so link.txt should be absent.
    insta::assert_json_snapshot!("symlinks_not_followed", listing);
}

// ---------------------------------------------------------------------------
// 6. Empty directories
// ---------------------------------------------------------------------------
#[test]
fn snapshot_empty_directories() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::create_dir_all(p.join("empty_a")).unwrap();
    fs::create_dir_all(p.join("empty_b").join("nested_empty")).unwrap();
    fs::write(p.join("file.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("empty_directories", listing);
}

// ---------------------------------------------------------------------------
// 7. Binary files
// ---------------------------------------------------------------------------
#[test]
fn snapshot_binary_files() {
    let src = tempdir().unwrap();
    let p = src.path();

    // PNG header
    fs::write(
        p.join("image.png"),
        [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    )
    .unwrap();
    // Null bytes
    fs::write(p.join("data.bin"), vec![0u8; 64]).unwrap();
    // Regular text
    fs::write(p.join("readme.txt"), "hello").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("binary_files", listing);

    // Verify binary content is preserved
    assert_eq!(fs::read(ws.path().join("data.bin")).unwrap(), vec![0u8; 64]);
    assert_eq!(
        fs::read(ws.path().join("image.png")).unwrap(),
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    );
}

// ---------------------------------------------------------------------------
// 8. Git initialization behavior
// ---------------------------------------------------------------------------
#[test]
fn snapshot_git_init_creates_baseline_commit() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::write(p.join("main.rs"), "fn main() {}").unwrap();
    fs::write(p.join("lib.rs"), "pub mod a;").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(true)
        .stage()
        .unwrap();

    assert!(ws.path().join(".git").exists(), ".git dir must exist");

    // Verify baseline commit exists
    let log_output = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(ws.path())
        .output()
        .expect("git log");

    let log = String::from_utf8_lossy(&log_output.stdout);
    insta::assert_snapshot!("git_init_baseline_commit", {
        // Redact the commit hash (first 7+ chars) and keep the message
        let trimmed = log.trim();
        if let Some((_hash, msg)) = trimmed.split_once(' ') {
            format!("<hash> {msg}")
        } else {
            trimmed.to_string()
        }
    });
}

#[test]
fn snapshot_git_init_disabled() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::write(p.join("file.txt"), "data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws.path().join(".git").exists(),
        ".git must NOT exist when git_init is disabled"
    );

    let listing = file_listing(ws.path());
    insta::assert_json_snapshot!("git_init_disabled", listing);
}
