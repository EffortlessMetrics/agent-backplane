// SPDX-License-Identifier: MIT OR Apache-2.0
//! Labeled (dimensioned) metric types for multi-dimensional telemetry.
//!
//! Each labeled metric type wraps a map from [`Labels`] to an underlying
//! metric primitive, enabling breakdowns by backend, dialect, execution mode,
//! error code, and arbitrary user-defined dimensions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;

use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

/// Sorted key-value label set used as a dimension key.
///
/// Backed by a [`BTreeMap`] for deterministic ordering and hashing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Labels(pub BTreeMap<String, String>);

impl Labels {
    /// Create an empty label set.
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Insert a label pair.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }

    /// Returns `true` if the label set is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Render as a Prometheus label string: `{k1="v1",k2="v2"}`.
    /// Returns an empty string if there are no labels.
    pub fn to_prometheus(&self) -> String {
        if self.0.is_empty() {
            return String::new();
        }
        let mut out = String::from("{");
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{}=\"{}\"", k, v);
        }
        out.push('}');
        out
    }
}

impl Default for Labels {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience macro-like builder: `labels("backend" => "mock", "dialect" => "openai")`.
pub fn labels<I, K, V>(pairs: I) -> Labels
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let map: BTreeMap<String, String> = pairs
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    Labels(map)
}

// ---------------------------------------------------------------------------
// LabeledCounter
// ---------------------------------------------------------------------------

/// A family of monotonically increasing counters keyed by [`Labels`].
#[derive(Debug, Clone, Default)]
pub struct LabeledCounter {
    inner: Arc<Mutex<BTreeMap<Labels, u64>>>,
}

impl LabeledCounter {
    /// Create an empty labeled counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the counter for the given labels by one.
    pub fn increment(&self, labels: &Labels) {
        self.increment_by(labels, 1);
    }

    /// Increment the counter for the given labels by `n`.
    pub fn increment_by(&self, labels: &Labels, n: u64) {
        let mut map = self.inner.lock().expect("labeled counter lock poisoned");
        *map.entry(labels.clone()).or_insert(0) += n;
    }

    /// Get the current value for specific labels (0 if unseen).
    pub fn get(&self, labels: &Labels) -> u64 {
        let map = self.inner.lock().expect("labeled counter lock poisoned");
        map.get(labels).copied().unwrap_or(0)
    }

    /// Total across all label combinations.
    pub fn total(&self) -> u64 {
        let map = self.inner.lock().expect("labeled counter lock poisoned");
        map.values().sum()
    }

    /// Number of distinct label combinations.
    pub fn cardinality(&self) -> usize {
        let map = self.inner.lock().expect("labeled counter lock poisoned");
        map.len()
    }

    /// Snapshot of all labels → values.
    pub fn snapshot(&self) -> BTreeMap<Labels, u64> {
        let map = self.inner.lock().expect("labeled counter lock poisoned");
        map.clone()
    }

    /// Reset all counters.
    pub fn reset(&self) {
        let mut map = self.inner.lock().expect("labeled counter lock poisoned");
        map.clear();
    }
}

// ---------------------------------------------------------------------------
// LabeledGauge
// ---------------------------------------------------------------------------

/// A family of gauges (can increase or decrease) keyed by [`Labels`].
#[derive(Debug, Clone, Default)]
pub struct LabeledGauge {
    inner: Arc<Mutex<BTreeMap<Labels, i64>>>,
}

impl LabeledGauge {
    /// Create an empty labeled gauge.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment gauge for the given labels by one.
    pub fn increment(&self, labels: &Labels) {
        let mut map = self.inner.lock().expect("labeled gauge lock poisoned");
        *map.entry(labels.clone()).or_insert(0) += 1;
    }

    /// Decrement gauge for the given labels by one.
    pub fn decrement(&self, labels: &Labels) {
        let mut map = self.inner.lock().expect("labeled gauge lock poisoned");
        *map.entry(labels.clone()).or_insert(0) -= 1;
    }

    /// Set gauge to an absolute value for the given labels.
    pub fn set(&self, labels: &Labels, value: i64) {
        let mut map = self.inner.lock().expect("labeled gauge lock poisoned");
        map.insert(labels.clone(), value);
    }

