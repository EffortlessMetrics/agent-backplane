#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for telemetry and metrics system.

use std::collections::BTreeMap;
use std::thread;

use chrono::{Duration, Utc};
use serde_json::json;

use abp_core::{
    AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, UsageNormalized,
    aggregate::{EventAggregator, RunAnalytics},
    chain::ReceiptChain,
    ext::ReceiptExt,
};
use abp_telemetry::{
    JsonExporter, MetricsCollector, MetricsSummary, RunMetrics, TelemetryExporter, TelemetrySpan,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_run(
    backend: &str,
    duration: u64,
    tokens_in: u64,
    tokens_out: u64,
    errors: u64,
) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "test".to_string(),
        duration_ms: duration,
        events_count: 5,
        tokens_in,
        tokens_out,
        tool_calls_count: 2,
        errors_count: errors,
        emulations_applied: 0,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn usage(input: u64, output: u64) -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

fn usage_with_cache(input: u64, output: u64, cache_read: u64, cache_write: u64) -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cache_read_tokens: Some(cache_read),
        cache_write_tokens: Some(cache_write),
        request_units: None,
        estimated_cost_usd: None,
    }
}

fn receipt_with_usage(backend: &str, u: UsageNormalized) -> Receipt {
    ReceiptBuilder::new(backend)
        .usage(u)
        .outcome(Outcome::Complete)
        .build()
}

fn receipt_with_timing(start_offset_ms: i64, end_offset_ms: i64) -> Receipt {
    let base = Utc::now();
    ReceiptBuilder::new("timing-test")
        .started_at(base + Duration::milliseconds(start_offset_ms))
        .finished_at(base + Duration::milliseconds(end_offset_ms))
        .outcome(Outcome::Complete)
        .build()
}

// =========================================================================
// 1. Metrics Collection (15 tests)
// =========================================================================

#[test]
fn mc01_token_counting_input() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 500, 200, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 500);
}

#[test]
fn mc02_token_counting_output() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 500, 300, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_out, 300);
}

#[test]
fn mc03_token_counting_total_across_runs() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 10, 100, 50, 0));
    c.record(make_run("b", 20, 200, 100, 0));
    c.record(make_run("c", 30, 300, 150, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 600);
    assert_eq!(s.total_tokens_out, 300);
}

#[test]
fn mc04_token_counting_with_cache_in_usage_normalized() {
    let u = usage_with_cache(1000, 500, 200, 100);
    assert_eq!(u.cache_read_tokens, Some(200));
    assert_eq!(u.cache_write_tokens, Some(100));
    assert_eq!(u.input_tokens, Some(1000));
}

#[test]
fn mc05_latency_tracking_total_duration() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 250, 0, 0, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 250.0).abs() < f64::EPSILON);
}

#[test]
fn mc06_latency_tracking_mean_across_runs() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 0, 0, 0));
    c.record(make_run("a", 300, 0, 0, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

#[test]
fn mc07_latency_per_event_via_aggregator() {
    let mut agg = EventAggregator::new();
    let base = Utc::now();
    for i in 0..5 {
        agg.add(&AgentEvent {
            ts: base + Duration::milliseconds(i * 100),
            kind: AgentEventKind::AssistantDelta { text: "x".into() },
            ext: None,
        });
    }
    let dur = agg.duration_ms().unwrap();
    assert_eq!(dur, 400);
}

#[test]
fn mc08_event_counting_by_type() {
    let mut agg = EventAggregator::new();
    agg.add(&make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    agg.add(&make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    }));
    agg.add(&make_event(AgentEventKind::AssistantMessage {
        text: "bye".into(),
    }));
    agg.add(&make_event(AgentEventKind::ToolCall {
        tool_name: "edit".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    }));
    let counts = agg.count_by_kind();
    assert_eq!(counts["run_started"], 1);
    assert_eq!(counts["assistant_message"], 2);
    assert_eq!(counts["tool_call"], 1);
}

#[test]
fn mc09_backend_specific_metrics_single() {
    let c = MetricsCollector::new();
    c.record(make_run("openai", 100, 100, 50, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["openai"], 1);
}

#[test]
fn mc10_backend_specific_metrics_multiple() {
    let c = MetricsCollector::new();
    c.record(make_run("openai", 100, 100, 50, 0));
    c.record(make_run("anthropic", 200, 200, 100, 0));
    c.record(make_run("openai", 150, 150, 75, 1));
    let s = c.summary();
    assert_eq!(s.backend_counts["openai"], 2);
    assert_eq!(s.backend_counts["anthropic"], 1);
    assert_eq!(s.backend_counts.len(), 2);
}

#[test]
fn mc11_error_rate_zero() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 0, 0, 0));
    c.record(make_run("a", 200, 0, 0, 0));
    let s = c.summary();
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn mc12_error_rate_all_errors() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 0, 0, 3));
    c.record(make_run("a", 200, 0, 0, 5));
    let s = c.summary();
    // error_rate = sum(errors_count) / count = (3+5)/2 = 4.0
    assert!((s.error_rate - 4.0).abs() < f64::EPSILON);
}

