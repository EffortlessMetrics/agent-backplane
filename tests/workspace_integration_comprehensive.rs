#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

use abp_core::{PolicyProfile, WorkspaceMode, WorkspaceSpec};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::audit::{AuditAction, AuditLog, PolicyAuditor, PolicyDecision};
use abp_policy::rate_limit::{RateLimitPolicy, RateLimitResult};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};
use abp_workspace::diff::{
    ChangeType, DiffAnalysis, DiffAnalyzer, DiffPolicy, DiffSummary, PolicyResult, WorkspaceDiff,
};
use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog, OperationSummary};
use abp_workspace::snapshot::{self, SnapshotDiff, WorkspaceSnapshot};
use abp_workspace::template::{TemplateRegistry, WorkspaceTemplate};
use abp_workspace::tracker::{ChangeKind, ChangeTracker, FileChange};
use abp_workspace::{PreparedWorkspace, WorkspaceManager, WorkspaceStager};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ── helpers ────────────────────────────────────────────────────────────────

fn make_source_dir() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("hello.txt"), "hello world").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn hi() {}").unwrap();
    dir
}

fn make_source_with_git() -> TempDir {
    let dir = make_source_dir();
    fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
    fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    dir
}

fn make_nested_source() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for sub in &["a/b/c", "d/e", "f"] {
        fs::create_dir_all(dir.path().join(sub)).unwrap();
    }
    fs::write(dir.path().join("a/b/c/deep.txt"), "deep").unwrap();
    fs::write(dir.path().join("d/e/mid.txt"), "mid").unwrap();
    fs::write(dir.path().join("f/leaf.txt"), "leaf").unwrap();
    fs::write(dir.path().join("root.txt"), "root").unwrap();
    dir
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 1: WorkspaceStager basic staging
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stager_basic_staging() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert!(ws.path().join("src/main.rs").exists());
}

#[test]
fn stager_copies_file_contents_correctly() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let content = fs::read_to_string(ws.path().join("hello.txt")).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn stager_copies_nested_files() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let content = fs::read_to_string(ws.path().join("src/lib.rs")).unwrap();
    assert_eq!(content, "pub fn hi() {}");
}

#[test]
fn stager_produces_different_path_from_source() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert_ne!(ws.path(), src.path());
}

#[test]
fn stager_requires_source_root() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
}

#[test]
fn stager_fails_on_nonexistent_source() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/zzzz")
        .stage();
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 2: Git auto-initialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stager_initializes_git_repo() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    assert!(ws.path().join(".git").exists());
}

#[test]
fn stager_git_init_creates_baseline_commit() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let out = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stdout);
    assert!(log.contains("baseline"));
}

#[test]
fn stager_no_git_init_when_disabled() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join(".git").exists());
}

#[test]
fn stager_clean_status_after_init() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(status.unwrap().trim().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 3: .git directory exclusion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stager_excludes_source_dot_git() {
    let src = make_source_with_git();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Source .git should NOT be copied; only the fresh one (if init enabled) or none.
    assert!(!ws.path().join(".git/HEAD").exists());
}

#[test]
fn stager_excludes_dot_git_but_copies_other_dotfiles() {
    let src = make_source_dir();
    fs::write(src.path().join(".gitignore"), "target/").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(".gitignore").exists());
}

#[test]
fn workspace_manager_staged_excludes_dot_git() {
    let src = make_source_with_git();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    // The fresh .git is from ensure_git_repo, not from source
    let out = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(ws.path())
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stdout);
    assert!(log.contains("baseline"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 4: Include/exclude glob filtering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stager_include_filter() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("src/main.rs").exists());
    assert!(!ws.path().join("hello.txt").exists());
}

#[test]
fn stager_exclude_filter() {
    let src = make_source_dir();
    fs::write(src.path().join("debug.log"), "log data").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join("debug.log").exists());
    assert!(ws.path().join("hello.txt").exists());
}

#[test]
fn stager_include_and_exclude_combined() {
    let src = make_source_dir();
    fs::create_dir_all(src.path().join("src/generated")).unwrap();
    fs::write(src.path().join("src/generated/out.rs"), "gen").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["src/**".into()])
        .exclude(vec!["src/generated/**".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("src/main.rs").exists());
    assert!(!ws.path().join("src/generated/out.rs").exists());
    assert!(!ws.path().join("hello.txt").exists());
}

