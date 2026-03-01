// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-telemetry
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Structured telemetry and metrics collection for Agent Backplane runs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tracing::info;

// ---------------------------------------------------------------------------
// RunMetrics
// ---------------------------------------------------------------------------

/// Metrics captured for a single agent run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunMetrics {
    /// Name of the backend used.
    pub backend_name: String,
    /// Dialect / vendor identifier.
    pub dialect: String,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Total number of events emitted.
    pub events_count: u64,
    /// Inbound token count.
    pub tokens_in: u64,
    /// Outbound token count.
    pub tokens_out: u64,
    /// Number of tool calls made.
    pub tool_calls_count: u64,
    /// Number of errors encountered.
    pub errors_count: u64,
    /// Number of emulation layers applied.
    pub emulations_applied: u64,
}

// ---------------------------------------------------------------------------
// MetricsSummary
// ---------------------------------------------------------------------------

/// Aggregated statistics across multiple runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsSummary {
    /// Number of runs recorded.
    pub count: usize,
    /// Mean duration in milliseconds.
    pub mean_duration_ms: f64,
    /// Median (p50) duration in milliseconds.
    pub p50_duration_ms: f64,
    /// 99th-percentile duration in milliseconds.
    pub p99_duration_ms: f64,
    /// Total inbound tokens across all runs.
    pub total_tokens_in: u64,
    /// Total outbound tokens across all runs.
    pub total_tokens_out: u64,
    /// Error rate (errors / total runs).
    pub error_rate: f64,
    /// Per-backend run counts (deterministic ordering).
    pub backend_counts: BTreeMap<String, usize>,
}

impl Default for MetricsSummary {
    fn default() -> Self {
        Self {
            count: 0,
            mean_duration_ms: 0.0,
            p50_duration_ms: 0.0,
            p99_duration_ms: 0.0,
            total_tokens_in: 0,
            total_tokens_out: 0,
            error_rate: 0.0,
            backend_counts: BTreeMap::new(),
        }
    }
}

/// Compute a percentile value from a **sorted** slice.
fn percentile(sorted: &[u64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0] as f64;
    }
    let rank = pct / 100.0 * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let frac = rank - lower as f64;
    sorted[lower] as f64 * (1.0 - frac) + sorted[upper] as f64 * frac
}

// ---------------------------------------------------------------------------
// MetricsCollector
// ---------------------------------------------------------------------------

/// Thread-safe collector for run metrics.
///
/// Wrap in an `Arc` to share across threads (the inner storage is already
/// behind a `Mutex`).
#[derive(Debug, Clone)]
pub struct MetricsCollector {
    inner: Arc<Mutex<Vec<RunMetrics>>>,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    /// Create a new, empty collector.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Record a completed run's metrics.
    pub fn record(&self, metrics: RunMetrics) {
        let mut data = self.inner.lock().expect("metrics lock poisoned");
        data.push(metrics);
    }

    /// Return all recorded run metrics.
    pub fn runs(&self) -> Vec<RunMetrics> {
        let data = self.inner.lock().expect("metrics lock poisoned");
        data.clone()
    }

    /// Number of runs recorded so far.
    pub fn len(&self) -> usize {
        let data = self.inner.lock().expect("metrics lock poisoned");
        data.len()
    }

    /// Whether the collector has no recorded runs.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Compute an aggregated summary of all recorded runs.
    pub fn summary(&self) -> MetricsSummary {
        let data = self.inner.lock().expect("metrics lock poisoned");
        if data.is_empty() {
            return MetricsSummary::default();
        }

        let count = data.len();
        let mut durations: Vec<u64> = data.iter().map(|r| r.duration_ms).collect();
        durations.sort_unstable();

        let total_duration: u64 = durations.iter().sum();
        let mean_duration_ms = total_duration as f64 / count as f64;
        let p50_duration_ms = percentile(&durations, 50.0);
        let p99_duration_ms = percentile(&durations, 99.0);

        let total_tokens_in: u64 = data.iter().map(|r| r.tokens_in).sum();
        let total_tokens_out: u64 = data.iter().map(|r| r.tokens_out).sum();

        let errors: u64 = data.iter().map(|r| r.errors_count).sum();
        let error_rate = errors as f64 / count as f64;

        let mut backend_counts: BTreeMap<String, usize> = BTreeMap::new();
        for r in data.iter() {
            *backend_counts.entry(r.backend_name.clone()).or_insert(0) += 1;
        }

        MetricsSummary {
            count,
            mean_duration_ms,
            p50_duration_ms,
            p99_duration_ms,
            total_tokens_in,
            total_tokens_out,
            error_rate,
            backend_counts,
        }
    }

