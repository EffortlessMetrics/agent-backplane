// SPDX-License-Identifier: MIT OR Apache-2.0
//! Atomic configuration transactions with rollback, and versioned history.
//!
//! [`ConfigTransaction`] wraps a [`crate::store::ConfigStore`] to provide
//! prepare → commit/rollback semantics.  [`ConfigHistory`] keeps the last
//! *N* config snapshots so the runtime can roll back to a known-good version.

#![allow(dead_code)]

use crate::store::ConfigStore;
use crate::{BackplaneConfig, ConfigError};
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

// ---------------------------------------------------------------------------
// ConfigTransaction
// ---------------------------------------------------------------------------

/// An in-progress configuration update that can be committed or rolled back.
///
/// # Lifecycle
///
/// 1. [`begin`](Self::begin) — captures the current config as a snapshot.
/// 2. [`commit`](Self::commit) — validates and applies the new config.
/// 3. [`rollback`](Self::rollback) — restores the captured snapshot.
///
/// A transaction can only be finalised once; subsequent calls are no-ops.
pub struct ConfigTransaction {
    store: ConfigStore,
    snapshot: Arc<BackplaneConfig>,
    committed: bool,
    rolled_back: bool,
}

impl ConfigTransaction {
    /// Begin a new transaction against `store`.
    pub fn begin(store: &ConfigStore) -> Self {
        let snapshot = store.get();
        Self {
            store: store.clone(),
            snapshot,
            committed: false,
            rolled_back: false,
        }
    }

    /// Validate and apply `new_config`.
    ///
    /// Returns `Ok(())` on success.  On validation failure the store is
    /// **not** modified and the error is returned.
    pub fn commit(&mut self, new_config: BackplaneConfig) -> Result<(), ConfigError> {
        if self.committed || self.rolled_back {
            return Err(ConfigError::MergeConflict {
                reason: "transaction already finalised".into(),
            });
        }
        self.store.update(new_config)?;
        self.committed = true;
        Ok(())
    }

    /// Discard any pending intent and restore the snapshot taken at
    /// [`begin`](Self::begin).
    ///
    /// This always succeeds because the snapshot was already validated when
    /// it was the active config.
    pub fn rollback(&mut self) -> Result<(), ConfigError> {
        if self.committed || self.rolled_back {
            return Err(ConfigError::MergeConflict {
                reason: "transaction already finalised".into(),
            });
        }
        // The snapshot was the *valid* active config, so update should succeed.
        self.store
            .update((*self.snapshot).clone())
            .expect("rollback to known-good config should never fail");
        self.rolled_back = true;
        Ok(())
    }

    /// Whether this transaction has been committed.
    pub fn is_committed(&self) -> bool {
        self.committed
    }

    /// Whether this transaction has been rolled back.
    pub fn is_rolled_back(&self) -> bool {
        self.rolled_back
    }

    /// The snapshot captured at the start of the transaction.
    pub fn snapshot(&self) -> &BackplaneConfig {
        &self.snapshot
    }
}

// ---------------------------------------------------------------------------
// ConfigHistory
// ---------------------------------------------------------------------------

/// Keeps the last *N* config snapshots for rollback support.
///
/// Each call to [`push`](Self::push) stores a version-tagged snapshot.
/// Once the capacity is exceeded the oldest entry is evicted.
pub struct ConfigHistory {
    inner: Arc<RwLock<HistoryInner>>,
}

struct HistoryInner {
    entries: VecDeque<HistoryEntry>,
    capacity: usize,
    next_version: u64,
}

/// A single versioned snapshot.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Monotonic version counter.
    pub version: u64,
    /// The config at this version.
    pub config: BackplaneConfig,
    /// Optional human-readable label.
    pub label: Option<String>,
}