    /// Get the current value for specific labels (0 if unseen).
    pub fn get(&self, labels: &Labels) -> i64 {
        let map = self.inner.lock().expect("labeled gauge lock poisoned");
        map.get(labels).copied().unwrap_or(0)
    }

    /// Number of distinct label combinations.
    pub fn cardinality(&self) -> usize {
        let map = self.inner.lock().expect("labeled gauge lock poisoned");
        map.len()
    }

    /// Snapshot of all labels → values.
    pub fn snapshot(&self) -> BTreeMap<Labels, i64> {
        let map = self.inner.lock().expect("labeled gauge lock poisoned");
        map.clone()
    }

    /// Reset all gauges.
    pub fn reset(&self) {
        let mut map = self.inner.lock().expect("labeled gauge lock poisoned");
        map.clear();
    }
}

// ---------------------------------------------------------------------------
// LabeledHistogram
// ---------------------------------------------------------------------------

/// A family of histograms keyed by [`Labels`].
#[derive(Debug, Clone, Default)]
pub struct LabeledHistogram {
    inner: Arc<Mutex<BTreeMap<Labels, Vec<f64>>>>,
}

/// Summary statistics for a single label set within a [`LabeledHistogram`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabeledHistogramStats {
    /// Number of observations.
    pub count: usize,
    /// Sum of all observed values.
    pub sum: f64,
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

impl LabeledHistogram {
    /// Create an empty labeled histogram.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a value for the given labels.
    pub fn record(&self, labels: &Labels, value: f64) {
        let mut map = self.inner.lock().expect("labeled histogram lock poisoned");
        map.entry(labels.clone()).or_default().push(value);
    }

    /// Number of observations for the given labels.
    pub fn count(&self, labels: &Labels) -> usize {
        let map = self.inner.lock().expect("labeled histogram lock poisoned");
        map.get(labels).map_or(0, |v| v.len())
    }

    /// Total observations across all label combinations.
    pub fn total_count(&self) -> usize {
        let map = self.inner.lock().expect("labeled histogram lock poisoned");
        map.values().map(|v| v.len()).sum()
    }

    /// Number of distinct label combinations.
    pub fn cardinality(&self) -> usize {
        let map = self.inner.lock().expect("labeled histogram lock poisoned");
        map.len()
    }

    /// Compute stats for a specific label set.
    pub fn stats(&self, labels: &Labels) -> Option<LabeledHistogramStats> {
        let map = self.inner.lock().expect("labeled histogram lock poisoned");
        let vals = map.get(labels)?;
        if vals.is_empty() {
            return None;
        }
        Some(compute_stats(vals))
    }

    /// Snapshot of all labels → stats.
    pub fn snapshot(&self) -> BTreeMap<Labels, LabeledHistogramStats> {
        let map = self.inner.lock().expect("labeled histogram lock poisoned");
        map.iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.clone(), compute_stats(v)))
            .collect()
    }

    /// Reset all histograms.
    pub fn reset(&self) {
        let mut map = self.inner.lock().expect("labeled histogram lock poisoned");
        map.clear();
    }
}

fn compute_stats(vals: &[f64]) -> LabeledHistogramStats {
    let count = vals.len();
    let sum: f64 = vals.iter().sum();
    let min = vals.iter().copied().reduce(f64::min);
    let max = vals.iter().copied().reduce(f64::max);

    let mut sorted = vals.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let percentile = |p: f64| -> Option<f64> {
        if sorted.is_empty() {
            return None;
        }
        let idx = ((p * (sorted.len() - 1) as f64).round()) as usize;
        let idx = idx.min(sorted.len() - 1);
        Some(sorted[idx])
    };

    LabeledHistogramStats {
        count,
        sum,
        min,
        max,
        p50: percentile(0.50),
        p90: percentile(0.90),
        p99: percentile(0.99),
    }
}

// ---------------------------------------------------------------------------
// RuntimeMetrics
// ---------------------------------------------------------------------------