#[test]
fn stager_exclude_multiple_patterns() {
    let src = make_source_dir();
    fs::write(src.path().join("a.log"), "").unwrap();
    fs::write(src.path().join("b.tmp"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into(), "*.tmp".into()])
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(!ws.path().join("a.log").exists());
    assert!(!ws.path().join("b.tmp").exists());
    assert!(ws.path().join("hello.txt").exists());
}

#[test]
fn stager_no_filters_copies_everything() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("hello.txt").exists());
    assert!(ws.path().join("src/main.rs").exists());
    assert!(ws.path().join("src/lib.rs").exists());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 5: WorkspaceManager (prepare)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_manager_passthrough_uses_source_path() {
    let src = make_source_dir();
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
fn workspace_manager_staged_creates_copy() {
    let src = make_source_dir();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_ne!(ws.path(), src.path());
    assert!(ws.path().join("hello.txt").exists());
}

#[test]
fn workspace_manager_staged_with_include() {
    let src = make_source_dir();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(ws.path().join("src/main.rs").exists());
    assert!(!ws.path().join("hello.txt").exists());
}

#[test]
fn workspace_manager_staged_with_exclude() {
    let src = make_source_dir();
    fs::write(src.path().join("build.log"), "").unwrap();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["*.log".into()],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(!ws.path().join("build.log").exists());
    assert!(ws.path().join("hello.txt").exists());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 6: Workspace diff generation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_workspace_empty_when_no_changes() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(diff.is_empty());
    assert_eq!(diff.file_count(), 0);
}

#[test]
fn diff_workspace_detects_new_file() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("new.txt"), "new content").unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(!diff.is_empty());
    assert!(diff.added.contains(&PathBuf::from("new.txt")));
}

#[test]
fn diff_workspace_detects_modification() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("hello.txt"), "changed").unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(diff.modified.contains(&PathBuf::from("hello.txt")));
}

#[test]
fn diff_workspace_detects_deletion() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::remove_file(ws.path().join("hello.txt")).unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(diff.deleted.contains(&PathBuf::from("hello.txt")));
}

#[test]
fn diff_workspace_total_changes() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("new.txt"), "line1\nline2\nline3\n").unwrap();
    let diff = abp_workspace::diff::diff_workspace(&ws).unwrap();
    assert!(diff.total_additions >= 3);
    assert!(diff.total_changes() >= 3);
}

#[test]
fn diff_summary_is_empty() {
    let d = DiffSummary::default();
    assert!(d.is_empty());
    assert_eq!(d.file_count(), 0);
    assert_eq!(d.total_changes(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 7: DiffAnalyzer
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_analyzer_no_changes() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(!analyzer.has_changes());
    assert!(analyzer.changed_files().is_empty());
}

#[test]
fn diff_analyzer_detects_changes() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("added.txt"), "hi").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.has_changes());
    assert!(!analyzer.changed_files().is_empty());
}

#[test]
fn diff_analyzer_file_was_modified() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("hello.txt"), "modified").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    assert!(analyzer.file_was_modified(Path::new("hello.txt")));
    assert!(!analyzer.file_was_modified(Path::new("src/main.rs")));
}

#[test]
fn diff_analyzer_analyze_returns_workspace_diff() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("new.rs"), "fn new() {}").unwrap();
    let analyzer = DiffAnalyzer::new(ws.path());
    let diff = analyzer.analyze().unwrap();
    assert!(!diff.is_empty());
    assert!(diff.file_count() > 0);
}

