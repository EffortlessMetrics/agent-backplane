// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for labeled metrics and RuntimeMetrics.

use abp_telemetry::labeled::{
    LabeledCounter, LabeledGauge, LabeledHistogram, Labels, RuntimeMetrics,
};

// ---------------------------------------------------------------------------
// Histogram accuracy with known distributions
// ---------------------------------------------------------------------------

#[test]
fn histogram_accuracy_uniform_1_to_100() {
    let h = LabeledHistogram::new();
    let l = Labels::new();
    for i in 1..=100 {
        h.record(&l, i as f64);
    }
    let stats = h.stats(&l).unwrap();
    assert_eq!(stats.count, 100);
    assert!((stats.sum - 5050.0).abs() < 0.001);
    assert_eq!(stats.min, Some(1.0));
    assert_eq!(stats.max, Some(100.0));
    assert!((stats.p50.unwrap() - 50.0).abs() < 2.0);
    assert!((stats.p90.unwrap() - 90.0).abs() < 2.0);
    assert!((stats.p99.unwrap() - 99.0).abs() < 2.0);
}

#[test]
fn histogram_accuracy_single_value() {
    let h = LabeledHistogram::new();
    let l = Labels::new();
    h.record(&l, 42.0);
    let stats = h.stats(&l).unwrap();
    assert_eq!(stats.count, 1);
    assert!((stats.sum - 42.0).abs() < 0.001);
    assert_eq!(stats.p50, Some(42.0));
    assert_eq!(stats.p90, Some(42.0));
    assert_eq!(stats.p99, Some(42.0));
}

#[test]
fn histogram_accuracy_bimodal() {
    let h = LabeledHistogram::new();
    let l = Labels::new();
    // 50 values at 10.0, 50 values at 100.0
    for _ in 0..50 {
        h.record(&l, 10.0);
    }
    for _ in 0..50 {
        h.record(&l, 100.0);
    }
    let stats = h.stats(&l).unwrap();
    assert_eq!(stats.count, 100);
    assert!((stats.sum - 5500.0).abs() < 0.001);
    assert_eq!(stats.min, Some(10.0));
    assert_eq!(stats.max, Some(100.0));
}

// ---------------------------------------------------------------------------
// Counter monotonicity
// ---------------------------------------------------------------------------

#[test]
fn counter_monotonicity_single_thread() {
    let c = LabeledCounter::new();
    let l = Labels::new().with("backend", "mock");
    let mut prev = 0u64;
    for _ in 0..500 {
        c.increment(&l);
        let curr = c.get(&l);
        assert!(
            curr > prev,
            "counter must strictly increase: prev={} curr={}",
            prev,
            curr
        );
        prev = curr;
    }
    assert_eq!(c.get(&l), 500);
}

#[test]
fn counter_monotonicity_multithreaded() {
    let c = LabeledCounter::new();
    let l = Labels::new().with("backend", "mock");
    let handles: Vec<_> = (0..8)
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
    assert_eq!(c.get(&l), 8000);
}

// ---------------------------------------------------------------------------
// Gauge up/down correctness
// ---------------------------------------------------------------------------

#[test]
fn gauge_up_down_basic() {
    let g = LabeledGauge::new();
    let l = Labels::new().with("backend", "mock");
    for _ in 0..10 {
        g.increment(&l);
    }
    assert_eq!(g.get(&l), 10);
    for _ in 0..7 {
        g.decrement(&l);
    }
    assert_eq!(g.get(&l), 3);
}

#[test]
fn gauge_negative_values() {
    let g = LabeledGauge::new();
    let l = Labels::new().with("backend", "mock");
    g.decrement(&l);
    g.decrement(&l);
    assert_eq!(g.get(&l), -2);
    g.increment(&l);
    assert_eq!(g.get(&l), -1);
}

#[test]
fn gauge_set_overrides() {
    let g = LabeledGauge::new();
    let l = Labels::new().with("backend", "mock");
    g.increment(&l);
    g.increment(&l);
    g.set(&l, 42);
    assert_eq!(g.get(&l), 42);
}

