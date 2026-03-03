// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive workspace staging tests (60+).
//!
//! Categories:
//! 1. Basic staging (15 tests): stage from directory, auto-git-init, baseline
//!    commit, cleanup on drop
//! 2. Glob filtering (15 tests): include/exclude patterns, dotfile handling,
//!    nested dirs
//! 3. WorkspaceSpec (10 tests): all fields, serde roundtrip, defaults
//! 4. Workspace modes (10 tests): Staged vs PassThrough, mode-specific behavior
//! 5. Edge cases (10+ tests): empty directory, missing source, long filenames,
//!    special characters, concurrent staging

use abp_core::{WorkspaceMode, WorkspaceSpec};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
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

fn staged_spec_globs(root: &Path, include: Vec<String>, exclude: Vec<String>) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
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

/// Collect sorted relative file paths (excluding `.git`) under `root`.
fn collect_files(root: &Path) -> Vec<String> {
    let mut files: Vec<String> = WalkDir::new(root)
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
    files.sort();
    files
}

/// Run a git command in `dir` and return trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Create a standard fixture tree with mixed file types.
fn create_fixture(root: &Path) {
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(root.join("README.md"), "# Hello").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("utils.rs"), "pub fn util() {}").unwrap();
    fs::write(root.join("src").join("data.json"), "{}").unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests").join("test_one.rs"), "#[test] fn t() {}").unwrap();
}

/// Helper: create source dir with given files and return TempDir.
fn make_source(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = tempdir().unwrap();
    for (rel, content) in files {
        let p = tmp.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
    }
    tmp
}

// ===========================================================================
// 1. Basic staging (15 tests)
// ===========================================================================

#[test]
fn basic_staging_copies_all_files() {
    let src = tempdir().unwrap();
    create_fixture(src.path());

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_eq!(
        collect_files(ws.path()),
        collect_files(src.path()),
        "staged workspace must mirror source"
    );
}

#[test]
fn basic_staging_path_differs_from_source() {
    let src = make_source(&[("a.txt", "a")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn basic_staging_preserves_file_content() {
    let body = "fn main() { println!(\"hello world\"); }";
    let src = make_source(&[("main.rs", body)]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read_to_string(ws.path().join("main.rs")).unwrap(), body);
}

#[test]
fn basic_staging_preserves_binary_content() {
    let src = tempdir().unwrap();
    let data: Vec<u8> = (0..=255).collect();
    fs::write(src.path().join("bin.dat"), &data).unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(fs::read(ws.path().join("bin.dat")).unwrap(), data);
}

#[test]
fn basic_staging_does_not_modify_source() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let before = collect_files(src.path());
    let content = fs::read_to_string(src.path().join("main.rs")).unwrap();

    let _ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert_eq!(collect_files(src.path()), before);
    assert_eq!(
        fs::read_to_string(src.path().join("main.rs")).unwrap(),
        content
    );
}

#[test]
fn basic_staging_auto_creates_git_repo() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn basic_staging_baseline_commit_message() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let log = git(ws.path(), &["log", "--format=%s"]);
    assert!(
        log.contains("baseline"),
        "expected 'baseline' commit, got: {log}"
    );
}

#[test]
fn basic_staging_exactly_one_commit() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "1");
}