#[test]
fn mc13_error_rate_partial() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 0, 0, 1));
    c.record(make_run("a", 200, 0, 0, 0));
    c.record(make_run("a", 300, 0, 0, 0));
    let s = c.summary();
    // 1 error / 3 runs ≈ 0.333…
    assert!((s.error_rate - 1.0 / 3.0).abs() < 1e-10);
}

#[test]
fn mc14_concurrent_token_accumulation() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..20 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(make_run("t", 10, (i + 1) * 10, (i + 1) * 5, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let s = c.summary();
    assert_eq!(s.count, 20);
    // sum of 10+20+...+200 = 10*(1+2+...+20) = 10*210 = 2100
    assert_eq!(s.total_tokens_in, 2100);
    // sum of 5+10+...+100 = 5*(1+2+...+20) = 5*210 = 1050
    assert_eq!(s.total_tokens_out, 1050);
}

#[test]
fn mc15_event_counting_with_runtime_metrics() {
    let rt = abp_runtime::telemetry::RunMetrics::new();
    rt.record_run(100, true, 10);
    rt.record_run(200, false, 5);
    let snap = rt.snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.total_events, 15);
    // average = (100+200)/2 = 150
    assert_eq!(snap.average_run_duration_ms, 150);
}

// =========================================================================
// 2. Usage Aggregation (10 tests)
// =========================================================================

#[test]
fn ua01_merge_usage_across_runs() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 10, 100, 50, 0));
    c.record(make_run("a", 20, 200, 100, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 300);
    assert_eq!(s.total_tokens_out, 150);
}

#[test]
fn ua02_empty_usage_handling() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn ua03_empty_collector_usage() {
    let c = MetricsCollector::new();
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 0);
    assert_eq!(s.total_tokens_out, 0);
}

#[test]
fn ua04_large_token_counts() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 10, u64::MAX / 2, u64::MAX / 2, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, u64::MAX / 2);
    assert_eq!(s.total_tokens_out, u64::MAX / 2);
}

#[test]
fn ua05_large_token_counts_sum() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 10, 1_000_000_000, 500_000_000, 0));
    c.record(make_run("b", 20, 2_000_000_000, 1_000_000_000, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 3_000_000_000);
    assert_eq!(s.total_tokens_out, 1_500_000_000);
}

#[test]
fn ua06_cache_hit_miss_ratio() {
    let u = usage_with_cache(1000, 500, 800, 200);
    let total_input = u.input_tokens.unwrap();
    let cache_read = u.cache_read_tokens.unwrap();
    let ratio = cache_read as f64 / total_input as f64;
    assert!((ratio - 0.8).abs() < f64::EPSILON);
}

#[test]
fn ua07_cache_miss_ratio() {
    let u = usage_with_cache(1000, 500, 0, 1000);
    let cache_read = u.cache_read_tokens.unwrap();
    let cache_write = u.cache_write_tokens.unwrap();
    assert_eq!(cache_read, 0);
    assert_eq!(cache_write, 1000);
}

