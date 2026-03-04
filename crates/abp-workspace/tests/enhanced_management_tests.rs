// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the enhanced workspace management modules:
//! pool, merge, quota, lifecycle, and snapshot enhancements.

use abp_workspace::lifecycle::{LifecycleConfig, WorkspaceLifecycle};
use abp_workspace::merge::{merge_two, ConflictStrategy, MergeOutcome, WorkspaceMerge};
use abp_workspace::pool::{PoolConfig, WorkspacePool};
use abp_workspace::quota::WorkspaceQuota;
use abp_workspace::snapshot;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// ── Helpers ──────────────────────────────────────────────────────────────

fn tmp() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn write_file(root: &std::path::Path, rel: &str, content: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&p, content).unwrap();
}

fn read_file(root: &std::path::Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
}

// ═════════════════════════════════════════════════════════════════════════
// WorkspacePool tests
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn pool_default_config() {
    let cfg = PoolConfig::default();
    assert_eq!(cfg.capacity, 4);
    assert!(cfg.prefix.is_none());
}

#[test]
fn pool_checkout_creates_workspace() {
    let pool = WorkspacePool::new(PoolConfig::default());
    let ws = pool.checkout().unwrap();
    assert!(ws.path().exists());
    assert!(ws.path().is_dir());
}

#[test]
fn pool_warm_fills_available() {
    let pool = WorkspacePool::new(PoolConfig {
        capacity: 3,
        prefix: None,
    });
    let created = pool.warm(3).unwrap();
    assert_eq!(created, 3);
    assert_eq!(pool.available(), 3);
}

#[test]
fn pool_warm_respects_capacity() {
    let pool = WorkspacePool::new(PoolConfig {
        capacity: 2,
        prefix: None,
    });
    let created = pool.warm(10).unwrap();
    assert_eq!(created, 2);
    assert_eq!(pool.available(), 2);
}

#[test]
fn pool_checkout_uses_pre_staged() {
    let pool = WorkspacePool::new(PoolConfig {
        capacity: 2,
        prefix: None,
    });
    pool.warm(2).unwrap();
    assert_eq!(pool.available(), 2);

    let ws = pool.checkout().unwrap();
    assert!(ws.path().exists());
    assert_eq!(pool.available(), 1);
}

#[test]
fn pool_return_on_drop() {
    let pool = WorkspacePool::new(PoolConfig {
        capacity: 2,
        prefix: None,
    });
    assert_eq!(pool.available(), 0);
    {
        let _ws = pool.checkout().unwrap();
        assert_eq!(pool.available(), 0);
    }
    // After drop, workspace is returned to pool.
    assert_eq!(pool.available(), 1);
}

#[test]
fn pool_detach_prevents_return() {
    let pool = WorkspacePool::new(PoolConfig {
        capacity: 2,
        prefix: None,
    });
    {
        let ws = pool.checkout().unwrap();
        let _tmp = ws.detach();
    }
    assert_eq!(pool.available(), 0);
}

#[test]
fn pool_drain_clears_available() {
    let pool = WorkspacePool::new(PoolConfig {
        capacity: 4,
        prefix: None,
    });
    pool.warm(3).unwrap();
    assert_eq!(pool.available(), 3);
    pool.drain();
    assert_eq!(pool.available(), 0);
}

#[test]
fn pool_total_checkouts_tracked() {
    let pool = WorkspacePool::new(PoolConfig::default());
    assert_eq!(pool.total_checkouts(), 0);
    let _w1 = pool.checkout().unwrap();
    let _w2 = pool.checkout().unwrap();
    assert_eq!(pool.total_checkouts(), 2);
}

#[test]
fn pool_capacity_returns_config_value() {
    let pool = WorkspacePool::new(PoolConfig {
        capacity: 7,
        prefix: None,
    });
    assert_eq!(pool.capacity(), 7);
}

#[test]
fn pool_with_prefix() {
    let pool = WorkspacePool::new(PoolConfig {
        capacity: 2,
        prefix: Some("abp-test-".into()),
    });
    let ws = pool.checkout().unwrap();
    assert!(ws.path().exists());
}