#[test]
fn gauge_concurrent_up_down_nets_zero() {
    let g = LabeledGauge::new();
    let l = Labels::new().with("backend", "mock");
    let handles: Vec<_> = (0..8)
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

// ---------------------------------------------------------------------------
// Label cardinality
// ---------------------------------------------------------------------------

#[test]
fn label_cardinality_counter() {
    let c = LabeledCounter::new();
    for i in 0..100 {
        let l = Labels::new().with("error_code", format!("E{:03}", i));
        c.increment(&l);
    }
    assert_eq!(c.cardinality(), 100);
    assert_eq!(c.total(), 100);
}

#[test]
fn label_cardinality_gauge() {
    let g = LabeledGauge::new();
    for i in 0..25 {
        let l = Labels::new()
            .with("backend", format!("b{}", i))
            .with("dialect", "openai");
        g.set(&l, i as i64);
    }
    assert_eq!(g.cardinality(), 25);
}

#[test]
fn label_cardinality_histogram() {
    let h = LabeledHistogram::new();
    for i in 0..10 {
        let l = Labels::new().with("execution_mode", format!("mode_{}", i));
        h.record(&l, i as f64);
    }
    assert_eq!(h.cardinality(), 10);
    assert_eq!(h.total_count(), 10);
}

// ---------------------------------------------------------------------------
// Prometheus format correctness
// ---------------------------------------------------------------------------

#[test]
fn prometheus_text_counters_have_type_lines() {
    let m = RuntimeMetrics::new();
    m.record_work_order("mock", "openai", "mapped", 100.0, 5, None);
    let text = m.to_prometheus_text();
    assert!(text.contains("# TYPE abp_work_orders_total counter"));
    assert!(text.contains("# TYPE abp_events_total counter"));
}

#[test]
fn prometheus_text_gauges_have_type_lines() {
    let m = RuntimeMetrics::new();
    m.run_started("mock");
    m.work_order_enqueued("mock");
    let text = m.to_prometheus_text();
    assert!(text.contains("# TYPE abp_active_runs gauge"));
    assert!(text.contains("# TYPE abp_pending_work_orders gauge"));
}

#[test]
fn prometheus_text_histograms_have_summary_type() {
    let m = RuntimeMetrics::new();
    m.record_work_order("mock", "openai", "mapped", 100.0, 5, None);
    let text = m.to_prometheus_text();
    assert!(text.contains("# TYPE abp_response_latency_ms summary"));
    assert!(text.contains("abp_response_latency_ms_count"));
    assert!(text.contains("abp_response_latency_ms_sum"));
}

#[test]
fn prometheus_text_labels_in_output() {
    let m = RuntimeMetrics::new();
    m.record_work_order("mock", "openai", "mapped", 100.0, 5, Some("timeout"));
    let text = m.to_prometheus_text();
    assert!(text.contains("backend=\"mock\""));
    assert!(text.contains("dialect=\"openai\""));
    assert!(text.contains("execution_mode=\"mapped\""));
    assert!(text.contains("error_code=\"timeout\""));
}

#[test]
fn prometheus_text_error_counter_with_code() {
    let m = RuntimeMetrics::new();
    m.record_work_order("mock", "openai", "mapped", 100.0, 5, Some("rate_limit"));
    m.record_work_order("mock", "openai", "mapped", 200.0, 3, Some("timeout"));
    let text = m.to_prometheus_text();
    assert!(text.contains("# TYPE abp_errors_total counter"));
    assert!(text.contains("error_code=\"rate_limit\""));
    assert!(text.contains("error_code=\"timeout\""));
}

#[test]
fn prometheus_text_is_parseable() {
    let m = RuntimeMetrics::new();
    m.record_work_order("mock", "openai", "mapped", 100.0, 5, None);
    m.record_work_order(
        "sidecar",
        "anthropic",
        "passthrough",
        200.0,
        10,
        Some("timeout"),
    );
    m.run_started("mock");
    m.work_order_enqueued("sidecar");

    let text = m.to_prometheus_text();

    // Every non-empty, non-comment line must have a metric name and value
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Should contain at least one space separating name from value
        assert!(
            line.contains(' '),
            "metric line must have name and value: {}",
            line
        );
        // The value part should be parseable as a number
        let parts: Vec<&str> = line.rsplitn(2, ' ').collect();
        assert!(
            parts[0].parse::<f64>().is_ok(),
            "value should be a number: {} in line: {}",
            parts[0],
            line
        );
    }
}

// ---------------------------------------------------------------------------
// Concurrent access safety
// ---------------------------------------------------------------------------