/// Pre-defined ABP runtime metrics with standard label dimensions.
///
/// Provides counters, gauges, and histograms for tracking work order
/// processing, errors, events, latency, and concurrency with labels for
/// `backend`, `dialect`, `execution_mode`, and `error_code`.
#[derive(Debug, Clone, Default)]
pub struct RuntimeMetrics {
    /// Total work orders processed, labeled by backend, dialect, execution_mode.
    pub work_orders_total: LabeledCounter,
    /// Total errors encountered, labeled by backend, dialect, error_code.
    pub errors_total: LabeledCounter,
    /// Total events emitted, labeled by backend, dialect.
    pub events_total: LabeledCounter,
    /// Active concurrent runs, labeled by backend.
    pub active_runs: LabeledGauge,
    /// Pending work orders, labeled by backend.
    pub pending_work_orders: LabeledGauge,
    /// Response latency distribution (ms), labeled by backend, dialect, execution_mode.
    pub response_latency: LabeledHistogram,
    /// Event count distribution per run, labeled by backend.
    pub event_count_distribution: LabeledHistogram,
}

impl RuntimeMetrics {
    /// Create a new set of runtime metrics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed work order.
    pub fn record_work_order(
        &self,
        backend: &str,
        dialect: &str,
        execution_mode: &str,
        latency_ms: f64,
        event_count: u64,
        error_code: Option<&str>,
    ) {
        let base = Labels::new()
            .with("backend", backend)
            .with("dialect", dialect)
            .with("execution_mode", execution_mode);

        self.work_orders_total.increment(&base);
        self.response_latency.record(&base, latency_ms);

        let backend_labels = Labels::new().with("backend", backend);
        self.event_count_distribution
            .record(&backend_labels, event_count as f64);

        let event_labels = Labels::new()
            .with("backend", backend)
            .with("dialect", dialect);
        self.events_total.increment_by(&event_labels, event_count);

        if let Some(code) = error_code {
            let err_labels = Labels::new()
                .with("backend", backend)
                .with("dialect", dialect)
                .with("error_code", code);
            self.errors_total.increment(&err_labels);
        }
    }

    /// Mark a run as started (increments active_runs gauge).
    pub fn run_started(&self, backend: &str) {
        let labels = Labels::new().with("backend", backend);
        self.active_runs.increment(&labels);
    }

    /// Mark a run as finished (decrements active_runs gauge).
    pub fn run_finished(&self, backend: &str) {
        let labels = Labels::new().with("backend", backend);
        self.active_runs.decrement(&labels);
    }

    /// Increment pending work orders gauge.
    pub fn work_order_enqueued(&self, backend: &str) {
        let labels = Labels::new().with("backend", backend);
        self.pending_work_orders.increment(&labels);
    }

    /// Decrement pending work orders gauge.
    pub fn work_order_dequeued(&self, backend: &str) {
        let labels = Labels::new().with("backend", backend);
        self.pending_work_orders.decrement(&labels);
    }

    /// Render all runtime metrics in Prometheus text exposition format.
    pub fn to_prometheus_text(&self) -> String {
        let mut out = String::new();

        render_labeled_counter(
            &mut out,
            "abp_work_orders_total",
            "counter",
            &self.work_orders_total,
        );
        render_labeled_counter(&mut out, "abp_errors_total", "counter", &self.errors_total);
        render_labeled_counter(&mut out, "abp_events_total", "counter", &self.events_total);
        render_labeled_gauge(&mut out, "abp_active_runs", &self.active_runs);
        render_labeled_gauge(
            &mut out,
            "abp_pending_work_orders",
            &self.pending_work_orders,
        );
        render_labeled_histogram(&mut out, "abp_response_latency_ms", &self.response_latency);
        render_labeled_histogram(
            &mut out,
            "abp_event_count_distribution",
            &self.event_count_distribution,
        );

        out
    }
}

// ---------------------------------------------------------------------------
// Prometheus rendering helpers
// ---------------------------------------------------------------------------

fn render_labeled_counter(out: &mut String, name: &str, prom_type: &str, counter: &LabeledCounter) {
    let snap = counter.snapshot();
    if snap.is_empty() {
        return;
    }
    let _ = writeln!(out, "# TYPE {} {}", name, prom_type);
    for (labels, value) in &snap {
        let _ = writeln!(out, "{}{} {}", name, labels.to_prometheus(), value);
    }
}

