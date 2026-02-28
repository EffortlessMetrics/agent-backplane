// SPDX-License-Identifier: MIT OR Apache-2.0
//! Edge case tests for workspace staging.
//!
//! Covers empty directories, all-excluded scenarios, binary-only staging,
//! deeply nested paths, Unicode filenames, large file counts, double-staging,
//! and .git exclusion guarantees.

use abp_workspace::{WorkspaceManager, WorkspaceStager};
use abp_core::{WorkspaceMode, WorkspaceSpec};
use std::fs;
use tempfile::tempdir;
use walkdir::WalkDir;

/// Collect a sorted, normalized file listing from `root`, excluding `.git`.
fn file_listing(root: &std::path::Path) -> Vec<String> {
    let mut files: Vec<String> = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            !e.path()
                .components()
                .any(|c| c.as_os_str() == ".git")
        })
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
// 1. Stage an empty directory
// ---------------------------------------------------------------------------
#[test]
fn stage_empty_directory() {
    let src = tempdir().unwrap();
    // source is completely empty — no files at all

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    assert!(listing.is_empty(), "empty source should produce empty staging");
}

#[test]
fn stage_empty_directory_with_git() {
    let src = tempdir().unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    // Only .git should exist, which file_listing excludes
    let listing = file_listing(ws.path());
    assert!(listing.is_empty(), "empty source with git should have no user files");
    assert!(ws.path().join(".git").exists(), ".git should be created");
}

// ---------------------------------------------------------------------------
// 2. Stage with ALL files excluded
// ---------------------------------------------------------------------------
#[test]
fn stage_with_all_files_excluded() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::write(p.join("a.rs"), "fn a() {}").unwrap();
    fs::write(p.join("b.txt"), "hello").unwrap();
    fs::write(p.join("c.log"), "log").unwrap();
    fs::create_dir_all(p.join("sub")).unwrap();
    fs::write(p.join("sub").join("d.rs"), "fn d() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .exclude(vec!["**/*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    // All files should be excluded; only directories that were created may remain
    // but with no files inside them
    let file_count = listing.iter().filter(|f| !f.ends_with('/')).count();
    assert_eq!(file_count, 0, "no files should survive a **/* exclude, got: {listing:?}");
}