#[test]
fn ua08_usage_serde_roundtrip() {
    let u = usage_with_cache(1234, 5678, 100, 200);
    let json = serde_json::to_string(&u).unwrap();
    let u2: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(u.input_tokens, u2.input_tokens);
    assert_eq!(u.output_tokens, u2.output_tokens);
    assert_eq!(u.cache_read_tokens, u2.cache_read_tokens);
    assert_eq!(u.cache_write_tokens, u2.cache_write_tokens);
}

#[test]
fn ua09_usage_with_request_units() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: Some(42),
        estimated_cost_usd: None,
    };
    assert_eq!(u.request_units, Some(42));
}

#[test]
fn ua10_usage_with_estimated_cost() {
    let u = UsageNormalized {
        input_tokens: Some(10000),
        output_tokens: Some(5000),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.0125),
    };
    assert!((u.estimated_cost_usd.unwrap() - 0.0125).abs() < f64::EPSILON);
}

// =========================================================================
// 3. Timing Metadata (10 tests)
// =========================================================================

#[test]
fn tm01_timestamp_ordering_in_events() {
    let base = Utc::now();
    let events: Vec<AgentEvent> = (0..5)
        .map(|i| AgentEvent {
            ts: base + Duration::milliseconds(i * 100),
            kind: AgentEventKind::AssistantDelta {
                text: format!("t{i}"),
            },
            ext: None,
        })
        .collect();
    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }
}

#[test]
fn tm02_duration_calculation_in_receipt() {
    let r = receipt_with_timing(0, 500);
    assert_eq!(r.meta.duration_ms, 500);
}

