// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-telemetry
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Structured telemetry and metrics collection for Agent Backplane runs.

pub mod events;
pub mod export;
pub mod hooks;
pub mod metrics;
pub mod pipeline;
pub mod report;
pub mod runtime_events;
pub mod spans;
pub mod tracing_integration;

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

    /// Compute per-backend reports from all recorded runs.
    pub fn backend_reports(&self) -> Vec<BackendReport> {
        let data = self.inner.lock().expect("metrics lock poisoned");
        let mut grouped: BTreeMap<String, Vec<&RunMetrics>> = BTreeMap::new();
        for r in data.iter() {
            grouped.entry(r.backend_name.clone()).or_default().push(r);
        }
        grouped
            .into_iter()
            .map(|(name, runs)| {
                let total_runs = runs.len() as u64;
                let error_count = runs.iter().filter(|r| r.errors_count > 0).count() as u64;
                let success_count = total_runs - error_count;
                let success_rate = if total_runs == 0 {
                    0.0
                } else {
                    success_count as f64 / total_runs as f64
                };

                let mut durations: Vec<u64> = runs.iter().map(|r| r.duration_ms).collect();
                durations.sort_unstable();
                let total_dur: u64 = durations.iter().sum();
                let mean_latency_ms = if durations.is_empty() {
                    0.0
                } else {
                    total_dur as f64 / durations.len() as f64
                };
                let p50_latency_ms = percentile(&durations, 50.0);
                let p99_latency_ms = percentile(&durations, 99.0);

                let total_tokens_in: u64 = runs.iter().map(|r| r.tokens_in).sum();
                let total_tokens_out: u64 = runs.iter().map(|r| r.tokens_out).sum();

                BackendReport {
                    backend_name: name,
                    total_runs,
                    success_count,
                    error_count,
                    success_rate,
                    mean_latency_ms,
                    p50_latency_ms,
                    p99_latency_ms,
                    total_tokens_in,
                    total_tokens_out,
                }
            })
            .collect()
    }

    /// Build a combined aggregated view of all metrics.
    ///
    /// Includes the global summary, per-backend breakdowns, and any
    /// dialect-pair statistics from the provided registry.
    pub fn aggregate(&self, dialect_registry: &DialectPairRegistry) -> AggregatedMetrics {
        let dialect_pairs = dialect_registry
            .snapshot()
            .into_iter()
            .map(|(pair, stats)| DialectPairReport {
                source: pair.source,
                target: pair.target,
                run_count: stats.run_count,
                total_duration_ms: stats.total_duration_ms,
                total_tokens_in: stats.total_tokens_in,
                total_tokens_out: stats.total_tokens_out,
                error_count: stats.error_count,
            })
            .collect();
        AggregatedMetrics {
            summary: self.summary(),
            backend_reports: self.backend_reports(),
            dialect_pairs,
        }
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
// RunSummary
// ---------------------------------------------------------------------------

/// Aggregated summary of a single agent run with event counts by type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunSummary {
    /// Event counts by kind name (e.g. "tool_call", "error").
    pub event_counts: BTreeMap<String, u64>,
    /// Total number of events recorded.
    pub total_events: u64,
    /// Wall-clock duration in milliseconds.
    pub total_duration_ms: u64,
    /// Number of error events.
    pub error_count: u64,
    /// Number of warning events.
    pub warning_count: u64,
    /// Number of tool-call events.
    pub tool_call_count: u64,
}

impl RunSummary {
    /// Create a new empty summary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an event by its kind name.
    pub fn record_event(&mut self, kind: &str) {
        *self.event_counts.entry(kind.to_string()).or_insert(0) += 1;
        self.total_events += 1;
        match kind {
            "error" => self.error_count += 1,
            "warning" => self.warning_count += 1,
            "tool_call" => self.tool_call_count += 1,
            _ => {}
        }
    }

    /// Set the total run duration.
    pub fn set_duration(&mut self, ms: u64) {
        self.total_duration_ms = ms;
    }

