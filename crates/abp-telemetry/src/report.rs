// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code)]
//! Aggregated telemetry reports combining metrics, events, and spans.
//!
//! [`TelemetryReport`] provides a unified view of all telemetry data
//! with a human-readable summary.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;

use crate::metrics::{MetricsRegistry, MetricsSnapshot};
use crate::runtime_events::{RuntimeEvent, RuntimeEventCollector};
use crate::tracing_integration::{SpanRecorder, TracingSpan};

// ---------------------------------------------------------------------------
// TelemetryReport
// ---------------------------------------------------------------------------

/// Aggregated telemetry report combining metrics, events, and spans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryReport {
    /// Metrics snapshot at time of report generation.
    pub metrics: MetricsSnapshot,
    /// Runtime events collected during the reporting period.
    pub events: Vec<RuntimeEvent>,
    /// Completed tracing spans.
    pub spans: Vec<TracingSpan>,
    /// ISO-8601 timestamp of report generation.
    pub generated_at: String,
}

impl TelemetryReport {
    /// Build a report from live collectors.
    pub fn build(
        registry: &MetricsRegistry,
        event_collector: &RuntimeEventCollector,
        span_recorder: &SpanRecorder,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            metrics: registry.snapshot(),
            events: event_collector.events(),
            spans: span_recorder.spans(),
            generated_at: timestamp.into(),
        }
    }

    /// Build a report from pre-captured components.
    pub fn from_parts(
        metrics: MetricsSnapshot,
        events: Vec<RuntimeEvent>,
        spans: Vec<TracingSpan>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            metrics,
            events,
            spans,
            generated_at: timestamp.into(),
        }
    }

    /// Total number of events in this report.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Total number of spans in this report.
    pub fn span_count(&self) -> usize {
        self.spans.len()
    }

    /// Count of events grouped by label.
    pub fn event_summary(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for ev in &self.events {
            *counts.entry(ev.label().to_string()).or_insert(0) += 1;
        }
        counts
    }

    /// Generate a human-readable summary string.
    pub fn to_summary(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "=== Telemetry Report ===");
        let _ = writeln!(out, "Generated: {}", self.generated_at);
        let _ = writeln!(out);

        // Metrics
        let _ = writeln!(out, "--- Metrics ---");
        if self.metrics.counters.is_empty() {
            let _ = writeln!(out, "  Counters: (none)");
        } else {
            let _ = writeln!(out, "  Counters:");
            for (name, value) in &self.metrics.counters {
                let _ = writeln!(out, "    {name}: {value}");
            }
        }
        if self.metrics.gauges.is_empty() {
            let _ = writeln!(out, "  Gauges: (none)");
        } else {
            let _ = writeln!(out, "  Gauges:");
            for (name, value) in &self.metrics.gauges {
                let _ = writeln!(out, "    {name}: {value}");
            }
        }
        if self.metrics.histograms.is_empty() {
            let _ = writeln!(out, "  Histograms: (none)");
        } else {
            let _ = writeln!(out, "  Histograms:");
            for (name, stats) in &self.metrics.histograms {
                let _ = writeln!(
                    out,
                    "    {name}: count={}, mean={:.2}, p50={:.2}, p99={:.2}",
                    stats.count,
                    stats.mean.unwrap_or(0.0),
                    stats.p50.unwrap_or(0.0),
                    stats.p99.unwrap_or(0.0),
                );
            }
        }

        // Events
        let _ = writeln!(out);
        let _ = writeln!(out, "--- Events ({}) ---", self.events.len());
        let summary = self.event_summary();
        for (label, count) in &summary {
            let _ = writeln!(out, "  {label}: {count}");
        }

        // Spans
        let _ = writeln!(out);
        let _ = writeln!(out, "--- Spans ({}) ---", self.spans.len());
        for span in &self.spans {
            let dur = span.duration_ms.map(|d| format!("{d:.2}ms")).unwrap_or_else(|| "open".into());
            let _ = writeln!(out, "  {} ({})", span.name, dur);
        }

        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricsRegistry;
    use crate::runtime_events::RuntimeEventCollector;
    use crate::tracing_integration::SpanRecorder;

    fn empty_snapshot() -> MetricsSnapshot {
        MetricsSnapshot {
            counters: BTreeMap::new(),
            gauges: BTreeMap::new(),
            histograms: BTreeMap::new(),
        }
    }

    #[test]
    fn report_empty() {
        let report = TelemetryReport::from_parts(
            empty_snapshot(),
            vec![],
            vec![],
            "2025-01-01T00:00:00Z",
        );
        assert_eq!(report.event_count(), 0);
        assert_eq!(report.span_count(), 0);
        assert!(report.event_summary().is_empty());
    }

    #[test]
    fn report_from_collectors() {
        let reg = MetricsRegistry::new();
        reg.counter("reqs").increment_by(5);
        let events = RuntimeEventCollector::new();
        events.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        let spans = SpanRecorder::new();
        spans.record(TracingSpan::completed("op", 10.0));

        let report = TelemetryReport::build(&reg, &events, &spans, "2025-01-01T00:00:00Z");
        assert_eq!(report.metrics.counters["reqs"], 5);
        assert_eq!(report.event_count(), 1);
        assert_eq!(report.span_count(), 1);
    }

    #[test]
    fn report_with_metrics() {
        let reg = MetricsRegistry::new();
        reg.counter("a").increment_by(10);
        reg.gauge("b").set(42);
        reg.histogram("c").record(100.0);

        let report = TelemetryReport::build(
            &reg,
            &RuntimeEventCollector::new(),
            &SpanRecorder::new(),
            "t",
        );
        assert_eq!(report.metrics.counters["a"], 10);
        assert_eq!(report.metrics.gauges["b"], 42);
        assert_eq!(report.metrics.histograms["c"].count, 1);
    }

    #[test]
    fn report_with_events() {
        let events = RuntimeEventCollector::new();
        events.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        events.emit(RuntimeEvent::PolicyEvaluated {
            run_id: "r1".into(),
            policy_name: "strict".into(),
            allowed: true,
        });
        events.emit(RuntimeEvent::RunCompleted {
            run_id: "r1".into(),
            duration_ms: 100,
            success: true,
            timestamp: "t".into(),
        });

        let report = TelemetryReport::build(
            &MetricsRegistry::new(),
            &events,
            &SpanRecorder::new(),
            "t",
        );
        assert_eq!(report.event_count(), 3);
    }

    #[test]
    fn report_with_spans() {
        let rec = SpanRecorder::new();
        rec.record(TracingSpan::completed("alpha", 10.0));
        rec.record(TracingSpan::completed("beta", 20.0));

        let report = TelemetryReport::build(
            &MetricsRegistry::new(),
            &RuntimeEventCollector::new(),
            &rec,
            "t",
        );
        assert_eq!(report.span_count(), 2);
    }

    #[test]
    fn report_event_summary() {
        let events = RuntimeEventCollector::new();
        events.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        events.emit(RuntimeEvent::RunStarted {
            run_id: "r2".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        events.emit(RuntimeEvent::ErrorOccurred {
            run_id: "r1".into(),
            code: "E001".into(),
            message: "fail".into(),
        });

        let report = TelemetryReport::build(
            &MetricsRegistry::new(),
            &events,
            &SpanRecorder::new(),
            "t",
        );
        let summary = report.event_summary();
        assert_eq!(summary["run_started"], 2);
        assert_eq!(summary["error_occurred"], 1);
    }

    #[test]
    fn report_summary_format() {
        let reg = MetricsRegistry::new();
        reg.counter("requests").increment_by(42);

        let events = RuntimeEventCollector::new();
        events.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });

        let rec = SpanRecorder::new();
        rec.record(TracingSpan::completed("process", 15.5));

        let report = TelemetryReport::build(&reg, &events, &rec, "2025-06-01T12:00:00Z");
        let text = report.to_summary();

        assert!(text.contains("Telemetry Report"));
        assert!(text.contains("2025-06-01T12:00:00Z"));
        assert!(text.contains("requests: 42"));
        assert!(text.contains("run_started: 1"));
        assert!(text.contains("process"));
    }

    #[test]
    fn report_summary_empty() {
        let report = TelemetryReport::from_parts(
            empty_snapshot(),
            vec![],
            vec![],
            "2025-01-01T00:00:00Z",
        );
        let text = report.to_summary();
        assert!(text.contains("Counters: (none)"));
        assert!(text.contains("Gauges: (none)"));
        assert!(text.contains("Histograms: (none)"));
        assert!(text.contains("Events (0)"));
        assert!(text.contains("Spans (0)"));
    }

    #[test]
    fn report_serde_roundtrip() {
        let reg = MetricsRegistry::new();
        reg.counter("c").increment();
        let events = RuntimeEventCollector::new();
        events.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        let rec = SpanRecorder::new();
        rec.record(TracingSpan::completed("op", 5.0));

        let report = TelemetryReport::build(&reg, &events, &rec, "2025-01-01T00:00:00Z");
        let json = serde_json::to_string(&report).unwrap();
        let back: TelemetryReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_count(), 1);
        assert_eq!(back.span_count(), 1);
        assert_eq!(back.metrics.counters["c"], 1);
    }

    #[test]
    fn report_combined_full() {
        let reg = MetricsRegistry::new();
        reg.counter("total_requests").increment_by(100);
        reg.counter("errors").increment_by(5);
        reg.gauge("active_connections").set(3);
        let h = reg.histogram("latency_ms");
        for i in 1..=50 {
            h.record(i as f64);
        }

        let events = RuntimeEventCollector::new();
        events.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        events.emit(RuntimeEvent::BackendSelected {
            run_id: "r1".into(),
            backend: "mock".into(),
        });
        events.emit(RuntimeEvent::PolicyEvaluated {
            run_id: "r1".into(),
            policy_name: "default".into(),
            allowed: true,
        });
        events.emit(RuntimeEvent::RewriteApplied {
            run_id: "r1".into(),
            from_dialect: "openai".into(),
            to_dialect: "anthropic".into(),
        });
        events.emit(RuntimeEvent::ReceiptGenerated {
            run_id: "r1".into(),
            receipt_hash: Some("sha256:abc".into()),
        });
        events.emit(RuntimeEvent::RunCompleted {
            run_id: "r1".into(),
            duration_ms: 250,
            success: true,
            timestamp: "t".into(),
        });

        let rec = SpanRecorder::new();
        rec.record(TracingSpan::completed("backend_call", 200.0));
        rec.record(TracingSpan::completed("policy_check", 5.0));

        let report = TelemetryReport::build(&reg, &events, &rec, "2025-06-15T10:00:00Z");

        assert_eq!(report.event_count(), 6);
        assert_eq!(report.span_count(), 2);
        assert_eq!(report.metrics.counters["total_requests"], 100);

        let text = report.to_summary();
        assert!(text.contains("total_requests: 100"));
        assert!(text.contains("active_connections: 3"));
        assert!(text.contains("latency_ms:"));
        assert!(text.contains("backend_call"));
        assert!(text.contains("policy_check"));
    }
}
