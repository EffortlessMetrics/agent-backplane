// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for semantic_diff, git_ops, approval, and archive modules.

use abp_workspace::approval::{ApprovalPolicy, ApprovalStatus, ChangeApproval};
use abp_workspace::archive::{ArchiveEntry, ArchiveMetadata, WorkspaceArchive};
use abp_workspace::diff::{
    ChangeType, DiffAnalysis, DiffChangeKind, DiffHunk, DiffLine, DiffLineKind, FileChange,
    FileDiff, FileType, WorkspaceDiff,
};
use abp_workspace::git_ops::{DiffStats, GitFileStatus, GitOps, GitStatusEntry, LogEntry};
use abp_workspace::semantic_diff::{
    LineChange, LineChangeKind, SemanticChangeKind, SemanticDiff, SemanticFileChange,
};
use std::fs;
use std::path::PathBuf;

// ── Helpers ────────────────────────────────────────────────────────────────

fn make_test_workspace() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world\n").unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    init_git(tmp.path());
    tmp
}

fn init_git(path: &std::path::Path) {
    use std::process::Command;
    let _ = Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .status();
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .status();
    let _ = Command::new("git")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test",
            "commit",
            "-qm",
            "baseline",
        ])
        .current_dir(path)
        .status();
}

fn make_file_diff(path: &str, kind: DiffChangeKind, adds: usize, dels: usize) -> FileDiff {
    FileDiff {
        path: path.to_string(),
        change_kind: kind,
        is_binary: false,
        hunks: Vec::new(),
        additions: adds,
        deletions: dels,
        old_mode: None,
        new_mode: None,
        file_type: FileType::Other,
        renamed_from: None,
    }
}

fn make_workspace_diff(
    added: Vec<(&str, usize, usize)>,
    modified: Vec<(&str, usize, usize)>,
    deleted: Vec<(&str, usize, usize)>,
) -> WorkspaceDiff {
    let mut diff = WorkspaceDiff::default();
    for (p, a, d) in &added {
        diff.total_additions += a;
        diff.total_deletions += d;
        diff.files_added.push(FileChange {
            path: PathBuf::from(p),
            change_type: ChangeType::Added,
            additions: *a,
            deletions: *d,
            is_binary: false,
        });
    }
    for (p, a, d) in &modified {
        diff.total_additions += a;
        diff.total_deletions += d;
        diff.files_modified.push(FileChange {
            path: PathBuf::from(p),
            change_type: ChangeType::Modified,
            additions: *a,
            deletions: *d,
            is_binary: false,
        });
    }
    for (p, a, d) in &deleted {
        diff.total_additions += a;
        diff.total_deletions += d;
        diff.files_deleted.push(FileChange {
            path: PathBuf::from(p),
            change_type: ChangeType::Deleted,
            additions: *a,
            deletions: *d,
            is_binary: false,
        });
    }
    diff
}

// ═══════════════════════════════════════════════════════════════════════════
// Semantic diff tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn semantic_diff_empty_analysis() {
    let analysis = DiffAnalysis::default();
    let sd = SemanticDiff::from_analysis(&analysis);
    assert!(sd.is_empty());
    assert_eq!(sd.file_count(), 0);
    assert_eq!(sd.summary(), "No changes detected.");
}

#[test]
fn semantic_diff_classifies_added() {
    let analysis = DiffAnalysis {
        files: vec![make_file_diff("new.rs", DiffChangeKind::Added, 10, 0)],
        total_additions: 10,
        total_deletions: 0,
        binary_file_count: 0,
    };
    let sd = SemanticDiff::from_analysis(&analysis);
    assert_eq!(sd.added_files().len(), 1);
    assert!(matches!(sd.files[0].kind, SemanticChangeKind::Added));
}

#[test]
fn semantic_diff_classifies_modified() {
    let analysis = DiffAnalysis {
        files: vec![make_file_diff("lib.rs", DiffChangeKind::Modified, 5, 3)],
        total_additions: 5,
        total_deletions: 3,
        binary_file_count: 0,
    };
    let sd = SemanticDiff::from_analysis(&analysis);
    assert_eq!(sd.modified_files().len(), 1);
    assert!(matches!(sd.files[0].kind, SemanticChangeKind::Modified));
}