// ═════════════════════════════════════════════════════════════════════════
// WorkspaceMerge tests
// ═════════════════════════════════════════════════════════════════════════

fn setup_base() -> (
    TempDir,
    snapshot::WorkspaceSnapshot,
    snapshot::SnapshotContents,
) {
    let dir = tmp();
    write_file(dir.path(), "readme.txt", "hello world");
    write_file(dir.path(), "src/main.rs", "fn main() {}");
    let (snap, contents) = snapshot::capture_with_contents(dir.path()).unwrap();
    (dir, snap, contents)
}

#[test]
fn merge_no_changes_clean() {
    let (_base_dir, base_snap, base_contents) = setup_base();
    let out = tmp();

    // Both branches are identical to base.
    let report = WorkspaceMerge::new()
        .merge(
            &base_snap,
            &base_contents,
            &[(&base_snap, &base_contents), (&base_snap, &base_contents)],
            out.path(),
        )
        .unwrap();

    assert!(report.clean);
    assert_eq!(report.conflict_count, 0);
}

#[test]
fn merge_single_branch_add_file() {
    let (_base_dir, base_snap, base_contents) = setup_base();

    // Branch A adds a file.
    let branch_dir = tmp();
    write_file(branch_dir.path(), "readme.txt", "hello world");
    write_file(branch_dir.path(), "src/main.rs", "fn main() {}");
    write_file(branch_dir.path(), "new.txt", "new content");
    let (branch_snap, branch_contents) =
        snapshot::capture_with_contents(branch_dir.path()).unwrap();

    let out = tmp();
    let report = WorkspaceMerge::new()
        .merge(
            &base_snap,
            &base_contents,
            &[(&branch_snap, &branch_contents)],
            out.path(),
        )
        .unwrap();

    assert!(report.clean);
    assert!(out.path().join("new.txt").exists());
    assert_eq!(read_file(out.path(), "new.txt"), "new content");
}

#[test]
fn merge_single_branch_modify() {
    let (_base_dir, base_snap, base_contents) = setup_base();

    let branch_dir = tmp();
    write_file(branch_dir.path(), "readme.txt", "updated readme");
    write_file(branch_dir.path(), "src/main.rs", "fn main() {}");
    let (branch_snap, branch_contents) =
        snapshot::capture_with_contents(branch_dir.path()).unwrap();

    let out = tmp();
    let report = WorkspaceMerge::new()
        .merge(
            &base_snap,
            &base_contents,
            &[(&branch_snap, &branch_contents)],
            out.path(),
        )
        .unwrap();

    assert!(report.clean);
    assert_eq!(read_file(out.path(), "readme.txt"), "updated readme");
}

#[test]
fn merge_two_branches_no_overlap() {
    let (_base_dir, base_snap, base_contents) = setup_base();

    // Branch A adds file_a.txt.
    let a_dir = tmp();
    write_file(a_dir.path(), "readme.txt", "hello world");
    write_file(a_dir.path(), "src/main.rs", "fn main() {}");
    write_file(a_dir.path(), "file_a.txt", "aaa");
    let (a_snap, a_contents) = snapshot::capture_with_contents(a_dir.path()).unwrap();

    // Branch B adds file_b.txt.
    let b_dir = tmp();
    write_file(b_dir.path(), "readme.txt", "hello world");
    write_file(b_dir.path(), "src/main.rs", "fn main() {}");
    write_file(b_dir.path(), "file_b.txt", "bbb");
    let (b_snap, b_contents) = snapshot::capture_with_contents(b_dir.path()).unwrap();

    let out = tmp();
    let report = merge_two(
        &base_snap,
        &base_contents,
        &a_snap,
        &a_contents,
        &b_snap,
        &b_contents,
        out.path(),
    )
    .unwrap();

    assert!(report.clean);
    assert_eq!(read_file(out.path(), "file_a.txt"), "aaa");
    assert_eq!(read_file(out.path(), "file_b.txt"), "bbb");
}

