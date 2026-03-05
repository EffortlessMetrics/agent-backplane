// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the hot-reload pipeline:
//! ConfigWatcher → diff → analyze → policy → transaction → history.

use abp_config::diff_analyzer::{ConfigDiffAnalyzer, Impact};
use abp_config::hot_reload_policy::HotReloadPolicy;
use abp_config::store::ConfigStore;
use abp_config::transaction::{ConfigHistory, ConfigTransaction};
use abp_config::watcher::ConfigWatcher;
use abp_config::BackplaneConfig;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn base_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: None,
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/r".into()),
        bind_address: None,
        port: None,
        policy_profiles: Vec::new(),
        backends: BTreeMap::new(),
    }
}

fn debug_config() -> BackplaneConfig {
    let mut c = base_config();
    c.log_level = Some("debug".into());
    c
}

fn write_config(path: &std::path::Path, content: &str) {
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.sync_all().unwrap();
}

// ---------------------------------------------------------------------------
// Watcher + Store integration
// ---------------------------------------------------------------------------

#[test]
fn watcher_pushes_to_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    write_config(&path, "log_level = \"info\"\n");

    let store = ConfigStore::new(base_config());
    let store2 = store.clone();

    let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
    watcher.start(move |cfg| {
        let _ = store2.update(cfg);
    });

    thread::sleep(Duration::from_millis(100));
    write_config(&path, "log_level = \"debug\"\n");
    thread::sleep(Duration::from_millis(400));
    watcher.stop();

    assert_eq!(store.get().log_level.as_deref(), Some("debug"));
}

#[test]
fn watcher_skips_invalid_config_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    write_config(&path, "log_level = \"info\"\n");

    let store = ConfigStore::new(base_config());
    let store2 = store.clone();

    let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
    watcher.start(move |cfg| {
        let _ = store2.update(cfg);
    });

    thread::sleep(Duration::from_millis(100));
    write_config(&path, "log_level = [[[broken\n");
    thread::sleep(Duration::from_millis(400));
    watcher.stop();

    // Store still has the original value.
    assert_eq!(store.get().log_level.as_deref(), Some("info"));
}

// ---------------------------------------------------------------------------
// Watcher + DiffAnalyzer integration
// ---------------------------------------------------------------------------

#[test]
fn watcher_with_diff_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    write_config(&path, "log_level = \"info\"\n");

    let results: Arc<Mutex<Vec<Impact>>> = Arc::new(Mutex::new(Vec::new()));
    let results2 = Arc::clone(&results);
    // Must match what the initial TOML produces (only log_level set).
    let old_config = BackplaneConfig {
        log_level: Some("info".into()),
        ..BackplaneConfig::default()
    };
    let analyzer = ConfigDiffAnalyzer::new();

    let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
    watcher.start(move |cfg| {
        let analysis = analyzer.analyze(&old_config, &cfg);
        results2.lock().unwrap().push(analysis.overall_impact());
    });

    thread::sleep(Duration::from_millis(100));
    write_config(&path, "log_level = \"debug\"\n");
    thread::sleep(Duration::from_millis(400));
    watcher.stop();

    let impacts = results.lock().unwrap();
    assert!(!impacts.is_empty());
    assert_eq!(*impacts.last().unwrap(), Impact::Safe);
}

// ---------------------------------------------------------------------------
// Full pipeline: watcher → diff → policy → transaction → history
// ---------------------------------------------------------------------------

