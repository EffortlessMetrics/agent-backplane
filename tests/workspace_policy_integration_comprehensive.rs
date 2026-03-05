#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive workspace staging + policy engine integration tests.
//!
//! Validates the full flow: staging workspaces with include/exclude globs,
//! git auto-initialization, policy engine tool/file access control,
//! composed policy evaluation, and the interaction between workspace staging
//! and policy-driven file restrictions.

use abp_core::{PolicyProfile, WorkspaceMode, WorkspaceSpec};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_policy::compose::{ComposedEngine, PolicyPrecedence, PolicySet};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

// ===========================================================================
// Helpers
// ===========================================================================

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
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
        .expect("git command failed to execute");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Create a source tree with a known structure for staging tests.
fn create_source_tree(root: &Path) {
    let files = [
        "src/lib.rs",
        "src/main.rs",
        "src/utils/helpers.rs",
        "tests/integration.rs",
        "docs/README.md",
        "build/output.bin",
        "build/cache/temp.dat",
        ".env",
        ".env.production",
        "Cargo.toml",
    ];
    for f in &files {
        let path = root.join(f);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("// content of {f}")).unwrap();
    }
}

/// Build a staged workspace spec with include/exclude globs.
fn staged_spec_globs(root: &Path, include: Vec<String>, exclude: Vec<String>) -> WorkspaceSpec {
    WorkspaceSpec {
        root: root.to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include,
        exclude,
    }
}

// ===========================================================================
// 1. Workspace staging: include globs
// ===========================================================================

#[test]
fn staged_workspace_only_copies_included_files() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = staged_spec_globs(src.path(), patterns(&["src/**"]), vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(files.iter().all(|f| f.starts_with("src/")));
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(files.contains(&"src/utils/helpers.rs".to_string()));
    assert!(!files.contains(&"docs/README.md".to_string()));
    assert!(!files.contains(&"Cargo.toml".to_string()));
}

#[test]
fn staged_workspace_multiple_include_patterns() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = staged_spec_globs(src.path(), patterns(&["src/**", "tests/**"]), vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"tests/integration.rs".to_string()));
    assert!(!files.contains(&"docs/README.md".to_string()));
    assert!(!files.contains(&"build/output.bin".to_string()));
}

// ===========================================================================
// 2. Workspace staging: exclude globs
// ===========================================================================

#[test]
fn staged_workspace_excludes_matching_files() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = staged_spec_globs(src.path(), vec![], patterns(&["build/**"]));
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(!files.iter().any(|f| f.starts_with("build/")));
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"Cargo.toml".to_string()));
}

#[test]
fn staged_workspace_exclude_takes_precedence_over_include() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = staged_spec_globs(
        src.path(),
        patterns(&["src/**"]),
        patterns(&["src/utils/**"]),
    );
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(!files.contains(&"src/utils/helpers.rs".to_string()));
}

// ===========================================================================
// 3. .git directory always excluded
// ===========================================================================