#[test]
fn basic_staging_baseline_commit_includes_all_files() {
    let src = make_source(&[("a.txt", "a"), ("b.txt", "b")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = git(ws.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(files.contains("a.txt"));
    assert!(files.contains("b.txt"));
}

#[test]
fn basic_staging_clean_working_tree() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let status = git(ws.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty(), "expected clean tree, got: {status}");
}

#[test]
fn basic_staging_cleanup_on_drop() {
    let src = make_source(&[("f.txt", "data")]);
    let staged_path;
    {
        let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists(), "workspace should exist before drop");
    }
    assert!(
        !staged_path.exists(),
        "staged directory should be removed after drop"
    );
}

#[test]
fn basic_staging_stager_cleanup_on_drop() {
    let src = make_source(&[("f.txt", "data")]);
    let staged_path;
    {
        let ws = WorkspaceStager::new()
            .source_root(src.path())
            .with_git_init(false)
            .stage()
            .unwrap();
        staged_path = ws.path().to_path_buf();
        assert!(staged_path.exists());
    }
    assert!(
        !staged_path.exists(),
        "stager workspace should be removed after drop"
    );
}

#[test]
fn basic_staging_nested_directory_preserved() {
    let src = make_source(&[("root.txt", "r"), ("d1/f1.txt", "1"), ("d1/d2/f2.txt", "2")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_eq!(
        collect_files(ws.path()),
        vec!["d1/d2/f2.txt", "d1/f1.txt", "root.txt"]
    );
}

#[test]
fn basic_staging_modified_file_produces_diff() {
    let src = make_source(&[("data.txt", "original content")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("data.txt"), "modified content").unwrap();

    let diff = WorkspaceManager::git_diff(ws.path()).expect("diff should succeed");
    assert!(diff.contains("data.txt"));
    assert!(diff.contains("modified content"));
    assert!(diff.contains("original content"));
}

#[test]
fn basic_staging_new_file_shows_in_status() {
    let src = make_source(&[("existing.txt", "hi")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("brand_new.txt"), "I am new").unwrap();

    let status = WorkspaceManager::git_status(ws.path()).expect("status should succeed");
    assert!(status.contains("brand_new.txt"));
    assert!(status.contains("??"), "new file should be untracked");
}

// ===========================================================================
// 2. Glob filtering (15 tests)
// ===========================================================================

#[test]
fn glob_include_only_rs() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    for f in &files {
        assert!(f.ends_with(".rs"), "unexpected non-.rs file: {f}");
    }
    assert!(
        files.iter().any(|f| f.contains('/')),
        "should include nested .rs files"
    );
}

#[test]
fn glob_exclude_md() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec!["*.md".into()]))
        .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.ends_with(".md")));
    assert!(!files.is_empty());
}

#[test]
fn glob_include_and_exclude_interact() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec!["tests/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("tests/")));
    assert!(files.iter().any(|f| f.ends_with(".rs")));
}

#[test]
fn glob_exclude_specific_subdirectory() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("vendor")).unwrap();
    fs::write(src.path().join("vendor").join("dep.rs"), "fn dep() {}").unwrap();
    fs::write(src.path().join("root.rs"), "fn root() {}").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["vendor/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("vendor/")));
    assert!(files.contains(&"root.rs".to_string()));
}

#[test]
fn glob_multiple_include_patterns() {
    let src = make_source(&[
        ("code.rs", "fn f() {}"),
        ("config.toml", "[pkg]"),
        ("notes.md", "# Notes"),
    ]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["*.rs".into(), "*.toml".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"code.rs".to_string()));
    assert!(files.contains(&"config.toml".to_string()));
    assert!(!files.contains(&"notes.md".to_string()));
}

#[test]
fn glob_multiple_exclude_patterns() {
    let src = make_source(&[("keep.rs", "x"), ("drop.log", "y"), ("drop.tmp", "z")]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec![],
        vec!["*.log".into(), "*.tmp".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files, vec!["keep.rs"]);
}

#[test]
fn glob_exclude_overrides_include() {
    let src = make_source(&[
        ("src/lib.rs", "pub fn a() {}"),
        ("src/generated/out.rs", "// gen"),
        ("tests/t.rs", "#[test] fn t() {}"),
    ]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into(), "tests/**".into()],
        vec!["src/generated/**".into(), "tests/fixtures/**".into()],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.iter().any(|f| f.starts_with("src/generated/")));
}

#[test]
fn glob_dotfile_handling() {
    let src = make_source(&[
        (".hidden", "secret"),
        (".config/app.toml", "[app]"),
        ("visible.txt", "hi"),
    ]);
    // Dotfiles are NOT excluded by default — only .git is hardcoded.
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    assert!(ws.path().join(".hidden").exists());
    assert!(ws.path().join(".config").join("app.toml").exists());
    assert!(ws.path().join("visible.txt").exists());
}