#[test]
fn semantic_diff_classifies_deleted() {
    let analysis = DiffAnalysis {
        files: vec![make_file_diff("old.rs", DiffChangeKind::Deleted, 0, 20)],
        total_additions: 0,
        total_deletions: 20,
        binary_file_count: 0,
    };
    let sd = SemanticDiff::from_analysis(&analysis);
    assert_eq!(sd.deleted_files().len(), 1);
    assert!(matches!(sd.files[0].kind, SemanticChangeKind::Deleted));
}

#[test]
fn semantic_diff_classifies_renamed() {
    let mut fd = make_file_diff("new_name.rs", DiffChangeKind::Renamed, 0, 0);
    fd.renamed_from = Some("old_name.rs".to_string());

    let analysis = DiffAnalysis {
        files: vec![fd],
        total_additions: 0,
        total_deletions: 0,
        binary_file_count: 0,
    };
    let sd = SemanticDiff::from_analysis(&analysis);
    assert_eq!(sd.renamed_files().len(), 1);
    match &sd.files[0].kind {
        SemanticChangeKind::Renamed { from, to } => {
            assert_eq!(from, "old_name.rs");
            assert_eq!(to, "new_name.rs");
        }
        other => panic!("expected Renamed, got {other:?}"),
    }
}

#[test]
fn semantic_diff_mixed_changes_summary() {
    let analysis = DiffAnalysis {
        files: vec![
            make_file_diff("a.rs", DiffChangeKind::Added, 10, 0),
            make_file_diff("b.rs", DiffChangeKind::Modified, 5, 2),
            make_file_diff("c.rs", DiffChangeKind::Deleted, 0, 15),
        ],
        total_additions: 15,
        total_deletions: 17,
        binary_file_count: 0,
    };
    let sd = SemanticDiff::from_analysis(&analysis);
    assert_eq!(sd.file_count(), 3);
    let summary = sd.summary();
    assert!(summary.contains("3 file(s) changed"));
    assert!(summary.contains("1 added"));
    assert!(summary.contains("1 modified"));
    assert!(summary.contains("1 deleted"));
    assert!(summary.contains("+15"));
    assert!(summary.contains("-17"));
}

#[test]
fn semantic_diff_display_trait() {
    let analysis = DiffAnalysis {
        files: vec![make_file_diff("x.rs", DiffChangeKind::Added, 1, 0)],
        total_additions: 1,
        total_deletions: 0,
        binary_file_count: 0,
    };
    let sd = SemanticDiff::from_analysis(&analysis);
    assert_eq!(format!("{sd}"), sd.summary());
}

#[test]
fn semantic_diff_line_changes_from_hunks() {
    let hunk = DiffHunk {
        old_start: 1,
        old_count: 3,
        new_start: 1,
        new_count: 4,
        header: "@@ -1,3 +1,4 @@".to_string(),
        lines: vec![
            DiffLine {
                kind: DiffLineKind::Context,
                content: "line1".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Removed,
                content: "old_line2".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Added,
                content: "new_line2".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Added,
                content: "new_line3".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Context,
                content: "line3".to_string(),
            },
        ],
    };

    let fd = FileDiff {
        path: "test.rs".to_string(),
        change_kind: DiffChangeKind::Modified,
        is_binary: false,
        hunks: vec![hunk],
        additions: 2,
        deletions: 1,
        old_mode: None,
        new_mode: None,
        file_type: FileType::Rust,
        renamed_from: None,
    };

    let analysis = DiffAnalysis {
        files: vec![fd],
        total_additions: 2,
        total_deletions: 1,
        binary_file_count: 0,
    };

    let sd = SemanticDiff::from_analysis(&analysis);
    let fc = &sd.files[0];
    assert_eq!(fc.line_changes.len(), 3);
    assert_eq!(fc.line_changes[0].kind, LineChangeKind::Removed);
    assert_eq!(fc.line_changes[0].line_number, 2);
    assert_eq!(fc.line_changes[0].content, "old_line2");
    assert_eq!(fc.line_changes[1].kind, LineChangeKind::Added);
    assert_eq!(fc.line_changes[1].line_number, 2);
    assert_eq!(fc.line_changes[2].kind, LineChangeKind::Added);
    assert_eq!(fc.line_changes[2].line_number, 3);
}