#[test]
fn workspace_diff_summary_text() {
    let empty = WorkspaceDiff::default();
    assert_eq!(empty.summary(), "No changes detected.");
    assert!(empty.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 8: DiffPolicy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_policy_pass_no_constraints() {
    let policy = DiffPolicy::default();
    let diff = WorkspaceDiff::default();
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_max_files_pass() {
    let policy = DiffPolicy {
        max_files: Some(5),
        ..Default::default()
    };
    let diff = WorkspaceDiff::default();
    let result = policy.check(&diff).unwrap();
    assert!(result.is_pass());
}

#[test]
fn diff_policy_max_files_fail() {
    let policy = DiffPolicy {
        max_files: Some(0),
        ..Default::default()
    };
    let fc = abp_workspace::diff::FileChange {
        path: PathBuf::from("a.txt"),
        change_type: ChangeType::Added,
        additions: 1,
        deletions: 0,
        is_binary: false,
    };
    let diff = WorkspaceDiff {
        files_added: vec![fc],
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

#[test]
fn diff_policy_max_additions_fail() {
    let policy = DiffPolicy {
        max_additions: Some(10),
        ..Default::default()
    };
    let diff = WorkspaceDiff {
        total_additions: 100,
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

#[test]
fn diff_policy_denied_paths() {
    let policy = DiffPolicy {
        denied_paths: vec!["*.secret".into()],
        ..Default::default()
    };
    let fc = abp_workspace::diff::FileChange {
        path: PathBuf::from("data.secret"),
        change_type: ChangeType::Added,
        additions: 0,
        deletions: 0,
        is_binary: false,
    };
    let diff = WorkspaceDiff {
        files_added: vec![fc],
        ..Default::default()
    };
    let result = policy.check(&diff).unwrap();
    assert!(!result.is_pass());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 9: PolicyEngine enforcement
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_engine_empty_allows_everything() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(engine.can_write_path(Path::new("any/file.txt")).allowed);
}

#[test]
fn policy_engine_disallowed_tool() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_engine_allowed_tool_list() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[test]
fn policy_engine_deny_overrides_allow() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_engine_deny_read() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_engine_deny_write() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_engine_multiple_deny_read() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/id_rsa".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_engine_glob_pattern_tools() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash*".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("BashExec").allowed);
    assert!(!engine.can_use_tool("BashRun").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_engine_decision_reason_on_deny() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.is_some());
    assert!(d.reason.unwrap().contains("Bash"));
}

#[test]
fn policy_engine_no_reason_on_allow() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let d = engine.can_use_tool("Read");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 10: PolicyAuditor
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auditor_records_tool_decisions() {
    let engine = PolicyEngine::new(&PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    })
    .unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    let d1 = auditor.check_tool("Read");
    let d2 = auditor.check_tool("Bash");
    assert_eq!(d1, PolicyDecision::Allow);
    assert!(matches!(d2, PolicyDecision::Deny { .. }));
    assert_eq!(auditor.entries().len(), 2);
    assert_eq!(auditor.allowed_count(), 1);
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn auditor_records_read_write() {
    let engine = PolicyEngine::new(&PolicyProfile {
        deny_read: vec!["secret*".into()],
        deny_write: vec!["locked*".into()],
        ..Default::default()
    })
    .unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    auditor.check_read("secret.txt");
    auditor.check_read("normal.txt");
    auditor.check_write("locked.dat");
    auditor.check_write("open.dat");
    let summary = auditor.summary();
    assert_eq!(summary.allowed, 2);
    assert_eq!(summary.denied, 2);
}

#[test]
fn auditor_summary_aggregation() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    for _ in 0..5 {
        auditor.check_tool("Read");
    }
    let summary = auditor.summary();
    assert_eq!(summary.allowed, 5);
    assert_eq!(summary.denied, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 11: AuditLog
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn audit_log_empty() {
    let log = AuditLog::new();
    assert!(log.is_empty());
    assert_eq!(log.len(), 0);
    assert_eq!(log.denied_count(), 0);
}

#[test]
fn audit_log_record_and_query() {
    let mut log = AuditLog::new();
    log.record(AuditAction::ToolAllowed, "Read", Some("default"), None);
    log.record(
        AuditAction::ToolDenied,
        "Bash",
        Some("strict"),
        Some("not allowed"),
    );
    assert_eq!(log.len(), 2);
    assert_eq!(log.denied_count(), 1);
    assert_eq!(log.filter_by_action(&AuditAction::ToolAllowed).len(), 1);
}

#[test]
fn audit_action_is_denied() {
    assert!(AuditAction::ToolDenied.is_denied());
    assert!(AuditAction::ReadDenied.is_denied());
    assert!(AuditAction::WriteDenied.is_denied());
    assert!(AuditAction::RateLimited.is_denied());
    assert!(!AuditAction::ToolAllowed.is_denied());
    assert!(!AuditAction::ReadAllowed.is_denied());
    assert!(!AuditAction::WriteAllowed.is_denied());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 12: Glob matching (IncludeExcludeGlobs)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn glob_no_patterns_allows_all() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
}

#[test]
fn glob_include_only() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_exclude_only() {
    let g = IncludeExcludeGlobs::new(&[], &["*.log".into()]).unwrap();
    assert_eq!(g.decide_str("debug.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

#[test]
fn glob_exclude_overrides_include() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &["src/secret/**".into()]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/secret/key.pem"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn glob_invalid_pattern_errors() {
    let result = IncludeExcludeGlobs::new(&["[invalid".into()], &[]);
    assert!(result.is_err());
}

#[test]
fn glob_decide_path_consistency() {
    let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
    assert_eq!(
        g.decide_str("src/main.rs"),
        g.decide_path(Path::new("src/main.rs"))
    );
}

#[test]
fn glob_match_decision_is_allowed() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 13: Workspace snapshot and comparison
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_capture_basic() {
    let src = make_source_dir();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.file_count() >= 3);
    assert!(snap.has_file(Path::new("hello.txt")));
}

#[test]
fn snapshot_file_details() {
    let src = make_source_dir();
    let snap = snapshot::capture(src.path()).unwrap();
    let f = snap.get_file(Path::new("hello.txt")).unwrap();
    assert_eq!(f.size, 11); // "hello world"
    assert!(!f.is_binary);
    assert!(!f.sha256.is_empty());
}

#[test]
fn snapshot_total_size() {
    let src = make_source_dir();
    let snap = snapshot::capture(src.path()).unwrap();
    assert!(snap.total_size() > 0);
}

#[test]
fn snapshot_compare_identical() {
    let src = make_source_dir();
    let a = snapshot::capture(src.path()).unwrap();
    let b = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&a, &b);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert!(!diff.unchanged.is_empty());
}

#[test]
fn snapshot_compare_added_file() {
    let src = make_source_dir();
    let a = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("new.txt"), "new").unwrap();
    let b = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&a, &b);
    assert!(diff.added.contains(&PathBuf::from("new.txt")));
}

#[test]
fn snapshot_compare_removed_file() {
    let src = make_source_dir();
    let a = snapshot::capture(src.path()).unwrap();
    fs::remove_file(src.path().join("hello.txt")).unwrap();
    let b = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&a, &b);
    assert!(diff.removed.contains(&PathBuf::from("hello.txt")));
}

#[test]
fn snapshot_compare_modified_file() {
    let src = make_source_dir();
    let a = snapshot::capture(src.path()).unwrap();
    fs::write(src.path().join("hello.txt"), "changed content").unwrap();
    let b = snapshot::capture(src.path()).unwrap();
    let diff = snapshot::compare(&a, &b);
    assert!(diff.modified.contains(&PathBuf::from("hello.txt")));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 14: WorkspaceTemplate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn template_new_empty() {
    let t = WorkspaceTemplate::new("test", "a test template");
    assert_eq!(t.name, "test");
    assert_eq!(t.file_count(), 0);
}

#[test]
fn template_add_file() {
    let mut t = WorkspaceTemplate::new("test", "desc");
    t.add_file("src/main.rs", "fn main() {}");
    assert!(t.has_file(Path::new("src/main.rs")));
    assert_eq!(t.file_count(), 1);
}

#[test]
fn template_apply() {
    let mut t = WorkspaceTemplate::new("test", "desc");
    t.add_file("hello.txt", "world");
    t.add_file("sub/deep.txt", "content");
    let dir = tempfile::tempdir().unwrap();
    let written = t.apply(dir.path()).unwrap();
    assert_eq!(written, 2);
    assert_eq!(fs::read_to_string(dir.path().join("hello.txt")).unwrap(), "world");
    assert!(dir.path().join("sub/deep.txt").exists());
}

#[test]
fn template_validate_ok() {
    let t = WorkspaceTemplate::new("good", "valid template");
    let issues = t.validate();
    assert!(issues.is_empty());
}

#[test]
fn template_validate_empty_name() {
    let t = WorkspaceTemplate::new("", "desc");
    let issues = t.validate();
    assert!(issues.iter().any(|i| i.contains("name")));
}

#[test]
fn template_validate_empty_description() {
    let t = WorkspaceTemplate::new("name", "");
    let issues = t.validate();
    assert!(issues.iter().any(|i| i.contains("description")));
}

#[test]
fn template_validate_absolute_path() {
    let mut t = WorkspaceTemplate::new("t", "d");
    // Use a platform-appropriate absolute path
    let abs = if cfg!(windows) {
        PathBuf::from("C:\\absolute\\path.txt")
    } else {
        PathBuf::from("/absolute/path.txt")
    };
    t.files.insert(abs, "content".into());
    let issues = t.validate();
    assert!(issues.iter().any(|i| i.contains("absolute")));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 15: TemplateRegistry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn registry_empty() {
    let reg = TemplateRegistry::new();
    assert_eq!(reg.count(), 0);
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = TemplateRegistry::new();
    let t = WorkspaceTemplate::new("first", "desc");
    reg.register(t);
    assert_eq!(reg.count(), 1);
    assert!(reg.get("first").is_some());
    assert!(reg.get("missing").is_none());
}

#[test]
fn registry_overwrite() {
    let mut reg = TemplateRegistry::new();
    let mut t1 = WorkspaceTemplate::new("t", "v1");
    t1.add_file("a.txt", "v1");
    reg.register(t1);
    let mut t2 = WorkspaceTemplate::new("t", "v2");
    t2.add_file("b.txt", "v2");
    reg.register(t2);
    assert_eq!(reg.count(), 1);
    let got = reg.get("t").unwrap();
    assert!(got.has_file(Path::new("b.txt")));
    assert!(!got.has_file(Path::new("a.txt")));
}

#[test]
fn registry_list_sorted() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("c", ""));
    reg.register(WorkspaceTemplate::new("a", ""));
    reg.register(WorkspaceTemplate::new("b", ""));
    assert_eq!(reg.list(), vec!["a", "b", "c"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 16: OperationLog and OperationFilter
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn operation_log_empty() {
    let log = OperationLog::new();
    assert!(log.operations().is_empty());
    let s = log.summary();
    assert_eq!(s.reads, 0);
}

#[test]
fn operation_log_record_read() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    assert_eq!(log.reads(), vec!["a.txt"]);
    assert_eq!(log.summary().reads, 1);
}

#[test]
fn operation_log_record_write() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Write {
        path: "a.txt".into(),
        size: 42,
    });
    assert_eq!(log.writes(), vec!["a.txt"]);
    let s = log.summary();
    assert_eq!(s.writes, 1);
    assert_eq!(s.total_writes_bytes, 42);
}

#[test]
fn operation_log_record_delete() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Delete {
        path: "a.txt".into(),
    });
    assert_eq!(log.deletes(), vec!["a.txt"]);
    assert_eq!(log.summary().deletes, 1);
}

#[test]
fn operation_log_affected_paths() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Write {
        path: "b.txt".into(),
        size: 10,
    });
    log.record(FileOperation::Move {
        from: "c.txt".into(),
        to: "d.txt".into(),
    });
    let paths = log.affected_paths();
    assert!(paths.contains("a.txt"));
    assert!(paths.contains("b.txt"));
    assert!(paths.contains("c.txt"));
    assert!(paths.contains("d.txt"));
}