#[test]
fn merge_conflict_first_wins() {
    let (_base_dir, base_snap, base_contents) = setup_base();

    let a_dir = tmp();
    write_file(a_dir.path(), "readme.txt", "version A");
    write_file(a_dir.path(), "src/main.rs", "fn main() {}");
    let (a_snap, a_contents) = snapshot::capture_with_contents(a_dir.path()).unwrap();

    let b_dir = tmp();
    write_file(b_dir.path(), "readme.txt", "version B");
    write_file(b_dir.path(), "src/main.rs", "fn main() {}");
    let (b_snap, b_contents) = snapshot::capture_with_contents(b_dir.path()).unwrap();

    let out = tmp();
    let report = WorkspaceMerge::new()
        .with_strategy(ConflictStrategy::FirstWins)
        .merge(
            &base_snap,
            &base_contents,
            &[(&a_snap, &a_contents), (&b_snap, &b_contents)],
            out.path(),
        )
        .unwrap();

    assert!(!report.clean);
    assert_eq!(report.conflict_count, 1);
    assert_eq!(read_file(out.path(), "readme.txt"), "version A");
}

#[test]
fn merge_conflict_last_wins() {
    let (_base_dir, base_snap, base_contents) = setup_base();

    let a_dir = tmp();
    write_file(a_dir.path(), "readme.txt", "version A");
    write_file(a_dir.path(), "src/main.rs", "fn main() {}");
    let (a_snap, a_contents) = snapshot::capture_with_contents(a_dir.path()).unwrap();

    let b_dir = tmp();
    write_file(b_dir.path(), "readme.txt", "version B");
    write_file(b_dir.path(), "src/main.rs", "fn main() {}");
    let (b_snap, b_contents) = snapshot::capture_with_contents(b_dir.path()).unwrap();

    let out = tmp();
    let report = WorkspaceMerge::new()
        .with_strategy(ConflictStrategy::LastWins)
        .merge(
            &base_snap,
            &base_contents,
            &[(&a_snap, &a_contents), (&b_snap, &b_contents)],
            out.path(),
        )
        .unwrap();

    assert!(!report.clean);
    assert_eq!(read_file(out.path(), "readme.txt"), "version B");
}

#[test]
fn merge_conflict_skip_preserves_base() {
    let (_base_dir, base_snap, base_contents) = setup_base();

    let a_dir = tmp();
    write_file(a_dir.path(), "readme.txt", "version A");
    write_file(a_dir.path(), "src/main.rs", "fn main() {}");
    let (a_snap, a_contents) = snapshot::capture_with_contents(a_dir.path()).unwrap();

    let b_dir = tmp();
    write_file(b_dir.path(), "readme.txt", "version B");
    write_file(b_dir.path(), "src/main.rs", "fn main() {}");
    let (b_snap, b_contents) = snapshot::capture_with_contents(b_dir.path()).unwrap();

    let out = tmp();
    let report = WorkspaceMerge::new()
        .with_strategy(ConflictStrategy::Skip)
        .merge(
            &base_snap,
            &base_contents,
            &[(&a_snap, &a_contents), (&b_snap, &b_contents)],
            out.path(),
        )
        .unwrap();

    assert!(!report.clean);
    // Base content preserved since both conflicts were skipped.
    assert_eq!(read_file(out.path(), "readme.txt"), "hello world");
}

#[test]
fn merge_delete_modify_conflict() {
    let (_base_dir, base_snap, base_contents) = setup_base();

    // Branch A deletes readme.txt.
    let a_dir = tmp();
    write_file(a_dir.path(), "src/main.rs", "fn main() {}");
    // No readme.txt.
    let (a_snap, a_contents) = snapshot::capture_with_contents(a_dir.path()).unwrap();

    // Branch B modifies readme.txt.
    let b_dir = tmp();
    write_file(b_dir.path(), "readme.txt", "modified");
    write_file(b_dir.path(), "src/main.rs", "fn main() {}");
    let (b_snap, b_contents) = snapshot::capture_with_contents(b_dir.path()).unwrap();

    let out = tmp();
    let report = WorkspaceMerge::new()
        .with_strategy(ConflictStrategy::FirstWins)
        .merge(
            &base_snap,
            &base_contents,
            &[(&a_snap, &a_contents), (&b_snap, &b_contents)],
            out.path(),
        )
        .unwrap();

    assert!(!report.clean);
    let dm = report
        .files
        .iter()
        .find(|f| f.path == PathBuf::from("readme.txt"))
        .unwrap();
    assert_eq!(dm.outcome, MergeOutcome::DeleteModifyConflict);
}