#[test]
fn semantic_file_change_total_changes() {
    let fc = SemanticFileChange {
        path: "x.rs".to_string(),
        kind: SemanticChangeKind::Modified,
        additions: 10,
        deletions: 5,
        line_changes: Vec::new(),
    };
    assert_eq!(fc.total_changes(), 15);
}

#[test]
fn semantic_change_kind_display() {
    assert_eq!(format!("{}", SemanticChangeKind::Added), "added");
    assert_eq!(format!("{}", SemanticChangeKind::Modified), "modified");
    assert_eq!(format!("{}", SemanticChangeKind::Deleted), "deleted");
    assert_eq!(
        format!(
            "{}",
            SemanticChangeKind::Renamed {
                from: "a".into(),
                to: "b".into()
            }
        ),
        "renamed (a -> b)"
    );
}

#[test]
fn line_change_kind_display() {
    assert_eq!(format!("{}", LineChangeKind::Added), "+");
    assert_eq!(format!("{}", LineChangeKind::Removed), "-");
}

// ═══════════════════════════════════════════════════════════════════════════
// Semantic diff serde roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn semantic_change_kind_serde_roundtrip() {
    let kinds = vec![
        SemanticChangeKind::Added,
        SemanticChangeKind::Modified,
        SemanticChangeKind::Deleted,
        SemanticChangeKind::Renamed {
            from: "old.rs".into(),
            to: "new.rs".into(),
        },
    ];
    for kind in &kinds {
        let json = serde_json::to_string(kind).unwrap();
        let back: SemanticChangeKind = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, kind);
    }
}

#[test]
fn semantic_diff_serde_roundtrip() {
    let sd = SemanticDiff {
        files: vec![SemanticFileChange {
            path: "main.rs".into(),
            kind: SemanticChangeKind::Added,
            additions: 5,
            deletions: 0,
            line_changes: vec![LineChange {
                line_number: 1,
                kind: LineChangeKind::Added,
                content: "fn main() {}".into(),
            }],
        }],
        total_additions: 5,
        total_deletions: 0,
    };
    let json = serde_json::to_string(&sd).unwrap();
    let back: SemanticDiff = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sd);
}

// ═══════════════════════════════════════════════════════════════════════════
// Git operations tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn git_ops_status_clean_workspace() {
    let ws = make_test_workspace();
    let ops = GitOps::new(ws.path());
    let status = ops.status().unwrap();
    assert!(
        status.is_empty(),
        "clean workspace should have empty status"
    );
}

#[test]
fn git_ops_status_after_modification() {
    let ws = make_test_workspace();
    fs::write(ws.path().join("hello.txt"), "modified\n").unwrap();
    let ops = GitOps::new(ws.path());
    let status = ops.status().unwrap();
    assert!(!status.is_empty());
    assert!(status.iter().any(|e| e.path.contains("hello.txt")));
}

#[test]
fn git_ops_status_new_file() {
    let ws = make_test_workspace();
    fs::write(ws.path().join("new_file.txt"), "new\n").unwrap();
    let ops = GitOps::new(ws.path());
    let status = ops.status().unwrap();
    assert!(status
        .iter()
        .any(|e| e.path.contains("new_file.txt") && e.status == GitFileStatus::Added));
}

#[test]
fn git_ops_diff_empty_on_clean() {
    let ws = make_test_workspace();
    let ops = GitOps::new(ws.path());
    let diff = ops.diff().unwrap();
    assert!(diff.trim().is_empty());
}

#[test]
fn git_ops_diff_shows_changes() {
    let ws = make_test_workspace();
    fs::write(ws.path().join("hello.txt"), "changed content\n").unwrap();
    let ops = GitOps::new(ws.path());
    let diff = ops.diff().unwrap();
    assert!(diff.contains("changed content"));
}

#[test]
fn git_ops_diff_stats_empty() {
    let ws = make_test_workspace();
    let ops = GitOps::new(ws.path());
    let stats = ops.diff_stats().unwrap();
    assert_eq!(stats.files_changed, 0);
    assert_eq!(stats.additions, 0);
    assert_eq!(stats.deletions, 0);
}