#[test]
fn operation_log_clear() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "x.txt".into(),
    });
    log.clear();
    assert!(log.operations().is_empty());
}

#[test]
fn operation_log_summary_all_types() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read { path: "r".into() });
    log.record(FileOperation::Write {
        path: "w".into(),
        size: 5,
    });
    log.record(FileOperation::Delete { path: "d".into() });
    log.record(FileOperation::Move {
        from: "a".into(),
        to: "b".into(),
    });
    log.record(FileOperation::Copy {
        from: "c".into(),
        to: "d".into(),
    });
    log.record(FileOperation::CreateDir { path: "dir".into() });
    let s = log.summary();
    assert_eq!(s.reads, 1);
    assert_eq!(s.writes, 1);
    assert_eq!(s.deletes, 1);
    assert_eq!(s.moves, 1);
    assert_eq!(s.copies, 1);
    assert_eq!(s.create_dirs, 1);
}

#[test]
fn operation_filter_no_constraints_allows_all() {
    let f = OperationFilter::new();
    assert!(f.is_allowed("anything"));
}

#[test]
fn operation_filter_denied_path() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.secret");
    assert!(!f.is_allowed("data.secret"));
    assert!(f.is_allowed("data.txt"));
}

#[test]
fn operation_filter_allowed_path() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("src/**");
    assert!(f.is_allowed("src/main.rs"));
    assert!(!f.is_allowed("README.md"));
}