#[test]
fn stage_with_include_matching_nothing() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::write(p.join("a.rs"), "fn a() {}").unwrap();
    fs::write(p.join("b.txt"), "hello").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .include(vec!["*.nonexistent_extension".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    let file_count = listing.iter().filter(|f| !f.ends_with('/')).count();
    assert_eq!(
        file_count, 0,
        "include matching nothing should produce no files, got: {listing:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. Stage with only binary files
// ---------------------------------------------------------------------------
#[test]
fn stage_only_binary_files() {
    let src = tempdir().unwrap();
    let p = src.path();

    // Various binary files
    let png_header: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    fs::write(p.join("image.png"), &png_header).unwrap();
    fs::write(p.join("zeros.bin"), vec![0u8; 256]).unwrap();
    fs::write(p.join("random.dat"), (0..128u8).collect::<Vec<u8>>()).unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    assert_eq!(listing.len(), 3, "all 3 binary files should be staged");

    // Verify binary content integrity
    assert_eq!(fs::read(ws.path().join("image.png")).unwrap(), png_header);
    assert_eq!(fs::read(ws.path().join("zeros.bin")).unwrap(), vec![0u8; 256]);
    assert_eq!(
        fs::read(ws.path().join("random.dat")).unwrap(),
        (0..128u8).collect::<Vec<u8>>()
    );
}

// ---------------------------------------------------------------------------
// 4. Stage a deeply nested directory (10+ levels)
// ---------------------------------------------------------------------------
#[test]
fn stage_deeply_nested_directory() {
    let src = tempdir().unwrap();
    let p = src.path();

    // Build 12 levels deep
    let mut deep = p.to_path_buf();
    for i in 0..12 {
        deep = deep.join(format!("level_{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("deep_file.txt"), "I am deep").unwrap();
    // Also put a file at the root
    fs::write(p.join("root.txt"), "I am root").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    // Verify deepest file exists
    let mut expected = ws.path().to_path_buf();
    for i in 0..12 {
        expected = expected.join(format!("level_{i}"));
    }
    assert!(
        expected.join("deep_file.txt").exists(),
        "deeply nested file must exist in staged workspace"
    );
    assert_eq!(
        fs::read_to_string(expected.join("deep_file.txt")).unwrap(),
        "I am deep"
    );

    // Root file also present
    assert!(ws.path().join("root.txt").exists());
}

// ---------------------------------------------------------------------------
// 5. Stage with Unicode filenames
// ---------------------------------------------------------------------------
#[test]
fn stage_unicode_filenames() {
    let src = tempdir().unwrap();
    let p = src.path();

    // Various Unicode filenames
    fs::write(p.join("données.txt"), "French data").unwrap();
    fs::write(p.join("日本語.rs"), "fn nihongo() {}").unwrap();
    fs::write(p.join("émojis_🦀.txt"), "Rust crab").unwrap();
    fs::create_dir_all(p.join("目录")).unwrap();
    fs::write(p.join("目录").join("文件.md"), "# Chinese").unwrap();
    fs::write(p.join("ascii.txt"), "plain ascii").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    // All files should be present
    assert!(ws.path().join("données.txt").exists());
    assert!(ws.path().join("日本語.rs").exists());
    assert!(ws.path().join("émojis_🦀.txt").exists());
    assert!(ws.path().join("目录").join("文件.md").exists());
    assert!(ws.path().join("ascii.txt").exists());

    // Verify content
    assert_eq!(
        fs::read_to_string(ws.path().join("données.txt")).unwrap(),
        "French data"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("目录").join("文件.md")).unwrap(),
        "# Chinese"
    );
}

// ---------------------------------------------------------------------------
// 6. Stage a very large number of files (100+)
// ---------------------------------------------------------------------------
#[test]
fn stage_large_number_of_files() {
    let src = tempdir().unwrap();
    let p = src.path();

    let file_count = 150;
    // Create files across several directories
    for i in 0..file_count {
        let dir = p.join(format!("dir_{}", i % 10));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("file_{i:03}.txt")), format!("content {i}")).unwrap();
    }

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    let staged_files: Vec<_> = listing.iter().filter(|f| !f.ends_with('/')).collect();
    assert_eq!(
        staged_files.len(),
        file_count,
        "all {file_count} files should be staged"
    );

    // Spot-check a few files
    assert_eq!(
        fs::read_to_string(ws.path().join("dir_0").join("file_000.txt")).unwrap(),
        "content 0"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("dir_9").join("file_149.txt")).unwrap(),
        "content 149"
    );
}

// ---------------------------------------------------------------------------
// 7. Double-staging the same workspace
// ---------------------------------------------------------------------------
#[test]
fn double_staging_same_source() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::write(p.join("file.txt"), "hello").unwrap();
    fs::create_dir_all(p.join("sub")).unwrap();
    fs::write(p.join("sub").join("nested.txt"), "nested").unwrap();

    let ws1 = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    let ws2 = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    // Both should be independent
    assert_ne!(ws1.path(), ws2.path());

    let listing1 = file_listing(ws1.path());
    let listing2 = file_listing(ws2.path());
    assert_eq!(listing1, listing2, "double-staged workspaces should be identical");

    // Mutation in one doesn't affect the other
    fs::write(ws1.path().join("file.txt"), "modified").unwrap();
    assert_eq!(
        fs::read_to_string(ws2.path().join("file.txt")).unwrap(),
        "hello",
        "ws2 must not be affected by ws1 mutation"
    );
}

#[test]
fn stage_already_staged_workspace() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::write(p.join("original.txt"), "original").unwrap();

    // First stage
    let ws1 = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(true)
        .stage()
        .unwrap();

    // Add a file in ws1
    fs::write(ws1.path().join("added.txt"), "added in ws1").unwrap();

    // Stage ws1 output as a new source
    let ws2 = WorkspaceStager::new()
        .source_root(ws1.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    // ws2 should have both files (minus .git from ws1)
    assert!(ws2.path().join("original.txt").exists());
    assert!(ws2.path().join("added.txt").exists());
    assert!(
        !ws2.path().join(".git").exists(),
        ".git from ws1 must not be copied to ws2"
    );
}

// ---------------------------------------------------------------------------
// 8. Verify .git is always excluded from source
// ---------------------------------------------------------------------------
#[test]
fn dot_git_always_excluded_from_source() {
    let src = tempdir().unwrap();
    let p = src.path();

    // Simulate a source that has a .git directory with unique content
    let git_dir = p.join(".git");
    fs::create_dir_all(git_dir.join("objects")).unwrap();
    fs::create_dir_all(git_dir.join("refs")).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(git_dir.join("config"), "[core]\nbare = false").unwrap();
    fs::write(git_dir.join("abp_sentinel"), "should_not_appear").unwrap();

    fs::write(p.join("code.rs"), "fn main() {}").unwrap();
    fs::create_dir_all(p.join("src")).unwrap();
    fs::write(p.join("src").join("lib.rs"), "pub fn f() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .with_git_init(false)
        .stage()
        .unwrap();

    // .git directory must NOT be copied
    assert!(
        !ws.path().join(".git").exists(),
        "source .git must not be copied (git_init disabled)"
    );

    // Source files should be present
    assert!(ws.path().join("code.rs").exists());
    assert!(ws.path().join("src").join("lib.rs").exists());
}

#[test]
fn dot_git_excluded_even_with_broad_include() {
    let src = tempdir().unwrap();
    let p = src.path();

    let git_dir = p.join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("sentinel"), "marker").unwrap();
    fs::write(p.join("file.txt"), "content").unwrap();

    // Use broad include that would match .git/**
    let ws = WorkspaceStager::new()
        .source_root(p)
        .include(vec!["**/*".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("file.txt").exists());
    assert!(
        !ws.path().join(".git").join("sentinel").exists(),
        ".git content must not be copied even with **/* include"
    );
}

#[test]
fn dot_git_excluded_via_workspace_manager() {
    let src = tempdir().unwrap();
    let p = src.path();

    let git_dir = p.join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("unique_test_marker"), "test123").unwrap();
    fs::write(p.join("app.rs"), "fn main() {}").unwrap();

    let spec = WorkspaceSpec {
        root: p.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(ws.path().join("app.rs").exists());
    // The staging git-init creates a new .git, but the source sentinel must not be there
    assert!(
        !ws.path().join(".git").join("unique_test_marker").exists(),
        "source .git contents must never be copied by WorkspaceManager"
    );
}

// ---------------------------------------------------------------------------
// Bonus: include/exclude interaction edge cases
// ---------------------------------------------------------------------------
#[test]
fn exclude_star_star_leaves_nothing() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::create_dir_all(p.join("a").join("b")).unwrap();
    fs::write(p.join("root.txt"), "root").unwrap();
    fs::write(p.join("a").join("mid.txt"), "mid").unwrap();
    fs::write(p.join("a").join("b").join("deep.txt"), "deep").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .exclude(vec!["**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    let file_count = listing.iter().filter(|f| !f.ends_with('/')).count();
    assert_eq!(file_count, 0, "exclude ** should leave no files, got: {listing:?}");
}

#[test]
fn include_specific_extension_across_dirs() {
    let src = tempdir().unwrap();
    let p = src.path();

    fs::create_dir_all(p.join("src")).unwrap();
    fs::create_dir_all(p.join("tests")).unwrap();
    fs::write(p.join("main.rs"), "fn main() {}").unwrap();
    fs::write(p.join("src").join("lib.rs"), "pub fn f() {}").unwrap();
    fs::write(p.join("tests").join("test.rs"), "#[test] fn t() {}").unwrap();
    fs::write(p.join("README.md"), "# README").unwrap();
    fs::write(p.join("src").join("data.json"), "{}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(p)
        .include(vec!["**/*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let listing = file_listing(ws.path());
    let files: Vec<_> = listing.iter().filter(|f| !f.ends_with('/')).collect();
    assert!(
        files.iter().all(|f| f.ends_with(".rs")),
        "only .rs files should be staged, got: {files:?}"
    );
    assert_eq!(files.len(), 3, "should have 3 .rs files");
}