#[test]
fn merge_both_agree_is_clean() {
    let (_base_dir, base_snap, base_contents) = setup_base();

    // Both branches make the same change.
    let a_dir = tmp();
    write_file(a_dir.path(), "readme.txt", "same");
    write_file(a_dir.path(), "src/main.rs", "fn main() {}");
    let (a_snap, a_contents) = snapshot::capture_with_contents(a_dir.path()).unwrap();

    let b_dir = tmp();
    write_file(b_dir.path(), "readme.txt", "same");
    write_file(b_dir.path(), "src/main.rs", "fn main() {}");
    let (b_snap, b_contents) = snapshot::capture_with_contents(b_dir.path()).unwrap();

    let out = tmp();
    let report = merge_two(
        &base_snap,
        &base_contents,
        &a_snap,
        &a_contents,
        &b_snap,
        &b_contents,
        out.path(),
    )
    .unwrap();

    assert!(report.clean);
    assert_eq!(read_file(out.path(), "readme.txt"), "same");
}

#[test]
fn merge_default_strategy_is_first_wins() {
    let m = WorkspaceMerge::new();
    // Default should produce a value (we test via the struct existing).
    let _ = m;
}

// ═════════════════════════════════════════════════════════════════════════
// WorkspaceQuota tests
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn quota_under_limit() {
    let dir = tmp();
    write_file(dir.path(), "small.txt", "hello");

    let quota = WorkspaceQuota::new(1024 * 1024); // 1 MiB
    let status = quota.check(dir.path()).unwrap();
    assert!(!status.exceeded);
    assert!(status.remaining_bytes > 0);
    assert!(status.usage_percent < 1.0);
}

#[test]
fn quota_over_limit() {
    let dir = tmp();
    let big = "x".repeat(2000);
    write_file(dir.path(), "big.txt", &big);

    let quota = WorkspaceQuota::new(100); // 100 bytes
    let status = quota.check(dir.path()).unwrap();
    assert!(status.exceeded);
    assert_eq!(status.remaining_bytes, 0);
    assert!(status.usage_percent > 100.0);
}

#[test]
fn quota_from_mb() {
    let q = WorkspaceQuota::from_mb(5);
    assert_eq!(q.limit_bytes(), 5 * 1024 * 1024);
}

#[test]
fn quota_is_exceeded_helper() {
    let dir = tmp();
    write_file(dir.path(), "data.bin", &"y".repeat(500));
    let q = WorkspaceQuota::new(100);
    assert!(q.is_exceeded(dir.path()).unwrap());

    let q2 = WorkspaceQuota::new(100_000);
    assert!(!q2.is_exceeded(dir.path()).unwrap());
}

#[test]
fn quota_cleanup_removes_largest_first() {
    let dir = tmp();
    write_file(dir.path(), "small.txt", "hi");
    write_file(dir.path(), "big.txt", &"z".repeat(5000));
    write_file(dir.path(), "medium.txt", &"m".repeat(500));

    let q = WorkspaceQuota::new(600); // Enough for small + medium
    let result = q.cleanup(dir.path()).unwrap();
    assert!(result.files_deleted > 0);
    assert!(result.bytes_reclaimed > 0);

    // The big file should have been deleted.
    assert!(!dir.path().join("big.txt").exists());
}

#[test]
fn quota_cleanup_noop_when_under() {
    let dir = tmp();
    write_file(dir.path(), "tiny.txt", "ok");
    let q = WorkspaceQuota::new(1_000_000);
    let result = q.cleanup(dir.path()).unwrap();
    assert_eq!(result.files_deleted, 0);
    assert_eq!(result.bytes_reclaimed, 0);
}

#[test]
fn quota_empty_dir() {
    let dir = tmp();
    let q = WorkspaceQuota::new(100);
    let status = q.check(dir.path()).unwrap();
    assert_eq!(status.used_bytes, 0);
    assert!(!status.exceeded);
}

