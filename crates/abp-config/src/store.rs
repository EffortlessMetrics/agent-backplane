// SPDX-License-Identifier: MIT OR Apache-2.0
//! Atomic configuration store with subscription support.
//!
//! [`ConfigStore`] wraps a [`BackplaneConfig`] in an `Arc<RwLock<…>>` so that
//! multiple threads can read the current config cheaply while a single writer
//! can atomically swap in a validated replacement.

#![allow(dead_code, unused_imports)]

use crate::{BackplaneConfig, ConfigError};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, RwLock};

// ---------------------------------------------------------------------------
// ConfigStore
// ---------------------------------------------------------------------------

/// Thread-safe, versioned configuration store with change notification.
///
/// # Examples
///
/// ```
/// use abp_config::{BackplaneConfig, store::ConfigStore};
///
/// let store = ConfigStore::new(BackplaneConfig::default());
/// assert_eq!(store.version(), 0);
///
/// let cfg = store.get();
/// assert_eq!(cfg.log_level.as_deref(), Some("info"));
/// ```
pub struct ConfigStore {
    inner: Arc<RwLock<StoreInner>>,
}

struct StoreInner {
    config: Arc<BackplaneConfig>,
    version: u64,
    subscribers: Vec<Sender<Arc<BackplaneConfig>>>,
}

impl ConfigStore {
    /// Create a new store seeded with `initial` config at version 0.
    pub fn new(initial: BackplaneConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(StoreInner {
                config: Arc::new(initial),
                version: 0,
                subscribers: Vec::new(),
            })),
        }
    }

    /// Return a cheap `Arc` handle to the current configuration.
    pub fn get(&self) -> Arc<BackplaneConfig> {
        let inner = self.inner.read().expect("config store lock poisoned");
        Arc::clone(&inner.config)
    }

    /// Atomically replace the stored config after validation.
    ///
    /// The config is validated with [`crate::validate_config`] before being
    /// applied.  On success all subscribers are notified.  Disconnected
    /// subscriber channels are silently pruned.
    pub fn update(&self, new_config: BackplaneConfig) -> Result<(), ConfigError> {
        // Validate *before* acquiring the write lock.
        crate::validate_config(&new_config)?;

        let mut inner = self.inner.write().expect("config store lock poisoned");
        inner.version += 1;
        let arc = Arc::new(new_config);
        inner.config = Arc::clone(&arc);

        // Notify subscribers, pruning disconnected ones.
        inner.subscribers.retain(|tx| tx.send(Arc::clone(&arc)).is_ok());

        Ok(())
    }

    /// Subscribe to configuration changes.
    ///
    /// Returns a [`Receiver`] that yields `Arc<BackplaneConfig>` each time
    /// [`update`](Self::update) succeeds.
    pub fn subscribe(&self) -> Receiver<Arc<BackplaneConfig>> {
        let (tx, rx) = mpsc::channel();
        let mut inner = self.inner.write().expect("config store lock poisoned");
        inner.subscribers.push(tx);
        rx
    }

    /// Return the current version counter (starts at 0, increments on each
    /// successful [`update`](Self::update)).
    pub fn version(&self) -> u64 {
        let inner = self.inner.read().expect("config store lock poisoned");
        inner.version
    }
}

impl Clone for ConfigStore {
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
    use std::thread;
    use std::time::Duration;

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

    #[test]
    fn get_returns_initial_config() {
        let cfg = valid_config();
        let store = ConfigStore::new(cfg.clone());
        assert_eq!(*store.get(), cfg);
    }

    #[test]
    fn update_replaces_config() {
        let store = ConfigStore::new(valid_config());

        let mut new_cfg = valid_config();
        new_cfg.log_level = Some("debug".into());
        store.update(new_cfg.clone()).unwrap();

        assert_eq!(*store.get(), new_cfg);
    }

    #[test]
    fn update_rejects_invalid_config() {
        let store = ConfigStore::new(valid_config());

        let mut bad = valid_config();
        bad.log_level = Some("INVALID_LEVEL".into());
        assert!(store.update(bad).is_err());

        // Original config is unchanged.
        assert_eq!(store.get().log_level.as_deref(), Some("info"));
    }

    #[test]
    fn version_increments_on_update() {
        let store = ConfigStore::new(valid_config());
        assert_eq!(store.version(), 0);

        store.update(valid_config()).unwrap();
        assert_eq!(store.version(), 1);

        store.update(valid_config()).unwrap();
        assert_eq!(store.version(), 2);
    }

    #[test]
    fn version_does_not_increment_on_failed_update() {
        let store = ConfigStore::new(valid_config());
        assert_eq!(store.version(), 0);

        let mut bad = valid_config();
        bad.log_level = Some("NOPE".into());
        let _ = store.update(bad);
        assert_eq!(store.version(), 0);
    }

    #[test]
    fn subscribe_receives_updates() {
        let store = ConfigStore::new(valid_config());
        let rx = store.subscribe();

        let mut cfg2 = valid_config();
        cfg2.log_level = Some("debug".into());
        store.update(cfg2.clone()).unwrap();

        let received = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(*received, cfg2);
    }

    #[test]
    fn multiple_subscribers_all_notified() {
        let store = ConfigStore::new(valid_config());
        let rx1 = store.subscribe();
        let rx2 = store.subscribe();

        store.update(valid_config()).unwrap();

        assert!(rx1.recv_timeout(Duration::from_secs(1)).is_ok());
        assert!(rx2.recv_timeout(Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn dropped_subscriber_is_pruned() {
        let store = ConfigStore::new(valid_config());
        let rx = store.subscribe();
        drop(rx);

        // Should not panic even though subscriber is gone.
        store.update(valid_config()).unwrap();
    }

    #[test]
    fn concurrent_reads_and_writes() {
        let store = ConfigStore::new(valid_config());
        let store2 = store.clone();

        let writer = thread::spawn(move || {
            for _ in 0..50 {
                let _ = store2.update(valid_config());
            }
        });

        // Concurrent reads should never panic.
        for _ in 0..100 {
            let _cfg = store.get();
            let _v = store.version();
        }

        writer.join().unwrap();
        assert!(store.version() <= 50);
    }

    #[test]
    fn clone_shares_state() {
        let store = ConfigStore::new(valid_config());
        let clone = store.clone();

        store.update(valid_config()).unwrap();
        assert_eq!(clone.version(), 1);
    }
}
