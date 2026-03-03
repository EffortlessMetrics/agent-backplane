// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive concurrency safety tests for Agent Backplane types.

use std::sync::Arc;
use std::thread;

use abp_core::{
    receipt_hash, AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::{Decision, PolicyEngine};
use abp_telemetry::{MetricsCollector, MetricsSummary, RunMetrics, TelemetrySpan};
use chrono::Utc;
use uuid::Uuid;

// =========================================================================
// Helpers
// =========================================================================

fn make_receipt(backend: &str) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: backend.into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn make_policy_engine() -> PolicyEngine {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    PolicyEngine::new(&policy).unwrap()
}

fn make_globs() -> IncludeExcludeGlobs {
    IncludeExcludeGlobs::new(
        &["src/**".into(), "tests/**".into()],
        &["src/generated/**".into()],
    )
    .unwrap()
}

fn sample_metrics(backend: &str, duration: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "test".to_string(),
        duration_ms: duration,
        events_count: 5,
        tokens_in: 100,
        tokens_out: 200,
        tool_calls_count: 3,
        errors_count: 0,
        emulations_applied: 0,
    }
}

fn openai_json() -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4",
        "choices": [{"message": {"role": "assistant", "content": "hello"}}],
        "messages": [{"role": "user", "content": "hi"}]
    })
}

fn claude_json() -> serde_json::Value {
    serde_json::json!({
        "type": "message",
        "model": "claude-3",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "stop_reason": "end_turn"
    })
}

fn gemini_json() -> serde_json::Value {
    serde_json::json!({
        "contents": [{"parts": [{"text": "hello"}]}],
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}]
    })
}

// =========================================================================
// Category 1: Send + Sync compile-time verification
// =========================================================================

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn work_order_is_send() {
    assert_send::<WorkOrder>();
}

#[test]
fn work_order_is_sync() {
    assert_sync::<WorkOrder>();
}

#[test]
fn receipt_is_send() {
    assert_send::<Receipt>();
}

#[test]
fn receipt_is_sync() {
    assert_sync::<Receipt>();
}

#[test]
fn policy_engine_is_send() {
    assert_send::<PolicyEngine>();
}

#[test]
fn policy_engine_is_sync() {
    assert_sync::<PolicyEngine>();
}

#[test]
fn include_exclude_globs_is_send() {
    assert_send::<IncludeExcludeGlobs>();
}

#[test]
fn include_exclude_globs_is_sync() {
    assert_sync::<IncludeExcludeGlobs>();
}

#[test]
fn dialect_is_send_sync() {
    assert_send_sync::<Dialect>();
}

#[test]
fn dialect_detector_is_send_sync() {
    assert_send_sync::<DialectDetector>();
}

#[test]
fn dialect_validator_is_send_sync() {
    assert_send_sync::<DialectValidator>();
}

#[test]
fn metrics_collector_is_send_sync() {
    assert_send_sync::<MetricsCollector>();
}

#[test]
fn run_metrics_is_send_sync() {
    assert_send_sync::<RunMetrics>();
}

#[test]
fn metrics_summary_is_send_sync() {
    assert_send_sync::<MetricsSummary>();
}

#[test]
fn telemetry_span_is_send_sync() {
    assert_send_sync::<TelemetrySpan>();
}

#[test]
fn decision_is_send_sync() {
    assert_send_sync::<Decision>();
}

#[test]
fn match_decision_is_send_sync() {
    assert_send_sync::<MatchDecision>();
}

#[test]
fn agent_event_is_send_sync() {
    assert_send_sync::<AgentEvent>();
}

#[test]
fn outcome_is_send_sync() {
    assert_send_sync::<Outcome>();
}

#[test]
fn execution_mode_is_send_sync() {
    assert_send_sync::<ExecutionMode>();
}

#[test]
fn policy_profile_is_send_sync() {
    assert_send_sync::<PolicyProfile>();
}