#[test]
fn operation_filter_filters_operations() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.secret");
    let ops = vec![
        FileOperation::Read {
            path: "ok.txt".into(),
        },
        FileOperation::Read {
            path: "bad.secret".into(),
        },
    ];
    let filtered = f.filter_operations(&ops);
    assert_eq!(filtered.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 17: ChangeTracker
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn change_tracker_empty() {
    let tracker = ChangeTracker::new();
    assert!(!tracker.has_changes());
    assert!(tracker.changes().is_empty());
}

#[test]
fn change_tracker_record_and_summarize() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "new.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: Some(100),
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "old.txt".into(),
        kind: ChangeKind::Deleted,
        size_before: Some(50),
        size_after: None,
        content_hash: None,
    });
    let s = tracker.summary();
    assert_eq!(s.created, 1);
    assert_eq!(s.deleted, 1);
    assert_eq!(s.total_size_delta, 100 - 50);
}

#[test]
fn change_tracker_by_kind() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "b.txt".into(),
        kind: ChangeKind::Modified,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    let created = tracker.by_kind(&ChangeKind::Created);
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].path, "a.txt");
}

#[test]
fn change_tracker_affected_paths() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    tracker.record(FileChange {
        path: "a.txt".into(),
        kind: ChangeKind::Modified,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    let paths = tracker.affected_paths();
    // Deduplication
    assert_eq!(paths.len(), 1);
}

#[test]
fn change_tracker_clear() {
    let mut tracker = ChangeTracker::new();
    tracker.record(FileChange {
        path: "x".into(),
        kind: ChangeKind::Created,
        size_before: None,
        size_after: None,
        content_hash: None,
    });
    tracker.clear();
    assert!(!tracker.has_changes());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 18: RuleEngine
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rule_engine_empty_allows() {
    let engine = RuleEngine::new();
    assert_eq!(engine.evaluate("anything"), RuleEffect::Allow);
}

#[test]
fn rule_engine_deny_rule() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "deny-bash".into(),
        description: "deny bash".into(),
        condition: RuleCondition::Pattern("Bash*".into()),
        effect: RuleEffect::Deny,
        priority: 10,
    });
    assert_eq!(engine.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(engine.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_priority_wins() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "low".into(),
        description: "".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    engine.add_rule(Rule {
        id: "high".into(),
        description: "".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 100,
    });
    assert_eq!(engine.evaluate("anything"), RuleEffect::Deny);
}

#[test]
fn rule_engine_remove_rule() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "r1".into(),
        description: "".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 1,
    });
    assert_eq!(engine.rule_count(), 1);
    engine.remove_rule("r1");
    assert_eq!(engine.rule_count(), 0);
}

