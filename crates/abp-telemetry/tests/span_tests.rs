// Span, metrics, trace-context, tracing-integration, and serde tests.
#![allow(clippy::float_cmp)]

use abp_telemetry::*;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample_run(backend: &str, duration: u64, errors: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.into(),
        dialect: "test".into(),
        duration_ms: duration,
        events_count: 1,
        tokens_in: 100,
        tokens_out: 200,
        tool_calls_count: 0,
        errors_count: errors,
        emulations_applied: 0,
    }
}

// =========================================================================
//  1. Span creation
// =========================================================================

#[test]
fn span_root_with_operation_name() {
    let span = TelemetrySpan::new("agent.run");
    assert_eq!(span.name, "agent.run");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_root_empty_name() {
    let span = TelemetrySpan::new("");
    assert_eq!(span.name, "");
}

#[test]
fn span_child_inherits_trace_context() {
    // Simulate parent→child by propagating a trace_id attribute.
    let parent = TelemetrySpan::new("parent.op")
        .with_attribute("trace_id", "abc-123")
        .with_attribute("span_id", "span-1");

    let child = TelemetrySpan::new("child.op")
        .with_attribute("trace_id", parent.attributes["trace_id"].clone())
        .with_attribute("parent_span_id", parent.attributes["span_id"].clone())
        .with_attribute("span_id", "span-2");

    assert_eq!(child.attributes["trace_id"], "abc-123");
    assert_eq!(child.attributes["parent_span_id"], "span-1");
    assert_eq!(child.attributes["span_id"], "span-2");
}

#[test]
fn span_attributes_set_correctly() {
    let span = TelemetrySpan::new("db.query")
        .with_attribute("db.system", "postgres")
        .with_attribute("db.statement", "SELECT 1")
        .with_attribute("db.name", "mydb");

    assert_eq!(span.attributes.len(), 3);
    assert_eq!(span.attributes["db.system"], "postgres");
    assert_eq!(span.attributes["db.statement"], "SELECT 1");
    assert_eq!(span.attributes["db.name"], "mydb");
}

#[test]
fn span_attribute_overwrite() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key", "first")
        .with_attribute("key", "second");

    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "second");
}

#[test]
fn span_timing_attributes() {
    let start = std::time::Instant::now();
    let span = TelemetrySpan::new("timed.op").with_attribute("start_ms", "0");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let elapsed = start.elapsed().as_millis();

    // Reconstruct with end time — attributes carry timing metadata.
    let span = span.with_attribute("end_ms", elapsed.to_string());
    let end: u128 = span.attributes["end_ms"].parse().unwrap();
    assert!(end >= 5);
}

// =========================================================================
//  2. Metrics — counter and gauge via MetricsCollector + RunSummary
// =========================================================================

#[test]
fn counter_increment_via_record_event() {
    let mut summary = RunSummary::new();
    summary.record_event("tool_call");
    summary.record_event("tool_call");
    summary.record_event("tool_call");
    assert_eq!(summary.tool_call_count, 3);
    assert_eq!(*summary.event_counts.get("tool_call").unwrap(), 3);
}

#[test]
fn gauge_value_via_collector_len() {
    let collector = MetricsCollector::new();
    assert_eq!(collector.len(), 0);
    collector.record(sample_run("a", 10, 0));
    assert_eq!(collector.len(), 1);
    collector.record(sample_run("b", 20, 0));
    assert_eq!(collector.len(), 2);
    collector.clear();
    assert_eq!(collector.len(), 0);
}

#[test]
fn metric_dimensions_via_backend_counts() {
    let collector = MetricsCollector::new();
    collector.record(sample_run("openai", 100, 0));
    collector.record(sample_run("anthropic", 200, 0));
    collector.record(sample_run("openai", 150, 0));

    let summary = collector.summary();
    assert_eq!(summary.backend_counts["openai"], 2);
    assert_eq!(summary.backend_counts["anthropic"], 1);
}

#[test]
fn multiple_metrics_recorded_consistently() {
    let collector = MetricsCollector::new();
    for i in 0..10 {
        collector.record(RunMetrics {
            backend_name: format!("backend_{}", i % 3),
            dialect: "test".into(),
            duration_ms: (i + 1) * 100,
            events_count: i + 1,
            tokens_in: (i + 1) * 10,
            tokens_out: (i + 1) * 20,
            tool_calls_count: i,
            errors_count: if i % 5 == 0 { 1 } else { 0 },
            emulations_applied: 0,
        });
    }

    let summary = collector.summary();
    assert_eq!(summary.count, 10);
    assert_eq!(
        summary.total_tokens_in,
        (1..=10).map(|i| i * 10u64).sum::<u64>()
    );
    assert_eq!(
        summary.total_tokens_out,
        (1..=10).map(|i| i * 20u64).sum::<u64>()
    );
}