#[test]
fn tm03_duration_zero_when_same_start_end() {
    let r = receipt_with_timing(0, 0);
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn tm04_first_timestamp_extraction_via_aggregator() {
    let base = Utc::now();
    let mut agg = EventAggregator::new();
    agg.add(&AgentEvent {
        ts: base,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    });
    agg.add(&AgentEvent {
        ts: base + Duration::milliseconds(500),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: None,
    });
    let first = agg.first_timestamp().unwrap();
    assert!(first.contains(&base.format("%Y-%m-%d").to_string()));
}

#[test]
fn tm05_last_timestamp_extraction_via_aggregator() {
    let base = Utc::now();
    let mut agg = EventAggregator::new();
    agg.add(&AgentEvent {
        ts: base,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    });
    let later = base + Duration::seconds(10);
    agg.add(&AgentEvent {
        ts: later,
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    });
    let last = agg.last_timestamp().unwrap();
    assert!(last.contains(&later.format("%Y-%m-%d").to_string()));
}

#[test]
fn tm06_duration_ms_from_aggregator() {
    let base = Utc::now();
    let mut agg = EventAggregator::new();
    agg.add(&AgentEvent {
        ts: base,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    });
    agg.add(&AgentEvent {
        ts: base + Duration::milliseconds(750),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    });
    assert_eq!(agg.duration_ms(), Some(750));
}

#[test]
fn tm07_no_duration_for_single_event() {
    let mut agg = EventAggregator::new();
    agg.add(&make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    assert_eq!(agg.duration_ms(), None);
}

#[test]
fn tm08_event_timeline_reconstruction() {
    let base = Utc::now();
    let events = vec![
        AgentEvent {
            ts: base,
            kind: AgentEventKind::RunStarted {
                message: "s".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: base + Duration::milliseconds(100),
            kind: AgentEventKind::AssistantDelta { text: "a".into() },
            ext: None,
        },
        AgentEvent {
            ts: base + Duration::milliseconds(300),
            kind: AgentEventKind::ToolCall {
                tool_name: "edit".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            ext: None,
        },
        AgentEvent {
            ts: base + Duration::milliseconds(500),
            kind: AgentEventKind::RunCompleted {
                message: "d".into(),
            },
            ext: None,
        },
    ];
    let analytics = RunAnalytics::from_events(&events);
    let summary = analytics.summary();
    assert_eq!(summary.total_events, 4);
    assert_eq!(summary.duration_ms, Some(500));
    assert_eq!(summary.tool_calls, 1);
}

#[test]
fn tm09_receipt_started_before_finished() {
    let r = receipt_with_timing(0, 1000);
    assert!(r.meta.started_at <= r.meta.finished_at);
}

#[test]
fn tm10_receipt_duration_secs() {
    let r = receipt_with_timing(0, 2500);
    assert!((r.duration_secs() - 2.5).abs() < f64::EPSILON);
}

// =========================================================================
// 4. Receipt Metrics Integration (15 tests)
// =========================================================================

#[test]
fn ri01_receipt_captures_usage() {
    let u = usage(500, 200);
    let r = receipt_with_usage("test", u);
    assert_eq!(r.usage.input_tokens, Some(500));
    assert_eq!(r.usage.output_tokens, Some(200));
}

#[test]
fn ri02_receipt_captures_cache_usage() {
    let u = usage_with_cache(1000, 500, 300, 100);
    let r = receipt_with_usage("test", u);
    assert_eq!(r.usage.cache_read_tokens, Some(300));
    assert_eq!(r.usage.cache_write_tokens, Some(100));
}

#[test]
fn ri03_receipt_captures_timing() {
    let r = receipt_with_timing(0, 3000);
    assert_eq!(r.meta.duration_ms, 3000);
    assert!(r.meta.started_at < r.meta.finished_at);
}

#[test]
fn ri04_receipt_chain_accumulates_token_metrics() {
    let mut chain = ReceiptChain::new();
    for i in 1..=3 {
        let r = ReceiptBuilder::new(format!("b{i}"))
            .usage(usage(100 * i as u64, 50 * i as u64))
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 3);
    let total_input: u64 = chain.iter().filter_map(|r| r.usage.input_tokens).sum();
    let total_output: u64 = chain.iter().filter_map(|r| r.usage.output_tokens).sum();
    assert_eq!(total_input, 600); // 100+200+300
    assert_eq!(total_output, 300); // 50+100+150
}

#[test]
fn ri05_receipt_chain_accumulates_duration() {
    let mut chain = ReceiptChain::new();
    for ms in [100, 200, 300] {
        let r = ReceiptBuilder::new("test")
            .started_at(Utc::now())
            .finished_at(Utc::now() + Duration::milliseconds(ms))
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    let (min_dur, max_dur) = chain.duration_range().unwrap();
    assert!(min_dur.as_millis() <= max_dur.as_millis());
}

#[test]
fn ri06_receipt_usage_survives_json_roundtrip() {
    let u = usage_with_cache(999, 888, 777, 666);
    let r = receipt_with_usage("serde", u);
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.usage.input_tokens, Some(999));
    assert_eq!(r2.usage.output_tokens, Some(888));
    assert_eq!(r2.usage.cache_read_tokens, Some(777));
    assert_eq!(r2.usage.cache_write_tokens, Some(666));
}

#[test]
fn ri07_receipt_timing_survives_json_roundtrip() {
    let r = receipt_with_timing(0, 4200);
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.meta.duration_ms, 4200);
    assert_eq!(r.meta.started_at, r2.meta.started_at);
    assert_eq!(r.meta.finished_at, r2.meta.finished_at);
}

#[test]
fn ri08_receipt_hash_preserves_usage() {
    let u = usage(100, 50);
    let r = ReceiptBuilder::new("hash-test")
        .usage(u)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(50));
}

#[test]
fn ri09_receipt_chain_success_rate_with_usage() {
    let mut chain = ReceiptChain::new();
    for outcome in [Outcome::Complete, Outcome::Complete, Outcome::Failed] {
        let r = ReceiptBuilder::new("test")
            .usage(usage(100, 50))
            .outcome(outcome)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    let rate = chain.success_rate();
    assert!((rate - 2.0 / 3.0).abs() < 1e-10);
}

#[test]
fn ri10_receipt_ext_fields_for_usage() {
    let mut ext = BTreeMap::new();
    ext.insert("custom_metric".to_string(), json!(42));
    ext.insert("billing_tier".to_string(), json!("premium"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: Some(ext),
    };
    let r = ReceiptBuilder::new("ext-test")
        .add_trace_event(event)
        .outcome(Outcome::Complete)
        .build();
    let ext = r.trace[0].ext.as_ref().unwrap();
    assert_eq!(ext["custom_metric"], json!(42));
    assert_eq!(ext["billing_tier"], json!("premium"));
}

#[test]
fn ri11_receipt_usage_raw_json() {
    let raw = json!({
        "prompt_tokens": 100,
        "completion_tokens": 50,
        "total_tokens": 150,
        "vendor_specific_field": true
    });
    let r = ReceiptBuilder::new("raw-test")
        .usage_raw(raw.clone())
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.usage_raw, raw);
}

#[test]
fn ri12_receipt_trace_event_count() {
    let r = ReceiptBuilder::new("trace-test")
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hi".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "d".into(),
        }))
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.trace.len(), 3);
    let counts = r.event_count_by_kind();
    assert_eq!(counts["run_started"], 1);
    assert_eq!(counts["assistant_message"], 1);
    assert_eq!(counts["run_completed"], 1);
}

