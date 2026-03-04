// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Telemetry export: point-in-time snapshots in JSON and Prometheus formats.
//!
//! [`TelemetrySnapshot`] captures the current state of a [`MetricsRegistry`]
//! and can be serialized to JSON or Prometheus text exposition format.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;

use crate::metrics::{HistogramStats, MetricsRegistry};

// ---------------------------------------------------------------------------
// TelemetrySnapshot
// ---------------------------------------------------------------------------

/// Point-in-time view of all metrics from a [`MetricsRegistry`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    /// Counter name → current value.
    pub counters: BTreeMap<String, u64>,
    /// Gauge name → current value.
    pub gauges: BTreeMap<String, i64>,
    /// Histogram name → summary statistics.
    pub histograms: BTreeMap<String, HistogramStats>,
}

impl TelemetrySnapshot {
    /// Capture a snapshot from a [`MetricsRegistry`].
    pub fn from_registry(registry: &MetricsRegistry) -> Self {
        Self {
            counters: registry.counter_snapshot(),
            gauges: registry.gauge_snapshot(),
            histograms: registry.histogram_snapshot(),
        }
    }

    /// Serialize the snapshot to a [`serde_json::Value`].
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("TelemetrySnapshot is always serializable")
    }

    /// Render the snapshot in Prometheus text exposition format.
    ///
    /// Counters are emitted as `<name>_total`, gauges as `<name>`, and
    /// histograms emit `_count`, `_sum`, `_p50`, `_p90`, `_p99` suffixed
    /// lines.
    pub fn to_prometheus_text(&self) -> String {
        let mut out = String::new();

        // Counters
        for (name, value) in &self.counters {
            let safe = prom_name(name);
            let _ = writeln!(out, "# TYPE {safe}_total counter");
            let _ = writeln!(out, "{safe}_total {value}");
        }

        // Gauges
        for (name, value) in &self.gauges {
            let safe = prom_name(name);
            let _ = writeln!(out, "# TYPE {safe} gauge");
            let _ = writeln!(out, "{safe} {value}");
        }

        // Histograms
        for (name, stats) in &self.histograms {
            let safe = prom_name(name);
            let _ = writeln!(out, "# TYPE {safe} summary");
            let _ = writeln!(out, "{safe}_count {}", stats.count);
            if let Some(mean) = stats.mean {
                let _ = writeln!(out, "{safe}_sum {}", mean * stats.count as f64);
            }
            if let Some(p50) = stats.p50 {
                let _ = writeln!(out, "{safe}{{quantile=\"0.5\"}} {p50}");
            }
            if let Some(p90) = stats.p90 {
                let _ = writeln!(out, "{safe}{{quantile=\"0.9\"}} {p90}");
            }
            if let Some(p99) = stats.p99 {
                let _ = writeln!(out, "{safe}{{quantile=\"0.99\"}} {p99}");
            }
        }

        out
    }
}

/// Sanitise a metric name into a valid Prometheus metric name.
fn prom_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricsRegistry;

    #[test]
    fn snapshot_from_empty_registry() {
        let reg = MetricsRegistry::new();
        let snap = TelemetrySnapshot::from_registry(&reg);
        assert!(snap.counters.is_empty());
        assert!(snap.gauges.is_empty());
        assert!(snap.histograms.is_empty());
    }

    #[test]
    fn snapshot_captures_counters() {
        let reg = MetricsRegistry::new();
        let c = reg.counter("requests");
        c.increment();
        c.increment();
        let snap = TelemetrySnapshot::from_registry(&reg);
        assert_eq!(snap.counters.get("requests"), Some(&2));
    }

    #[test]
    fn snapshot_captures_gauges() {
        let reg = MetricsRegistry::new();
        let g = reg.gauge("in_flight");
        g.increment();
        g.increment();
        g.decrement();
        let snap = TelemetrySnapshot::from_registry(&reg);
        assert_eq!(snap.gauges.get("in_flight"), Some(&1));
    }

    #[test]
    fn snapshot_captures_histograms() {
        let reg = MetricsRegistry::new();
        let h = reg.histogram("latency_ms");
        h.record(10.0);
        h.record(20.0);
        h.record(30.0);
        let snap = TelemetrySnapshot::from_registry(&reg);
        let stats = snap.histograms.get("latency_ms").unwrap();
        assert_eq!(stats.count, 3);
        assert!(stats.mean.unwrap() > 19.0 && stats.mean.unwrap() < 21.0);
    }

    #[test]
    fn to_json_basic() {
        let reg = MetricsRegistry::new();
        reg.counter("reqs").increment_by(5);
        reg.gauge("active").set(3);
        let snap = TelemetrySnapshot::from_registry(&reg);
        let json = snap.to_json();
        assert_eq!(json["counters"]["reqs"], 5);
        assert_eq!(json["gauges"]["active"], 3);
    }

    #[test]
    fn to_json_histogram_stats() {
        let reg = MetricsRegistry::new();
        let h = reg.histogram("dur");
        h.record(100.0);
        h.record(200.0);
        let snap = TelemetrySnapshot::from_registry(&reg);
        let json = snap.to_json();
        assert_eq!(json["histograms"]["dur"]["count"], 2);
    }

    #[test]
    fn to_prometheus_text_counters() {
        let reg = MetricsRegistry::new();
        reg.counter("http_requests").increment_by(42);
        let snap = TelemetrySnapshot::from_registry(&reg);
        let text = snap.to_prometheus_text();
        assert!(text.contains("# TYPE http_requests_total counter"));
        assert!(text.contains("http_requests_total 42"));
    }

    #[test]
    fn to_prometheus_text_gauges() {
        let reg = MetricsRegistry::new();
        reg.gauge("connections").set(7);
        let snap = TelemetrySnapshot::from_registry(&reg);
        let text = snap.to_prometheus_text();
        assert!(text.contains("# TYPE connections gauge"));
        assert!(text.contains("connections 7"));
    }

    #[test]
    fn to_prometheus_text_histograms() {
        let reg = MetricsRegistry::new();
        let h = reg.histogram("latency");
        for i in 1..=100 {
            h.record(i as f64);
        }
        let snap = TelemetrySnapshot::from_registry(&reg);
        let text = snap.to_prometheus_text();
        assert!(text.contains("# TYPE latency summary"));
        assert!(text.contains("latency_count 100"));
        assert!(text.contains("latency{quantile=\"0.5\"}"));
        assert!(text.contains("latency{quantile=\"0.9\"}"));
        assert!(text.contains("latency{quantile=\"0.99\"}"));
    }

    #[test]
    fn prom_name_sanitises_special_chars() {
        assert_eq!(prom_name("a.b-c/d"), "a_b_c_d");
        assert_eq!(prom_name("simple"), "simple");
        assert_eq!(prom_name("under_score"), "under_score");
    }

    #[test]
    fn snapshot_serde_roundtrip() {
        let reg = MetricsRegistry::new();
        reg.counter("c").increment();
        reg.gauge("g").set(42);
        reg.histogram("h").record(1.0);
        let snap = TelemetrySnapshot::from_registry(&reg);
        let json_str = serde_json::to_string(&snap).unwrap();
        let back: TelemetrySnapshot = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.counters["c"], 1);
        assert_eq!(back.gauges["g"], 42);
        assert_eq!(back.histograms["h"].count, 1);
    }
}
