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
//! Comprehensive tests for workspace staging: spec construction, file copying,
//! git initialization, glob filtering, error handling, and cleanup.

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::{PreparedWorkspace, WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::Path;
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

fn staged_spec_globs(root: &Path, inc: &[&str], exc: &[&str]) -> WorkspaceSpec {
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

/// List all regular files relative to `root`, excluding `.git`, sorted, with `/` separators.
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

fn git(path: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

/// Create a small project tree for reuse.
fn populate(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"").unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn lib() {}").unwrap();
    fs::write(root.join("src/util.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("tests/integration.rs"), "#[test] fn t() {}").unwrap();
    fs::write(root.join("README.md"), "# Demo").unwrap();
}

// ===========================================================================
// 1. WorkspaceSpec construction (~10 tests)
// ===========================================================================

#[test]
fn wd_spec_default_fields() {
    let spec = WorkspaceSpec {
        root: "/tmp/ws".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    assert_eq!(spec.root, "/tmp/ws");
    assert!(spec.include.is_empty());
    assert!(spec.exclude.is_empty());
}

#[test]
fn wd_spec_with_include_globs() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into(), "*.toml".into()],
        exclude: vec![],
    };
    assert_eq!(spec.include.len(), 2);
    assert!(spec.include.contains(&"*.rs".to_string()));
}

#[test]
fn wd_spec_with_exclude_globs() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["target/**".into(), "*.log".into()],
    };
    assert_eq!(spec.exclude.len(), 2);
}