#[test]
fn rule_engine_evaluate_all() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: "a".into(),
        description: "".into(),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    engine.add_rule(Rule {
        id: "b".into(),
        description: "".into(),
        condition: RuleCondition::Never,
        effect: RuleEffect::Deny,
        priority: 2,
    });
    let evals = engine.evaluate_all("test");
    assert_eq!(evals.len(), 2);
    assert!(evals[0].matched);
    assert!(!evals[1].matched);
}

#[test]
fn rule_condition_and_or_not() {
    let c = RuleCondition::And(vec![RuleCondition::Always, RuleCondition::Always]);
    assert!(c.matches("x"));

    let c = RuleCondition::And(vec![RuleCondition::Always, RuleCondition::Never]);
    assert!(!c.matches("x"));

    let c = RuleCondition::Or(vec![RuleCondition::Never, RuleCondition::Always]);
    assert!(c.matches("x"));

    let c = RuleCondition::Not(Box::new(RuleCondition::Never));
    assert!(c.matches("x"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 19: RateLimitPolicy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rate_limit_unlimited() {
    let policy = RateLimitPolicy::unlimited();
    assert!(policy.check_rate_limit(1000, 1_000_000, 100).is_allowed());
}

#[test]
fn rate_limit_rpm_exceeded() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        ..Default::default()
    };
    let result = policy.check_rate_limit(10, 0, 0);
    assert!(result.is_throttled());
}

#[test]
fn rate_limit_tpm_exceeded() {
    let policy = RateLimitPolicy {
        max_tokens_per_minute: Some(100),
        ..Default::default()
    };
    let result = policy.check_rate_limit(0, 100, 0);
    assert!(result.is_throttled());
}

#[test]
fn rate_limit_concurrent_exceeded() {
    let policy = RateLimitPolicy {
        max_concurrent: Some(5),
        ..Default::default()
    };
    let result = policy.check_rate_limit(0, 0, 5);
    assert!(result.is_denied());
}

#[test]
fn rate_limit_within_limits() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(100),
        max_tokens_per_minute: Some(10000),
        max_concurrent: Some(10),
    };
    assert!(policy.check_rate_limit(50, 5000, 5).is_allowed());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 20: Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stager_empty_source_directory() {
    let dir = tempfile::tempdir().unwrap();
    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    // Should succeed with empty workspace
    assert!(ws.path().exists());
}