    /// Whether any errors were recorded.
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }

    /// Error rate as a fraction of total events (0.0 if no events).
    pub fn error_rate(&self) -> f64 {
        if self.total_events == 0 {
            return 0.0;
        }
        self.error_count as f64 / self.total_events as f64
    }

    /// Merge another summary into this one.
    pub fn merge(&mut self, other: &RunSummary) {
        for (kind, count) in &other.event_counts {
            *self.event_counts.entry(kind.clone()).or_insert(0) += count;
        }
        self.total_events += other.total_events;
        self.total_duration_ms += other.total_duration_ms;
        self.error_count += other.error_count;
        self.warning_count += other.warning_count;
        self.tool_call_count += other.tool_call_count;
    }

    /// Build a RunSummary from a slice of event-kind strings.
    pub fn from_events(kinds: &[&str], duration_ms: u64) -> Self {
        let mut s = Self::new();
        for kind in kinds {
            s.record_event(kind);
        }
        s.set_duration(duration_ms);
        s
    }
}

// ---------------------------------------------------------------------------
// LatencyHistogram
// ---------------------------------------------------------------------------

/// Simple histogram for tracking latency values in milliseconds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LatencyHistogram {
    values: Vec<f64>,
}

impl LatencyHistogram {
    /// Create a new empty histogram.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a latency value (in milliseconds).
    pub fn record(&mut self, value: f64) {
        self.values.push(value);
    }

    /// Number of recorded values.
    pub fn count(&self) -> usize {
        self.values.len()
    }

    /// Whether the histogram is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Minimum recorded value, or `None` if empty.
    pub fn min(&self) -> Option<f64> {
        self.values
            .iter()
            .copied()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Maximum recorded value, or `None` if empty.
    pub fn max(&self) -> Option<f64> {
        self.values
            .iter()
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Mean of all recorded values (0.0 if empty).
    pub fn mean(&self) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.values.iter().sum();
        sum / self.values.len() as f64
    }

    /// Compute a percentile (0–100) from recorded values.
    pub fn percentile(&self, pct: f64) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self.values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if sorted.len() == 1 {
            return sorted[0];
        }
        let rank = pct / 100.0 * (sorted.len() - 1) as f64;
        let lower = rank.floor() as usize;
        let upper = rank.ceil() as usize;
        let frac = rank - lower as f64;
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
    }

    /// 50th percentile (median).
    pub fn p50(&self) -> f64 {
        self.percentile(50.0)
    }

    /// 95th percentile.
    pub fn p95(&self) -> f64 {
        self.percentile(95.0)
    }

    /// 99th percentile.
    pub fn p99(&self) -> f64 {
        self.percentile(99.0)
    }

    /// Merge another histogram into this one.
    pub fn merge(&mut self, other: &LatencyHistogram) {
        self.values.extend_from_slice(&other.values);
    }

    /// Count values falling into the given bucket boundaries.
    ///
    /// Given boundaries `[b0, b1, b2, ...]`, returns counts for:
    /// `[0, b0)`, `[b0, b1)`, `[b1, b2)`, ..., `[bN, ∞)`.
    pub fn buckets(&self, boundaries: &[f64]) -> Vec<u64> {
        let mut counts = vec![0u64; boundaries.len() + 1];
        for &v in &self.values {
            let idx = boundaries
                .iter()
                .position(|&b| v < b)
                .unwrap_or(boundaries.len());
            counts[idx] += 1;
        }
        counts
    }
}

// ---------------------------------------------------------------------------
// CostEstimator
// ---------------------------------------------------------------------------

/// Pricing for a single model (cost per token in USD).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelPricing {
    /// Cost per input token in USD.
    pub input_cost_per_token: f64,
    /// Cost per output token in USD.
    pub output_cost_per_token: f64,
}

/// Estimates monetary cost based on token usage and model pricing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostEstimator {
    pricing: BTreeMap<String, ModelPricing>,
}

impl CostEstimator {
    /// Create a new estimator with no pricing data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register pricing for a model.
    pub fn set_pricing(&mut self, model: &str, pricing: ModelPricing) {
        self.pricing.insert(model.to_string(), pricing);
    }

    /// Get pricing for a model, if registered.
    pub fn get_pricing(&self, model: &str) -> Option<&ModelPricing> {
        self.pricing.get(model)
    }

    /// Estimate cost for a single model usage. Returns `None` if the model
    /// has no registered pricing.
    pub fn estimate(&self, model: &str, input_tokens: u64, output_tokens: u64) -> Option<f64> {
        let p = self.pricing.get(model)?;
        Some(
            input_tokens as f64 * p.input_cost_per_token
                + output_tokens as f64 * p.output_cost_per_token,
        )
    }

