// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Fine-grained metrics counters, gauges, histograms, timers, and a registry.
//!
//! These types complement the existing [`super::MetricsCollector`] by exposing
//! individual metric primitives that can be updated independently and queried
//! at any time.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ---------------------------------------------------------------------------
// RequestCounter
// ---------------------------------------------------------------------------

/// Counts requests by `(backend, dialect, outcome)` dimensions.
///
/// Thread-safe — all mutations go through an internal `Mutex`.
#[derive(Debug, Clone, Default)]
pub struct RequestCounter {
    inner: Arc<Mutex<BTreeMap<RequestKey, u64>>>,
}

/// Composite key for [`RequestCounter`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RequestKey {
    /// Backend name (e.g. `"mock"`, `"sidecar:node"`).
    pub backend: String,
    /// Dialect / vendor identifier (e.g. `"openai"`, `"anthropic"`).
    pub dialect: String,
    /// Outcome label (e.g. `"success"`, `"error"`, `"timeout"`).
    pub outcome: String,
}

impl RequestCounter {
    /// Create a new, empty counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the counter for the given dimensions by one.
    pub fn increment(&self, backend: &str, dialect: &str, outcome: &str) {
        let key = RequestKey {
            backend: backend.to_string(),
            dialect: dialect.to_string(),
            outcome: outcome.to_string(),
        };
        let mut map = self.inner.lock().expect("request counter lock poisoned");
        *map.entry(key).or_insert(0) += 1;
    }

    /// Get the current count for a specific dimension tuple.
    pub fn get(&self, backend: &str, dialect: &str, outcome: &str) -> u64 {
        let key = RequestKey {
            backend: backend.to_string(),
            dialect: dialect.to_string(),
            outcome: outcome.to_string(),
        };
        let map = self.inner.lock().expect("request counter lock poisoned");
        map.get(&key).copied().unwrap_or(0)
    }

    /// Return a snapshot of all recorded counts.
    pub fn snapshot(&self) -> BTreeMap<RequestKey, u64> {
        let map = self.inner.lock().expect("request counter lock poisoned");
        map.clone()
    }

    /// Total count across all dimension tuples.
    pub fn total(&self) -> u64 {
        let map = self.inner.lock().expect("request counter lock poisoned");
        map.values().sum()
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        let mut map = self.inner.lock().expect("request counter lock poisoned");
        map.clear();
    }
}

// ---------------------------------------------------------------------------
// ErrorCounter
// ---------------------------------------------------------------------------