fn render_labeled_gauge(out: &mut String, name: &str, gauge: &LabeledGauge) {
    let snap = gauge.snapshot();
    if snap.is_empty() {
        return;
    }
    let _ = writeln!(out, "# TYPE {} gauge", name);
    for (labels, value) in &snap {
        let _ = writeln!(out, "{}{} {}", name, labels.to_prometheus(), value);
    }
}

fn render_labeled_histogram(out: &mut String, name: &str, histogram: &LabeledHistogram) {
    let snap = histogram.snapshot();
    if snap.is_empty() {
        return;
    }
    let _ = writeln!(out, "# TYPE {} summary", name);
    for (labels, stats) in &snap {
        let lbl = labels.to_prometheus();
        let _ = writeln!(out, "{}_count{} {}", name, lbl, stats.count);
        let _ = writeln!(out, "{}_sum{} {}", name, lbl, stats.sum);
        if let Some(p50) = stats.p50 {
            let combined = combine_quantile_label(labels, "0.5");
            let _ = writeln!(out, "{}{} {}", name, combined, p50);
        }
        if let Some(p90) = stats.p90 {
            let combined = combine_quantile_label(labels, "0.9");
            let _ = writeln!(out, "{}{} {}", name, combined, p90);
        }
        if let Some(p99) = stats.p99 {
            let combined = combine_quantile_label(labels, "0.99");
            let _ = writeln!(out, "{}{} {}", name, combined, p99);
        }
    }
}

