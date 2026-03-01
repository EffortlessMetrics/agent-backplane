// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the sidecar pool management module.

use abp_host::pool::{PoolConfig, PoolEntryState, PoolStats, SidecarPool};
use std::time::Duration;

// ── PoolConfig defaults ─────────────────────────────────────────────

#[test]
fn pool_config_default_values() {
    let cfg = PoolConfig::default();
    assert_eq!(cfg.min_size, 1);
    assert_eq!(cfg.max_size, 4);
    assert_eq!(cfg.idle_timeout, Duration::from_secs(300));
    assert_eq!(cfg.health_check_interval, Duration::from_secs(30));
}

#[test]
fn pool_config_serde_roundtrip() {
    let cfg = PoolConfig {
        min_size: 2,
        max_size: 8,
        idle_timeout: Duration::from_millis(60_000),
        health_check_interval: Duration::from_millis(5_000),
    };

    let json = serde_json::to_string(&cfg).unwrap();
    let back: PoolConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(back.min_size, cfg.min_size);
    assert_eq!(back.max_size, cfg.max_size);
    assert_eq!(back.idle_timeout, cfg.idle_timeout);
    assert_eq!(back.health_check_interval, cfg.health_check_interval);
}

#[test]
fn pool_config_durations_serialize_as_millis() {
    let cfg = PoolConfig {
        idle_timeout: Duration::from_millis(1500),
        health_check_interval: Duration::from_millis(2500),
        ..Default::default()
    };

    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("1500"), "expected 1500 in json: {json}");
    assert!(json.contains("2500"), "expected 2500 in json: {json}");
}

// ── PoolEntryState ──────────────────────────────────────────────────

#[test]
fn pool_entry_state_variants_are_distinct() {
    let states = [
        PoolEntryState::Idle,
        PoolEntryState::Busy,
        PoolEntryState::Draining,
        PoolEntryState::Failed,
    ];
    for (i, a) in states.iter().enumerate() {
        for (j, b) in states.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

// ── SidecarPool — creation ──────────────────────────────────────────

#[test]
fn new_pool_is_empty() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert_eq!(pool.total_count(), 0);
    assert_eq!(pool.idle_count(), 0);
    assert_eq!(pool.active_count(), 0);
}

#[test]
fn add_entry_to_pool() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    assert_eq!(pool.total_count(), 1);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn add_respects_max_size() {
    let cfg = PoolConfig {
        max_size: 2,
        ..Default::default()
    };
    let pool = SidecarPool::new(cfg);

    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3"), "should reject when at max_size");
    assert_eq!(pool.total_count(), 2);
}

// ── SidecarPool — acquire / release ─────────────────────────────────

#[test]
fn acquire_returns_idle_entry_and_marks_busy() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");

    let entry = pool.acquire().expect("should acquire idle entry");
    assert_eq!(entry.id, "s1");
    assert_eq!(entry.state, PoolEntryState::Busy);

    assert_eq!(pool.idle_count(), 0);
    assert_eq!(pool.active_count(), 1);
}

#[test]
fn acquire_returns_none_when_no_idle() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.acquire(); // marks s1 as Busy

    assert!(pool.acquire().is_none());
}

#[test]
fn release_returns_entry_to_idle() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.acquire();

    pool.release("s1");
    assert_eq!(pool.idle_count(), 1);

    // Can acquire again.
    let entry = pool.acquire().expect("should be acquirable again");
    assert_eq!(entry.id, "s1");
}

// ── SidecarPool — state transitions ─────────────────────────────────

#[test]
fn mark_failed_sets_failed_state() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");

    pool.mark_failed("s1");

    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.idle, 0);
    // Failed entries are not active.
    assert_eq!(pool.active_count(), 0);
}

#[test]
fn drain_sets_draining_state() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");

    pool.drain("s1");

    let stats = pool.stats();
    assert_eq!(stats.draining, 1);
    assert_eq!(stats.idle, 0);
    // Draining entries are not active (can't accept new work).
    assert_eq!(pool.active_count(), 0);
}

#[test]
fn drain_does_not_affect_missing_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.drain("nonexistent");
    assert_eq!(pool.total_count(), 0);
}

#[test]
fn mark_failed_does_not_affect_missing_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.mark_failed("nonexistent");
    assert_eq!(pool.total_count(), 0);
}

// ── SidecarPool — remove ────────────────────────────────────────────

#[test]
fn remove_entry_from_pool() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");

    let removed = pool.remove("s1");
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().id, "s1");
    assert_eq!(pool.total_count(), 1);

    assert!(pool.remove("s1").is_none());
}

// ── PoolStats ───────────────────────────────────────────────────────

#[test]
fn stats_reflect_all_states() {
    let cfg = PoolConfig {
        max_size: 10,
        ..Default::default()
    };
    let pool = SidecarPool::new(cfg);

    pool.add("a_idle");
    pool.add("b_busy");
    pool.add("c_drain");
    pool.add("d_fail");

    // acquire picks first idle in sorted order → "a_idle"
    pool.acquire();
    pool.drain("c_drain");
    pool.mark_failed("d_fail");

    let stats = pool.stats();
    assert_eq!(stats.total, 4);
    assert_eq!(stats.idle, 1); // b_busy remains idle
    assert_eq!(stats.busy, 1); // a_idle was acquired
    assert_eq!(stats.draining, 1);
    assert_eq!(stats.failed, 1);
}

#[test]
fn utilization_empty_pool() {
    let stats = PoolStats {
        total: 0,
        idle: 0,
        busy: 0,
        draining: 0,
        failed: 0,
    };
    assert_eq!(stats.utilization(), 0.0);
}

#[test]
fn utilization_computed_correctly() {
    let stats = PoolStats {
        total: 4,
        idle: 2,
        busy: 2,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn pool_stats_serde_roundtrip() {
    let stats = PoolStats {
        total: 5,
        idle: 2,
        busy: 1,
        draining: 1,
        failed: 1,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let back: PoolStats = serde_json::from_str(&json).unwrap();
    assert_eq!(back, stats);
}

// ── SidecarPool — config access ─────────────────────────────────────

#[test]
fn pool_exposes_config() {
    let cfg = PoolConfig {
        min_size: 3,
        max_size: 10,
        ..Default::default()
    };
    let pool = SidecarPool::new(cfg.clone());
    assert_eq!(pool.config().min_size, 3);
    assert_eq!(pool.config().max_size, 10);
}

// ── SidecarPool — acquire selects idle only ─────────────────────────

#[test]
fn acquire_skips_non_idle_entries() {
    let cfg = PoolConfig {
        max_size: 10,
        ..Default::default()
    };
    let pool = SidecarPool::new(cfg);

    pool.add("failed1");
    pool.add("draining1");
    pool.add("idle1");

    pool.mark_failed("failed1");
    pool.drain("draining1");

    let entry = pool.acquire().expect("should find idle1");
    assert_eq!(entry.id, "idle1");

    // No more idle entries.
    assert!(pool.acquire().is_none());
}

// ── SidecarPool — release after failed ──────────────────────────────

#[test]
fn release_recovers_failed_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.mark_failed("s1");

    // Release overrides state back to Idle.
    pool.release("s1");
    assert_eq!(pool.idle_count(), 1);
    assert!(pool.acquire().is_some());
}