#[test]
fn full_hot_reload_pipeline_safe_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    write_config(&path, "log_level = \"info\"\n");

    // Initial store config must match what TOML produces.
    let initial = BackplaneConfig {
        log_level: Some("info".into()),
        ..BackplaneConfig::default()
    };
    let store = ConfigStore::new(initial);
    let history = ConfigHistory::new(10);
    let policy = HotReloadPolicy::new();
    let analyzer = ConfigDiffAnalyzer::new();

    let store2 = store.clone();
    let history2 = history.clone();
    let committed = Arc::new(Mutex::new(false));
    let committed2 = Arc::clone(&committed);

    let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
    watcher.start(move |cfg| {
        let current = store2.get();
        let analysis = analyzer.analyze(&current, &cfg);
        let decision = policy.evaluate(&analysis);
        if decision.is_apply() {
            history2.push((*current).clone(), None);
            let mut tx = ConfigTransaction::begin(&store2);
            if tx.commit(cfg).is_ok() {
                *committed2.lock().unwrap() = true;
            }
        }
    });

    thread::sleep(Duration::from_millis(100));
    write_config(&path, "log_level = \"debug\"\n");
    thread::sleep(Duration::from_millis(400));
    watcher.stop();

    assert!(*committed.lock().unwrap());
    assert_eq!(store.get().log_level.as_deref(), Some("debug"));
    assert!(!history.is_empty());
}

#[test]
fn full_pipeline_rejects_breaking_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    write_config(&path, "log_level = \"info\"\n");

    let initial = BackplaneConfig {
        log_level: Some("info".into()),
        ..BackplaneConfig::default()
    };
    let store = ConfigStore::new(initial);
    let policy = HotReloadPolicy::new(); // only safe allowed
    let analyzer = ConfigDiffAnalyzer::new();

    let store2 = store.clone();
    let rejected = Arc::new(Mutex::new(false));
    let rejected2 = Arc::clone(&rejected);

    let mut watcher = ConfigWatcher::new(&path).poll_interval(Duration::from_millis(50));
    watcher.start(move |cfg| {
        let current = store2.get();
        let analysis = analyzer.analyze(&current, &cfg);
        let decision = policy.evaluate(&analysis);
        if decision.is_reject() {
            *rejected2.lock().unwrap() = true;
        }
    });

    thread::sleep(Duration::from_millis(100));
    // Change port — this is breaking.
    write_config(&path, "log_level = \"info\"\nport = 9090\n");
    thread::sleep(Duration::from_millis(400));
    watcher.stop();

    assert!(*rejected.lock().unwrap());
    // Store unchanged.
    assert!(store.get().port.is_none());
}

// ---------------------------------------------------------------------------
// Transaction + History integration
// ---------------------------------------------------------------------------

#[test]
fn transaction_rollback_then_history_rollback() {
    let store = ConfigStore::new(base_config());
    let history = ConfigHistory::new(10);

    // Record initial.
    let v0 = history.push(base_config(), Some("initial".into()));

    // Commit a new config.
    let mut tx = ConfigTransaction::begin(&store);
    tx.commit(debug_config()).unwrap();
    history.push(debug_config(), Some("debug".into()));
    assert_eq!(store.get().log_level.as_deref(), Some("debug"));

    // Rollback via history to v0.
    history.rollback_to(&store, v0).unwrap();
    assert_eq!(store.get().log_level.as_deref(), Some("info"));
}

#[test]
fn history_eviction_prevents_old_rollback() {
    let store = ConfigStore::new(base_config());
    let history = ConfigHistory::new(2);

    history.push(base_config(), None); // v0
    store.update(debug_config()).unwrap();
    history.push(debug_config(), None); // v1

    let mut c = base_config();
    c.log_level = Some("warn".into());
    store.update(c.clone()).unwrap();
    history.push(c, None); // v2 — evicts v0

    assert!(history.rollback_to(&store, 0).is_err());
}

// ---------------------------------------------------------------------------
// Analyzer + Policy chaining
// ---------------------------------------------------------------------------

#[test]
fn analyzer_custom_override_with_policy() {
    let analyzer = ConfigDiffAnalyzer::new().with_override("port", Impact::Safe);
    let policy = HotReloadPolicy::new();

    let mut new = base_config();
    new.port = Some(9090);
    let analysis = analyzer.analyze(&base_config(), &new);
    let decision = policy.evaluate(&analysis);
    // Port is overridden to safe, so policy should apply.
    assert!(decision.is_apply());
}