    /// Estimate total cost across multiple model usages.
    ///
    /// Each tuple is `(model_name, input_tokens, output_tokens)`.
    /// Models without registered pricing are skipped.
    pub fn estimate_total(&self, usages: &[(&str, u64, u64)]) -> f64 {
        usages
            .iter()
            .filter_map(|(model, inp, out)| self.estimate(model, *inp, *out))
            .sum()
    }

    /// List all registered model names.
    pub fn models(&self) -> Vec<&str> {
        self.pricing.keys().map(|s| s.as_str()).collect()
    }
}

// ---------------------------------------------------------------------------
// DialectPairRegistry
// ---------------------------------------------------------------------------

/// Key identifying a source→target dialect mapping.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DialectPair {
    /// Source dialect (e.g. `"openai"`).
    pub source: String,
    /// Target dialect (e.g. `"anthropic"`).
    pub target: String,
}

/// Accumulated statistics for a single dialect pair.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DialectPairStats {
    /// Number of runs using this pair.
    pub run_count: u64,
    /// Cumulative duration in milliseconds.
    pub total_duration_ms: u64,
    /// Cumulative inbound tokens.
    pub total_tokens_in: u64,
    /// Cumulative outbound tokens.
    pub total_tokens_out: u64,
    /// Number of runs that encountered errors.
    pub error_count: u64,
}

/// Thread-safe registry tracking usage across dialect pairs.
///
/// Each pair represents a source→target dialect mapping used during a run,
/// useful for understanding which projection matrix paths are active.
#[derive(Debug, Clone, Default)]
pub struct DialectPairRegistry {
    inner: Arc<Mutex<BTreeMap<DialectPair, DialectPairStats>>>,
}

impl DialectPairRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a run that used the given dialect pair.
    pub fn record(
        &self,
        source: &str,
        target: &str,
        duration_ms: u64,
        tokens_in: u64,
        tokens_out: u64,
        had_errors: bool,
    ) {
        let key = DialectPair {
            source: source.to_string(),
            target: target.to_string(),
        };
        let mut map = self.inner.lock().expect("dialect pair lock poisoned");
        let stats = map.entry(key).or_default();
        stats.run_count += 1;
        stats.total_duration_ms += duration_ms;
        stats.total_tokens_in += tokens_in;
        stats.total_tokens_out += tokens_out;
        if had_errors {
            stats.error_count += 1;
        }
    }

    /// Return a snapshot of all recorded pairs and their stats.
    pub fn snapshot(&self) -> BTreeMap<DialectPair, DialectPairStats> {
        let map = self.inner.lock().expect("dialect pair lock poisoned");
        map.clone()
    }

    /// Total number of distinct dialect pairs observed.
    pub fn pair_count(&self) -> usize {
        let map = self.inner.lock().expect("dialect pair lock poisoned");
        map.len()
    }

    /// Total runs across all pairs.
    pub fn total_runs(&self) -> u64 {
        let map = self.inner.lock().expect("dialect pair lock poisoned");
        map.values().map(|s| s.run_count).sum()
    }

    /// Reset all tracked pairs.
    pub fn reset(&self) {
        let mut map = self.inner.lock().expect("dialect pair lock poisoned");
        map.clear();
    }
}

// ---------------------------------------------------------------------------
// BackendReport
// ---------------------------------------------------------------------------

/// Per-backend aggregated metrics derived from recorded runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BackendReport {
    /// Backend name.
    pub backend_name: String,
    /// Total number of runs on this backend.
    pub total_runs: u64,
    /// Runs that completed without errors.
    pub success_count: u64,
    /// Runs that had at least one error.
    pub error_count: u64,
    /// Success rate (0.0–1.0).
    pub success_rate: f64,
    /// Mean latency in milliseconds.
    pub mean_latency_ms: f64,
    /// Median (p50) latency in milliseconds.
    pub p50_latency_ms: f64,
    /// 99th-percentile latency in milliseconds.
    pub p99_latency_ms: f64,
    /// Total inbound tokens.
    pub total_tokens_in: u64,
    /// Total outbound tokens.
    pub total_tokens_out: u64,
}

// ---------------------------------------------------------------------------
// AggregatedMetrics
// ---------------------------------------------------------------------------