#[test]
fn wd_spec_with_both_include_exclude() {
    let spec = WorkspaceSpec {
        root: "/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["src/generated/**".into()],
    };
    assert_eq!(spec.include.len(), 1);
    assert_eq!(spec.exclude.len(), 1);
}

#[test]
fn wd_spec_serde_roundtrip_staged() {
    let spec = WorkspaceSpec {
        root: "/tmp/test".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec!["target/**".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let deserialized: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.root, spec.root);
    assert_eq!(deserialized.include, spec.include);
    assert_eq!(deserialized.exclude, spec.exclude);
}

#[test]
fn wd_spec_serde_roundtrip_passthrough() {
    let spec = WorkspaceSpec {
        root: "/home/user/project".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let deserialized: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.root, spec.root);
    assert!(deserialized.include.is_empty());
    assert!(deserialized.exclude.is_empty());
}

#[test]
fn wd_spec_serde_mode_string_staged() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let json = serde_json::to_string(&spec).unwrap();
    assert!(
        json.contains("staged"),
        "mode should serialize as snake_case: {json}"
    );
}

#[test]
fn wd_spec_serde_mode_string_passthrough() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let json = serde_json::to_string(&spec).unwrap();
    assert!(
        json.contains("pass_through"),
        "mode should serialize as snake_case: {json}"
    );
}

#[test]
fn wd_spec_serde_deserialize_from_json_literal() {
    let json = r#"{"root":"/ws","mode":"staged","include":["*.rs"],"exclude":["*.log"]}"#;
    let spec: WorkspaceSpec = serde_json::from_str(json).unwrap();
    assert_eq!(spec.root, "/ws");
    assert_eq!(spec.include, vec!["*.rs"]);
    assert_eq!(spec.exclude, vec!["*.log"]);
}

#[test]
fn wd_spec_clone_independence() {
    let spec = WorkspaceSpec {
        root: "/a".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec![],
    };
    let mut cloned = spec.clone();
    cloned.root = "/b".into();
    cloned.include.push("*.toml".into());
    assert_eq!(spec.root, "/a");
    assert_eq!(spec.include.len(), 1);
}

// ===========================================================================
// 2. File copying (~15 tests)
// ===========================================================================

#[test]
fn wd_copy_preserves_text_content() {
    let src = tempdir().unwrap();
    let content = "Hello, world!\nSecond line.\r\nThird.";
    fs::write(src.path().join("file.txt"), content).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("file.txt")).unwrap(),
        content
    );
}

#[test]
fn wd_copy_preserves_binary_content() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = (0..=255).cycle().take(4096).collect();
    fs::write(src.path().join("binary.dat"), &data).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("binary.dat")).unwrap(), data);
}

#[test]
fn wd_copy_include_only_rs_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("config.yaml"), "key: val").unwrap();
    fs::write(src.path().join("data.json"), "{}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    let f = files(ws.path());
    assert_eq!(f, vec!["main.rs"]);
}

#[test]
fn wd_copy_exclude_env_files() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".env"), "SECRET=xyz").unwrap();
    fs::write(src.path().join(".env.local"), "LOCAL=1").unwrap();
    fs::write(src.path().join("app.rs"), "fn app(){}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &[".env*"])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"app.rs".to_string()));
    assert!(!f.iter().any(|p| p.starts_with(".env")));
}

#[test]
fn wd_copy_dot_git_always_excluded() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git/objects")).unwrap();
    fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("code.rs"), "fn main(){}").unwrap();

    // Even with a ** include, .git should be excluded from copy
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn wd_copy_hidden_files_included_by_default() {
    let src = tempdir().unwrap();
    fs::write(src.path().join(".eslintrc"), "{}").unwrap();
    fs::write(src.path().join(".prettierrc"), "{}").unwrap();
    fs::write(src.path().join("index.js"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(".eslintrc").exists());
    assert!(ws.path().join(".prettierrc").exists());
    assert!(ws.path().join("index.js").exists());
}

#[test]
fn wd_copy_large_file_integrity() {
    let src = tempdir().unwrap();
    // 2MB file
    let data = vec![0x42u8; 2 * 1024 * 1024];
    fs::write(src.path().join("large.bin"), &data).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let staged = fs::read(ws.path().join("large.bin")).unwrap();
    assert_eq!(staged.len(), data.len());
    assert_eq!(staged, data);
}

#[test]
fn wd_copy_empty_file() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("empty"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("empty").exists());
    assert_eq!(fs::read_to_string(ws.path().join("empty")).unwrap(), "");
}

#[test]
fn wd_copy_preserves_nested_structure() {
    let src = tempdir().unwrap();
    populate(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join("src/lib.rs").exists());
    assert!(ws.path().join("src/util.rs").exists());
    assert!(ws.path().join("tests/integration.rs").exists());
    assert!(ws.path().join("Cargo.toml").exists());
    assert!(ws.path().join("README.md").exists());
}

#[test]
fn wd_copy_source_untouched() {
    let src = tempdir().unwrap();
    populate(src.path());
    let before = files(src.path());
    let _ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(files(src.path()), before);
}

#[test]
fn wd_copy_mutation_isolated_from_source() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("data.txt"), "original").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "mutated").unwrap();
    assert_eq!(
        fs::read_to_string(src.path().join("data.txt")).unwrap(),
        "original"
    );
}

#[test]
fn wd_copy_file_count_matches() {
    let src = tempdir().unwrap();
    for i in 0..25 {
        fs::write(src.path().join(format!("f{i:03}.txt")), format!("{i}")).unwrap();
    }
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(files(ws.path()).len(), 25);
}

#[test]
fn wd_copy_staged_path_is_different() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn wd_copy_file_with_spaces_in_name() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("my document.txt"), "content with spaces").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("my document.txt")).unwrap(),
        "content with spaces"
    );
}

#[test]
fn wd_copy_file_without_extension() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Makefile"), "all: build").unwrap();
    fs::write(src.path().join("Dockerfile"), "FROM rust").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("Makefile").exists());
    assert!(ws.path().join("Dockerfile").exists());
}

// ===========================================================================
// 3. Git initialization (~10 tests)
// ===========================================================================

#[test]
fn wd_git_has_dot_git_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.rs"), "fn f(){}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").is_dir());
}

#[test]
fn wd_git_baseline_commit_exists() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.rs"), "fn f(){}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--oneline"]).unwrap();
    assert!(log.contains("baseline"));
}

#[test]
fn wd_git_exactly_one_commit() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.rs"), "fn f(){}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]).unwrap();
    assert_eq!(count.trim(), "1");
}

#[test]
fn wd_git_working_tree_clean() {
    let src = tempdir().unwrap();
    populate(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(
        status.trim().is_empty(),
        "expected clean tree, got: {status}"
    );
}

#[test]
fn wd_git_diff_empty_on_clean() {
    let src = tempdir().unwrap();
    populate(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.trim().is_empty(), "expected empty diff, got: {diff}");
}

#[test]
fn wd_git_all_staged_files_tracked() {
    let src = tempdir().unwrap();
    populate(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let tracked = git(ws.path(), &["ls-files"]).unwrap();
    for f in files(ws.path()) {
        let normalized = f.replace('\\', "/");
        assert!(tracked.contains(&normalized), "file {f} not tracked in git");
    }
}

#[test]
fn wd_git_modification_detected_in_diff() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("config.toml"), "key = \"old\"").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("config.toml"), "key = \"new\"").unwrap();
    let diff = WorkspaceManager::git_diff(ws.path()).unwrap();
    assert!(diff.contains("config.toml"));
    assert!(diff.contains("new"));
}

#[test]
fn wd_git_new_file_detected_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("existing.rs"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("brand_new.rs"), "fn new(){}").unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("brand_new.rs"));
    assert!(status.contains("??"), "new file should be untracked");
}

#[test]
fn wd_git_deletion_detected_in_status() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("doomed.txt"), "will be removed").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::remove_file(ws.path().join("doomed.txt")).unwrap();
    let status = WorkspaceManager::git_status(ws.path()).unwrap();
    assert!(status.contains("doomed.txt"));
    assert!(status.contains(" D "), "should show deletion marker");
}

#[test]
fn wd_git_author_is_abp() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let author = git(ws.path(), &["log", "--format=%an <%ae>"]).unwrap();
    assert!(
        author.contains("abp"),
        "author should contain 'abp', got: {author}"
    );
}

// ===========================================================================
// 4. Glob filtering (~15 tests)
// ===========================================================================

#[test]
fn wd_glob_include_rs_only() {
    let src = tempdir().unwrap();
    populate(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    for f in files(ws.path()) {
        assert!(f.ends_with(".rs"), "non-.rs file found: {f}");
    }
}

#[test]
fn wd_glob_exclude_target_dir() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("target/debug")).unwrap();
    fs::write(src.path().join("target/debug/binary"), "").unwrap();
    fs::write(src.path().join("src.rs"), "fn main(){}").unwrap();
    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["target/**"])).unwrap();
    let f = files(ws.path());
    assert!(!f.iter().any(|p| p.starts_with("target/")));
    assert!(f.contains(&"src.rs".to_string()));
}

#[test]
fn wd_glob_nested_include_pattern() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("crates/core/src")).unwrap();
    fs::create_dir_all(src.path().join("crates/util/src")).unwrap();
    fs::write(src.path().join("crates/core/src/lib.rs"), "").unwrap();
    fs::write(src.path().join("crates/util/src/lib.rs"), "").unwrap();
    fs::write(src.path().join("build.sh"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["crates/**/src/**"], &[]))
        .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"crates/core/src/lib.rs".to_string()));
    assert!(f.contains(&"crates/util/src/lib.rs".to_string()));
    assert!(!f.contains(&"build.sh".to_string()));
}

#[test]
fn wd_glob_multiple_include_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.toml"), "").unwrap();
    fs::write(src.path().join("c.md"), "").unwrap();
    fs::write(src.path().join("d.log"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs", "*.toml"], &[]))
        .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"a.rs".to_string()));
    assert!(f.contains(&"b.toml".to_string()));
    assert!(!f.contains(&"c.md".to_string()));
    assert!(!f.contains(&"d.log".to_string()));
}

#[test]
fn wd_glob_multiple_exclude_patterns() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "").unwrap();
    fs::write(src.path().join("debug.log"), "").unwrap();
    fs::write(src.path().join("cache.tmp"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["*.log", "*.tmp"]))
        .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"code.rs".to_string()));
    assert!(!f.contains(&"debug.log".to_string()));
    assert!(!f.contains(&"cache.tmp".to_string()));
}

#[test]
fn wd_glob_exclude_overrides_include() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("src/gen")).unwrap();
    fs::write(src.path().join("src/lib.rs"), "pub").unwrap();
    fs::write(src.path().join("src/gen/out.rs"), "gen").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["src/**"], &["**/gen/**"]))
        .unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"src/lib.rs".to_string()));
    assert!(!f.iter().any(|p| p.contains("gen/")));
}

#[test]
fn wd_glob_brace_expansion_include() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("app.rs"), "").unwrap();
    fs::write(src.path().join("app.ts"), "").unwrap();
    fs::write(src.path().join("app.py"), "").unwrap();
    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.{rs,ts}"], &[])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"app.rs".to_string()));
    assert!(f.contains(&"app.ts".to_string()));
    assert!(!f.contains(&"app.py".to_string()));
}

#[test]
fn wd_glob_question_mark_wildcard() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("v1.txt"), "").unwrap();
    fs::write(src.path().join("v2.txt"), "").unwrap();
    fs::write(src.path().join("va.txt"), "").unwrap();
    fs::write(src.path().join("v12.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["v?.txt"], &[])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"v1.txt".to_string()));
    assert!(f.contains(&"v2.txt".to_string()));
    assert!(f.contains(&"va.txt".to_string()));
    // v12.txt has two chars after v, ? matches exactly one
    assert!(!f.contains(&"v12.txt".to_string()));
}

#[test]
fn wd_glob_char_class_exclude() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("test1.rs"), "").unwrap();
    fs::write(src.path().join("test2.rs"), "").unwrap();
    fs::write(src.path().join("testX.rs"), "").unwrap();
    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["test[0-9].rs"])).unwrap();
    let f = files(ws.path());
    assert!(!f.contains(&"test1.rs".to_string()));
    assert!(!f.contains(&"test2.rs".to_string()));
    assert!(f.contains(&"testX.rs".to_string()));
}

#[test]
fn wd_glob_no_match_yields_empty() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("readme.md"), "# Hello").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs"], &[])).unwrap();
    assert!(files(ws.path()).is_empty());
}

#[test]
fn wd_glob_exclude_all_with_star() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["*"])).unwrap();
    assert!(files(ws.path()).is_empty());
}

#[test]
fn wd_glob_same_include_and_exclude_denies() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("code.rs"), "").unwrap();
    let ws =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["*.rs"], &["*.rs"])).unwrap();
    assert!(files(ws.path()).is_empty());
}

#[test]
fn wd_glob_double_star_catches_all_depths() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("a/b/c")).unwrap();
    fs::write(src.path().join("top.rs"), "").unwrap();
    fs::write(src.path().join("a/mid.rs"), "").unwrap();
    fs::write(src.path().join("a/b/c/deep.rs"), "").unwrap();
    fs::write(src.path().join("a/b/c/skip.txt"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["**/*.rs"], &[])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"top.rs".to_string()));
    assert!(f.contains(&"a/mid.rs".to_string()));
    assert!(f.contains(&"a/b/c/deep.rs".to_string()));
    assert!(!f.contains(&"a/b/c/skip.txt".to_string()));
}

#[test]
fn wd_glob_include_specific_filenames() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("Cargo.toml"), "").unwrap();
    fs::write(src.path().join("Cargo.lock"), "").unwrap();
    fs::write(src.path().join("README.md"), "").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["Cargo.*"], &[])).unwrap();
    let f = files(ws.path());
    assert!(f.contains(&"Cargo.toml".to_string()));
    assert!(f.contains(&"Cargo.lock".to_string()));
    assert!(!f.contains(&"README.md".to_string()));
}