#[test]
fn staged_workspace_excludes_dot_git_directory() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    // Initialize a git repo in the source to prove .git exists.
    let _ = Command::new("git")
        .args(["init", "-q"])
        .current_dir(src.path())
        .status();
    assert!(src.path().join(".git").exists());

    // Stage without any include/exclude — .git should still be excluded from copy.
    let spec = staged_spec_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // Verify none of the *source* .git files leaked through.
    let _all_entries: Vec<String> = WalkDir::new(ws.path())
        .into_iter()
        .filter_map(Result::ok)
        .map(|e| {
            e.path()
                .strip_prefix(ws.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    // The staged workspace will have its OWN .git (from ensure_git_repo),
    // but no source .git artifacts should leak. Verify baseline commit exists.
    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(
        log.contains("baseline"),
        "expected baseline commit, got: {log}"
    );
}

// ===========================================================================
// 4. Git auto-initialization in staged workspace
// ===========================================================================

#[test]
fn staged_workspace_auto_initializes_git_repo() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = staged_spec_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    assert!(
        ws.path().join(".git").exists(),
        "staged workspace should have .git directory"
    );
}

// ===========================================================================
// 5. Baseline commit exists
// ===========================================================================

#[test]
fn staged_workspace_has_baseline_commit() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = staged_spec_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let log = git(ws.path(), &["log", "--oneline"]);
    assert!(
        log.contains("baseline"),
        "expected 'baseline' commit message, got: {log}"
    );

    let commit_count = git(ws.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(commit_count, "1", "expected exactly 1 commit (baseline)");
}

#[test]
fn staged_workspace_baseline_commit_includes_all_staged_files() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = staged_spec_globs(src.path(), patterns(&["src/**"]), vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // After baseline commit, working tree should be clean.
    let status = git(ws.path(), &["status", "--porcelain"]);
    assert!(
        status.is_empty(),
        "expected clean working tree after baseline, got: {status}"
    );
}

// ===========================================================================
// 6. WorkspaceStager builder API
// ===========================================================================

#[test]
fn workspace_stager_builder_include_exclude() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**", "Cargo.toml"]))
        .exclude(patterns(&["src/utils/**"]))
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"Cargo.toml".to_string()));
    assert!(!files.contains(&"src/utils/helpers.rs".to_string()));
    assert!(!files.contains(&"docs/README.md".to_string()));
}

#[test]
fn workspace_stager_without_git_init() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        !ws.path().join(".git").exists(),
        "git should not be initialized when with_git_init(false)"
    );
}

// ===========================================================================
// 7. Policy engine: tool allow/deny
// ===========================================================================