/// Combined metrics view across all dimensions.
///
/// Brings together the global summary, per-backend breakdowns, and
/// per-dialect-pair statistics for comprehensive monitoring.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AggregatedMetrics {
    /// Global summary across all runs.
    pub summary: MetricsSummary,
    /// Per-backend breakdowns.
    pub backend_reports: Vec<BackendReport>,
    /// Per-dialect-pair statistics (flattened for JSON compatibility).
    pub dialect_pairs: Vec<DialectPairReport>,
}

/// Flattened per-dialect-pair report suitable for serialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DialectPairReport {
    /// Source dialect.
    pub source: String,
    /// Target dialect.
    pub target: String,
    /// Number of runs using this pair.
    pub run_count: u64,
    /// Cumulative duration in milliseconds.
    pub total_duration_ms: u64,
    /// Cumulative inbound tokens.
    pub total_tokens_in: u64,
    /// Cumulative outbound tokens.
    pub total_tokens_out: u64,
    /// Number of runs that encountered errors.
    pub error_count: u64,
}

// ---------------------------------------------------------------------------
// MetricsExporter
// ---------------------------------------------------------------------------

/// Export format for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Pretty-printed JSON.
    Json,
    /// Comma-separated values.
    Csv,
    /// Structured key=value format.
    Structured,
    /// Prometheus text exposition format.
    Prometheus,
}

/// Multi-format metrics exporter.
#[derive(Debug, Default)]
pub struct MetricsExporter;

impl MetricsExporter {
    /// Export a `MetricsSummary` as JSON.
    pub fn export_json(summary: &MetricsSummary) -> Result<String, String> {
        serde_json::to_string_pretty(summary).map_err(|e| e.to_string())
    }