// ===========================================================================
// 5. Error handling (~10 tests)
// ===========================================================================

#[test]
fn wd_error_nonexistent_source_stager() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/xyzzy_438")
        .stage();
    assert!(result.is_err());
}

#[test]
fn wd_error_no_source_root_stager() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("source_root"),
        "error should mention source_root: {err}"
    );
}

#[test]
fn wd_error_invalid_include_glob() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let result = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["["], &[]));
    assert!(result.is_err());
}

#[test]
fn wd_error_invalid_exclude_glob() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let result = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &[], &["[bad"]));
    assert!(result.is_err());
}

#[test]
fn wd_error_invalid_both_globs() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let result = WorkspaceManager::prepare(&staged_spec_globs(src.path(), &["["], &["["]));
    assert!(result.is_err());
}

#[test]
fn wd_error_stager_invalid_glob_pattern() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let result = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["[unclosed".into()])
        .stage();
    assert!(result.is_err());
}

#[test]
fn wd_empty_source_stages_successfully() {
    let src = tempdir().unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(files(ws.path()).is_empty());
}

#[test]
fn wd_empty_source_with_git_init_succeeds() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(files(ws.path()).is_empty());
    // .git should exist even for empty workspace
    assert!(ws.path().join(".git").is_dir());
}

#[test]
fn wd_source_with_only_dirs_no_files() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_a")).unwrap();
    fs::create_dir_all(src.path().join("empty_b/nested")).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(files(ws.path()).is_empty());
    // Dirs should exist though
    assert!(ws.path().join("empty_a").is_dir());
}