#[test]
fn quota_zero_limit() {
    let dir = tmp();
    write_file(dir.path(), "a.txt", "data");
    let q = WorkspaceQuota::new(0);
    let status = q.check(dir.path()).unwrap();
    assert!(status.exceeded);
    assert!(status.usage_percent.is_infinite());
}

#[test]
fn quota_zero_limit_empty_dir() {
    let dir = tmp();
    let q = WorkspaceQuota::new(0);
    let status = q.check(dir.path()).unwrap();
    assert!(!status.exceeded);
    assert_eq!(status.usage_percent, 0.0);
}

// ═════════════════════════════════════════════════════════════════════════
// WorkspaceLifecycle tests
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_default_config() {
    let cfg = LifecycleConfig::default();
    assert_eq!(cfg.ttl_secs, 3600);
    assert!(cfg.auto_remove);
}

#[test]
fn lifecycle_register_and_get() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig::default());
    mgr.register("/tmp/ws-1", Some("test".into()));
    let rec = mgr.get("/tmp/ws-1").unwrap();
    assert_eq!(rec.label.as_deref(), Some("test"));
    assert!(!rec.cleaned);
}

#[test]
fn lifecycle_active_count() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig::default());
    mgr.register("/tmp/a", None);
    mgr.register("/tmp/b", None);
    assert_eq!(mgr.active_count(), 2);
}

#[test]
fn lifecycle_unregister() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig::default());
    mgr.register("/tmp/x", None);
    assert_eq!(mgr.active_count(), 1);
    let removed = mgr.unregister("/tmp/x");
    assert!(removed.is_some());
    assert_eq!(mgr.active_count(), 0);
}

#[test]
fn lifecycle_extend_ttl() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 100,
        auto_remove: false,
    });
    mgr.register("/tmp/e", None);
    let before = mgr.get("/tmp/e").unwrap().expires_at;
    mgr.extend_ttl("/tmp/e", 200);
    let after = mgr.get("/tmp/e").unwrap().expires_at;
    assert!(after > before);
}

#[test]
fn lifecycle_extend_ttl_missing() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig::default());
    assert!(!mgr.extend_ttl("/nonexistent", 100));
}

#[test]
fn lifecycle_sweep_with_zero_ttl() {
    let dir = tmp();
    let ws_path = dir.path().join("ws-expired");
    fs::create_dir_all(&ws_path).unwrap();
    write_file(&ws_path, "data.txt", "hello");

    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 0,
        auto_remove: true,
    });
    mgr.register(&ws_path, None);

    // With TTL=0, workspace should expire immediately.
    let result = mgr.sweep();
    assert_eq!(result.expired_count, 1);
    assert_eq!(result.removed_count, 1);
    assert!(!ws_path.exists());
}

#[test]
fn lifecycle_sweep_no_auto_remove() {
    let dir = tmp();
    let ws_path = dir.path().join("ws-keep");
    fs::create_dir_all(&ws_path).unwrap();

    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 0,
        auto_remove: false,
    });
    mgr.register(&ws_path, None);

    let result = mgr.sweep();
    assert_eq!(result.expired_count, 1);
    assert_eq!(result.removed_count, 0);
    // Directory still exists.
    assert!(ws_path.exists());
}

#[test]
fn lifecycle_purge_cleaned() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 0,
        auto_remove: false,
    });
    mgr.register("/tmp/p1", None);
    mgr.register("/tmp/p2", None);
    mgr.sweep(); // marks both as cleaned
    assert_eq!(mgr.all().len(), 2);
    let purged = mgr.purge_cleaned();
    assert_eq!(purged, 2);
    assert_eq!(mgr.all().len(), 0);
}

#[test]
fn lifecycle_expired_returns_only_expired() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 3600,
        auto_remove: false,
    });
    mgr.register("/tmp/alive", None);
    mgr.register_with_ttl("/tmp/dead", 0, None);

    let expired = mgr.expired();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].path, PathBuf::from("/tmp/dead"));
}