    /// Export a slice of `RunMetrics` as CSV.
    pub fn export_csv(runs: &[RunMetrics]) -> Result<String, String> {
        let mut out = String::from(
            "backend_name,dialect,duration_ms,events_count,tokens_in,tokens_out,tool_calls_count,errors_count,emulations_applied\n",
        );
        for r in runs {
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\n",
                r.backend_name,
                r.dialect,
                r.duration_ms,
                r.events_count,
                r.tokens_in,
                r.tokens_out,
                r.tool_calls_count,
                r.errors_count,
                r.emulations_applied,
            ));
        }
        Ok(out)
    }

    /// Export a `MetricsSummary` in structured `key=value` format.
    pub fn export_structured(summary: &MetricsSummary) -> Result<String, String> {
        let mut lines = Vec::new();
        lines.push(format!("count={}", summary.count));
        lines.push(format!("mean_duration_ms={:.2}", summary.mean_duration_ms));
        lines.push(format!("p50_duration_ms={:.2}", summary.p50_duration_ms));
        lines.push(format!("p99_duration_ms={:.2}", summary.p99_duration_ms));
        lines.push(format!("total_tokens_in={}", summary.total_tokens_in));
        lines.push(format!("total_tokens_out={}", summary.total_tokens_out));
        lines.push(format!("error_rate={:.4}", summary.error_rate));
        for (k, v) in &summary.backend_counts {
            lines.push(format!("backend.{}={}", k, v));
        }
        Ok(lines.join("\n"))
    }

    /// Export a `MetricsSummary` in the specified format.
    pub fn export(summary: &MetricsSummary, format: ExportFormat) -> Result<String, String> {
        match format {
            ExportFormat::Json => Self::export_json(summary),
            ExportFormat::Csv => {
                // For summary-only CSV export, produce a single-row CSV
                let header = "count,mean_duration_ms,p50_duration_ms,p99_duration_ms,total_tokens_in,total_tokens_out,error_rate\n";
                let row = format!(
                    "{},{:.2},{:.2},{:.2},{},{},{:.4}\n",
                    summary.count,
                    summary.mean_duration_ms,
                    summary.p50_duration_ms,
                    summary.p99_duration_ms,
                    summary.total_tokens_in,
                    summary.total_tokens_out,
                    summary.error_rate,
                );
                Ok(format!("{}{}", header, row))
            }
            ExportFormat::Structured => Self::export_structured(summary),
            ExportFormat::Prometheus => Self::export_prometheus(summary),
        }
    }

    /// Export a `MetricsSummary` in Prometheus text exposition format.
    pub fn export_prometheus(summary: &MetricsSummary) -> Result<String, String> {
        let mut out = String::new();

        out.push_str("# HELP abp_runs_total Total number of completed agent runs.\n");
        out.push_str("# TYPE abp_runs_total counter\n");
        out.push_str(&format!("abp_runs_total {}\n", summary.count));
        out.push('\n');

        out.push_str("# HELP abp_run_duration_ms Run duration in milliseconds.\n");
        out.push_str("# TYPE abp_run_duration_ms summary\n");
        out.push_str(&format!(
            "abp_run_duration_ms{{quantile=\"0.5\"}} {:.2}\n",
            summary.p50_duration_ms
        ));
        out.push_str(&format!(
            "abp_run_duration_ms{{quantile=\"0.99\"}} {:.2}\n",
            summary.p99_duration_ms
        ));
        out.push_str(&format!(
            "abp_run_duration_ms_mean {:.2}\n",
            summary.mean_duration_ms
        ));
        out.push('\n');

        out.push_str("# HELP abp_tokens_total Total token usage.\n");
        out.push_str("# TYPE abp_tokens_total counter\n");
        out.push_str(&format!(
            "abp_tokens_total{{direction=\"input\"}} {}\n",
            summary.total_tokens_in
        ));
        out.push_str(&format!(
            "abp_tokens_total{{direction=\"output\"}} {}\n",
            summary.total_tokens_out
        ));
        out.push('\n');

        out.push_str("# HELP abp_error_rate Fraction of runs with errors.\n");
        out.push_str("# TYPE abp_error_rate gauge\n");
        out.push_str(&format!("abp_error_rate {:.4}\n", summary.error_rate));
        out.push('\n');

        out.push_str("# HELP abp_backend_runs_total Runs per backend.\n");
        out.push_str("# TYPE abp_backend_runs_total counter\n");
        for (backend, count) in &summary.backend_counts {
            out.push_str(&format!(
                "abp_backend_runs_total{{backend=\"{}\"}} {}\n",
                backend, count
            ));
        }

        Ok(out)
    }

    /// Export full aggregated metrics in Prometheus text exposition format.
    ///
    /// Includes per-backend latency/success and per-dialect-pair breakdowns
    /// in addition to the global summary.
    pub fn export_prometheus_aggregated(metrics: &AggregatedMetrics) -> Result<String, String> {
        let mut out = Self::export_prometheus(&metrics.summary)?;
        out.push('\n');

        out.push_str("# HELP abp_backend_success_rate Per-backend success rate.\n");
        out.push_str("# TYPE abp_backend_success_rate gauge\n");
        for br in &metrics.backend_reports {
            out.push_str(&format!(
                "abp_backend_success_rate{{backend=\"{}\"}} {:.4}\n",
                br.backend_name, br.success_rate
            ));
        }
        out.push('\n');

        out.push_str("# HELP abp_backend_latency_ms Per-backend latency in milliseconds.\n");
        out.push_str("# TYPE abp_backend_latency_ms summary\n");
        for br in &metrics.backend_reports {
            out.push_str(&format!(
                "abp_backend_latency_ms{{backend=\"{}\",quantile=\"0.5\"}} {:.2}\n",
                br.backend_name, br.p50_latency_ms
            ));
            out.push_str(&format!(
                "abp_backend_latency_ms{{backend=\"{}\",quantile=\"0.99\"}} {:.2}\n",
                br.backend_name, br.p99_latency_ms
            ));
        }
        out.push('\n');

        out.push_str("# HELP abp_backend_tokens_total Per-backend token usage.\n");
        out.push_str("# TYPE abp_backend_tokens_total counter\n");
        for br in &metrics.backend_reports {
            out.push_str(&format!(
                "abp_backend_tokens_total{{backend=\"{}\",direction=\"input\"}} {}\n",
                br.backend_name, br.total_tokens_in
            ));
            out.push_str(&format!(
                "abp_backend_tokens_total{{backend=\"{}\",direction=\"output\"}} {}\n",
                br.backend_name, br.total_tokens_out
            ));
        }
        out.push('\n');

        if !metrics.dialect_pairs.is_empty() {
            out.push_str("# HELP abp_dialect_pair_runs_total Runs per dialect pair.\n");
            out.push_str("# TYPE abp_dialect_pair_runs_total counter\n");
            for dp in &metrics.dialect_pairs {
                out.push_str(&format!(
                    "abp_dialect_pair_runs_total{{source=\"{}\",target=\"{}\"}} {}\n",
                    dp.source, dp.target, dp.run_count
                ));
            }
            out.push('\n');

            out.push_str("# HELP abp_dialect_pair_errors_total Errors per dialect pair.\n");
            out.push_str("# TYPE abp_dialect_pair_errors_total counter\n");
            for dp in &metrics.dialect_pairs {
                out.push_str(&format!(
                    "abp_dialect_pair_errors_total{{source=\"{}\",target=\"{}\"}} {}\n",
                    dp.source, dp.target, dp.error_count
                ));
            }
        }

        Ok(out)
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

    // --- DialectPairRegistry ---

    #[test]
    fn dialect_registry_empty() {
        let r = DialectPairRegistry::new();
        assert_eq!(r.pair_count(), 0);
        assert_eq!(r.total_runs(), 0);
        assert!(r.snapshot().is_empty());
    }

    #[test]
    fn dialect_registry_record_and_snapshot() {
        let r = DialectPairRegistry::new();
        r.record("openai", "anthropic", 100, 50, 60, false);
        r.record("openai", "anthropic", 200, 40, 80, true);
        r.record("openai", "gemini", 150, 30, 70, false);

        assert_eq!(r.pair_count(), 2);
        assert_eq!(r.total_runs(), 3);

        let snap = r.snapshot();
        let pair_oa = DialectPair {
            source: "openai".into(),
            target: "anthropic".into(),
        };
        let stats_oa = &snap[&pair_oa];
        assert_eq!(stats_oa.run_count, 2);
        assert_eq!(stats_oa.total_duration_ms, 300);
        assert_eq!(stats_oa.total_tokens_in, 90);
        assert_eq!(stats_oa.total_tokens_out, 140);
        assert_eq!(stats_oa.error_count, 1);

        let pair_og = DialectPair {
            source: "openai".into(),
            target: "gemini".into(),
        };
        let stats_og = &snap[&pair_og];
        assert_eq!(stats_og.run_count, 1);
        assert_eq!(stats_og.error_count, 0);
    }

    #[test]
    fn dialect_registry_reset() {
        let r = DialectPairRegistry::new();
        r.record("a", "b", 10, 1, 2, false);
        r.reset();
        assert_eq!(r.pair_count(), 0);
        assert_eq!(r.total_runs(), 0);
    }

    #[test]
    fn dialect_registry_thread_safety() {
        let r = DialectPairRegistry::new();
        let mut handles = vec![];
        for i in 0..10 {
            let rr = r.clone();
            handles.push(thread::spawn(move || {
                rr.record("src", "tgt", i * 10, 10, 20, i % 3 == 0);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(r.total_runs(), 10);
        assert_eq!(r.pair_count(), 1);
    }

    // --- BackendReport ---

    #[test]
    fn backend_reports_empty_collector() {
        let c = MetricsCollector::new();
        assert!(c.backend_reports().is_empty());
    }

    #[test]
    fn backend_reports_single_backend() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 100, 0));
        c.record(sample_metrics("mock", 200, 1));
        c.record(sample_metrics("mock", 300, 0));

        let reports = c.backend_reports();
        assert_eq!(reports.len(), 1);

        let r = &reports[0];
        assert_eq!(r.backend_name, "mock");
        assert_eq!(r.total_runs, 3);
        assert_eq!(r.success_count, 2);
        assert_eq!(r.error_count, 1);
        assert!((r.success_rate - 2.0 / 3.0).abs() < 1e-10);
        assert!((r.mean_latency_ms - 200.0).abs() < f64::EPSILON);
        assert_eq!(r.total_tokens_in, 300); // 3 * 100
        assert_eq!(r.total_tokens_out, 600); // 3 * 200
    }

    #[test]
    fn backend_reports_multiple_backends() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("alpha", 50, 0));
        c.record(sample_metrics("beta", 100, 1));
        c.record(sample_metrics("alpha", 150, 0));

        let reports = c.backend_reports();
        assert_eq!(reports.len(), 2);
        // BTreeMap ordering: alpha before beta
        assert_eq!(reports[0].backend_name, "alpha");
        assert_eq!(reports[0].total_runs, 2);
        assert_eq!(reports[0].success_rate, 1.0);
        assert_eq!(reports[1].backend_name, "beta");
        assert_eq!(reports[1].total_runs, 1);
        assert_eq!(reports[1].success_rate, 0.0);
    }

    // --- AggregatedMetrics ---

    #[test]
    fn aggregate_combines_all_dimensions() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 100, 0));
        c.record(sample_metrics("sidecar", 200, 1));

        let dr = DialectPairRegistry::new();
        dr.record("openai", "anthropic", 100, 50, 60, false);

        let agg = c.aggregate(&dr);
        assert_eq!(agg.summary.count, 2);
        assert_eq!(agg.backend_reports.len(), 2);
        assert_eq!(agg.dialect_pairs.len(), 1);
    }

    #[test]
    fn aggregate_serde_roundtrip() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 42, 0));
        let dr = DialectPairRegistry::new();
        dr.record("a", "b", 42, 10, 20, false);

        let agg = c.aggregate(&dr);
        let json = serde_json::to_string(&agg).unwrap();
        let agg2: AggregatedMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(agg, agg2);
    }

    // --- Prometheus export ---

    #[test]
    fn prometheus_export_contains_expected_metrics() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 100, 0));
        c.record(sample_metrics("sidecar", 200, 1));
        let s = c.summary();

        let prom = MetricsExporter::export_prometheus(&s).unwrap();
        assert!(prom.contains("# TYPE abp_runs_total counter"));
        assert!(prom.contains("abp_runs_total 2"));
        assert!(prom.contains("abp_run_duration_ms{quantile=\"0.5\"}"));
        assert!(prom.contains("abp_run_duration_ms{quantile=\"0.99\"}"));
        assert!(prom.contains("abp_tokens_total{direction=\"input\"} 200"));
        assert!(prom.contains("abp_tokens_total{direction=\"output\"} 400"));
        assert!(prom.contains("abp_error_rate"));
        assert!(prom.contains("abp_backend_runs_total{backend=\"mock\"} 1"));
        assert!(prom.contains("abp_backend_runs_total{backend=\"sidecar\"} 1"));
    }

    #[test]
    fn prometheus_export_empty_summary() {
        let s = MetricsSummary::default();
        let prom = MetricsExporter::export_prometheus(&s).unwrap();
        assert!(prom.contains("abp_runs_total 0"));
        assert!(prom.contains("abp_error_rate 0.0000"));
    }

    #[test]
    fn prometheus_export_via_format_enum() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 50, 0));
        let s = c.summary();
        let prom = MetricsExporter::export(&s, ExportFormat::Prometheus).unwrap();
        assert!(prom.contains("abp_runs_total 1"));
    }

    #[test]
    fn prometheus_aggregated_export() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 100, 0));
        c.record(sample_metrics("mock", 200, 1));

        let dr = DialectPairRegistry::new();
        dr.record("openai", "anthropic", 100, 50, 60, false);
        dr.record("openai", "anthropic", 200, 40, 80, true);

        let agg = c.aggregate(&dr);
        let prom = MetricsExporter::export_prometheus_aggregated(&agg).unwrap();

        // Global summary present
        assert!(prom.contains("abp_runs_total 2"));
        // Per-backend success rate
        assert!(prom.contains("abp_backend_success_rate{backend=\"mock\"}"));
        // Per-backend latency
        assert!(prom.contains("abp_backend_latency_ms{backend=\"mock\",quantile=\"0.5\"}"));
        // Per-backend tokens
        assert!(
            prom.contains("abp_backend_tokens_total{backend=\"mock\",direction=\"input\"} 200")
        );
        // Dialect pair runs
        assert!(
            prom.contains("abp_dialect_pair_runs_total{source=\"openai\",target=\"anthropic\"} 2")
        );
        // Dialect pair errors
        assert!(
            prom.contains(
                "abp_dialect_pair_errors_total{source=\"openai\",target=\"anthropic\"} 1"
            )
        );
    }

    #[test]
    fn prometheus_aggregated_no_dialect_pairs() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("mock", 100, 0));
        let dr = DialectPairRegistry::new();
        let agg = c.aggregate(&dr);
        let prom = MetricsExporter::export_prometheus_aggregated(&agg).unwrap();
        // Should not contain dialect pair sections when empty
        assert!(!prom.contains("abp_dialect_pair_runs_total"));
    }
}