#[test]
fn runtime_config_is_send_sync() {
    assert_send_sync::<RuntimeConfig>();
}

// =========================================================================
// Category 2: Concurrent receipt hashing produces consistent results
// =========================================================================

#[test]
fn receipt_hash_deterministic_across_threads() {
    let receipt = make_receipt("mock");
    let expected = receipt_hash(&receipt).unwrap();
    let receipt = Arc::new(receipt);

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let r = Arc::clone(&receipt);
            thread::spawn(move || receipt_hash(&r).unwrap())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

#[test]
fn receipt_with_hash_deterministic_across_threads() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let expected = receipt_hash(&receipt).unwrap();

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let r = receipt.clone();
            thread::spawn(move || {
                let hashed = r.with_hash().unwrap();
                hashed.receipt_sha256.unwrap()
            })
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

#[test]
fn different_receipts_produce_different_hashes_concurrently() {
    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let r = make_receipt(&format!("backend-{i}"));
                receipt_hash(&r).unwrap()
            })
        })
        .collect();

    let hashes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All 10 should be unique (different backend IDs)
    let unique: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(unique.len(), 10);
}

#[test]
fn concurrent_receipt_builder_hashing() {
    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                ReceiptBuilder::new(format!("backend-{i}"))
                    .outcome(Outcome::Complete)
                    .with_hash()
                    .unwrap()
            })
        })
        .collect();

    for h in handles {
        let receipt = h.join().unwrap();
        assert!(receipt.receipt_sha256.is_some());
        assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
    }
}

// =========================================================================
// Category 3: Concurrent policy checks don't interfere
// =========================================================================