#[test]
fn histogram_records_gauge_style_values() {
    let mut hist = LatencyHistogram::new();
    hist.record(10.0);
    hist.record(20.0);
    hist.record(30.0);

    assert_eq!(hist.count(), 3);
    assert_eq!(hist.min(), Some(10.0));
    assert_eq!(hist.max(), Some(30.0));
    assert!((hist.mean() - 20.0).abs() < f64::EPSILON);
}

// =========================================================================
//  3. Trace context — propagation via span attributes
// =========================================================================

#[test]
fn trace_context_created_correctly() {
    let span = TelemetrySpan::new("root")
        .with_attribute("trace_id", "trace-001")
        .with_attribute("span_id", "span-001");

    assert_eq!(span.attributes["trace_id"], "trace-001");
    assert_eq!(span.attributes["span_id"], "span-001");
}

#[test]
fn trace_context_propagation_chain() {
    // root → child → grandchild all share same trace_id
    let root = TelemetrySpan::new("root")
        .with_attribute("trace_id", "t-abc")
        .with_attribute("span_id", "s-1");

    let child = TelemetrySpan::new("child")
        .with_attribute("trace_id", root.attributes["trace_id"].clone())
        .with_attribute("parent_span_id", root.attributes["span_id"].clone())
        .with_attribute("span_id", "s-2");

    let grandchild = TelemetrySpan::new("grandchild")
        .with_attribute("trace_id", child.attributes["trace_id"].clone())
        .with_attribute("parent_span_id", child.attributes["span_id"].clone())
        .with_attribute("span_id", "s-3");

    // All share the same trace_id.
    assert_eq!(root.attributes["trace_id"], "t-abc");
    assert_eq!(child.attributes["trace_id"], "t-abc");
    assert_eq!(grandchild.attributes["trace_id"], "t-abc");

    // Parent chain is correct.
    assert_eq!(grandchild.attributes["parent_span_id"], "s-2");
    assert_eq!(child.attributes["parent_span_id"], "s-1");
}

#[test]
fn trace_context_serialization_json() {
    let span = TelemetrySpan::new("traced.op")
        .with_attribute("trace_id", "deadbeef")
        .with_attribute("span_id", "cafebabe");

    let json = serde_json::to_string(&span).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["name"], "traced.op");
    assert_eq!(parsed["attributes"]["trace_id"], "deadbeef");
    assert_eq!(parsed["attributes"]["span_id"], "cafebabe");
}

#[test]
fn trace_context_deterministic_attribute_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("z_key", "z")
        .with_attribute("a_key", "a")
        .with_attribute("m_key", "m");

    let json = serde_json::to_string(&span).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let m_pos = json.find("m_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(
        a_pos < m_pos && m_pos < z_pos,
        "BTreeMap keys must be sorted"
    );
}

// =========================================================================
//  4. Integration — with tracing crate
// =========================================================================

#[test]
fn tracing_span_emit_does_not_panic() {
    // TelemetrySpan::emit() calls tracing::info! — must not panic even
    // without a subscriber installed.
    let span = TelemetrySpan::new("emit.test").with_attribute("backend", "mock");
    span.emit();
}

#[test]
fn tracing_span_emit_with_empty_attributes() {
    let span = TelemetrySpan::new("bare");
    span.emit(); // no panic
}

#[test]
fn tracing_subscriber_captures_span_emit() {
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct CapturingSubscriber {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl tracing::Subscriber for CapturingSubscriber {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut visitor = FieldCapture(String::new());
            event.record(&mut visitor);
            self.events.lock().unwrap().push(visitor.0);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    struct FieldCapture(String);
    impl tracing::field::Visit for FieldCapture {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(&format!("{}={:?}", field.name(), value));
        }
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(&format!("{}={}", field.name(), value));
        }
    }

    let captured = Arc::new(Mutex::new(Vec::new()));
    let subscriber = CapturingSubscriber {
        events: captured.clone(),
    };

    tracing::subscriber::with_default(subscriber, || {
        let span = TelemetrySpan::new("captured.op").with_attribute("backend", "mock");
        span.emit();
    });

    let events = captured.lock().unwrap();
    assert!(
        !events.is_empty(),
        "should have captured at least one event"
    );
    let combined = events.join(" ");
    assert!(
        combined.contains("captured.op"),
        "event should contain span name"
    );
}

#[test]
fn tracing_info_level_used_by_emit() {
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct LevelCapture {
        levels: Arc<Mutex<Vec<tracing::Level>>>,
    }

    impl tracing::Subscriber for LevelCapture {
        fn enabled(&self, meta: &tracing::Metadata<'_>) -> bool {
            *meta.level() <= tracing::Level::INFO
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            self.levels.lock().unwrap().push(*event.metadata().level());
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    let levels = Arc::new(Mutex::new(Vec::new()));
    let subscriber = LevelCapture {
        levels: levels.clone(),
    };

    tracing::subscriber::with_default(subscriber, || {
        TelemetrySpan::new("level.check").emit();
    });

    let captured = levels.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0], tracing::Level::INFO);
}