#[test]
fn runtime_metrics_concurrent_recording() {
    let m = RuntimeMetrics::new();
    let m = std::sync::Arc::new(m);
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let m = m.clone();
            std::thread::spawn(move || {
                let backend = format!("backend_{}", i % 2);
                for j in 0..100 {
                    m.run_started(&backend);
                    m.record_work_order(
                        &backend,
                        "openai",
                        "mapped",
                        (j as f64) * 10.0,
                        5,
                        if j % 10 == 0 { Some("err") } else { None },
                    );
                    m.run_finished(&backend);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    // 4 threads × 100 iterations = 400 work orders total
    assert_eq!(m.work_orders_total.total(), 400);
    // Events: 4 threads × 100 × 5 = 2000
    assert_eq!(m.events_total.total(), 2000);
    // Errors: 4 threads × 10 (every 10th) = 40
    assert_eq!(m.errors_total.total(), 40);
    // All runs finished, gauge should net to 0 for each backend
    let b0 = Labels::new().with("backend", "backend_0");
    let b1 = Labels::new().with("backend", "backend_1");
    assert_eq!(m.active_runs.get(&b0), 0);
    assert_eq!(m.active_runs.get(&b1), 0);
}

#[test]
fn concurrent_histogram_record_and_read() {
    let h = LabeledHistogram::new();
    let l = Labels::new().with("backend", "mock");

    let writers: Vec<_> = (0..4)
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

    // Also read concurrently
    let readers: Vec<_> = (0..2)
        .map(|_| {
            let h = h.clone();
            let l = l.clone();
            std::thread::spawn(move || {
                for _ in 0..50 {
                    let _ = h.count(&l);
                    let _ = h.stats(&l);
                }
            })
        })
        .collect();

    for h in writers {
        h.join().unwrap();
    }
    for h in readers {
        h.join().unwrap();
    }

    assert_eq!(h.count(&l), 1000);
}

// ---------------------------------------------------------------------------
// RuntimeMetrics end-to-end simulation
// ---------------------------------------------------------------------------

#[test]
fn runtime_integration_full_lifecycle() {
    let metrics = RuntimeMetrics::new();

    // Simulate 3 work orders across 2 backends
    // WO 1: mock/openai/mapped - success
    metrics.work_order_enqueued("mock");
    metrics.work_order_dequeued("mock");
    metrics.run_started("mock");
    metrics.record_work_order("mock", "openai", "mapped", 150.0, 8, None);
    metrics.run_finished("mock");

    // WO 2: sidecar/anthropic/passthrough - error
    metrics.work_order_enqueued("sidecar");
    metrics.work_order_dequeued("sidecar");
    metrics.run_started("sidecar");
    metrics.record_work_order(
        "sidecar",
        "anthropic",
        "passthrough",
        500.0,
        3,
        Some("timeout"),
    );
    metrics.run_finished("sidecar");

    // WO 3: mock/openai/mapped - success
    metrics.work_order_enqueued("mock");
    metrics.work_order_dequeued("mock");
    metrics.run_started("mock");
    metrics.record_work_order("mock", "openai", "mapped", 200.0, 12, None);
    metrics.run_finished("mock");

    // Verify counters
    assert_eq!(metrics.work_orders_total.total(), 3);
    assert_eq!(metrics.errors_total.total(), 1);
    assert_eq!(metrics.events_total.total(), 23); // 8 + 3 + 12

    // Verify gauges are back to zero
    let mock_labels = Labels::new().with("backend", "mock");
    let sidecar_labels = Labels::new().with("backend", "sidecar");
    assert_eq!(metrics.active_runs.get(&mock_labels), 0);
    assert_eq!(metrics.active_runs.get(&sidecar_labels), 0);
    assert_eq!(metrics.pending_work_orders.get(&mock_labels), 0);
    assert_eq!(metrics.pending_work_orders.get(&sidecar_labels), 0);

    // Verify histograms
    let mock_openai = Labels::new()
        .with("backend", "mock")
        .with("dialect", "openai")
        .with("execution_mode", "mapped");
    let latency_stats = metrics.response_latency.stats(&mock_openai).unwrap();
    assert_eq!(latency_stats.count, 2);
    assert!((latency_stats.sum - 350.0).abs() < 0.001); // 150 + 200

    // Verify Prometheus output is non-empty and well-formed
    let prom = metrics.to_prometheus_text();
    assert!(!prom.is_empty());
    assert!(prom.contains("abp_work_orders_total"));
    assert!(prom.contains("abp_errors_total"));
    assert!(prom.contains("abp_active_runs"));
    assert!(prom.contains("abp_response_latency_ms"));
}