impl ConfigHistory {
    /// Create a new history buffer that retains at most `capacity` entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HistoryInner {
                entries: VecDeque::with_capacity(capacity),
                capacity,
                next_version: 0,
            })),
        }
    }

    /// Record a config snapshot, optionally with a human-readable label.
    pub fn push(&self, config: BackplaneConfig, label: Option<String>) -> u64 {
        let mut inner = self.inner.write().unwrap();
        let version = inner.next_version;
        inner.next_version += 1;
        if inner.entries.len() == inner.capacity {
            inner.entries.pop_front();
        }
        inner.entries.push_back(HistoryEntry {
            version,
            config,
            label,
        });
        version
    }

    /// Number of entries currently stored.
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().entries.len()
    }

    /// `true` when there are no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Maximum entries this history will retain.
    pub fn capacity(&self) -> usize {
        self.inner.read().unwrap().capacity
    }

    /// Retrieve the entry for a given version, if still retained.
    pub fn get(&self, version: u64) -> Option<HistoryEntry> {
        let inner = self.inner.read().unwrap();
        inner.entries.iter().find(|e| e.version == version).cloned()
    }

    /// Retrieve the most recent entry.
    pub fn latest(&self) -> Option<HistoryEntry> {
        let inner = self.inner.read().unwrap();
        inner.entries.back().cloned()
    }

    /// Retrieve the oldest retained entry.
    pub fn oldest(&self) -> Option<HistoryEntry> {
        let inner = self.inner.read().unwrap();
        inner.entries.front().cloned()
    }

    /// Return all entries, oldest first.
    pub fn entries(&self) -> Vec<HistoryEntry> {
        let inner = self.inner.read().unwrap();
        inner.entries.iter().cloned().collect()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.entries.clear();
    }

    /// Rollback a [`ConfigStore`] to the config recorded at `version`.
    ///
    /// Returns `Err` if the version has been evicted.
    pub fn rollback_to(&self, store: &ConfigStore, version: u64) -> Result<(), ConfigError> {
        let entry = self
            .get(version)
            .ok_or_else(|| ConfigError::MergeConflict {
                reason: format!("version {version} not found in history"),
            })?;
        store.update(entry.config)
    }
}

impl Clone for ConfigHistory {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn valid_config() -> BackplaneConfig {
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
        let mut c = valid_config();
        c.log_level = Some("debug".into());
        c
    }

    // -- ConfigTransaction ---------------------------------------------------

    #[test]
    fn transaction_commit_applies_new_config() {
        let store = ConfigStore::new(valid_config());
        let mut tx = ConfigTransaction::begin(&store);
        tx.commit(debug_config()).unwrap();
        assert_eq!(store.get().log_level.as_deref(), Some("debug"));
        assert!(tx.is_committed());
    }

    #[test]
    fn transaction_rollback_restores_snapshot() {
        let store = ConfigStore::new(valid_config());
        let mut tx = ConfigTransaction::begin(&store);
        // The transaction was begun but we decide to rollback without
        // committing a new value — this restores the original.
        tx.rollback().unwrap();
        assert_eq!(store.get().log_level.as_deref(), Some("info"));
        assert!(tx.is_rolled_back());
    }

    #[test]
    fn transaction_commit_rejects_invalid_config() {
        let store = ConfigStore::new(valid_config());
        let mut tx = ConfigTransaction::begin(&store);
        let mut bad = valid_config();
        bad.log_level = Some("INVALID".into());
        assert!(tx.commit(bad).is_err());
        // Store unchanged.
        assert_eq!(store.get().log_level.as_deref(), Some("info"));
        assert!(!tx.is_committed());
    }

    #[test]
    fn transaction_double_commit_is_error() {
        let store = ConfigStore::new(valid_config());
        let mut tx = ConfigTransaction::begin(&store);
        tx.commit(debug_config()).unwrap();
        assert!(tx.commit(valid_config()).is_err());
    }

    #[test]
    fn transaction_double_rollback_is_error() {
        let store = ConfigStore::new(valid_config());
        let mut tx = ConfigTransaction::begin(&store);
        tx.rollback().unwrap();
        assert!(tx.rollback().is_err());
    }

    #[test]
    fn transaction_commit_then_rollback_is_error() {
        let store = ConfigStore::new(valid_config());
        let mut tx = ConfigTransaction::begin(&store);
        tx.commit(debug_config()).unwrap();
        assert!(tx.rollback().is_err());
    }