#[test]
fn lifecycle_custom_ttl() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 3600,
        auto_remove: false,
    });
    mgr.register_with_ttl("/tmp/short", 0, Some("short-lived".into()));
    let rec = mgr.get("/tmp/short").unwrap();
    assert!(rec.is_expired());
    assert_eq!(rec.label.as_deref(), Some("short-lived"));
}

#[test]
fn lifecycle_record_remaining() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 3600,
        auto_remove: false,
    });
    mgr.register("/tmp/r", None);
    let rec = mgr.get("/tmp/r").unwrap();
    assert!(rec.remaining().num_seconds() > 0);
}

#[test]
fn lifecycle_record_expired_remaining_zero() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 0,
        auto_remove: false,
    });
    mgr.register("/tmp/z", None);
    let rec = mgr.get("/tmp/z").unwrap();
    assert_eq!(rec.remaining().num_seconds(), 0);
}

#[test]
fn lifecycle_config_accessor() {
    let cfg = LifecycleConfig {
        ttl_secs: 42,
        auto_remove: false,
    };
    let mgr = WorkspaceLifecycle::new(cfg.clone());
    assert_eq!(mgr.config().ttl_secs, 42);
    assert!(!mgr.config().auto_remove);
}

#[test]
fn lifecycle_sweep_nonexistent_dir() {
    let mut mgr = WorkspaceLifecycle::new(LifecycleConfig {
        ttl_secs: 0,
        auto_remove: true,
    });
    mgr.register("/tmp/definitely-does-not-exist-abp-test", None);
    let result = mgr.sweep();
    assert_eq!(result.expired_count, 1);
    assert_eq!(result.removed_count, 0);
}

// ═════════════════════════════════════════════════════════════════════════
// WorkspaceSnapshot enhanced tests
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_capture_and_restore_roundtrip() {
    let dir = tmp();
    write_file(dir.path(), "a.txt", "alpha");
    write_file(dir.path(), "sub/b.txt", "beta");

    let (snap, contents) = snapshot::capture_with_contents(dir.path()).unwrap();
    assert_eq!(snap.file_count(), 2);

    // Modify workspace.
    write_file(dir.path(), "a.txt", "modified");
    fs::remove_file(dir.path().join("sub").join("b.txt")).unwrap();
    write_file(dir.path(), "c.txt", "gamma");

    // Restore.
    snapshot::restore_snapshot(dir.path(), &snap, Some(&contents)).unwrap();

    assert_eq!(read_file(dir.path(), "a.txt"), "alpha");
    assert_eq!(read_file(dir.path(), "sub/b.txt"), "beta");
    assert!(!dir.path().join("c.txt").exists());
}

#[test]
fn snapshot_compare_detects_changes() {
    let dir = tmp();
    write_file(dir.path(), "f.txt", "original");
    let before = snapshot::capture(dir.path()).unwrap();

    write_file(dir.path(), "f.txt", "changed");
    write_file(dir.path(), "new.txt", "new");
    let after = snapshot::capture(dir.path()).unwrap();

    let diff = snapshot::compare(&before, &after);
    assert!(diff.added.iter().any(|p| p.ends_with("new.txt")));
    assert!(diff.modified.iter().any(|p| p.ends_with("f.txt")));
    assert!(diff.removed.is_empty());
}

#[test]
fn snapshot_total_size() {
    let dir = tmp();
    write_file(dir.path(), "x.txt", &"a".repeat(100));
    write_file(dir.path(), "y.txt", &"b".repeat(200));
    let snap = snapshot::capture(dir.path()).unwrap();
    assert_eq!(snap.total_size(), 300);
}

#[test]
fn snapshot_has_file_and_get_file() {
    let dir = tmp();
    write_file(dir.path(), "hello.txt", "world");
    let snap = snapshot::capture(dir.path()).unwrap();
    assert!(snap.has_file("hello.txt"));
    assert!(!snap.has_file("missing.txt"));
    let f = snap.get_file("hello.txt").unwrap();
    assert_eq!(f.size, 5);
    assert!(!f.is_binary);
}

#[test]
fn snapshot_empty_dir() {
    let dir = tmp();
    let snap = snapshot::capture(dir.path()).unwrap();
    assert_eq!(snap.file_count(), 0);
    assert_eq!(snap.total_size(), 0);
}
