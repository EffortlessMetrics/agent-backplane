// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar pool management — maintains a pool of warm sidecar instances.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Serde helper for `Duration` as milliseconds.
mod duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(val: &Duration, ser: S) -> Result<S::Ok, S::Error> {
        val.as_millis().serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Duration, D::Error> {
        let ms: u64 = u64::deserialize(de)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Configuration for a sidecar pool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Minimum number of warm (idle) instances to maintain.
    pub min_size: usize,
    /// Maximum number of instances allowed in the pool.
    pub max_size: usize,
    /// Kill idle instances after this duration.
    #[serde(with = "duration_millis")]
    pub idle_timeout: Duration,
    /// Interval between health checks on pooled instances.
    #[serde(with = "duration_millis")]
    pub health_check_interval: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_size: 1,
            max_size: 4,
            idle_timeout: Duration::from_secs(300),
            health_check_interval: Duration::from_secs(30),
        }
    }
}

/// State of an individual pool entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoolEntryState {
    /// The entry is available for use.
    Idle,
    /// The entry is currently handling a work order.
    Busy,
    /// The entry is finishing current work but will not accept new work.
    Draining,
    /// The entry has encountered an error and is unusable.
    Failed,
}

/// A single sidecar instance managed by the pool.
#[derive(Clone, Debug)]
pub struct PoolEntry {
    /// Unique identifier for this pool entry.
    pub id: String,
    /// Current state of the entry.
    pub state: PoolEntryState,
    /// When this entry was created.
    pub created_at: Instant,
    /// When this entry was last used (acquired or released).
    pub last_used: Instant,
}

/// Aggregate statistics for a [`SidecarPool`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolStats {
    /// Total number of entries in the pool.
    pub total: usize,
    /// Number of idle entries.
    pub idle: usize,
    /// Number of busy entries.
    pub busy: usize,
    /// Number of draining entries.
    pub draining: usize,
    /// Number of failed entries.
    pub failed: usize,
}

impl PoolStats {
    /// Pool utilization as a fraction (0.0–1.0).
    ///
    /// Returns 0.0 if the pool is empty.
    pub fn utilization(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.busy as f64 / self.total as f64
    }
}

/// Manages a pool of sidecar instances with configurable sizing.
///
/// # Examples
///
/// ```
/// use abp_host::pool::{SidecarPool, PoolConfig, PoolEntryState};
///
/// let pool = SidecarPool::new(PoolConfig::default());
/// assert!(pool.add("s1"));
///
/// let entry = pool.acquire().unwrap();
/// assert_eq!(entry.state, PoolEntryState::Busy);
///
/// pool.release(&entry.id);
/// assert_eq!(pool.idle_count(), 1);
/// ```
#[derive(Debug)]
pub struct SidecarPool {
    config: PoolConfig,
    entries: Mutex<BTreeMap<String, PoolEntry>>,
}

impl SidecarPool {
    /// Create a new, empty pool with the given configuration.
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    /// Return the pool configuration.
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// Add an entry to the pool in the `Idle` state.
    ///
    /// Returns `false` if the pool is already at `max_size`.
    pub fn add(&self, id: impl Into<String>) -> bool {
        let mut entries = self.entries.lock().expect("pool lock poisoned");
        if entries.len() >= self.config.max_size {
            return false;
        }
        let now = Instant::now();
        let id = id.into();
        entries.insert(
            id.clone(),
            PoolEntry {
                id,
                state: PoolEntryState::Idle,
                created_at: now,
                last_used: now,
            },
        );
        true
    }

    /// Acquire an idle entry from the pool, marking it as `Busy`.
    ///
    /// Returns `None` if no idle entry is available.
    pub fn acquire(&self) -> Option<PoolEntry> {
        let mut entries = self.entries.lock().expect("pool lock poisoned");
        let id = entries
            .values()
            .find(|e| e.state == PoolEntryState::Idle)
            .map(|e| e.id.clone())?;
        let entry = entries.get_mut(&id)?;
        entry.state = PoolEntryState::Busy;
        entry.last_used = Instant::now();
        Some(entry.clone())
    }

    /// Release an entry back to the pool, marking it as `Idle`.
    pub fn release(&self, id: &str) {
        let mut entries = self.entries.lock().expect("pool lock poisoned");
        if let Some(entry) = entries.get_mut(id) {
            entry.state = PoolEntryState::Idle;
            entry.last_used = Instant::now();
        }
    }

    /// Mark an entry as `Failed`.
    pub fn mark_failed(&self, id: &str) {
        let mut entries = self.entries.lock().expect("pool lock poisoned");
        if let Some(entry) = entries.get_mut(id) {
            entry.state = PoolEntryState::Failed;
        }
    }

    /// Mark an entry as `Draining` — it will finish current work but not accept new.
    pub fn drain(&self, id: &str) {
        let mut entries = self.entries.lock().expect("pool lock poisoned");
        if let Some(entry) = entries.get_mut(id) {
            entry.state = PoolEntryState::Draining;
        }
    }

    /// Remove an entry from the pool entirely.
    pub fn remove(&self, id: &str) -> Option<PoolEntry> {
        let mut entries = self.entries.lock().expect("pool lock poisoned");
        entries.remove(id)
    }

    /// Number of entries that are `Busy` or `Idle` (actively usable).
    pub fn active_count(&self) -> usize {
        let entries = self.entries.lock().expect("pool lock poisoned");
        entries
            .values()
            .filter(|e| matches!(e.state, PoolEntryState::Idle | PoolEntryState::Busy))
            .count()
    }

    /// Number of entries in the `Idle` state.
    pub fn idle_count(&self) -> usize {
        let entries = self.entries.lock().expect("pool lock poisoned");
        entries
            .values()
            .filter(|e| e.state == PoolEntryState::Idle)
            .count()
    }

    /// Total number of entries in the pool (all states).
    pub fn total_count(&self) -> usize {
        let entries = self.entries.lock().expect("pool lock poisoned");
        entries.len()
    }

    /// Compute aggregate statistics for the pool.
    pub fn stats(&self) -> PoolStats {
        let entries = self.entries.lock().expect("pool lock poisoned");
        let mut stats = PoolStats {
            total: entries.len(),
            idle: 0,
            busy: 0,
            draining: 0,
            failed: 0,
        };
        for entry in entries.values() {
            match entry.state {
                PoolEntryState::Idle => stats.idle += 1,
                PoolEntryState::Busy => stats.busy += 1,
                PoolEntryState::Draining => stats.draining += 1,
                PoolEntryState::Failed => stats.failed += 1,
            }
        }
        stats
    }

    /// Return entries whose idle time exceeds `idle_timeout`.
    pub fn expired_idle_entries(&self) -> Vec<PoolEntry> {
        let entries = self.entries.lock().expect("pool lock poisoned");
        let now = Instant::now();
        entries
            .values()
            .filter(|e| {
                e.state == PoolEntryState::Idle
                    && now.duration_since(e.last_used) > self.config.idle_timeout
            })
            .cloned()
            .collect()
    }
}