#[test]
fn glob_exclude_dotfiles_explicitly() {
    let src = make_source(&[
        (".hidden", "secret"),
        (".env", "KEY=VAL"),
        ("visible.txt", "hi"),
    ]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec![".*".into()]))
        .unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with('.')));
    assert!(files.contains(&"visible.txt".to_string()));
}

#[test]
fn glob_nested_directory_include() {
    let src = make_source(&[
        ("src/a/b/c/deep.rs", "fn deep() {}"),
        ("src/top.rs", "fn top() {}"),
        ("docs/guide.md", "# Guide"),
    ]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["src/**".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/a/b/c/deep.rs".to_string()));
    assert!(files.contains(&"src/top.rs".to_string()));
    assert!(!files.iter().any(|f| f.starts_with("docs/")));
}

#[test]
fn glob_empty_patterns_copies_everything() {
    let src = tempdir().unwrap();
    create_fixture(src.path());
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec![])).unwrap();
    assert_eq!(collect_files(ws.path()), collect_files(src.path()));
}

#[test]
fn glob_dot_git_always_excluded_even_with_star_star() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join(".git")).unwrap();
    fs::write(src.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();

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
fn glob_source_dot_git_never_copied() {
    let src = tempdir().unwrap();
    let fake_git = src.path().join(".git");
    fs::create_dir_all(fake_git.join("objects")).unwrap();
    fs::write(fake_git.join("HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(fake_git.join("sentinel"), "MUST_NOT_COPY").unwrap();
    fs::write(src.path().join("code.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws.path().join(".git").exists(),
        "source .git must never be copied"
    );
    assert!(ws.path().join("code.rs").exists());
}

#[test]
fn glob_include_pattern_matching_no_files() {
    let src = make_source(&[("readme.md", "hello"), ("data.json", "{}")]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(
        src.path(),
        vec!["**/*.rs".into()],
        vec![],
    ))
    .unwrap();

    let files = collect_files(ws.path());
    assert!(files.is_empty(), "no .rs files exist, should be empty");
}

#[test]
fn glob_exclude_everything_yields_empty_workspace() {
    let src = make_source(&[("a.txt", "a"), ("b.rs", "fn b() {}")]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec!["*".into()]))
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.is_empty());
}

// ===========================================================================
// 3. WorkspaceSpec (10 tests)
// ===========================================================================

#[test]
fn workspace_spec_all_fields() {
    let spec = WorkspaceSpec {
        root: "/tmp/project".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["*.log".into()],
    };
    assert_eq!(spec.root, "/tmp/project");
    assert!(matches!(spec.mode, WorkspaceMode::Staged));
    assert_eq!(spec.include, vec!["src/**".to_string()]);
    assert_eq!(spec.exclude, vec!["*.log".to_string()]);
}

#[test]
fn workspace_spec_serde_roundtrip_staged() {
    let spec = WorkspaceSpec {
        root: "/workspace".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into(), "tests/**".into()],
        exclude: vec!["*.tmp".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.root, spec.root);
    assert_eq!(back.include, spec.include);
    assert_eq!(back.exclude, spec.exclude);
    assert!(matches!(back.mode, WorkspaceMode::Staged));
}

#[test]
fn workspace_spec_serde_roundtrip_passthrough() {
    let spec = WorkspaceSpec {
        root: "/project".to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.mode, WorkspaceMode::PassThrough));
    assert!(back.include.is_empty());
    assert!(back.exclude.is_empty());
}

#[test]
fn workspace_spec_empty_include_exclude() {
    let spec = WorkspaceSpec {
        root: ".".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    assert!(spec.include.is_empty());
    assert!(spec.exclude.is_empty());
}

#[test]
fn workspace_spec_root_path_preserved() {
    let paths = vec![
        "/absolute/path",
        "relative/path",
        "./dot-relative",
        "C:\\windows\\path",
    ];
    for p in paths {
        let spec = WorkspaceSpec {
            root: p.to_string(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        assert_eq!(spec.root, p, "root path should be preserved verbatim");
    }
}

#[test]
fn workspace_spec_include_patterns_preserved_in_serde() {
    let spec = WorkspaceSpec {
        root: "/r".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![
            "*.rs".into(),
            "src/**/*.rs".into(),
            "tests/integration/**".into(),
        ],
        exclude: vec![],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.include.len(), 3);
    assert_eq!(back.include[0], "*.rs");
    assert_eq!(back.include[1], "src/**/*.rs");
    assert_eq!(back.include[2], "tests/integration/**");
}

#[test]
fn workspace_spec_exclude_patterns_preserved_in_serde() {
    let spec = WorkspaceSpec {
        root: "/r".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["target/**".into(), "*.log".into(), ".env".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.exclude.len(), 3);
    assert!(back.exclude.contains(&"target/**".to_string()));
    assert!(back.exclude.contains(&"*.log".to_string()));
    assert!(back.exclude.contains(&".env".to_string()));
}

#[test]
fn workspace_spec_clone() {
    let spec = WorkspaceSpec {
        root: "/project".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["*.tmp".into()],
    };
    let cloned = spec.clone();
    assert_eq!(cloned.root, spec.root);
    assert_eq!(cloned.include, spec.include);
    assert_eq!(cloned.exclude, spec.exclude);
}

#[test]
fn workspace_spec_serde_mode_rename() {
    // WorkspaceMode uses rename_all = "snake_case"
    let json_staged = r#"{"root":"/r","mode":"staged","include":[],"exclude":[]}"#;
    let spec: WorkspaceSpec = serde_json::from_str(json_staged).unwrap();
    assert!(matches!(spec.mode, WorkspaceMode::Staged));

    let json_pass = r#"{"root":"/r","mode":"pass_through","include":[],"exclude":[]}"#;
    let spec: WorkspaceSpec = serde_json::from_str(json_pass).unwrap();
    assert!(matches!(spec.mode, WorkspaceMode::PassThrough));
}

#[test]
fn workspace_spec_debug_impl() {
    let spec = WorkspaceSpec {
        root: "/project".to_string(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec![],
    };
    let debug = format!("{spec:?}");
    assert!(debug.contains("WorkspaceSpec"));
    assert!(debug.contains("/project"));
}

// ===========================================================================
// 4. Workspace modes (10 tests)
// ===========================================================================

#[test]
fn mode_passthrough_returns_original_path() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert_eq!(ws.path(), src.path());
}

#[test]
fn mode_passthrough_does_not_create_temp_dir() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    // Path should be exactly the source, no temp dir involved.
    assert_eq!(ws.path().to_string_lossy(), src.path().to_string_lossy());
}

#[test]
fn mode_passthrough_does_not_create_git_repo() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "a").unwrap();
    // Source has no .git, passthrough should not create one.
    let ws = WorkspaceManager::prepare(&passthrough_spec(src.path())).unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn mode_staged_creates_copy_in_temp() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert_ne!(ws.path(), src.path());
    assert!(ws.path().join("f.txt").exists());
}

#[test]
fn mode_staged_workspace_is_writable() {
    let src = make_source(&[("f.txt", "original")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "overwritten").unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("f.txt")).unwrap(),
        "overwritten"
    );
}

#[test]
fn mode_staged_modifications_do_not_affect_source() {
    let src = make_source(&[("f.txt", "original")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    fs::write(ws.path().join("f.txt"), "changed").unwrap();
    fs::write(ws.path().join("new.txt"), "new").unwrap();

    assert_eq!(
        fs::read_to_string(src.path().join("f.txt")).unwrap(),
        "original"
    );
    assert!(!src.path().join("new.txt").exists());
}

#[test]
fn mode_staged_creates_git_repo() {
    let src = make_source(&[("f.txt", "data")]);
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn mode_staged_with_globs_filters_files() {
    let src = make_source(&[("keep.rs", "fn f() {}"), ("skip.md", "# Skip")]);
    let ws = WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec!["*.rs".into()], vec![]))
        .unwrap();

    assert!(ws.path().join("keep.rs").exists());
    assert!(!ws.path().join("skip.md").exists());
}

#[test]
fn mode_workspace_mode_serde_roundtrip() {
    let modes = vec![WorkspaceMode::Staged, WorkspaceMode::PassThrough];
    for mode in modes {
        let json = serde_json::to_string(&mode).unwrap();
        let back: WorkspaceMode = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{mode:?}"), format!("{back:?}"));
    }
}

#[test]
fn mode_stager_builder_with_git_init_toggle() {
    let src = make_source(&[("f.txt", "data")]);

    let ws_with = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();
    assert!(ws_with.path().join(".git").exists());

    let ws_without = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws_without.path().join(".git").exists());
}

// ===========================================================================
// 5. Edge cases (10+ tests)
// ===========================================================================

#[test]
fn edge_empty_directory_stages_successfully() {
    let src = tempdir().unwrap();
    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let files = collect_files(ws.path());
    assert!(files.is_empty());
    assert!(ws.path().join(".git").exists());
}

#[test]
fn edge_missing_source_directory_errors() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist/anywhere")
        .stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("does not exist"),
        "error should mention nonexistence: {msg}"
    );
}