#[test]
fn ri13_receipt_chain_total_events() {
    let mut chain = ReceiptChain::new();
    for n in [2, 3, 5] {
        let mut builder = ReceiptBuilder::new("ev-test").outcome(Outcome::Complete);
        for _ in 0..n {
            builder = builder.add_trace_event(make_event(AgentEventKind::AssistantDelta {
                text: "x".into(),
            }));
        }
        chain.push(builder.with_hash().unwrap()).unwrap();
    }
    assert_eq!(chain.total_events(), 10);
}

#[test]
fn ri14_receipt_ext_survives_roundtrip() {
    let mut ext = BTreeMap::new();
    ext.insert("tokens_cached".to_string(), json!(500));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "test".into(),
        },
        ext: Some(ext),
    };
    let r = ReceiptBuilder::new("ext-rt")
        .add_trace_event(event)
        .outcome(Outcome::Complete)
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let ext2 = r2.trace[0].ext.as_ref().unwrap();
    assert_eq!(ext2["tokens_cached"], json!(500));
}

#[test]
fn ri15_receipt_default_usage_is_empty() {
    let r = ReceiptBuilder::new("default")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(r.usage.cache_read_tokens.is_none());
    assert!(r.usage.cache_write_tokens.is_none());
    assert!(r.usage.request_units.is_none());
    assert!(r.usage.estimated_cost_usd.is_none());
}

// =========================================================================
// 5. Additional coverage: TelemetrySpan, JsonExporter, Runtime metrics
// =========================================================================

#[test]
fn extra01_telemetry_span_empty_attributes() {
    let span = TelemetrySpan::new("empty");
    assert_eq!(span.name, "empty");
    assert!(span.attributes.is_empty());
}

#[test]
fn extra02_telemetry_span_overwrite_attribute() {
    let span = TelemetrySpan::new("test")
        .with_attribute("key", "v1")
        .with_attribute("key", "v2");
    assert_eq!(span.attributes["key"], "v2");
    assert_eq!(span.attributes.len(), 1);
}

#[test]
fn extra03_json_exporter_roundtrip() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 500, 200, 1));
    c.record(make_run("b", 200, 300, 100, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json_str = exporter.export(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json_str).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn extra04_metrics_collector_clear_and_reuse() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 100, 50, 0));
    assert_eq!(c.len(), 1);
    c.clear();
    assert!(c.is_empty());
    c.record(make_run("b", 200, 200, 100, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.backend_counts["b"], 1);
    assert!(!s.backend_counts.contains_key("a"));
}