/// Counts errors by error-code string.
///
/// Thread-safe via an internal `Mutex`.
#[derive(Debug, Clone, Default)]
pub struct ErrorCounter {
    inner: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl ErrorCounter {
    /// Create a new, empty error counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the count for `error_code` by one.
    pub fn increment(&self, error_code: &str) {
        let mut map = self.inner.lock().expect("error counter lock poisoned");
        *map.entry(error_code.to_string()).or_insert(0) += 1;
    }

    /// Get the current count for `error_code`.
    pub fn get(&self, error_code: &str) -> u64 {
        let map = self.inner.lock().expect("error counter lock poisoned");
        map.get(error_code).copied().unwrap_or(0)
    }

    /// Return a snapshot of all error counts.
    pub fn snapshot(&self) -> BTreeMap<String, u64> {
        let map = self.inner.lock().expect("error counter lock poisoned");
        map.clone()
    }

    /// Total number of errors across all codes.
    pub fn total(&self) -> u64 {
        let map = self.inner.lock().expect("error counter lock poisoned");
        map.values().sum()
    }

    /// Reset all counters.
    pub fn reset(&self) {
        let mut map = self.inner.lock().expect("error counter lock poisoned");
        map.clear();
    }
}

// ---------------------------------------------------------------------------
// ActiveRequestGauge
// ---------------------------------------------------------------------------

/// Atomic gauge tracking the number of in-flight requests.
///
/// Use [`increment`](Self::increment) when a request starts and
/// [`decrement`](Self::decrement) when it completes.
#[derive(Debug, Default)]
pub struct ActiveRequestGauge {
    value: AtomicI64,
}

impl ActiveRequestGauge {
    /// Create a gauge initialised to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one to the gauge (request started).
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Subtract one from the gauge (request finished).
    pub fn decrement(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    /// Current gauge value.
    pub fn get(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// TokenAccumulator
// ---------------------------------------------------------------------------

/// Atomic accumulator for token usage (input + output).
#[derive(Debug, Default)]
pub struct TokenAccumulator {
    input: AtomicU64,
    output: AtomicU64,
}

impl TokenAccumulator {
    /// Create an accumulator initialised to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add token counts from a single request.
    pub fn add(&self, input_tokens: u64, output_tokens: u64) {
        self.input.fetch_add(input_tokens, Ordering::Relaxed);
        self.output.fetch_add(output_tokens, Ordering::Relaxed);
    }

    /// Total input tokens recorded so far.
    pub fn total_input(&self) -> u64 {
        self.input.load(Ordering::Relaxed)
    }

    /// Total output tokens recorded so far.
    pub fn total_output(&self) -> u64 {
        self.output.load(Ordering::Relaxed)
    }

    /// Combined total of input + output tokens.
    pub fn total(&self) -> u64 {
        self.total_input() + self.total_output()
    }

    /// Reset both counters to zero.
    pub fn reset(&self) {
        self.input.store(0, Ordering::Relaxed);
        self.output.store(0, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Counter (generic)
// ---------------------------------------------------------------------------

/// A thread-safe monotonically increasing counter.
///
/// Unlike [`RequestCounter`], this is a simple scalar counter without
/// multi-dimensional keys.  Cloning produces a handle to the **same**
/// underlying atomic value.
#[derive(Debug, Default)]
pub struct Counter {
    value: Arc<AtomicU64>,
}

impl Counter {
    /// Create a counter initialised to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the counter by one.
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the counter by `n`.
    pub fn increment_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Current value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset to zero.
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

impl Clone for Counter {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
        }
    }
}

// ---------------------------------------------------------------------------
// Gauge (generic)
// ---------------------------------------------------------------------------

/// A thread-safe gauge that can increase or decrease.
///
/// Cloning produces a handle to the **same** underlying atomic value.
#[derive(Debug, Default)]
pub struct Gauge {
    value: Arc<AtomicI64>,
}

impl Gauge {
    /// Create a gauge initialised to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment by one.
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement by one.
    pub fn decrement(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    /// Add an arbitrary signed value.
    pub fn add(&self, n: i64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Set the gauge to an absolute value.
    pub fn set(&self, n: i64) {
        self.value.store(n, Ordering::Relaxed);
    }

    /// Current value.
    pub fn get(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset to zero.
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

impl Clone for Gauge {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
        }
    }
}

// ---------------------------------------------------------------------------
// Histogram
// ---------------------------------------------------------------------------

/// Distribution tracker with percentile computation.
///
/// Values are stored in an internal `Vec` protected by a `Mutex`.
/// Percentiles are computed on-demand by sorting a snapshot.
#[derive(Debug, Clone, Default)]
pub struct Histogram {
    values: Arc<Mutex<Vec<f64>>>,
}

impl Histogram {
    /// Create an empty histogram.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a value.
    pub fn record(&self, value: f64) {
        let mut vals = self.values.lock().expect("histogram lock poisoned");
        vals.push(value);
    }

    /// Observe a value (alias for [`record`](Self::record)).
    pub fn observe(&self, value: f64) {
        self.record(value);
    }

    /// Number of recorded values.
    pub fn count(&self) -> usize {
        let vals = self.values.lock().expect("histogram lock poisoned");
        vals.len()
    }

    /// Compute the given percentile (0.0–1.0).
    ///
    /// Returns `None` if no values have been recorded.
    pub fn percentile(&self, p: f64) -> Option<f64> {
        let vals = self.values.lock().expect("histogram lock poisoned");
        if vals.is_empty() {
            return None;
        }
        let mut sorted = vals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((p * (sorted.len() - 1) as f64).round()) as usize;
        let idx = idx.min(sorted.len() - 1);
        Some(sorted[idx])
    }

    /// 50th percentile (median).
    pub fn p50(&self) -> Option<f64> {
        self.percentile(0.50)
    }

    /// 90th percentile.
    pub fn p90(&self) -> Option<f64> {
        self.percentile(0.90)
    }

    /// 99th percentile.
    pub fn p99(&self) -> Option<f64> {
        self.percentile(0.99)
    }

    /// Arithmetic mean, or `None` if empty.
    pub fn mean(&self) -> Option<f64> {
        let vals = self.values.lock().expect("histogram lock poisoned");
        if vals.is_empty() {
            return None;
        }
        Some(vals.iter().sum::<f64>() / vals.len() as f64)
    }

    /// Minimum recorded value.
    pub fn min(&self) -> Option<f64> {
        let vals = self.values.lock().expect("histogram lock poisoned");
        vals.iter().copied().reduce(f64::min)
    }

    /// Maximum recorded value.
    pub fn max(&self) -> Option<f64> {
        let vals = self.values.lock().expect("histogram lock poisoned");
        vals.iter().copied().reduce(f64::max)
    }

    /// Return a copy of all recorded values.
    pub fn snapshot(&self) -> Vec<f64> {
        let vals = self.values.lock().expect("histogram lock poisoned");
        vals.clone()
    }

    /// Clear all recorded values.
    pub fn reset(&self) {
        let mut vals = self.values.lock().expect("histogram lock poisoned");
        vals.clear();
    }
}

// ---------------------------------------------------------------------------
// HistogramStats
// ---------------------------------------------------------------------------

/// Summary statistics for a [`Histogram`], used by export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramStats {
    /// Number of observations.
    pub count: usize,
    /// Arithmetic mean.
    pub mean: Option<f64>,
    /// Minimum value.
    pub min: Option<f64>,
    /// Maximum value.
    pub max: Option<f64>,
    /// 50th percentile.
    pub p50: Option<f64>,
    /// 90th percentile.
    pub p90: Option<f64>,
    /// 99th percentile.
    pub p99: Option<f64>,
}

// ---------------------------------------------------------------------------
// Timer
// ---------------------------------------------------------------------------

/// Measures elapsed time and records durations to a [`Histogram`].
#[derive(Debug, Clone)]
pub struct Timer {
    histogram: Histogram,
}

impl Timer {
    /// Create a timer backed by the given histogram.
    pub fn new(histogram: Histogram) -> Self {
        Self { histogram }
    }

    /// Start a timing measurement. Returns a [`TimerGuard`] that records
    /// elapsed milliseconds when stopped or dropped.
    pub fn start(&self) -> TimerGuard {
        TimerGuard {
            start: Instant::now(),
            histogram: self.histogram.clone(),
            stopped: false,
        }
    }

    /// Access the underlying histogram.
    pub fn histogram(&self) -> &Histogram {
        &self.histogram
    }
}

/// Guard returned by [`Timer::start`].
///
/// Records elapsed milliseconds to the backing histogram on
/// [`stop`](Self::stop) or when dropped.
#[derive(Debug)]
pub struct TimerGuard {
    start: Instant,
    histogram: Histogram,
    stopped: bool,
}

impl TimerGuard {
    /// Stop timing and record the elapsed duration. Returns milliseconds.
    pub fn stop(mut self) -> f64 {
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        self.histogram.record(ms);
        self.stopped = true;
        ms
    }

    /// Elapsed time so far without stopping.
    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

impl Drop for TimerGuard {
    fn drop(&mut self) {
        if !self.stopped {
            let ms = self.start.elapsed().as_secs_f64() * 1000.0;
            self.histogram.record(ms);
        }
    }
}

// ---------------------------------------------------------------------------
// MetricsRegistry
// ---------------------------------------------------------------------------

/// Central registry for named metrics.
///
/// Metrics obtained via [`counter`](Self::counter), [`gauge`](Self::gauge),
/// and [`histogram`](Self::histogram) share state with the registry — cloning
/// the returned handle gives a shared view.
#[derive(Debug, Clone, Default)]
pub struct MetricsRegistry {
    counters: Arc<Mutex<BTreeMap<String, Counter>>>,
    gauges: Arc<Mutex<BTreeMap<String, Gauge>>>,
    histograms: Arc<Mutex<BTreeMap<String, Histogram>>>,
}

impl MetricsRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a named counter.
    pub fn counter(&self, name: &str) -> Counter {
        let mut map = self
            .counters
            .lock()
            .expect("counter registry lock poisoned");
        map.entry(name.to_string()).or_default().clone()
    }

    /// Get or create a named gauge.
    pub fn gauge(&self, name: &str) -> Gauge {
        let mut map = self.gauges.lock().expect("gauge registry lock poisoned");
        map.entry(name.to_string()).or_default().clone()
    }

    /// Get or create a named histogram.
    pub fn histogram(&self, name: &str) -> Histogram {
        let mut map = self
            .histograms
            .lock()
            .expect("histogram registry lock poisoned");
        map.entry(name.to_string()).or_default().clone()
    }

    /// Snapshot of all counter names and their current values.
    pub fn counter_snapshot(&self) -> BTreeMap<String, u64> {
        let map = self
            .counters
            .lock()
            .expect("counter registry lock poisoned");
        map.iter().map(|(k, v)| (k.clone(), v.get())).collect()
    }

    /// Snapshot of all gauge names and their current values.
    pub fn gauge_snapshot(&self) -> BTreeMap<String, i64> {
        let map = self.gauges.lock().expect("gauge registry lock poisoned");
        map.iter().map(|(k, v)| (k.clone(), v.get())).collect()
    }

    /// Snapshot of all histogram names and their summary statistics.
    pub fn histogram_snapshot(&self) -> BTreeMap<String, HistogramStats> {
        let map = self
            .histograms
            .lock()
            .expect("histogram registry lock poisoned");
        map.iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    HistogramStats {
                        count: v.count(),
                        mean: v.mean(),
                        min: v.min(),
                        max: v.max(),
                        p50: v.p50(),
                        p90: v.p90(),
                        p99: v.p99(),
                    },
                )
            })
            .collect()
    }

    /// Capture a combined snapshot of all metrics in the registry.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            counters: self.counter_snapshot(),
            gauges: self.gauge_snapshot(),
            histograms: self.histogram_snapshot(),
        }
    }
}

// ---------------------------------------------------------------------------
// MetricsSnapshot
// ---------------------------------------------------------------------------

/// Point-in-time snapshot of all metrics from a [`MetricsRegistry`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Counter name → current value.
    pub counters: BTreeMap<String, u64>,
    /// Gauge name → current value.
    pub gauges: BTreeMap<String, i64>,
    /// Histogram name → summary statistics.
    pub histograms: BTreeMap<String, HistogramStats>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- RequestCounter ---