#[test]
fn tracing_structured_fields_in_emit() {
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct FieldNames {
        names: Arc<Mutex<Vec<String>>>,
    }

    impl tracing::Subscriber for FieldNames {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut names = Vec::new();
            struct Visitor<'a>(&'a mut Vec<String>);
            impl tracing::field::Visit for Visitor<'_> {
                fn record_debug(&mut self, field: &tracing::field::Field, _: &dyn std::fmt::Debug) {
                    self.0.push(field.name().to_string());
                }
                fn record_str(&mut self, field: &tracing::field::Field, _: &str) {
                    self.0.push(field.name().to_string());
                }
            }
            event.record(&mut Visitor(&mut names));
            self.names.lock().unwrap().extend(names);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    let names = Arc::new(Mutex::new(Vec::new()));
    let subscriber = FieldNames {
        names: names.clone(),
    };

    tracing::subscriber::with_default(subscriber, || {
        TelemetrySpan::new("structured")
            .with_attribute("k", "v")
            .emit();
    });

    let captured = names.lock().unwrap();
    // emit() produces span_name, attributes, and message fields
    assert!(captured.contains(&"span_name".to_string()));
    assert!(captured.contains(&"attributes".to_string()));
    assert!(captured.contains(&"message".to_string()));
}

// =========================================================================
//  5. Serde — type roundtrips
// =========================================================================

#[test]
fn serde_run_metrics_roundtrip() {
    let m = RunMetrics {
        backend_name: "serde-test".into(),
        dialect: "openai".into(),
        duration_ms: 1234,
        events_count: 42,
        tokens_in: 500,
        tokens_out: 1000,
        tool_calls_count: 7,
        errors_count: 2,
        emulations_applied: 1,
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn serde_metrics_summary_roundtrip() {
    let s = MetricsSummary {
        count: 5,
        mean_duration_ms: 123.45,
        p50_duration_ms: 100.0,
        p99_duration_ms: 450.0,
        total_tokens_in: 5000,
        total_tokens_out: 10000,
        error_rate: 0.1,
        backend_counts: {
            let mut m = BTreeMap::new();
            m.insert("mock".into(), 3);
            m.insert("sidecar".into(), 2);
            m
        },
    };
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn serde_telemetry_span_roundtrip() {
    let span = TelemetrySpan::new("serde.span")
        .with_attribute("k1", "v1")
        .with_attribute("k2", "v2");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "serde.span");
    assert_eq!(span2.attributes.len(), 2);
    assert_eq!(span2.attributes["k1"], "v1");
}

#[test]
fn serde_run_summary_roundtrip() {
    let s = RunSummary::from_events(&["tool_call", "error", "tool_call", "warning"], 999);
    let json = serde_json::to_string(&s).unwrap();
    let s2: RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn serde_latency_histogram_roundtrip() {
    let mut h = LatencyHistogram::new();
    h.record(1.1);
    h.record(2.2);
    h.record(3.3);
    let json = serde_json::to_string(&h).unwrap();
    let h2: LatencyHistogram = serde_json::from_str(&json).unwrap();
    assert_eq!(h, h2);
    assert_eq!(h2.count(), 3);
}

#[test]
fn serde_model_pricing_roundtrip() {
    let p = ModelPricing {
        input_cost_per_token: 0.00003,
        output_cost_per_token: 0.00006,
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: ModelPricing = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn serde_export_format_roundtrip() {
    for fmt in [
        ExportFormat::Json,
        ExportFormat::Csv,
        ExportFormat::Structured,
    ] {
        let json = serde_json::to_string(&fmt).unwrap();
        let fmt2: ExportFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, fmt2);
    }
}

#[test]
fn serde_export_format_snake_case_names() {
    assert_eq!(
        serde_json::to_string(&ExportFormat::Json).unwrap(),
        "\"json\""
    );
    assert_eq!(
        serde_json::to_string(&ExportFormat::Csv).unwrap(),
        "\"csv\""
    );
    assert_eq!(
        serde_json::to_string(&ExportFormat::Structured).unwrap(),
        "\"structured\""
    );
}

#[test]
fn serde_cost_estimator_roundtrip() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let json = serde_json::to_string(&ce).unwrap();
    let ce2: CostEstimator = serde_json::from_str(&json).unwrap();
    assert_eq!(
        ce2.estimate("gpt-4", 1000, 500),
        ce.estimate("gpt-4", 1000, 500)
    );
}

#[test]
fn serde_span_json_schema_fields() {
    let span = TelemetrySpan::new("schema.check");
    let json = serde_json::to_string(&span).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert!(v.get("name").is_some(), "must have 'name' field");
    assert!(
        v.get("attributes").is_some(),
        "must have 'attributes' field"
    );
    // No extra fields beyond name and attributes.
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 2);
}