#[test]
fn stager_deeply_nested_directories() {
    let src = make_nested_source();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("a/b/c/deep.txt").exists());
    assert!(ws.path().join("d/e/mid.txt").exists());
    assert!(ws.path().join("f/leaf.txt").exists());
}

#[test]
fn stager_binary_file_handling() {
    let dir = tempfile::tempdir().unwrap();
    let binary = vec![0u8, 1, 2, 255, 254, 0, 128];
    fs::write(dir.path().join("binary.bin"), &binary).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let copied = fs::read(ws.path().join("binary.bin")).unwrap();
    assert_eq!(copied, binary);
}

#[test]
fn stager_large_file() {
    let dir = tempfile::tempdir().unwrap();
    let content = "x".repeat(1_000_000);
    fs::write(dir.path().join("large.txt"), &content).unwrap();
    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let read = fs::read_to_string(ws.path().join("large.txt")).unwrap();
    assert_eq!(read.len(), 1_000_000);
}

#[test]
fn stager_unicode_filename() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("données.txt"), "unicode").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join("données.txt").exists());
}

#[test]
fn stager_dotfiles_preserved() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".env"), "SECRET=123").unwrap();
    fs::write(dir.path().join(".gitignore"), "target/").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    assert!(ws.path().join(".env").exists());
    assert!(ws.path().join(".gitignore").exists());
}

#[test]
fn stager_empty_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("empty.txt"), "").unwrap();
    let ws = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();
    let content = fs::read_to_string(ws.path().join("empty.txt")).unwrap();
    assert!(content.is_empty());
}

#[test]
fn workspace_manager_staged_with_include_exclude() {
    let src = make_source_dir();
    fs::write(src.path().join("debug.log"), "log").unwrap();
    let spec = WorkspaceSpec {
        root: src.path().to_string_lossy().to_string(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec!["*.log".into()],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert!(!ws.path().join("debug.log").exists());
    assert!(ws.path().join("hello.txt").exists());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 21: DiffAnalysis (parsing)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_analysis_empty_input() {
    let analysis = DiffAnalysis::parse("");
    assert!(analysis.is_empty());
    assert_eq!(analysis.file_count(), 0);
}

#[test]
fn diff_analysis_parse_add() {
    let raw = "diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+hello\n+world\n";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.total_additions, 2);
    assert_eq!(analysis.total_deletions, 0);
}

#[test]
fn diff_analysis_parse_delete() {
    let raw = "diff --git a/old.txt b/old.txt\ndeleted file mode 100644\n--- a/old.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n";
    let analysis = DiffAnalysis::parse(raw);
    assert_eq!(analysis.file_count(), 1);
    assert_eq!(analysis.total_deletions, 1);
}

#[test]
fn diff_analysis_file_stats() {
    let raw = "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+content\n";
    let analysis = DiffAnalysis::parse(raw);
    let stats = analysis.file_stats();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].additions, 1);
}