fn combine_quantile_label(labels: &Labels, quantile: &str) -> String {
    let mut combined = labels.0.clone();
    combined.insert("quantile".to_string(), quantile.to_string());
    Labels(combined).to_prometheus()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Labels ---

    #[test]
    fn labels_empty() {
        let l = Labels::new();
        assert!(l.is_empty());
        assert_eq!(l.to_prometheus(), "");
    }

    #[test]
    fn labels_single() {
        let l = Labels::new().with("backend", "mock");
        assert_eq!(l.to_prometheus(), "{backend=\"mock\"}");
    }

    #[test]
    fn labels_multiple_sorted() {
        let l = Labels::new()
            .with("dialect", "openai")
            .with("backend", "sidecar");
        // BTreeMap sorts: backend < dialect
        assert_eq!(
            l.to_prometheus(),
            "{backend=\"sidecar\",dialect=\"openai\"}"
        );
    }

    #[test]
    fn labels_builder_fn() {
        let l = labels([("a", "1"), ("b", "2")]);
        assert_eq!(l.0.len(), 2);
        assert_eq!(l.0["a"], "1");
    }

    // --- LabeledCounter ---

    #[test]
    fn labeled_counter_empty() {
        let c = LabeledCounter::new();
        assert_eq!(c.total(), 0);
        assert_eq!(c.cardinality(), 0);
    }

    #[test]
    fn labeled_counter_increment() {
        let c = LabeledCounter::new();
        let l1 = Labels::new().with("backend", "mock");
        let l2 = Labels::new().with("backend", "sidecar");
        c.increment(&l1);
        c.increment(&l1);
        c.increment(&l2);
        assert_eq!(c.get(&l1), 2);
        assert_eq!(c.get(&l2), 1);
        assert_eq!(c.total(), 3);
        assert_eq!(c.cardinality(), 2);
    }

    #[test]
    fn labeled_counter_increment_by() {
        let c = LabeledCounter::new();
        let l = Labels::new().with("backend", "mock");
        c.increment_by(&l, 10);
        c.increment_by(&l, 5);
        assert_eq!(c.get(&l), 15);
    }

    #[test]
    fn labeled_counter_monotonicity() {
        let c = LabeledCounter::new();
        let l = Labels::new().with("backend", "mock");
        let mut prev = 0u64;
        for _ in 0..100 {
            c.increment(&l);
            let curr = c.get(&l);
            assert!(curr >= prev, "counter must be monotonically increasing");
            prev = curr;
        }
    }

    #[test]
    fn labeled_counter_reset() {
        let c = LabeledCounter::new();
        let l = Labels::new().with("backend", "mock");
        c.increment(&l);
        c.reset();
        assert_eq!(c.total(), 0);
        assert_eq!(c.cardinality(), 0);
    }

    #[test]
    fn labeled_counter_concurrent() {
        let c = LabeledCounter::new();
        let l = Labels::new().with("backend", "mock");
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let c = c.clone();
                let l = l.clone();
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        c.increment(&l);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(c.get(&l), 4000);
    }

    // --- LabeledGauge ---

    #[test]
    fn labeled_gauge_empty() {
        let g = LabeledGauge::new();
        let l = Labels::new().with("backend", "mock");
        assert_eq!(g.get(&l), 0);
        assert_eq!(g.cardinality(), 0);
    }

    #[test]
    fn labeled_gauge_up_down() {
        let g = LabeledGauge::new();
        let l = Labels::new().with("backend", "mock");
        g.increment(&l);
        g.increment(&l);
        g.decrement(&l);
        assert_eq!(g.get(&l), 1);
    }

    #[test]
    fn labeled_gauge_set() {
        let g = LabeledGauge::new();
        let l = Labels::new().with("backend", "mock");
        g.set(&l, 42);
        assert_eq!(g.get(&l), 42);
        g.set(&l, -5);
        assert_eq!(g.get(&l), -5);
    }

    #[test]
    fn labeled_gauge_negative() {
        let g = LabeledGauge::new();
        let l = Labels::new().with("backend", "mock");
        g.decrement(&l);
        g.decrement(&l);
        assert_eq!(g.get(&l), -2);
    }

    #[test]
    fn labeled_gauge_concurrent_up_down() {
        let g = LabeledGauge::new();
        let l = Labels::new().with("backend", "mock");
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let g = g.clone();
                let l = l.clone();
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        g.increment(&l);
                    }
                    for _ in 0..500 {
                        g.decrement(&l);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(g.get(&l), 0);
    }

    // --- LabeledHistogram ---

    #[test]
    fn labeled_histogram_empty() {
        let h = LabeledHistogram::new();
        let l = Labels::new().with("backend", "mock");
        assert_eq!(h.count(&l), 0);
        assert!(h.stats(&l).is_none());
        assert_eq!(h.total_count(), 0);
    }

    #[test]
    fn labeled_histogram_record_and_stats() {
        let h = LabeledHistogram::new();
        let l = Labels::new().with("backend", "mock");
        for i in 1..=100 {
            h.record(&l, i as f64);
        }
        let stats = h.stats(&l).unwrap();
        assert_eq!(stats.count, 100);
        assert!((stats.sum - 5050.0).abs() < 0.001);
        assert_eq!(stats.min, Some(1.0));
        assert_eq!(stats.max, Some(100.0));
        // p50 ≈ 50
        assert!((stats.p50.unwrap() - 50.0).abs() < 2.0);
        // p90 ≈ 90
        assert!((stats.p90.unwrap() - 90.0).abs() < 2.0);
        // p99 ≈ 99
        assert!((stats.p99.unwrap() - 99.0).abs() < 2.0);
    }

    #[test]
    fn labeled_histogram_accuracy_known_distribution() {
        let h = LabeledHistogram::new();
        let l = Labels::new();
        // Record values 1..=1000 uniformly
        for i in 1..=1000 {
            h.record(&l, i as f64);
        }
        let stats = h.stats(&l).unwrap();
        assert_eq!(stats.count, 1000);
        // Sum = n*(n+1)/2 = 500500
        assert!((stats.sum - 500_500.0).abs() < 0.001);
        // p50 should be near 500
        assert!((stats.p50.unwrap() - 500.0).abs() < 5.0);
        // p90 should be near 900
        assert!((stats.p90.unwrap() - 900.0).abs() < 5.0);
        // p99 should be near 990
        assert!((stats.p99.unwrap() - 990.0).abs() < 15.0);
    }

    #[test]
    fn labeled_histogram_multiple_labels() {
        let h = LabeledHistogram::new();
        let l1 = Labels::new().with("backend", "mock");
        let l2 = Labels::new().with("backend", "sidecar");
        h.record(&l1, 10.0);
        h.record(&l2, 20.0);
        h.record(&l2, 30.0);
        assert_eq!(h.count(&l1), 1);
        assert_eq!(h.count(&l2), 2);
        assert_eq!(h.total_count(), 3);
        assert_eq!(h.cardinality(), 2);
    }

    #[test]
    fn labeled_histogram_concurrent() {
        let h = LabeledHistogram::new();
        let l = Labels::new().with("backend", "mock");
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let h = h.clone();
                let l = l.clone();
                std::thread::spawn(move || {
                    for i in 0..250 {
                        h.record(&l, i as f64);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(h.count(&l), 1000);
    }

    // --- RuntimeMetrics ---

    #[test]
    fn runtime_metrics_record_work_order() {
        let m = RuntimeMetrics::new();
        m.record_work_order("mock", "openai", "mapped", 150.0, 10, None);
        m.record_work_order("mock", "openai", "mapped", 200.0, 5, Some("timeout"));

        let base = Labels::new()
            .with("backend", "mock")
            .with("dialect", "openai")
            .with("execution_mode", "mapped");
        assert_eq!(m.work_orders_total.get(&base), 2);

        let err = Labels::new()
            .with("backend", "mock")
            .with("dialect", "openai")
            .with("error_code", "timeout");
        assert_eq!(m.errors_total.get(&err), 1);

        let event_labels = Labels::new()
            .with("backend", "mock")
            .with("dialect", "openai");
        assert_eq!(m.events_total.get(&event_labels), 15);
    }

    #[test]
    fn runtime_metrics_active_runs() {
        let m = RuntimeMetrics::new();
        m.run_started("mock");
        m.run_started("mock");
        m.run_started("sidecar");
        m.run_finished("mock");

        let mock = Labels::new().with("backend", "mock");
        let sidecar = Labels::new().with("backend", "sidecar");
        assert_eq!(m.active_runs.get(&mock), 1);
        assert_eq!(m.active_runs.get(&sidecar), 1);
    }

    #[test]
    fn runtime_metrics_pending_work_orders() {
        let m = RuntimeMetrics::new();
        m.work_order_enqueued("mock");
        m.work_order_enqueued("mock");
        m.work_order_dequeued("mock");

        let l = Labels::new().with("backend", "mock");
        assert_eq!(m.pending_work_orders.get(&l), 1);
    }

    #[test]
    fn runtime_metrics_prometheus_output() {
        let m = RuntimeMetrics::new();
        m.record_work_order("mock", "openai", "mapped", 100.0, 5, None);
        m.run_started("mock");

        let text = m.to_prometheus_text();
        assert!(text.contains("# TYPE abp_work_orders_total counter"));
        assert!(text.contains("abp_work_orders_total{"));
        assert!(text.contains("backend=\"mock\""));
        assert!(text.contains("# TYPE abp_active_runs gauge"));
        assert!(text.contains("abp_active_runs{backend=\"mock\"} 1"));
        assert!(text.contains("# TYPE abp_response_latency_ms summary"));
        assert!(text.contains("abp_response_latency_ms_count"));
    }

    // --- Label cardinality ---

    #[test]
    fn label_cardinality_tracking() {
        let c = LabeledCounter::new();
        for i in 0..50 {
            let l = Labels::new().with("error_code", format!("E{:03}", i));
            c.increment(&l);
        }
        assert_eq!(c.cardinality(), 50);
        assert_eq!(c.total(), 50);
    }

    // --- Prometheus format correctness ---

    #[test]
    fn prometheus_format_type_line() {
        let m = RuntimeMetrics::new();
        m.record_work_order("mock", "openai", "mapped", 50.0, 3, None);
        let text = m.to_prometheus_text();
        // Each section must start with # TYPE
        for line in text.lines() {
            if line.starts_with("# TYPE") {
                assert!(
                    line.contains("counter") || line.contains("gauge") || line.contains("summary"),
                    "TYPE line must declare a valid metric type: {}",
                    line
                );
            }
        }
    }

    #[test]
    fn prometheus_format_no_empty_braces() {
        // Labeled metrics should always have non-empty label sets
        let m = RuntimeMetrics::new();
        m.record_work_order("mock", "openai", "mapped", 50.0, 3, None);
        let text = m.to_prometheus_text();
        // No line should contain `{}`
        for line in text.lines() {
            if !line.starts_with('#') && !line.is_empty() {
                assert!(!line.contains("{}"), "empty braces in: {}", line);
            }
        }
    }
}
