// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backend-level metrics collection for performance tracking.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, RwLock};

/// Thread-safe, atomic metrics for a single backend.
///
/// # Examples
///
/// ```
/// use abp_integrations::metrics::BackendMetrics;
///
/// let m = BackendMetrics::new();
/// m.record_run(true, 10, 500);
/// m.record_run(false, 5, 300);
///
/// assert_eq!(m.total_runs(), 2);
/// assert!((m.success_rate() - 0.5).abs() < f64::EPSILON);
/// assert!((m.average_duration_ms() - 400.0).abs() < f64::EPSILON);
/// ```
pub struct BackendMetrics {
    total_runs: AtomicU64,
    successful_runs: AtomicU64,
    failed_runs: AtomicU64,
    total_events: AtomicU64,
    total_duration_ms: AtomicU64,
}

impl BackendMetrics {
    /// Create a new, zero-initialised metrics collector.
    #[must_use]
    pub fn new() -> Self {
        Self {
            total_runs: AtomicU64::new(0),
            successful_runs: AtomicU64::new(0),
            failed_runs: AtomicU64::new(0),
            total_events: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
        }
    }

    /// Record the outcome of a single backend run.
    pub fn record_run(&self, success: bool, events: u64, duration_ms: u64) {
        self.total_runs.fetch_add(1, Relaxed);
        if success {
            self.successful_runs.fetch_add(1, Relaxed);
        } else {
            self.failed_runs.fetch_add(1, Relaxed);
        }
        self.total_events.fetch_add(events, Relaxed);
        self.total_duration_ms.fetch_add(duration_ms, Relaxed);
    }

    /// Fraction of runs that succeeded (0.0 if no runs recorded).
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        let total = self.total_runs.load(Relaxed);
        if total == 0 {
            return 0.0;
        }
        self.successful_runs.load(Relaxed) as f64 / total as f64
    }

    /// Average run duration in milliseconds (0.0 if no runs recorded).
    #[must_use]
    pub fn average_duration_ms(&self) -> f64 {
        let total = self.total_runs.load(Relaxed);
        if total == 0 {
            return 0.0;
        }
        self.total_duration_ms.load(Relaxed) as f64 / total as f64
    }

    /// Average number of events per run (0.0 if no runs recorded).
    #[must_use]
    pub fn average_events_per_run(&self) -> f64 {
        let total = self.total_runs.load(Relaxed);
        if total == 0 {
            return 0.0;
        }
        self.total_events.load(Relaxed) as f64 / total as f64
    }

    /// Total number of runs recorded.
    #[must_use]
    pub fn total_runs(&self) -> u64 {
        self.total_runs.load(Relaxed)
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.total_runs.store(0, Relaxed);
        self.successful_runs.store(0, Relaxed);
        self.failed_runs.store(0, Relaxed);
        self.total_events.store(0, Relaxed);
        self.total_duration_ms.store(0, Relaxed);
    }

    /// Take a point-in-time snapshot of the current metric values.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        let total_runs = self.total_runs.load(Relaxed);
        let successful_runs = self.successful_runs.load(Relaxed);
        let failed_runs = self.failed_runs.load(Relaxed);
        let total_events = self.total_events.load(Relaxed);
        let total_duration_ms = self.total_duration_ms.load(Relaxed);

        let success_rate = if total_runs == 0 {
            0.0
        } else {
            successful_runs as f64 / total_runs as f64
        };
        let average_duration_ms = if total_runs == 0 {
            0.0
        } else {
            total_duration_ms as f64 / total_runs as f64
        };
        let average_events_per_run = if total_runs == 0 {
            0.0
        } else {
            total_events as f64 / total_runs as f64
        };

        MetricsSnapshot {
            total_runs,
            successful_runs,
            failed_runs,
            total_events,
            total_duration_ms,
            success_rate,
            average_duration_ms,
            average_events_per_run,
        }
    }
}

impl Default for BackendMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for BackendMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendMetrics")
            .field("total_runs", &self.total_runs.load(Relaxed))
            .field("successful_runs", &self.successful_runs.load(Relaxed))
            .field("failed_runs", &self.failed_runs.load(Relaxed))
            .field("total_events", &self.total_events.load(Relaxed))
            .field("total_duration_ms", &self.total_duration_ms.load(Relaxed))
            .finish()
    }
}

/// Non-atomic, serialisable snapshot of [`BackendMetrics`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Total number of runs recorded.
    pub total_runs: u64,
    /// Number of runs that completed successfully.
    pub successful_runs: u64,
    /// Number of runs that failed.
    pub failed_runs: u64,
    /// Cumulative number of events across all runs.
    pub total_events: u64,
    /// Cumulative duration across all runs in milliseconds.
    pub total_duration_ms: u64,
    /// Fraction of runs that succeeded.
    pub success_rate: f64,
    /// Average run duration in milliseconds.
    pub average_duration_ms: f64,
    /// Average number of events per run.
    pub average_events_per_run: f64,
}

/// Registry mapping backend names to their [`BackendMetrics`].
pub struct MetricsRegistry {
    backends: RwLock<BTreeMap<String, Arc<BackendMetrics>>>,
}

impl MetricsRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            backends: RwLock::new(BTreeMap::new()),
        }
    }

    /// Get existing metrics for `name`, or create a new entry.
    pub fn get_or_create(&self, name: &str) -> Arc<BackendMetrics> {
        // Fast path: read lock.
        {
            let map = self.backends.read().expect("metrics registry poisoned");
            if let Some(m) = map.get(name) {
                return Arc::clone(m);
            }
        }
        // Slow path: write lock.
        let mut map = self.backends.write().expect("metrics registry poisoned");
        Arc::clone(
            map.entry(name.to_string())
                .or_insert_with(|| Arc::new(BackendMetrics::new())),
        )
    }

    /// Take a snapshot of every registered backend's metrics.
    #[must_use]
    pub fn snapshot_all(&self) -> BTreeMap<String, MetricsSnapshot> {
        let map = self.backends.read().expect("metrics registry poisoned");
        map.iter().map(|(k, v)| (k.clone(), v.snapshot())).collect()
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MetricsRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let map = self.backends.read().expect("metrics registry poisoned");
        f.debug_struct("MetricsRegistry")
            .field("backends", &map.keys().collect::<Vec<_>>())
            .finish()
    }
}