    #[test]
    fn transaction_snapshot_matches_initial() {
        let store = ConfigStore::new(valid_config());
        let tx = ConfigTransaction::begin(&store);
        assert_eq!(*tx.snapshot(), valid_config());
    }

    #[test]
    fn transaction_version_increments_on_commit() {
        let store = ConfigStore::new(valid_config());
        assert_eq!(store.version(), 0);
        let mut tx = ConfigTransaction::begin(&store);
        tx.commit(debug_config()).unwrap();
        assert_eq!(store.version(), 1);
    }

    // -- ConfigHistory -------------------------------------------------------

    #[test]
    fn history_push_and_get() {
        let h = ConfigHistory::new(5);
        let v = h.push(valid_config(), None);
        assert_eq!(v, 0);
        assert_eq!(h.len(), 1);
        let entry = h.get(0).unwrap();
        assert_eq!(entry.config, valid_config());
    }

    #[test]
    fn history_capacity_eviction() {
        let h = ConfigHistory::new(2);
        h.push(valid_config(), Some("v0".into()));
        h.push(debug_config(), Some("v1".into()));
        h.push(valid_config(), Some("v2".into()));
        assert_eq!(h.len(), 2);
        assert!(h.get(0).is_none()); // evicted
        assert!(h.get(1).is_some());
        assert!(h.get(2).is_some());
    }

    #[test]
    fn history_latest() {
        let h = ConfigHistory::new(5);
        h.push(valid_config(), None);
        h.push(debug_config(), None);
        let latest = h.latest().unwrap();
        assert_eq!(latest.config.log_level.as_deref(), Some("debug"));
    }

    #[test]
    fn history_oldest() {
        let h = ConfigHistory::new(5);
        h.push(valid_config(), Some("first".into()));
        h.push(debug_config(), None);
        let oldest = h.oldest().unwrap();
        assert_eq!(oldest.label.as_deref(), Some("first"));
    }

    #[test]
    fn history_entries_ordered() {
        let h = ConfigHistory::new(5);
        h.push(valid_config(), None);
        h.push(debug_config(), None);
        let entries = h.entries();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].version < entries[1].version);
    }

    #[test]
    fn history_clear() {
        let h = ConfigHistory::new(5);
        h.push(valid_config(), None);
        h.push(debug_config(), None);
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn history_is_empty() {
        let h = ConfigHistory::new(5);
        assert!(h.is_empty());
        h.push(valid_config(), None);
        assert!(!h.is_empty());
    }

    #[test]
    fn history_capacity_accessor() {
        let h = ConfigHistory::new(10);
        assert_eq!(h.capacity(), 10);
    }

    #[test]
    fn history_rollback_to() {
        let store = ConfigStore::new(valid_config());
        let h = ConfigHistory::new(5);
        let v0 = h.push(valid_config(), None);
        store.update(debug_config()).unwrap();
        h.push(debug_config(), None);
        // Now store has debug, rollback to v0 (info).
        h.rollback_to(&store, v0).unwrap();
        assert_eq!(store.get().log_level.as_deref(), Some("info"));
    }

    #[test]
    fn history_rollback_evicted_version_fails() {
        let store = ConfigStore::new(valid_config());
        let h = ConfigHistory::new(1);
        h.push(valid_config(), None); // v0
        h.push(debug_config(), None); // v1 — evicts v0
        assert!(h.rollback_to(&store, 0).is_err());
    }

    #[test]
    fn history_clone_shares_state() {
        let h = ConfigHistory::new(5);
        h.push(valid_config(), None);
        let h2 = h.clone();
        h2.push(debug_config(), None);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn history_labels_are_optional() {
        let h = ConfigHistory::new(5);
        let v = h.push(valid_config(), None);
        assert!(h.get(v).unwrap().label.is_none());
    }

    #[test]
    fn history_version_counter_is_monotonic() {
        let h = ConfigHistory::new(2);
        let v0 = h.push(valid_config(), None);
        let v1 = h.push(valid_config(), None);
        let v2 = h.push(valid_config(), None);
        assert_eq!(v0, 0);
        assert_eq!(v1, 1);
        assert_eq!(v2, 2);
    }
}