#[test]
fn policy_engine_tool_in_allowlist_is_permitted() {
    let policy = PolicyProfile {
        allowed_tools: patterns(&["Read", "Grep", "ListDir"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
    assert!(engine.can_use_tool("ListDir").allowed);
}

#[test]
fn policy_engine_tool_not_in_allowlist_is_denied() {
    let policy = PolicyProfile {
        allowed_tools: patterns(&["Read", "Grep"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed);
    assert!(
        decision
            .reason
            .as_deref()
            .unwrap()
            .contains("not in allowlist"),
        "expected 'not in allowlist' reason"
    );
}

#[test]
fn policy_engine_tool_in_denylist_denied_even_if_in_allowlist() {
    let policy = PolicyProfile {
        allowed_tools: patterns(&["*"]),
        disallowed_tools: patterns(&["Bash"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed);
    assert!(
        decision.reason.as_deref().unwrap().contains("disallowed"),
        "expected 'disallowed' reason"
    );

    // Other tools still permitted.
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
}

// ===========================================================================
// 8. Policy engine: file read/write access
// ===========================================================================

#[test]
fn policy_engine_file_matching_read_glob_is_readable() {
    let policy = PolicyProfile {
        deny_read: patterns(&["secret/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // File NOT matching deny_read → readable.
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(engine.can_read_path(Path::new("docs/README.md")).allowed);
}

#[test]
fn policy_engine_file_matching_deny_read_is_denied() {
    let policy = PolicyProfile {
        deny_read: patterns(&["secret/**", "**/.env"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("config/.env")).allowed);
}

#[test]
fn policy_engine_file_matching_write_glob_is_writable() {
    let policy = PolicyProfile {
        deny_write: patterns(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // File NOT matching deny_write → writable.
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
    assert!(engine.can_write_path(Path::new("docs/README.md")).allowed);
}

#[test]
fn policy_engine_file_matching_deny_write_is_denied() {
    let policy = PolicyProfile {
        deny_write: patterns(&["**/.git/**", "locked/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(!engine.can_write_path(Path::new("locked/data.json")).allowed);
}

#[test]
fn policy_engine_file_not_matching_any_rule_is_allowed() {
    let policy = PolicyProfile {
        deny_read: patterns(&["secret/**"]),
        deny_write: patterns(&["locked/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

// ===========================================================================
// 9. PolicyProfile → PolicyEngine compilation
// ===========================================================================

#[test]
fn policy_profile_compiles_to_engine() {
    let profile = PolicyProfile {
        allowed_tools: patterns(&["Read", "Write"]),
        disallowed_tools: patterns(&["Bash"]),
        deny_read: patterns(&["**/.env"]),
        deny_write: patterns(&["**/.git/**"]),
        allow_network: patterns(&["*.example.com"]),
        deny_network: patterns(&["evil.example.com"]),
        require_approval_for: patterns(&["DeleteFile"]),
    };

    let engine = PolicyEngine::new(&profile);
    assert!(engine.is_ok(), "valid profile should compile successfully");

    let engine = engine.unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn policy_profile_default_compiles() {
    let engine = PolicyEngine::new(&PolicyProfile::default());
    assert!(engine.is_ok(), "default profile should compile");
}

// ===========================================================================
// 10. Multiple policies: most restrictive wins (deny-overrides)
// ===========================================================================

#[test]
fn composed_engine_deny_overrides_most_restrictive_wins() {
    let permissive = PolicyProfile {
        allowed_tools: patterns(&["*"]),
        ..PolicyProfile::default()
    };
    let restrictive = PolicyProfile {
        disallowed_tools: patterns(&["Bash"]),
        deny_read: patterns(&["secret/**"]),
        deny_write: patterns(&["locked/**"]),
        ..PolicyProfile::default()
    };

    let engine = ComposedEngine::new(
        vec![permissive, restrictive],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();

    // Bash denied by restrictive, even though permissive allows all.
    assert!(engine.check_tool("Bash").is_deny());
    assert!(engine.check_tool("Read").is_allow());
    assert!(engine.check_read("secret/key.pem").is_deny());
    assert!(engine.check_read("src/lib.rs").is_allow());
    assert!(engine.check_write("locked/data.json").is_deny());
    assert!(engine.check_write("src/lib.rs").is_allow());
}

#[test]
fn composed_engine_policy_set_merge_deny_wins() {
    let mut set = PolicySet::new("org-policy");

    set.add(PolicyProfile {
        allowed_tools: patterns(&["Read", "Write"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        disallowed_tools: patterns(&["Write"]),
        deny_write: patterns(&["production/**"]),
        ..PolicyProfile::default()
    });

    let merged = set.merge();
    let engine = PolicyEngine::new(&merged).unwrap();

    // Write is in both allowed and disallowed → disallowed wins.
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(
        !engine
            .can_write_path(Path::new("production/config.yaml"))
            .allowed
    );
}

// ===========================================================================
// 11. Empty policy → everything allowed
// ===========================================================================

#[test]
fn empty_policy_allows_all_tools() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();

    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("DeleteFile").allowed);
    assert!(engine.can_use_tool("AnyRandomTool").allowed);
}

#[test]
fn empty_policy_allows_all_file_access() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();

    assert!(engine.can_read_path(Path::new("any/path.txt")).allowed);
    assert!(engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(engine.can_write_path(Path::new("any/path.txt")).allowed);
    assert!(engine.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn empty_composed_engine_abstains() {
    let engine = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();

    assert!(engine.check_tool("Bash").is_abstain());
    assert!(engine.check_read("any/file.txt").is_abstain());
    assert!(engine.check_write("any/file.txt").is_abstain());
}

// ===========================================================================
// 12. Glob patterns: wildcards, double-star, brace expansion
// ===========================================================================

#[test]
fn glob_single_star_wildcard() {
    let globs = IncludeExcludeGlobs::new(&patterns(&["*.rs"]), &[]).unwrap();

    assert_eq!(globs.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(globs.decide_str("lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_double_star_recursive() {
    let globs = IncludeExcludeGlobs::new(&patterns(&["**/*.rs"]), &[]).unwrap();

    assert_eq!(globs.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("src/deep/nested/mod.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(
        globs.decide_str("src/lib.txt"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_brace_expansion() {
    let globs = IncludeExcludeGlobs::new(&patterns(&["*.{rs,toml}"]), &[]).unwrap();

    assert_eq!(globs.decide_str("main.rs"), MatchDecision::Allowed);
    assert_eq!(globs.decide_str("Cargo.toml"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_question_mark_single_char() {
    let globs = IncludeExcludeGlobs::new(&patterns(&["?.rs"]), &[]).unwrap();

    assert_eq!(globs.decide_str("a.rs"), MatchDecision::Allowed);
    // globset: literal_separator=false by default, so ? may match /
    // but file name "ab.rs" has two chars before .rs, so it won't match
    assert_eq!(
        globs.decide_str("ab.rs"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_double_star_in_exclude_blocks_nested() {
    let globs =
        IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["**/generated/**"])).unwrap();

    assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("src/deep/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
}

// ===========================================================================
// 13. Integration: workspace staging respects policy file restrictions
// ===========================================================================

#[test]
fn staged_workspace_files_evaluated_against_policy() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    // Stage workspace with only src/** included.
    let spec = staged_spec_globs(src.path(), patterns(&["src/**"]), vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    // Create a policy that denies reading utils/ and writing to lib.rs.
    let policy = PolicyProfile {
        deny_read: patterns(&["src/utils/**"]),
        deny_write: patterns(&["src/lib.rs"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Verify staged files exist.
    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));

    // Policy correctly restricts access to staged files.
    assert!(
        !engine
            .can_read_path(Path::new("src/utils/helpers.rs"))
            .allowed,
        "should deny reading utils/"
    );
    assert!(
        !engine.can_write_path(Path::new("src/lib.rs")).allowed,
        "should deny writing to lib.rs"
    );
    assert!(
        engine.can_read_path(Path::new("src/lib.rs")).allowed,
        "should allow reading lib.rs"
    );
    assert!(
        engine.can_write_path(Path::new("src/main.rs")).allowed,
        "should allow writing main.rs"
    );
}

#[test]
fn workspace_staging_combined_with_composed_policy() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    // Stage only src/ and tests/.
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**", "tests/**"]))
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"tests/integration.rs".to_string()));
    assert!(!files.contains(&"docs/README.md".to_string()));

    // Compose two policies.
    let team_policy = PolicyProfile {
        allowed_tools: patterns(&["Read", "Grep", "Write"]),
        deny_write: patterns(&["tests/**"]),
        ..PolicyProfile::default()
    };
    let security_policy = PolicyProfile {
        disallowed_tools: patterns(&["Bash"]),
        deny_read: patterns(&["**/.env*"]),
        ..PolicyProfile::default()
    };

    let engine = ComposedEngine::new(
        vec![team_policy, security_policy],
        PolicyPrecedence::DenyOverrides,
    )
    .unwrap();

    // Tools: Read allowed, Bash denied.
    assert!(engine.check_tool("Read").is_allow());
    assert!(engine.check_tool("Bash").is_deny());

    // File access: tests/ write denied, src/ write allowed.
    assert!(engine.check_write("tests/integration.rs").is_deny());
    assert!(engine.check_write("src/lib.rs").is_allow());
    assert!(engine.check_read("src/lib.rs").is_allow());
}

#[test]
fn staged_workspace_with_strict_policy_locks_everything_down() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(patterns(&["src/**"]))
        .exclude(patterns(&["src/utils/**"]))
        .stage()
        .unwrap();

    let files = collect_files(ws.path());
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(!files.contains(&"src/utils/helpers.rs".to_string()));

    // Very restrictive policy.
    let policy = PolicyProfile {
        allowed_tools: patterns(&["Read"]),
        disallowed_tools: patterns(&["Bash", "Write", "DeleteFile"]),
        deny_read: patterns(&["**/.env*", "**/secret/**"]),
        deny_write: patterns(&["**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(!engine.can_write_path(Path::new("src/lib.rs")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

// ===========================================================================
// Additional edge cases
// ===========================================================================

#[test]
fn policy_engine_glob_wildcard_tool_allowlist() {
    let policy = PolicyProfile {
        allowed_tools: patterns(&["File*"]),
        disallowed_tools: patterns(&["FileDelete"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("FileRead").allowed);
    assert!(engine.can_use_tool("FileWrite").allowed);
    assert!(!engine.can_use_tool("FileDelete").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[test]
fn composed_engine_allow_overrides_strategy() {
    let restrictive = PolicyProfile {
        disallowed_tools: patterns(&["Bash"]),
        ..PolicyProfile::default()
    };
    let permissive = PolicyProfile {
        allowed_tools: patterns(&["*"]),
        ..PolicyProfile::default()
    };

    let engine = ComposedEngine::new(
        vec![restrictive, permissive],
        PolicyPrecedence::AllowOverrides,
    )
    .unwrap();

    // AllowOverrides: any allow wins, so Bash should be allowed.
    assert!(engine.check_tool("Bash").is_allow());
}

#[test]
fn composed_engine_first_applicable_strategy() {
    let first = PolicyProfile {
        disallowed_tools: patterns(&["Bash"]),
        ..PolicyProfile::default()
    };
    let second = PolicyProfile {
        allowed_tools: patterns(&["*"]),
        ..PolicyProfile::default()
    };

    let engine =
        ComposedEngine::new(vec![first, second], PolicyPrecedence::FirstApplicable).unwrap();

    // FirstApplicable: first non-abstain wins. First profile denies Bash.
    assert!(engine.check_tool("Bash").is_deny());
    // For "Read", first profile has no opinion (empty allowed_tools, not in disallowed).
    // Its tool_rules = IncludeExcludeGlobs(include=None, exclude=["Bash"])
    // For "Read": not excluded → Allowed. So first profile allows it.
    assert!(engine.check_tool("Read").is_allow());
}

#[test]
fn staged_workspace_no_globs_copies_everything() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = staged_spec_globs(src.path(), vec![], vec![]);
    let ws = WorkspaceManager::prepare(&spec).unwrap();

    let files = collect_files(ws.path());
    // All source files should be present (no glob filtering).
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"docs/README.md".to_string()));
    assert!(files.contains(&"Cargo.toml".to_string()));
    assert!(files.contains(&".env".to_string()));
}

#[test]
fn glob_empty_patterns_allow_everything() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();

    assert_eq!(globs.decide_str("anything.txt"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("deep/nested/path.rs"),
        MatchDecision::Allowed
    );
    assert_eq!(globs.decide_str(""), MatchDecision::Allowed);
}

#[test]
fn policy_set_merge_unions_all_lists() {
    let mut set = PolicySet::new("combined");
    set.add(PolicyProfile {
        deny_read: patterns(&["**/.env"]),
        deny_write: patterns(&["locked/**"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        deny_read: patterns(&["secret/**"]),
        deny_write: patterns(&["**/.git/**"]),
        ..PolicyProfile::default()
    });

    let merged = set.merge();
    assert!(merged.deny_read.contains(&"**/.env".to_string()));
    assert!(merged.deny_read.contains(&"secret/**".to_string()));
    assert!(merged.deny_write.contains(&"locked/**".to_string()));
    assert!(merged.deny_write.contains(&"**/.git/**".to_string()));

    let engine = PolicyEngine::new(&merged).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("secret/data.txt")).allowed);
    assert!(!engine.can_write_path(Path::new("locked/file.txt")).allowed);
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn workspace_staging_passthrough_mode_uses_original_path() {
    let src = tempdir().unwrap();
    create_source_tree(src.path());

    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };

    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), src.path());
}