#[test]
fn edge_no_source_root_set_errors() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("source_root"),
        "error should mention source_root: {msg}"
    );
}

#[test]
fn edge_very_long_filename() {
    let src = tempdir().unwrap();
    // 200-char filename (within most filesystem limits)
    let long_name = "a".repeat(200) + ".txt";
    fs::write(src.path().join(&long_name), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join(&long_name).exists());
    assert_eq!(
        fs::read_to_string(ws.path().join(&long_name)).unwrap(),
        "content"
    );
}

#[test]
fn edge_special_characters_in_filename() {
    let src = make_source(&[
        ("hello world.txt", "spaces"),
        ("file (1).txt", "parens"),
        ("data-v2.0.txt", "dashes"),
        ("config_backup.txt", "underscores"),
    ]);

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert_eq!(
        fs::read_to_string(ws.path().join("hello world.txt")).unwrap(),
        "spaces"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("file (1).txt")).unwrap(),
        "parens"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("data-v2.0.txt")).unwrap(),
        "dashes"
    );
}

#[test]
fn edge_unicode_filenames() {
    let src = make_source(&[("données/résumé.txt", "contenu")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("données").join("résumé.txt")).unwrap(),
        "contenu"
    );
}

#[test]
fn edge_concurrent_staging_from_same_source() {
    let src = make_source(&[("shared.txt", "original")]);
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws2 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();
    let ws3 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // All workspaces are at different paths.
    assert_ne!(ws1.path(), ws2.path());
    assert_ne!(ws2.path(), ws3.path());
    assert_ne!(ws1.path(), ws3.path());

    // Mutating one does not affect the others.
    fs::write(ws1.path().join("shared.txt"), "ws1").unwrap();
    fs::write(ws2.path().join("shared.txt"), "ws2").unwrap();

    assert_eq!(
        fs::read_to_string(ws1.path().join("shared.txt")).unwrap(),
        "ws1"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("shared.txt")).unwrap(),
        "ws2"
    );
    assert_eq!(
        fs::read_to_string(ws3.path().join("shared.txt")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(src.path().join("shared.txt")).unwrap(),
        "original"
    );
}