#[test]
fn wd_error_stager_nonexistent_path_message() {
    let result = WorkspaceStager::new()
        .source_root("/definitely/not/a/real/path")
        .stage();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("does not exist"),
        "error should mention non-existence: {err}"
    );
}

// ===========================================================================
// 6. Cleanup (~10 tests)
// ===========================================================================

#[test]
fn wd_cleanup_on_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let path = ws.path().to_path_buf();
    assert!(path.exists());
    drop(ws);
    assert!(!path.exists(), "staged dir should be removed on drop");
}

#[test]
fn wd_cleanup_staged_with_git() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.rs"), "fn f(){}").unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let path = ws.path().to_path_buf();
    assert!(path.exists());
    drop(ws);
    assert!(
        !path.exists(),
        "staged dir with git should be removed on drop"
    );
}

#[test]
fn wd_cleanup_passthrough_does_not_delete() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f.txt"), "keep me").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    let path = ws.path().to_path_buf();
    drop(ws);
    assert!(path.exists(), "passthrough should not delete original dir");
    assert!(path.join("f.txt").exists());
}

#[test]
fn wd_cleanup_multiple_independent() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
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
    assert_ne!(p1, p2);

    drop(ws1);
    assert!(!p1.exists(), "ws1 should be cleaned up");
    assert!(p2.exists(), "ws2 should survive ws1 drop");

    drop(ws2);
    assert!(!p2.exists(), "ws2 should be cleaned up");
}