#[test]
fn git_ops_diff_stats_with_changes() {
    let ws = make_test_workspace();
    fs::write(ws.path().join("hello.txt"), "line1\nline2\nline3\n").unwrap();
    let ops = GitOps::new(ws.path());
    let stats = ops.diff_stats().unwrap();
    assert_eq!(stats.files_changed, 1);
    assert!(stats.additions > 0);
}

#[test]
fn git_ops_add_and_commit() {
    let ws = make_test_workspace();
    fs::write(ws.path().join("new.txt"), "content\n").unwrap();
    let ops = GitOps::new(ws.path());
    ops.add(&["."]).unwrap();
    let sha = ops.commit("add new file").unwrap();
    assert!(!sha.is_empty());
    assert!(sha.len() >= 7); // at least short hash
}

#[test]
fn git_ops_log() {
    let ws = make_test_workspace();
    let ops = GitOps::new(ws.path());
    let log = ops.log(10).unwrap();
    assert!(!log.is_empty());
    assert_eq!(log[0].message, "baseline");
}

#[test]
fn git_ops_log_after_multiple_commits() {
    let ws = make_test_workspace();
    let ops = GitOps::new(ws.path());
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    ops.commit("commit a").unwrap();
    fs::write(ws.path().join("b.txt"), "b\n").unwrap();
    ops.commit("commit b").unwrap();
    let log = ops.log(10).unwrap();
    assert!(log.len() >= 3);
    assert_eq!(log[0].message, "commit b");
    assert_eq!(log[1].message, "commit a");
}

#[test]
fn git_ops_workspace_path() {
    let ws = make_test_workspace();
    let ops = GitOps::new(ws.path());
    assert_eq!(ops.workspace_path(), ws.path());
}

// ── Git ops serde roundtrips ───────────────────────────────────────────────