#[test]
fn edge_restage_from_already_staged_workspace() {
    let src = make_source(&[("original.txt", "v1")]);
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // Mutate the first staged workspace.
    fs::write(ws1.path().join("original.txt"), "v2").unwrap();
    fs::write(ws1.path().join("added.txt"), "new in ws1").unwrap();

    // Re-stage from ws1.
    let ws2 = WorkspaceManager::prepare(&staged_spec(ws1.path())).unwrap();

    assert_ne!(ws1.path(), ws2.path());
    assert_eq!(
        fs::read_to_string(ws2.path().join("original.txt")).unwrap(),
        "v2"
    );
    assert_eq!(
        fs::read_to_string(ws2.path().join("added.txt")).unwrap(),
        "new in ws1"
    );
    // ws2 gets its own clean baseline
    let status = git(ws2.path(), &["status", "--porcelain=v1"]);
    assert!(status.is_empty(), "re-staged workspace should be clean");
}

#[test]
fn edge_deeply_nested_directories() {
    let src = tempdir().unwrap();
    let depth = 15;
    let mut deep = src.path().to_path_buf();
    for i in 0..depth {
        deep = deep.join(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), "bottom").unwrap();

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let mut expected = ws.path().to_path_buf();
    for i in 0..depth {
        expected = expected.join(format!("d{i}"));
    }
    assert!(expected.join("leaf.txt").exists());
    assert_eq!(
        fs::read_to_string(expected.join("leaf.txt")).unwrap(),
        "bottom"
    );
}