#[test]
fn concurrent_tool_checks_same_engine() {
    let engine = Arc::new(make_policy_engine());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let e = Arc::clone(&engine);
            thread::spawn(move || {
                assert!(e.can_use_tool("Read").allowed);
                assert!(!e.can_use_tool("Bash").allowed);
                assert!(e.can_use_tool("Grep").allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_read_path_checks() {
    let engine = Arc::new(make_policy_engine());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let e = Arc::clone(&engine);
            thread::spawn(move || {
                assert!(!e.can_read_path(std::path::Path::new(".env")).allowed);
                assert!(e.can_read_path(std::path::Path::new("src/lib.rs")).allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_write_path_checks() {
    let engine = Arc::new(make_policy_engine());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let e = Arc::clone(&engine);
            thread::spawn(move || {
                assert!(
                    !e.can_write_path(std::path::Path::new(".git/config"))
                        .allowed
                );
                assert!(e.can_write_path(std::path::Path::new("src/lib.rs")).allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_mixed_policy_checks() {
    let engine = Arc::new(make_policy_engine());

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let e = Arc::clone(&engine);
            thread::spawn(move || match i % 3 {
                0 => e.can_use_tool("Read").allowed,
                1 => e.can_read_path(std::path::Path::new("src/lib.rs")).allowed,
                _ => e.can_write_path(std::path::Path::new("src/lib.rs")).allowed,
            })
        })
        .collect();

    for h in handles {
        assert!(h.join().unwrap());
    }
}

#[test]
fn concurrent_policy_engine_construction() {
    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let policy = PolicyProfile {
                    disallowed_tools: vec![format!("Tool{i}")],
                    ..PolicyProfile::default()
                };
                let engine = PolicyEngine::new(&policy).unwrap();
                assert!(!engine.can_use_tool(&format!("Tool{i}")).allowed);
                assert!(engine.can_use_tool("Other").allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn policy_check_results_consistent_under_contention() {
    let engine = Arc::new(make_policy_engine());
    let tools = ["Read", "Write", "Grep", "Bash", "Delete", "Execute"];

    let handles: Vec<_> = (0..20)
        .map(|i| {
            let e = Arc::clone(&engine);
            let tool = tools[i % tools.len()];
            thread::spawn(move || (tool, e.can_use_tool(tool).allowed))
        })
        .collect();

    for h in handles {
        let (tool, allowed) = h.join().unwrap();
        let expected = matches!(tool, "Read" | "Write" | "Grep");
        assert_eq!(allowed, expected, "tool {tool} mismatch");
    }
}

// =========================================================================
// Category 4: Concurrent glob matching is safe
// =========================================================================

#[test]
fn concurrent_glob_decide_str() {
    let globs = Arc::new(make_globs());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let g = Arc::clone(&globs);
            thread::spawn(move || {
                assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
                assert_eq!(
                    g.decide_str("src/generated/out.rs"),
                    MatchDecision::DeniedByExclude
                );
                assert_eq!(
                    g.decide_str("README.md"),
                    MatchDecision::DeniedByMissingInclude
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_glob_decide_path() {
    let globs = Arc::new(make_globs());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let g = Arc::clone(&globs);
            thread::spawn(move || {
                let path = std::path::Path::new("tests/it.rs");
                assert_eq!(g.decide_path(path), MatchDecision::Allowed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_glob_construction() {
    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let globs = IncludeExcludeGlobs::new(
                    &[format!("dir{i}/**")],
                    &[format!("dir{i}/excluded/**")],
                )
                .unwrap();
                assert_eq!(
                    globs.decide_str(&format!("dir{i}/file.rs")),
                    MatchDecision::Allowed
                );
                assert_eq!(
                    globs.decide_str(&format!("dir{i}/excluded/x.rs")),
                    MatchDecision::DeniedByExclude
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn glob_empty_patterns_concurrent() {
    let globs = Arc::new(IncludeExcludeGlobs::new(&[], &[]).unwrap());

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let g = Arc::clone(&globs);
            thread::spawn(move || {
                assert_eq!(
                    g.decide_str(&format!("any/path/{i}.txt")),
                    MatchDecision::Allowed
                );
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// =========================================================================
// Category 5: Concurrent dialect detection is safe
// =========================================================================

#[test]
fn concurrent_dialect_detect_openai() {
    let detector = Arc::new(DialectDetector::new());
    let value = Arc::new(openai_json());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let d = Arc::clone(&detector);
            let v = Arc::clone(&value);
            thread::spawn(move || {
                let result = d.detect(&v).unwrap();
                assert_eq!(result.dialect, Dialect::OpenAi);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_dialect_detect_claude() {
    let detector = Arc::new(DialectDetector::new());
    let value = Arc::new(claude_json());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let d = Arc::clone(&detector);
            let v = Arc::clone(&value);
            thread::spawn(move || {
                let result = d.detect(&v).unwrap();
                assert_eq!(result.dialect, Dialect::Claude);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_dialect_detect_gemini() {
    let detector = Arc::new(DialectDetector::new());
    let value = Arc::new(gemini_json());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let d = Arc::clone(&detector);
            let v = Arc::clone(&value);
            thread::spawn(move || {
                let result = d.detect(&v).unwrap();
                assert_eq!(result.dialect, Dialect::Gemini);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_dialect_detect_all() {
    let detector = Arc::new(DialectDetector::new());
    let value = Arc::new(openai_json());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let d = Arc::clone(&detector);
            let v = Arc::clone(&value);
            thread::spawn(move || {
                let results = d.detect_all(&v);
                assert!(!results.is_empty());
                assert_eq!(results[0].dialect, Dialect::OpenAi);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_dialect_validation() {
    let validator = Arc::new(DialectValidator::new());
    let value = Arc::new(openai_json());

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let v_ref = Arc::clone(&validator);
            let val = Arc::clone(&value);
            thread::spawn(move || {
                let result = v_ref.validate(&val, Dialect::OpenAi);
                result.valid
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All threads should get the same result
    assert!(results.iter().all(|&r| r == results[0]));
}

#[test]
fn concurrent_detection_different_dialects() {
    let detector = Arc::new(DialectDetector::new());
    let openai = Arc::new(openai_json());
    let claude = Arc::new(claude_json());
    let gemini = Arc::new(gemini_json());

    let handles: Vec<_> = (0..12)
        .map(|i| {
            let d = Arc::clone(&detector);
            let val = match i % 3 {
                0 => Arc::clone(&openai),
                1 => Arc::clone(&claude),
                _ => Arc::clone(&gemini),
            };
            let expected = match i % 3 {
                0 => Dialect::OpenAi,
                1 => Dialect::Claude,
                _ => Dialect::Gemini,
            };
            thread::spawn(move || {
                let result = d.detect(&val).unwrap();
                assert_eq!(result.dialect, expected);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn dialect_label_concurrent() {
    let handles: Vec<_> = Dialect::all()
        .iter()
        .map(|&d| {
            thread::spawn(move || {
                let label = d.label();
                assert!(!label.is_empty());
                label
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// =========================================================================
// Category 6: MetricsCollector thread safety
// =========================================================================

#[test]
fn metrics_concurrent_record() {
    let collector = MetricsCollector::new();
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let c = collector.clone();
            thread::spawn(move || {
                c.record(sample_metrics(&format!("b{i}"), i * 10));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(collector.len(), 20);
}

#[test]
fn metrics_concurrent_record_and_summary() {
    let collector = MetricsCollector::new();
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let c = collector.clone();
            thread::spawn(move || {
                c.record(sample_metrics("test", i * 10));
                let _ = c.summary();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(collector.len(), 20);
}

#[test]
fn metrics_concurrent_record_and_len() {
    let collector = MetricsCollector::new();
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let c = collector.clone();
            thread::spawn(move || {
                c.record(sample_metrics("test", i));
                c.len()
            })
        })
        .collect();

    for h in handles {
        let len = h.join().unwrap();
        assert!((1..=20).contains(&len));
    }
    assert_eq!(collector.len(), 20);
}

#[test]
fn metrics_concurrent_runs_snapshot() {
    let collector = MetricsCollector::new();
    for i in 0..5 {
        collector.record(sample_metrics(&format!("b{i}"), i * 10));
    }

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let c = collector.clone();
            thread::spawn(move || {
                let runs = c.runs();
                assert_eq!(runs.len(), 5);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn metrics_concurrent_summary_consistent() {
    let collector = MetricsCollector::new();
    collector.record(sample_metrics("a", 100));
    collector.record(sample_metrics("b", 200));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let c = collector.clone();
            thread::spawn(move || c.summary())
        })
        .collect();

    let summaries: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for s in &summaries {
        assert_eq!(s.count, 2);
        assert_eq!(s.total_tokens_in, 200);
        assert_eq!(s.total_tokens_out, 400);
    }
}

#[test]
fn metrics_concurrent_clear_and_record() {
    let collector = MetricsCollector::new();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let c = collector.clone();
            thread::spawn(move || {
                if i % 3 == 0 {
                    c.clear();
                } else {
                    c.record(sample_metrics("test", i));
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
    // Just verify no panic — final count depends on interleaving
    let _ = collector.len();
}

#[test]
fn metrics_clone_independence() {
    let collector = MetricsCollector::new();
    let clone = collector.clone();

    let h1 = {
        let c = collector.clone();
        thread::spawn(move || c.record(sample_metrics("a", 10)))
    };
    let h2 = {
        let c = clone.clone();
        thread::spawn(move || c.record(sample_metrics("b", 20)))
    };

    h1.join().unwrap();
    h2.join().unwrap();

    // Both share internal state (Arc<Mutex<...>>)
    assert_eq!(collector.len(), 2);
    assert_eq!(clone.len(), 2);
}

// =========================================================================
// Category 7: Stress tests — 100 threads doing operations simultaneously
// =========================================================================

#[test]
fn stress_100_threads_receipt_hashing() {
    let receipt = Arc::new(make_receipt("stress"));
    let expected = receipt_hash(&receipt).unwrap();

    let handles: Vec<_> = (0..100)
        .map(|_| {
            let r = Arc::clone(&receipt);
            thread::spawn(move || receipt_hash(&r).unwrap())
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

#[test]
fn stress_100_threads_policy_checks() {
    let engine = Arc::new(make_policy_engine());

    let handles: Vec<_> = (0..100)
        .map(|i| {
            let e = Arc::clone(&engine);
            thread::spawn(move || {
                let tool = if i % 2 == 0 { "Read" } else { "Bash" };
                e.can_use_tool(tool).allowed
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        let allowed = h.join().unwrap();
        if i % 2 == 0 {
            assert!(allowed);
        } else {
            assert!(!allowed);
        }
    }
}

#[test]
fn stress_100_threads_glob_matching() {
    let globs = Arc::new(make_globs());

    let handles: Vec<_> = (0..100)
        .map(|i| {
            let g = Arc::clone(&globs);
            thread::spawn(move || {
                let path = format!("src/module{i}/file.rs");
                g.decide_str(&path)
            })
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), MatchDecision::Allowed);
    }
}

#[test]
fn stress_100_threads_dialect_detection() {
    let detector = Arc::new(DialectDetector::new());
    let values: Vec<Arc<serde_json::Value>> = vec![
        Arc::new(openai_json()),
        Arc::new(claude_json()),
        Arc::new(gemini_json()),
    ];

    let handles: Vec<_> = (0..100)
        .map(|i| {
            let d = Arc::clone(&detector);
            let v = Arc::clone(&values[i % 3]);
            let expected = match i % 3 {
                0 => Dialect::OpenAi,
                1 => Dialect::Claude,
                _ => Dialect::Gemini,
            };
            thread::spawn(move || {
                let result = d.detect(&v).unwrap();
                assert_eq!(result.dialect, expected);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn stress_100_threads_metrics_recording() {
    let collector = MetricsCollector::new();

    let handles: Vec<_> = (0..100)
        .map(|i| {
            let c = collector.clone();
            thread::spawn(move || {
                c.record(sample_metrics(&format!("b{i}"), i));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(collector.len(), 100);

    let summary = collector.summary();
    assert_eq!(summary.count, 100);
    assert_eq!(summary.total_tokens_in, 100 * 100);
    assert_eq!(summary.total_tokens_out, 100 * 200);
}

#[test]
fn stress_100_threads_mixed_operations() {
    let engine = Arc::new(make_policy_engine());
    let globs = Arc::new(make_globs());
    let detector = Arc::new(DialectDetector::new());
    let collector = MetricsCollector::new();
    let receipt = Arc::new(make_receipt("mixed"));

    let handles: Vec<_> = (0..100)
        .map(|i| {
            let e = Arc::clone(&engine);
            let g = Arc::clone(&globs);
            let d = Arc::clone(&detector);
            let c = collector.clone();
            let r = Arc::clone(&receipt);
            thread::spawn(move || match i % 5 {
                0 => {
                    let _ = e.can_use_tool("Read");
                }
                1 => {
                    let _ = g.decide_str("src/lib.rs");
                }
                2 => {
                    let _ = d.detect(&openai_json());
                }
                3 => {
                    c.record(sample_metrics("stress", i));
                }
                _ => {
                    receipt_hash(&r).unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// =========================================================================
// Category 8: No data races with shared read-only data
// =========================================================================

#[test]
fn shared_readonly_work_order() {
    let wo = Arc::new(make_work_order("shared task"));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let w = Arc::clone(&wo);
            thread::spawn(move || {
                assert_eq!(w.task, "shared task");
                assert_eq!(w.workspace.root, ".");
                let _ = serde_json::to_string(&*w).unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn shared_readonly_receipt() {
    let receipt = Arc::new(make_receipt("readonly"));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let r = Arc::clone(&receipt);
            thread::spawn(move || {
                assert_eq!(r.backend.id, "readonly");
                assert_eq!(r.outcome, Outcome::Complete);
                let _ = serde_json::to_string(&*r).unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn shared_readonly_policy_engine_many_reads() {
    let engine = Arc::new(make_policy_engine());
    let tools = [
        "Read", "Write", "Grep", "Bash", "Delete", "Execute", "Fetch", "Search",
    ];

    let handles: Vec<_> = (0..20)
        .map(|i| {
            let e = Arc::clone(&engine);
            let tool = tools[i % tools.len()].to_string();
            thread::spawn(move || e.can_use_tool(&tool).allowed)
        })
        .collect();

    for h in handles {
        let _ = h.join().unwrap();
    }
}

#[test]
fn shared_readonly_globs_many_paths() {
    let globs = Arc::new(make_globs());
    let paths: Vec<String> = (0..20).map(|i| format!("src/mod{i}/file.rs")).collect();

    let handles: Vec<_> = paths
        .into_iter()
        .map(|p| {
            let g = Arc::clone(&globs);
            thread::spawn(move || g.decide_str(&p))
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), MatchDecision::Allowed);
    }
}

#[test]
fn shared_readonly_contract_version() {
    let handles: Vec<_> = (0..10)
        .map(|_| {
            thread::spawn(|| {
                assert_eq!(CONTRACT_VERSION, "abp/v0.1");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_serde_roundtrip_receipt() {
    let receipt = Arc::new(make_receipt("serde"));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let r = Arc::clone(&receipt);
            thread::spawn(move || {
                let json = serde_json::to_string(&*r).unwrap();
                let deserialized: Receipt = serde_json::from_str(&json).unwrap();
                assert_eq!(deserialized.backend.id, "serde");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_serde_roundtrip_work_order() {
    let wo = Arc::new(make_work_order("serde task"));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let w = Arc::clone(&wo);
            thread::spawn(move || {
                let json = serde_json::to_string(&*w).unwrap();
                let deserialized: WorkOrder = serde_json::from_str(&json).unwrap();
                assert_eq!(deserialized.task, "serde task");
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_serde_roundtrip_policy_profile() {
    let profile = Arc::new(PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    });

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let p = Arc::clone(&profile);
            thread::spawn(move || {
                let json = serde_json::to_string(&*p).unwrap();
                let deserialized: PolicyProfile = serde_json::from_str(&json).unwrap();
                assert_eq!(deserialized.disallowed_tools, vec!["Bash"]);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_dialect_display() {
    let handles: Vec<_> = Dialect::all()
        .iter()
        .map(|&d| {
            thread::spawn(move || {
                let s = format!("{d}");
                assert!(!s.is_empty());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_telemetry_span_creation() {
    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let span =
                    TelemetrySpan::new(format!("op-{i}")).with_attribute("thread", format!("{i}"));
                assert_eq!(span.attributes["thread"], format!("{i}"));
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_agent_event_creation() {
    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let event = AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantMessage {
                        text: format!("msg-{i}"),
                    },
                    ext: None,
                };
                let json = serde_json::to_string(&event).unwrap();
                let parsed: AgentEvent = serde_json::from_str(&json).unwrap();
                if let AgentEventKind::AssistantMessage { text } = &parsed.kind {
                    assert_eq!(text, &format!("msg-{i}"));
                } else {
                    panic!("wrong event kind");
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn shared_readonly_execution_mode() {
    let mode = Arc::new(ExecutionMode::Mapped);

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let m = Arc::clone(&mode);
            thread::spawn(move || {
                assert_eq!(*m, ExecutionMode::Mapped);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_canonical_json_hashing() {
    let value = Arc::new(serde_json::json!({"z": 1, "a": 2, "m": 3}));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let v = Arc::clone(&value);
            thread::spawn(move || abp_core::canonical_json(&*v).unwrap())
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn concurrent_sha256_hex() {
    let handles: Vec<_> = (0..10)
        .map(|_| thread::spawn(|| abp_core::sha256_hex(b"hello world")))
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.windows(2).all(|w| w[0] == w[1]));
    assert_eq!(results[0].len(), 64);
}