    #[test]
    fn request_counter_empty() {
        let c = RequestCounter::new();
        assert_eq!(c.total(), 0);
        assert_eq!(c.get("mock", "openai", "success"), 0);
    }

    #[test]
    fn request_counter_increment_and_get() {
        let c = RequestCounter::new();
        c.increment("mock", "openai", "success");
        c.increment("mock", "openai", "success");
        c.increment("mock", "openai", "error");
        assert_eq!(c.get("mock", "openai", "success"), 2);
        assert_eq!(c.get("mock", "openai", "error"), 1);
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn request_counter_snapshot() {
        let c = RequestCounter::new();
        c.increment("a", "d1", "ok");
        c.increment("b", "d2", "ok");
        let snap = c.snapshot();
        assert_eq!(snap.len(), 2);
    }

    #[test]
    fn request_counter_reset() {
        let c = RequestCounter::new();
        c.increment("a", "d", "ok");
        c.reset();
        assert_eq!(c.total(), 0);
    }

    // --- ErrorCounter ---

    #[test]
    fn error_counter_empty() {
        let c = ErrorCounter::new();
        assert_eq!(c.total(), 0);
        assert_eq!(c.get("E001"), 0);
    }

    #[test]
    fn error_counter_increment_and_get() {
        let c = ErrorCounter::new();
        c.increment("E001");
        c.increment("E001");
        c.increment("E002");
        assert_eq!(c.get("E001"), 2);
        assert_eq!(c.get("E002"), 1);
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn error_counter_reset() {
        let c = ErrorCounter::new();
        c.increment("E001");
        c.reset();
        assert_eq!(c.total(), 0);
    }

    // --- ActiveRequestGauge ---

    #[test]
    fn gauge_starts_at_zero() {
        let g = ActiveRequestGauge::new();
        assert_eq!(g.get(), 0);
    }

    #[test]
    fn gauge_increment_decrement() {
        let g = ActiveRequestGauge::new();
        g.increment();
        g.increment();
        assert_eq!(g.get(), 2);
        g.decrement();
        assert_eq!(g.get(), 1);
    }

    // --- TokenAccumulator ---

    #[test]
    fn token_accumulator_empty() {
        let t = TokenAccumulator::new();
        assert_eq!(t.total_input(), 0);
        assert_eq!(t.total_output(), 0);
        assert_eq!(t.total(), 0);
    }

    #[test]
    fn token_accumulator_add() {
        let t = TokenAccumulator::new();
        t.add(100, 200);
        t.add(50, 75);
        assert_eq!(t.total_input(), 150);
        assert_eq!(t.total_output(), 275);
        assert_eq!(t.total(), 425);
    }

    #[test]
    fn token_accumulator_reset() {
        let t = TokenAccumulator::new();
        t.add(100, 200);
        t.reset();
        assert_eq!(t.total(), 0);
    }

    // --- Counter (generic) ---

    #[test]
    fn counter_starts_at_zero() {
        let c = Counter::new();
        assert_eq!(c.get(), 0);
    }

    #[test]
    fn counter_increment() {
        let c = Counter::new();
        c.increment();
        c.increment();
        assert_eq!(c.get(), 2);
    }

    #[test]
    fn counter_increment_by() {
        let c = Counter::new();
        c.increment_by(10);
        c.increment_by(5);
        assert_eq!(c.get(), 15);
    }

    #[test]
    fn counter_reset() {
        let c = Counter::new();
        c.increment_by(42);
        c.reset();
        assert_eq!(c.get(), 0);
    }

    #[test]
    fn counter_clone_shares_state() {
        let c1 = Counter::new();
        let c2 = c1.clone();
        c1.increment();
        assert_eq!(c2.get(), 1);
    }

    #[test]
    fn counter_thread_safety() {
        let c = Counter::new();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let c = c.clone();
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        c.increment();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(c.get(), 4000);
    }

    // --- Gauge (generic) ---

    #[test]
    fn gauge_starts_at_zero_generic() {
        let g = Gauge::new();
        assert_eq!(g.get(), 0);
    }

    #[test]
    fn gauge_increment_decrement_generic() {
        let g = Gauge::new();
        g.increment();
        g.increment();
        g.decrement();
        assert_eq!(g.get(), 1);
    }

    #[test]
    fn gauge_add() {
        let g = Gauge::new();
        g.add(10);
        g.add(-3);
        assert_eq!(g.get(), 7);
    }

    #[test]
    fn gauge_set() {
        let g = Gauge::new();
        g.set(42);
        assert_eq!(g.get(), 42);
        g.set(-5);
        assert_eq!(g.get(), -5);
    }

    #[test]
    fn gauge_reset() {
        let g = Gauge::new();
        g.set(99);
        g.reset();
        assert_eq!(g.get(), 0);
    }

    #[test]
    fn gauge_clone_shares_state() {
        let g1 = Gauge::new();
        let g2 = g1.clone();
        g1.set(7);
        assert_eq!(g2.get(), 7);
    }

    #[test]
    fn gauge_thread_safety() {
        let g = Gauge::new();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let g = g.clone();
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        g.increment();
                    }
                    for _ in 0..500 {
                        g.decrement();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(g.get(), 0);
    }

    // --- Histogram ---

    #[test]
    fn histogram_empty() {
        let h = Histogram::new();
        assert_eq!(h.count(), 0);
        assert!(h.p50().is_none());
        assert!(h.mean().is_none());
        assert!(h.min().is_none());
        assert!(h.max().is_none());
    }

    #[test]
    fn histogram_record_and_count() {
        let h = Histogram::new();
        h.record(1.0);
        h.record(2.0);
        h.record(3.0);
        assert_eq!(h.count(), 3);
    }

    #[test]
    fn histogram_mean() {
        let h = Histogram::new();
        h.record(10.0);
        h.record(20.0);
        h.record(30.0);
        let m = h.mean().unwrap();
        assert!((m - 20.0).abs() < 0.001);
    }

    #[test]
    fn histogram_min_max() {
        let h = Histogram::new();
        h.record(5.0);
        h.record(1.0);
        h.record(9.0);
        assert_eq!(h.min().unwrap(), 1.0);
        assert_eq!(h.max().unwrap(), 9.0);
    }

    #[test]
    fn histogram_percentiles() {
        let h = Histogram::new();
        for i in 1..=100 {
            h.record(i as f64);
        }
        let p50 = h.p50().unwrap();
        let p90 = h.p90().unwrap();
        let p99 = h.p99().unwrap();
        assert!((p50 - 50.0).abs() < 2.0, "p50={p50}");
        assert!((p90 - 90.0).abs() < 2.0, "p90={p90}");
        assert!((p99 - 99.0).abs() < 2.0, "p99={p99}");
    }

    #[test]
    fn histogram_single_value() {
        let h = Histogram::new();
        h.record(42.0);
        assert_eq!(h.p50().unwrap(), 42.0);
        assert_eq!(h.p90().unwrap(), 42.0);
        assert_eq!(h.p99().unwrap(), 42.0);
    }

    #[test]
    fn histogram_snapshot_and_reset() {
        let h = Histogram::new();
        h.record(1.0);
        h.record(2.0);
        let snap = h.snapshot();
        assert_eq!(snap.len(), 2);
        h.reset();
        assert_eq!(h.count(), 0);
    }

    #[test]
    fn histogram_thread_safety() {
        let h = Histogram::new();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let h = h.clone();
                std::thread::spawn(move || {
                    for i in 0..100 {
                        h.record(i as f64);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(h.count(), 400);
    }

    // --- Timer ---

    #[test]
    fn timer_records_to_histogram() {
        let h = Histogram::new();
        let t = Timer::new(h.clone());
        let guard = t.start();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let ms = guard.stop();
        assert!(ms >= 1.0, "elapsed={ms}ms");
        assert_eq!(h.count(), 1);
    }

    #[test]
    fn timer_guard_drop_records() {
        let h = Histogram::new();
        let t = Timer::new(h.clone());
        {
            let _guard = t.start();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // Guard was dropped, should have recorded
        assert_eq!(h.count(), 1);
    }

    #[test]
    fn timer_elapsed_ms_before_stop() {
        let h = Histogram::new();
        let t = Timer::new(h);
        let guard = t.start();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let e = guard.elapsed_ms();
        assert!(e >= 1.0);
        // Guard dropped here, records to histogram
    }

    // --- MetricsRegistry ---

    #[test]
    fn registry_counter_get_or_create() {
        let reg = MetricsRegistry::new();
        let c1 = reg.counter("reqs");
        let c2 = reg.counter("reqs");
        c1.increment();
        assert_eq!(c2.get(), 1, "same counter should be returned");
    }

    #[test]
    fn registry_gauge_get_or_create() {
        let reg = MetricsRegistry::new();
        let g1 = reg.gauge("active");
        let g2 = reg.gauge("active");
        g1.set(5);
        assert_eq!(g2.get(), 5);
    }

    #[test]
    fn registry_histogram_get_or_create() {
        let reg = MetricsRegistry::new();
        let h1 = reg.histogram("latency");
        let h2 = reg.histogram("latency");
        h1.record(42.0);
        assert_eq!(h2.count(), 1);
    }

    #[test]
    fn registry_snapshots() {
        let reg = MetricsRegistry::new();
        reg.counter("a").increment_by(3);
        reg.gauge("b").set(7);
        reg.histogram("c").record(10.0);

        let cs = reg.counter_snapshot();
        assert_eq!(cs["a"], 3);

        let gs = reg.gauge_snapshot();
        assert_eq!(gs["b"], 7);

        let hs = reg.histogram_snapshot();
        assert_eq!(hs["c"].count, 1);
    }

    #[test]
    fn registry_multiple_metrics() {
        let reg = MetricsRegistry::new();
        reg.counter("x").increment();
        reg.counter("y").increment_by(10);
        let snap = reg.counter_snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap["x"], 1);
        assert_eq!(snap["y"], 10);
    }

    // --- Histogram::observe ---

    #[test]
    fn histogram_observe_alias() {
        let h = Histogram::new();
        h.observe(5.0);
        h.observe(10.0);
        assert_eq!(h.count(), 2);
        assert!((h.mean().unwrap() - 7.5).abs() < 0.001);
    }

    #[test]
    fn histogram_observe_same_as_record() {
        let h1 = Histogram::new();
        let h2 = Histogram::new();
        h1.record(42.0);
        h2.observe(42.0);
        assert_eq!(h1.count(), h2.count());
        assert_eq!(h1.mean(), h2.mean());
    }

    // --- MetricsSnapshot ---

    #[test]
    fn metrics_snapshot_empty_registry() {
        let reg = MetricsRegistry::new();
        let snap = reg.snapshot();
        assert!(snap.counters.is_empty());
        assert!(snap.gauges.is_empty());
        assert!(snap.histograms.is_empty());
    }

    #[test]
    fn metrics_snapshot_with_data() {
        let reg = MetricsRegistry::new();
        reg.counter("reqs").increment_by(5);
        reg.gauge("active").set(3);
        reg.histogram("latency").record(100.0);

        let snap = reg.snapshot();
        assert_eq!(snap.counters["reqs"], 5);
        assert_eq!(snap.gauges["active"], 3);
        assert_eq!(snap.histograms["latency"].count, 1);
    }

    #[test]
    fn metrics_snapshot_serde_roundtrip() {
        let reg = MetricsRegistry::new();
        reg.counter("c").increment();
        reg.gauge("g").set(42);
        reg.histogram("h").record(1.0);

        let snap = reg.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        let back: MetricsSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.counters["c"], 1);
        assert_eq!(back.gauges["g"], 42);
        assert_eq!(back.histograms["h"].count, 1);
    }

    #[test]
    fn metrics_snapshot_multiple_counters() {
        let reg = MetricsRegistry::new();
        reg.counter("a").increment_by(10);
        reg.counter("b").increment_by(20);
        reg.counter("c").increment_by(30);
        let snap = reg.snapshot();
        assert_eq!(snap.counters.len(), 3);
        assert_eq!(snap.counters["a"], 10);
        assert_eq!(snap.counters["b"], 20);
        assert_eq!(snap.counters["c"], 30);
    }

    #[test]
    fn registry_snapshot_thread_safety() {
        let reg = MetricsRegistry::new();
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let reg = reg.clone();
                std::thread::spawn(move || {
                    let c = reg.counter(&format!("counter_{i}"));
                    for _ in 0..100 {
                        c.increment();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let snap = reg.snapshot();
        assert_eq!(snap.counters.len(), 4);
        for v in snap.counters.values() {
            assert_eq!(*v, 100);
        }
    }
}