#[test]
fn git_status_entry_serde_roundtrip() {
    let entry = GitStatusEntry {
        path: "src/main.rs".to_string(),
        status: GitFileStatus::Modified,
        raw_status: " M".to_string(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: GitStatusEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entry);
}

#[test]
fn diff_stats_serde_roundtrip() {
    let stats = DiffStats {
        files_changed: 3,
        additions: 42,
        deletions: 7,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let back: DiffStats = serde_json::from_str(&json).unwrap();
    assert_eq!(back, stats);
}

#[test]
fn log_entry_serde_roundtrip() {
    let entry = LogEntry {
        sha: "abc123".to_string(),
        author: "test".to_string(),
        message: "hello".to_string(),
        timestamp: "2024-01-01T00:00:00+00:00".to_string(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: LogEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entry);
}

// ═══════════════════════════════════════════════════════════════════════════
// Approval workflow tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn approval_submit_starts_pending() {
    let diff = WorkspaceDiff::default();
    let approval = ChangeApproval::submit_changes(diff);
    assert!(approval.is_pending());
    assert!(!approval.is_approved());
    assert!(!approval.is_rejected());
}

#[test]
fn approval_approve_workflow() {
    let diff = WorkspaceDiff::default();
    let mut approval = ChangeApproval::submit_changes(diff);
    approval.approve().unwrap();
    assert!(approval.is_approved());
    assert!(!approval.is_pending());
}

#[test]
fn approval_reject_workflow() {
    let diff = WorkspaceDiff::default();
    let mut approval = ChangeApproval::submit_changes(diff);
    approval.reject("too many changes").unwrap();
    assert!(approval.is_rejected());
    match approval.status() {
        ApprovalStatus::Rejected { reason, .. } => {
            assert_eq!(reason, "too many changes");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[test]
fn approval_apply_after_approve() {
    let diff = make_workspace_diff(vec![("new.rs", 10, 0)], vec![], vec![]);
    let mut approval = ChangeApproval::submit_changes(diff.clone());
    approval.approve().unwrap();
    let applied = approval.apply().unwrap();
    assert_eq!(applied.files_added.len(), diff.files_added.len());
}

#[test]
fn approval_apply_without_approve_fails() {
    let diff = WorkspaceDiff::default();
    let approval = ChangeApproval::submit_changes(diff);
    assert!(approval.apply().is_err());
}

#[test]
fn approval_apply_after_reject_fails() {
    let diff = WorkspaceDiff::default();
    let mut approval = ChangeApproval::submit_changes(diff);
    approval.reject("nope").unwrap();
    assert!(approval.apply().is_err());
}

#[test]
fn approval_double_approve_fails() {
    let diff = WorkspaceDiff::default();
    let mut approval = ChangeApproval::submit_changes(diff);
    approval.approve().unwrap();
    assert!(approval.approve().is_err());
}

#[test]
fn approval_double_reject_fails() {
    let diff = WorkspaceDiff::default();
    let mut approval = ChangeApproval::submit_changes(diff);
    approval.reject("first").unwrap();
    assert!(approval.reject("second").is_err());
}

// ── Approval policy tests ──────────────────────────────────────────────────

#[test]
fn approval_policy_permissive_no_review() {
    let policy = ApprovalPolicy::permissive();
    let diff = make_workspace_diff(vec![("a.rs", 100, 0)], vec![("b.rs", 50, 10)], vec![]);
    assert!(!policy.needs_review(&diff));
}

#[test]
fn approval_policy_strict_always_review() {
    let policy = ApprovalPolicy::strict();
    let diff = WorkspaceDiff::default();
    assert!(policy.needs_review(&diff));
}

#[test]
fn approval_policy_max_files_triggers_review() {
    let policy = ApprovalPolicy {
        max_files_without_review: Some(1),
        ..ApprovalPolicy::default()
    };
    let diff = make_workspace_diff(vec![("a.rs", 1, 0), ("b.rs", 1, 0)], vec![], vec![]);
    assert!(policy.needs_review(&diff));
}

#[test]
fn approval_policy_max_files_no_review_when_under() {
    let policy = ApprovalPolicy {
        max_files_without_review: Some(5),
        ..ApprovalPolicy::default()
    };
    let diff = make_workspace_diff(vec![("a.rs", 1, 0)], vec![], vec![]);
    assert!(!policy.needs_review(&diff));
}

#[test]
fn approval_policy_max_additions_triggers_review() {
    let policy = ApprovalPolicy {
        max_additions_without_review: Some(10),
        ..ApprovalPolicy::default()
    };
    let diff = make_workspace_diff(vec![("big.rs", 100, 0)], vec![], vec![]);
    assert!(policy.needs_review(&diff));
}

#[test]
fn approval_policy_sensitive_paths_triggers_review() {
    let policy = ApprovalPolicy {
        sensitive_paths: vec!["*.env".to_string()],
        ..ApprovalPolicy::default()
    };
    let diff = make_workspace_diff(vec![("prod.env", 1, 0)], vec![], vec![]);
    assert!(policy.needs_review(&diff));
}

// ── Approval serde roundtrip ───────────────────────────────────────────────

#[test]
fn approval_status_serde_roundtrip() {
    let statuses = vec![
        ApprovalStatus::Pending,
        ApprovalStatus::Approved {
            approved_at: chrono::Utc::now(),
        },
        ApprovalStatus::Rejected {
            reason: "too big".to_string(),
            rejected_at: chrono::Utc::now(),
        },
    ];
    for status in &statuses {
        let json = serde_json::to_string(status).unwrap();
        let back: ApprovalStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, status);
    }
}

#[test]
fn approval_policy_serde_roundtrip() {
    let policy = ApprovalPolicy {
        max_files_without_review: Some(10),
        max_additions_without_review: Some(500),
        require_review_for_all: false,
        sensitive_paths: vec!["*.pem".to_string()],
    };
    let json = serde_json::to_string(&policy).unwrap();
    let back: ApprovalPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.max_files_without_review,
        policy.max_files_without_review
    );
    assert_eq!(back.sensitive_paths, policy.sensitive_paths);
}

// ═══════════════════════════════════════════════════════════════════════════
// Archive tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn archive_create_and_list() {
    let ws = make_test_workspace();
    let bytes = WorkspaceArchive::create_bytes(ws.path()).unwrap();
    let entries = WorkspaceArchive::list_bytes(&bytes).unwrap();
    assert!(entries.len() >= 2); // hello.txt + src/main.rs
    assert!(entries.iter().any(|e| e.path == "hello.txt"));
    assert!(entries.iter().any(|e| e.path == "src/main.rs"));
}

#[test]
fn archive_create_to_file_and_restore() {
    let ws = make_test_workspace();
    let archive_dir = tempfile::tempdir().unwrap();
    let archive_path = archive_dir.path().join("snapshot.tar.gz");

    let metadata = WorkspaceArchive::create(ws.path(), &archive_path).unwrap();
    assert!(metadata.file_count >= 2);
    assert!(metadata.compressed_size > 0);
    assert!(archive_path.exists());

    let restore_dir = tempfile::tempdir().unwrap();
    WorkspaceArchive::restore(&archive_path, restore_dir.path()).unwrap();
    assert!(restore_dir.path().join("hello.txt").exists());
    assert!(restore_dir.path().join("src/main.rs").exists());
    assert_eq!(
        fs::read_to_string(restore_dir.path().join("hello.txt")).unwrap(),
        "hello world\n"
    );
}

#[test]
fn archive_roundtrip_in_memory() {
    let ws = make_test_workspace();
    let bytes = WorkspaceArchive::create_bytes(ws.path()).unwrap();

    let restore_dir = tempfile::tempdir().unwrap();
    WorkspaceArchive::restore_bytes(&bytes, restore_dir.path()).unwrap();

    assert_eq!(
        fs::read_to_string(restore_dir.path().join("hello.txt")).unwrap(),
        "hello world\n"
    );
    assert_eq!(
        fs::read_to_string(restore_dir.path().join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn archive_excludes_git_directory() {
    let ws = make_test_workspace();
    let bytes = WorkspaceArchive::create_bytes(ws.path()).unwrap();
    let entries = WorkspaceArchive::list_bytes(&bytes).unwrap();
    assert!(!entries.iter().any(|e| e.path.contains(".git")));
}

#[test]
fn archive_list_from_file() {
    let ws = make_test_workspace();
    let archive_dir = tempfile::tempdir().unwrap();
    let archive_path = archive_dir.path().join("test.tar.gz");
    WorkspaceArchive::create(ws.path(), &archive_path).unwrap();

    let entries = WorkspaceArchive::list(&archive_path).unwrap();
    assert!(!entries.is_empty());
}

#[test]
fn archive_restore_preserves_content() {
    let ws = make_test_workspace();
    fs::write(ws.path().join("data.bin"), [0u8, 1, 2, 3, 255]).unwrap();

    let bytes = WorkspaceArchive::create_bytes(ws.path()).unwrap();
    let restore_dir = tempfile::tempdir().unwrap();
    WorkspaceArchive::restore_bytes(&bytes, restore_dir.path()).unwrap();

    assert_eq!(
        fs::read(restore_dir.path().join("data.bin")).unwrap(),
        vec![0u8, 1, 2, 3, 255]
    );
}

#[test]
fn archive_metadata_serde_roundtrip() {
    let meta = ArchiveMetadata {
        file_count: 5,
        uncompressed_size: 1024,
        compressed_size: 512,
        created_at: chrono::Utc::now(),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: ArchiveMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back, meta);
}

#[test]
fn archive_entry_serde_roundtrip() {
    let entry = ArchiveEntry {
        path: "src/main.rs".to_string(),
        size: 42,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: ArchiveEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entry);
}

#[test]
fn archive_empty_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = WorkspaceArchive::create_bytes(tmp.path()).unwrap();
    let entries = WorkspaceArchive::list_bytes(&bytes).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn archive_nested_directories() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("a/b/c")).unwrap();
    fs::write(tmp.path().join("a/b/c/deep.txt"), "deep\n").unwrap();

    let bytes = WorkspaceArchive::create_bytes(tmp.path()).unwrap();
    let entries = WorkspaceArchive::list_bytes(&bytes).unwrap();
    assert!(entries.iter().any(|e| e.path == "a/b/c/deep.txt"));

    let restore_dir = tempfile::tempdir().unwrap();
    WorkspaceArchive::restore_bytes(&bytes, restore_dir.path()).unwrap();
    assert_eq!(
        fs::read_to_string(restore_dir.path().join("a/b/c/deep.txt")).unwrap(),
        "deep\n"
    );
}