#[test]
fn extra05_percentile_p50_two_elements() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 100, 0, 0, 0));
    c.record(make_run("a", 200, 0, 0, 0));
    let s = c.summary();
    assert!((s.p50_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn extra06_percentile_p99_single_element() {
    let c = MetricsCollector::new();
    c.record(make_run("a", 42, 0, 0, 0));
    let s = c.summary();
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn extra07_runtime_metrics_average_duration() {
    let rt = abp_runtime::telemetry::RunMetrics::new();
    rt.record_run(100, true, 5);
    rt.record_run(200, true, 10);
    rt.record_run(300, true, 15);
    let snap = rt.snapshot();
    assert_eq!(snap.average_run_duration_ms, 200);
    assert_eq!(snap.total_events, 30);
}

#[test]
fn extra08_run_analytics_tool_ratio() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "edit".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "grep".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "done".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "d".into(),
        }),
    ];
    let analytics = RunAnalytics::from_events(&events);
    // 2 tool calls / 5 total events = 0.4
    assert!((analytics.tool_usage_ratio() - 0.4).abs() < f64::EPSILON);
}

#[test]
fn extra09_run_analytics_empty() {
    let analytics = RunAnalytics::from_events(&[]);
    assert!(analytics.is_successful());
    assert!((analytics.tool_usage_ratio() - 0.0).abs() < f64::EPSILON);
    assert!((analytics.average_text_per_event() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn extra10_run_analytics_with_errors() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        }),
    ];
    let analytics = RunAnalytics::from_events(&events);
    assert!(!analytics.is_successful());
}

#[test]
fn extra11_receipt_has_errors_trait() {
    let r = ReceiptBuilder::new("err")
        .add_trace_event(make_event(AgentEventKind::Error {
            message: "fail".into(),
            error_code: None,
        }))
        .outcome(Outcome::Failed)
        .build();
    assert!(r.has_errors());
    assert!(r.is_failure());
    assert!(!r.is_success());
}

#[test]
fn extra12_metrics_summary_default() {
    let s = MetricsSummary::default();
    assert_eq!(s.count, 0);
    assert!((s.mean_duration_ms - 0.0).abs() < f64::EPSILON);
    assert!(s.backend_counts.is_empty());
}

#[test]
fn extra13_run_metrics_default() {
    let m = RunMetrics::default();
    assert_eq!(m.backend_name, "");
    assert_eq!(m.duration_ms, 0);
    assert_eq!(m.tokens_in, 0);
    assert_eq!(m.tokens_out, 0);
    assert_eq!(m.events_count, 0);
    assert_eq!(m.tool_calls_count, 0);
    assert_eq!(m.errors_count, 0);
    assert_eq!(m.emulations_applied, 0);
}

#[test]
fn extra14_aggregator_unique_tools() {
    let mut agg = EventAggregator::new();
    for name in ["edit", "grep", "edit", "bash", "grep"] {
        agg.add(&make_event(AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }));
    }
    assert_eq!(agg.unique_tool_count(), 3);
    assert_eq!(agg.tool_calls().len(), 5);
}

#[test]
fn extra15_aggregator_text_length() {
    let mut agg = EventAggregator::new();
    agg.add(&make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    }));
    agg.add(&make_event(AgentEventKind::AssistantDelta {
        text: "world!".into(),
    }));
    assert_eq!(agg.text_length(), 11); // 5 + 6
}

#[test]
fn extra16_receipt_chain_find_by_backend() {
    let mut chain = ReceiptChain::new();
    for b in ["alpha", "beta", "alpha"] {
        chain
            .push(
                ReceiptBuilder::new(b)
                    .outcome(Outcome::Complete)
                    .with_hash()
                    .unwrap(),
            )
            .unwrap();
    }
    assert_eq!(chain.find_by_backend("alpha").len(), 2);
    assert_eq!(chain.find_by_backend("beta").len(), 1);
    assert_eq!(chain.find_by_backend("gamma").len(), 0);
}