    /// Clear all recorded metrics.
    pub fn clear(&self) {
        let mut data = self.inner.lock().expect("metrics lock poisoned");
        data.clear();
    }
}

// ---------------------------------------------------------------------------
// TelemetrySpan
// ---------------------------------------------------------------------------

/// A structured span for tracing integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySpan {
    /// Span name / operation.
    pub name: String,
    /// Arbitrary key-value attributes (deterministic ordering).
    pub attributes: BTreeMap<String, String>,
}

impl TelemetrySpan {
    /// Create a new span with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: BTreeMap::new(),
        }
    }

    /// Insert an attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Emit the span via `tracing::info!`.
    pub fn emit(&self) {
        info!(
            span_name = %self.name,
            attributes = ?self.attributes,
            "telemetry_span"
        );
    }
}

// ---------------------------------------------------------------------------
// TelemetryExporter
// ---------------------------------------------------------------------------

/// Trait for exporting collected metrics.
pub trait TelemetryExporter: Send + Sync {
    /// Export the given summary. Returns the serialized output on success.
    fn export(&self, summary: &MetricsSummary) -> Result<String, String>;
}

/// Exports metrics as pretty-printed JSON to a string.
#[derive(Debug, Default)]
pub struct JsonExporter;

impl TelemetryExporter for JsonExporter {
    fn export(&self, summary: &MetricsSummary) -> Result<String, String> {
        serde_json::to_string_pretty(summary).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn sample_metrics(backend: &str, duration: u64, errors: u64) -> RunMetrics {
        RunMetrics {
            backend_name: backend.to_string(),
            dialect: "test".to_string(),
            duration_ms: duration,
            events_count: 5,
            tokens_in: 100,
            tokens_out: 200,
            tool_calls_count: 3,
            errors_count: errors,
            emulations_applied: 1,
        }
    }

    // --- MetricsCollector basic ---

    #[test]
    fn collector_new_is_empty() {
        let c = MetricsCollector::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn collector_record_and_len() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 100, 0));
        assert_eq!(c.len(), 1);
        assert!(!c.is_empty());
    }

    #[test]
    fn collector_runs_returns_all() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("a", 10, 0));
        c.record(sample_metrics("b", 20, 0));
        let runs = c.runs();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].backend_name, "a");
        assert_eq!(runs[1].backend_name, "b");
    }

    #[test]
    fn collector_clear() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("x", 50, 0));
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn empty_collector_summary() {
        let c = MetricsCollector::new();
        let s = c.summary();
        assert_eq!(s.count, 0);
        assert_eq!(s.mean_duration_ms, 0.0);
        assert_eq!(s.p50_duration_ms, 0.0);
        assert_eq!(s.p99_duration_ms, 0.0);
        assert_eq!(s.total_tokens_in, 0);
        assert_eq!(s.total_tokens_out, 0);
        assert_eq!(s.error_rate, 0.0);
        assert!(s.backend_counts.is_empty());
    }

    // --- Single-run summary ---

    #[test]
    fn single_run_summary_matches() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 42, 0));
        let s = c.summary();
        assert_eq!(s.count, 1);
        assert_eq!(s.mean_duration_ms, 42.0);
        assert_eq!(s.p50_duration_ms, 42.0);
        assert_eq!(s.p99_duration_ms, 42.0);
        assert_eq!(s.total_tokens_in, 100);
        assert_eq!(s.total_tokens_out, 200);
        assert_eq!(s.error_rate, 0.0);
        assert_eq!(s.backend_counts["mock"], 1);
    }

    // --- Aggregation math ---

    #[test]
    fn summary_mean_duration() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("a", 100, 0));
        c.record(sample_metrics("a", 200, 0));
        c.record(sample_metrics("a", 300, 0));
        let s = c.summary();
        assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn summary_p50_odd_count() {
        let c = MetricsCollector::new();
        for d in [10, 20, 30, 40, 50] {
            c.record(sample_metrics("a", d, 0));
        }
        let s = c.summary();
        assert!((s.p50_duration_ms - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn summary_p50_even_count() {
        let c = MetricsCollector::new();
        for d in [10, 20, 30, 40] {
            c.record(sample_metrics("a", d, 0));
        }
        let s = c.summary();
        assert!((s.p50_duration_ms - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn summary_p99() {
        let c = MetricsCollector::new();
        for d in 1..=100 {
            c.record(sample_metrics("a", d, 0));
        }
        let s = c.summary();
        // p99 of 1..=100 should be close to 99.01
        assert!(s.p99_duration_ms > 98.0);
        assert!(s.p99_duration_ms <= 100.0);
    }

    #[test]
    fn summary_total_tokens() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("a", 10, 0));
        c.record(sample_metrics("b", 20, 0));
        let s = c.summary();
        assert_eq!(s.total_tokens_in, 200);
        assert_eq!(s.total_tokens_out, 400);
    }

    #[test]
    fn summary_error_rate() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("a", 10, 1));
        c.record(sample_metrics("a", 20, 0));
        c.record(sample_metrics("a", 30, 2));
        let s = c.summary();
        assert!((s.error_rate - 1.0).abs() < f64::EPSILON); // 3 errors / 3 runs
    }

    #[test]
    fn summary_backend_counts() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("alpha", 10, 0));
        c.record(sample_metrics("beta", 20, 0));
        c.record(sample_metrics("alpha", 30, 0));
        let s = c.summary();
        assert_eq!(s.backend_counts["alpha"], 2);
        assert_eq!(s.backend_counts["beta"], 1);
    }

    // --- RunMetrics serde ---

    #[test]
    fn run_metrics_serde_roundtrip() {
        let m = sample_metrics("serde_test", 999, 2);
        let json = serde_json::to_string(&m).unwrap();
        let m2: RunMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn run_metrics_default_values() {
        let m = RunMetrics::default();
        assert_eq!(m.backend_name, "");
        assert_eq!(m.duration_ms, 0);
        assert_eq!(m.tokens_in, 0);
        assert_eq!(m.tokens_out, 0);
        assert_eq!(m.errors_count, 0);
    }

    #[test]
    fn metrics_summary_serde_roundtrip() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("a", 50, 1));
        let s = c.summary();
        let json = serde_json::to_string(&s).unwrap();
        let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(s, s2);
    }

    // --- Thread safety ---

    #[test]
    fn concurrent_recording() {
        let c = MetricsCollector::new();
        let mut handles = vec![];
        for i in 0..10 {
            let cc = c.clone();
            handles.push(thread::spawn(move || {
                cc.record(sample_metrics("thread", i * 10, 0));
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(c.len(), 10);
    }

    #[test]
    fn concurrent_summary_while_recording() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("pre", 10, 0));
        let mut handles = vec![];
        for _ in 0..5 {
            let cc = c.clone();
            handles.push(thread::spawn(move || {
                cc.record(sample_metrics("t", 20, 0));
                let _ = cc.summary();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(c.len(), 6);
    }

    // --- TelemetrySpan ---

    #[test]
    fn telemetry_span_attributes() {
        let span = TelemetrySpan::new("op")
            .with_attribute("key", "val")
            .with_attribute("another", "thing");
        assert_eq!(span.name, "op");
        assert_eq!(span.attributes.len(), 2);
        assert_eq!(span.attributes["key"], "val");
    }

    #[test]
    fn telemetry_span_serde_roundtrip() {
        let span = TelemetrySpan::new("run").with_attribute("backend", "mock");
        let json = serde_json::to_string(&span).unwrap();
        let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
        assert_eq!(span2.name, "run");
        assert_eq!(span2.attributes["backend"], "mock");
    }

    // --- JsonExporter ---

    #[test]
    fn json_exporter_valid_output() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 100, 0));
        let s = c.summary();
        let exporter = JsonExporter;
        let json = exporter.export(&s).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["count"], 1);
    }

    #[test]
    fn json_exporter_empty_summary() {
        let s = MetricsSummary::default();
        let exporter = JsonExporter;
        let json = exporter.export(&s).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["count"], 0);
    }

    #[test]
    fn json_exporter_backend_counts_deterministic() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("zebra", 10, 0));
        c.record(sample_metrics("alpha", 20, 0));
        let s = c.summary();
        let exporter = JsonExporter;
        let json = exporter.export(&s).unwrap();
        // BTreeMap ensures alphabetical key order
        let keys_start = json.find("\"alpha\"").unwrap();
        let keys_end = json.find("\"zebra\"").unwrap();
        assert!(keys_start < keys_end);
    }

    // --- Percentile edge cases ---

    #[test]
    fn percentile_empty() {
        assert_eq!(percentile(&[], 50.0), 0.0);
    }

    #[test]
    fn percentile_single() {
        assert_eq!(percentile(&[42], 99.0), 42.0);
    }
}