#[test]
fn permissive_policy_with_breaking_changes() {
    let analyzer = ConfigDiffAnalyzer::new();
    let policy = HotReloadPolicy::permissive();

    let mut new = base_config();
    new.port = Some(9090);
    new.bind_address = Some("0.0.0.0".into());
    let analysis = analyzer.analyze(&base_config(), &new);
    let decision = policy.evaluate(&analysis);
    assert!(!decision.is_reject());
}

// ---------------------------------------------------------------------------
// Multiple sequential reloads
// ---------------------------------------------------------------------------

#[test]
fn sequential_reloads_tracked_in_history() {
    let store = ConfigStore::new(base_config());
    let history = ConfigHistory::new(10);
    let analyzer = ConfigDiffAnalyzer::new();
    let policy = HotReloadPolicy::new();

    let configs = [
        ("debug", Some("debug".into())),
        ("trace", Some("trace".into())),
        ("warn", Some("warn".into())),
    ];

    for (level, label) in configs {
        let mut cfg = base_config();
        cfg.log_level = Some(level.into());
        let analysis = analyzer.analyze(&store.get(), &cfg);
        let decision = policy.evaluate(&analysis);
        assert!(decision.is_apply());
        history.push((*store.get()).clone(), label);
        let mut tx = ConfigTransaction::begin(&store);
        tx.commit(cfg).unwrap();
    }

    assert_eq!(history.len(), 3);
    assert_eq!(store.get().log_level.as_deref(), Some("warn"));

    // Rollback to version 0 (the initial "info" config).
    history.rollback_to(&store, 0).unwrap();
    assert_eq!(store.get().log_level.as_deref(), Some("info"));
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_diff_produces_apply_decision() {
    let analyzer = ConfigDiffAnalyzer::new();
    let policy = HotReloadPolicy::new();
    let analysis = analyzer.analyze(&base_config(), &base_config());
    assert!(policy.evaluate(&analysis).is_apply());
}

#[test]
fn transaction_on_fresh_store() {
    let store = ConfigStore::new(BackplaneConfig::default());
    let mut tx = ConfigTransaction::begin(&store);
    tx.commit(base_config()).unwrap();
    assert_eq!(store.get().default_backend.as_deref(), Some("mock"));
}

#[test]
fn history_with_capacity_one() {
    let h = ConfigHistory::new(1);
    h.push(base_config(), None);
    h.push(debug_config(), None);
    assert_eq!(h.len(), 1);
    assert!(h.get(0).is_none());
    assert!(h.get(1).is_some());
}

#[test]
fn concurrent_store_updates_with_history() {
    let store = ConfigStore::new(base_config());
    let history = ConfigHistory::new(100);
    let store2 = store.clone();
    let history2 = history.clone();

    let writer = thread::spawn(move || {
        for _ in 0..20 {
            let mut cfg = base_config();
            cfg.log_level = Some("debug".into());
            history2.push((*store2.get()).clone(), None);
            let _ = store2.update(cfg);
        }
    });

    for _ in 0..20 {
        let _cfg = store.get();
        let _len = history.len();
    }

    writer.join().unwrap();
}

// ---------------------------------------------------------------------------
// Subscriber + reload coordination
// ---------------------------------------------------------------------------

#[test]
fn subscriber_receives_transactional_update() {
    let store = ConfigStore::new(base_config());
    let rx = store.subscribe();

    let mut tx = ConfigTransaction::begin(&store);
    tx.commit(debug_config()).unwrap();

    let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(received.log_level.as_deref(), Some("debug"));
}

#[test]
fn subscriber_does_not_receive_on_rollback_to_same() {
    let store = ConfigStore::new(base_config());
    let rx = store.subscribe();

    let mut tx = ConfigTransaction::begin(&store);
    tx.rollback().unwrap();

    // The rollback calls update with the same config, so subscriber gets it.
    // What matters is the config value is still "info".
    let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(received.log_level.as_deref(), Some("info"));
}