#[test]
fn wd_cleanup_workspace_path_is_valid_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("f"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().is_dir());
    assert!(ws.path().is_absolute());
}

#[test]
fn wd_cleanup_workspace_path_accessor() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("test.txt"), "hello").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // The path() method should return a usable path
    let content = fs::read_to_string(ws.path().join("test.txt")).unwrap();
    assert_eq!(content, "hello");
}

#[test]
fn wd_cleanup_workspace_writable_before_drop() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("original.txt"), "orig").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Should be able to write new files
    fs::write(ws.path().join("new.txt"), "new content").unwrap();
    assert!(ws.path().join("new.txt").exists());
    // Should be able to modify existing files
    fs::write(ws.path().join("original.txt"), "modified").unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("original.txt")).unwrap(),
        "modified"
    );
}

#[test]
fn wd_cleanup_three_concurrent_workspaces() {
    let src = tempdir().unwrap();
    populate(src.path());
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws3 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // All paths should be unique
    assert_ne!(ws1.path(), ws2.path());
    assert_ne!(ws2.path(), ws3.path());
    assert_ne!(ws1.path(), ws3.path());

    // All should have same content
    assert_eq!(files(ws1.path()), files(ws2.path()));
    assert_eq!(files(ws2.path()), files(ws3.path()));
}

#[test]
fn wd_cleanup_threaded_staging() {
    let src = tempdir().unwrap();
    populate(src.path());
    let src_path = src.path().to_path_buf();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let p = src_path.clone();
            std::thread::spawn(move || -> PreparedWorkspace {
                WorkspaceStager::new()
                    .source_root(&p)
                    .with_git_init(false)
                    .stage()
                    .unwrap()
            })
        })
        .collect();

    let workspaces: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // All unique
    for i in 0..workspaces.len() {
        for j in (i + 1)..workspaces.len() {
            assert_ne!(workspaces[i].path(), workspaces[j].path());
        }
    }

    // All same files
    let baseline = files(workspaces[0].path());
    for ws in &workspaces[1..] {
        assert_eq!(files(ws.path()), baseline);
    }
}

#[test]
fn wd_cleanup_mutations_isolated() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("shared.txt"), "original").unwrap();
    let ws_a = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws_b = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Mutate ws_a
    fs::write(ws_a.path().join("shared.txt"), "mutated_a").unwrap();

    // ws_b should be unaffected
    assert_eq!(
        fs::read_to_string(ws_b.path().join("shared.txt")).unwrap(),
        "original"
    );
    // source should be unaffected
    assert_eq!(
        fs::read_to_string(src.path().join("shared.txt")).unwrap(),
        "original"
    );
}