#[test]
fn diff_analysis_files_by_kind() {
    let raw = "diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+a\n";
    let analysis = DiffAnalysis::parse(raw);
    use abp_workspace::diff::DiffChangeKind;
    let added = analysis.files_by_kind(DiffChangeKind::Added);
    assert_eq!(added.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 22: git_status / git_diff via WorkspaceManager
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_manager_git_diff_empty_on_clean() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    let diff = WorkspaceManager::git_diff(ws.path());
    assert!(diff.is_some());
    assert!(diff.unwrap().trim().is_empty());
}

#[test]
fn workspace_manager_git_status_detects_changes() {
    let src = make_source_dir();
    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .stage()
        .unwrap();
    fs::write(ws.path().join("untracked.txt"), "new").unwrap();
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
    assert!(!status.unwrap().trim().is_empty());
}

#[test]
fn workspace_manager_git_status_none_for_non_git() {
    let dir = tempfile::tempdir().unwrap();
    let status = WorkspaceManager::git_status(dir.path());
    // Non-git directory returns None
    assert!(status.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 23: FileOperation paths method
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn file_operation_read_paths() {
    let op = FileOperation::Read {
        path: "a.txt".into(),
    };
    assert_eq!(op.paths(), vec!["a.txt"]);
}

#[test]
fn file_operation_write_paths() {
    let op = FileOperation::Write {
        path: "b.txt".into(),
        size: 10,
    };
    assert_eq!(op.paths(), vec!["b.txt"]);
}

#[test]
fn file_operation_move_paths() {
    let op = FileOperation::Move {
        from: "a.txt".into(),
        to: "b.txt".into(),
    };
    let paths = op.paths();
    assert_eq!(paths.len(), 2);
    assert!(paths.contains(&"a.txt"));
    assert!(paths.contains(&"b.txt"));
}

#[test]
fn file_operation_copy_paths() {
    let op = FileOperation::Copy {
        from: "src.txt".into(),
        to: "dst.txt".into(),
    };
    assert_eq!(op.paths().len(), 2);
}

#[test]
fn file_operation_delete_paths() {
    let op = FileOperation::Delete {
        path: "gone.txt".into(),
    };
    assert_eq!(op.paths(), vec!["gone.txt"]);
}

#[test]
fn file_operation_create_dir_paths() {
    let op = FileOperation::CreateDir {
        path: "newdir".into(),
    };
    assert_eq!(op.paths(), vec!["newdir"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 24: Decision type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn decision_allow_fields() {
    let d = Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn decision_deny_fields() {
    let d = Decision::deny("forbidden");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("forbidden"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 25: Snapshot on binary content
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_detects_binary_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut content = vec![0u8; 100];
    content[0] = 0; // null byte → binary detection
    fs::write(dir.path().join("bin.dat"), &content).unwrap();
    let snap = snapshot::capture(dir.path()).unwrap();
    let f = snap.get_file(Path::new("bin.dat")).unwrap();
    assert!(f.is_binary);
}

#[test]
fn snapshot_text_not_binary() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("text.txt"), "just text").unwrap();
    let snap = snapshot::capture(dir.path()).unwrap();
    let f = snap.get_file(Path::new("text.txt")).unwrap();
    assert!(!f.is_binary);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 26: WorkspaceSpec configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_spec_clone() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["*.log".into()],
    };
    let cloned = spec.clone();
    assert_eq!(cloned.root, ".");
    assert_eq!(cloned.include, vec!["src/**"]);
    assert_eq!(cloned.exclude, vec!["*.log"]);
}

#[test]
fn policy_profile_default_is_permissive() {
    let p = PolicyProfile::default();
    assert!(p.allowed_tools.is_empty());
    assert!(p.disallowed_tools.is_empty());
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
    assert!(p.require_approval_for.is_empty());
}

#[test]
fn policy_profile_serialization() {
    let p = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..Default::default()
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(back.disallowed_tools, vec!["Bash"]);
    assert_eq!(back.deny_write, vec!["**/.git/**"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 27: identify_file_type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn identify_rust_file() {
    use abp_workspace::diff::{identify_file_type, FileType};
    assert_eq!(identify_file_type("src/main.rs"), FileType::Rust);
}

#[test]
fn identify_javascript_file() {
    use abp_workspace::diff::{identify_file_type, FileType};
    assert_eq!(identify_file_type("app.js"), FileType::JavaScript);
}

#[test]
fn identify_unknown_file() {
    use abp_workspace::diff::{identify_file_type, FileType};
    assert_eq!(identify_file_type("readme"), FileType::Other);
}

#[test]
fn identify_binary_extension() {
    use abp_workspace::diff::{identify_file_type, FileType};
    assert_eq!(identify_file_type("image.png"), FileType::Binary);
}