#[test]
fn edge_empty_subdirectories_preserved() {
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("empty_child")).unwrap();
    fs::write(src.path().join("root.txt"), "hi").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("empty_child").exists());
    assert!(ws.path().join("root.txt").exists());
}

#[test]
fn edge_invalid_include_glob_errors() {
    let src = make_source(&[("f.txt", "x")]);
    let result =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec!["[".into()], vec![]));
    assert!(result.is_err());
}

#[test]
fn edge_invalid_exclude_glob_errors() {
    let src = make_source(&[("f.txt", "x")]);
    let result =
        WorkspaceManager::prepare(&staged_spec_globs(src.path(), vec![], vec!["[".into()]));
    assert!(result.is_err());
}

#[test]
fn edge_restage_does_not_copy_dot_git() {
    let src = make_source(&[("code.rs", "fn main() {}")]);
    let ws1 = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    // ws1 has .git; re-stage without git init to verify it's not copied.
    let ws2 = WorkspaceStager::new()
        .source_root(ws1.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws2.path().join(".git").exists(),
        ".git from first stage must not be copied into second stage"
    );
    assert!(ws2.path().join("code.rs").exists());
}

#[test]
fn edge_stager_default_equals_new() {
    let s = WorkspaceStager::default();
    // Default is same as new() — no source_root set, so stage should fail.
    assert!(s.stage().is_err());
}

#[test]
fn edge_symlinks_handled_without_error() {
    let src = tempdir().unwrap();
    fs::write(src.path().join("real.txt"), "real").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src.path().join("real.txt"), src.path().join("link.txt"))
            .unwrap();
    }
    #[cfg(windows)]
    {
        let _ = std::os::windows::fs::symlink_file(
            src.path().join("real.txt"),
            src.path().join("link.txt"),
        );
    }

    // Staging must succeed regardless of symlink presence.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("real.txt").exists());
}

#[test]
fn edge_large_number_of_files() {
    let src = tempdir().unwrap();
    let count = 200;
    for i in 0..count {
        fs::write(
            src.path().join(format!("file_{i:04}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let ws = WorkspaceManager::prepare(&staged_spec(src.path())).unwrap();

    let files = collect_files(ws.path());
    assert_eq!(files.len(), count, "all {count} files must be staged");
    assert_eq!(
        fs::read_to_string(ws.path().join("file_0000.txt")).unwrap(),
        "content 0"
    );
    assert_eq!(
        fs::read_to_string(ws.path().join(format!("file_{:04}.txt", count - 1))).unwrap(),
        format!("content {}", count - 1)
    );
}

#[test]
fn edge_stager_builder_chaining() {
    let src = make_source(&[("src/lib.rs", "pub fn x() {}"), ("target/out.o", "bin")]);
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["*.o".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.iter().any(|f| f.ends_with(".o")));
}
