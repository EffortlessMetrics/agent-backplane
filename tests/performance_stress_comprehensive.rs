// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive performance and stress tests for Agent Backplane.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    ExecutionLane, ExecutionMode, Outcome, PolicyProfile, Receipt, ReceiptBuilder,
    VerificationReport, WorkOrder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_glob::IncludeExcludeGlobs;
use abp_policy::PolicyEngine;
use chrono::Utc;
use serde_json::json;

// ─── Helpers ────────────────────────────────────────────────────────────

fn make_work_order(i: usize) -> WorkOrder {
    WorkOrderBuilder::new(format!("Task {i}"))
        .root("/tmp/ws")
        .model("gpt-4")
        .max_turns(10)
        .build()
}

fn make_receipt(i: usize) -> Receipt {
    ReceiptBuilder::new(format!("backend-{i}"))
        .outcome(Outcome::Complete)
        .build()
}

fn make_event(i: usize) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: format!("message {i}"),
        },
        ext: None,
    }
}

fn make_policy_engine() -> PolicyEngine {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into()],
        disallowed_tools: vec!["Bash*".into()],
        deny_read: vec!["**/.env".into(), "**/secret/**".into()],
        deny_write: vec!["**/.git/**".into(), "**/locked/**".into()],
        ..PolicyProfile::default()
    };
    PolicyEngine::new(&policy).expect("compile policy")
}

fn make_globs() -> IncludeExcludeGlobs {
    IncludeExcludeGlobs::new(
        &["src/**".into(), "tests/**".into(), "*.rs".into()],
        &["target/**".into(), "*.log".into(), "**/.git/**".into()],
    )
    .expect("compile globs")
}

fn make_openai_json() -> serde_json::Value {
    json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}],
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "temperature": 0.7
    })
}

fn make_claude_json() -> serde_json::Value {
    json!({
        "type": "message",
        "model": "claude-3",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}],
        "stop_reason": "end_turn"
    })
}

fn make_gemini_json() -> serde_json::Value {
    json!({
        "contents": [{"parts": [{"text": "hello"}]}],
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}],
        "generationConfig": {"temperature": 0.5}
    })
}

// ═══════════════════════════════════════════════════════════════════════
// Category 1: WorkOrder serialization throughput
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_work_order_serialize_1000_under_1s() {
    let orders: Vec<_> = (0..1000).map(make_work_order).collect();
    let start = Instant::now();
    for wo in &orders {
        let _ = serde_json::to_string(wo).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_work_order_deserialize_1000_under_1s() {
    let jsons: Vec<_> = (0..1000)
        .map(|i| serde_json::to_string(&make_work_order(i)).unwrap())
        .collect();
    let start = Instant::now();
    for j in &jsons {
        let _: WorkOrder = serde_json::from_str(j).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_work_order_roundtrip_1000_under_1s() {
    let start = Instant::now();
    for i in 0..1000 {
        let wo = make_work_order(i);
        let json = serde_json::to_string(&wo).unwrap();
        let _: WorkOrder = serde_json::from_str(&json).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_work_order_canonical_json_1000_under_1s() {
    let orders: Vec<_> = (0..1000).map(make_work_order).collect();
    let start = Instant::now();
    for wo in &orders {
        let _ = canonical_json(wo).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_work_order_builder_1000_under_1s() {
    let start = Instant::now();
    for i in 0..1000 {
        let _ = WorkOrderBuilder::new(format!("task-{i}"))
            .lane(ExecutionLane::WorkspaceFirst)
            .root("/tmp")
            .model("gpt-4")
            .max_turns(5)
            .build();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_work_order_clone_1000_under_1s() {
    let wo = make_work_order(0);
    let start = Instant::now();
    for _ in 0..1000 {
        let _ = wo.clone();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Category 2: Receipt hashing throughput
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_receipt_hash_1000_under_1s() {
    let receipts: Vec<_> = (0..1000).map(make_receipt).collect();
    let start = Instant::now();
    for r in &receipts {
        let _ = receipt_hash(r).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_with_hash_1000_under_1s() {
    let start = Instant::now();
    for i in 0..1000 {
        let r = make_receipt(i);
        let _ = r.with_hash().unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_serialize_1000_under_1s() {
    let receipts: Vec<_> = (0..1000).map(make_receipt).collect();
    let start = Instant::now();
    for r in &receipts {
        let _ = serde_json::to_string(r).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_deserialize_1000_under_1s() {
    let jsons: Vec<_> = (0..1000)
        .map(|i| serde_json::to_string(&make_receipt(i)).unwrap())
        .collect();
    let start = Instant::now();
    for j in &jsons {
        let _: Receipt = serde_json::from_str(j).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_builder_1000_under_1s() {
    let start = Instant::now();
    for i in 0..1000 {
        let _ = ReceiptBuilder::new(format!("mock-{i}"))
            .outcome(Outcome::Complete)
            .mode(ExecutionMode::Mapped)
            .build();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_sha256_hex_10000_under_1s() {
    let data = b"deterministic hashing benchmark input data";
    let start = Instant::now();
    for _ in 0..10_000 {
        let _ = sha256_hex(data);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Category 3: Policy check throughput
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_policy_tool_check_10000_under_1s() {
    let engine = make_policy_engine();
    let tools = ["Read", "Write", "Grep", "Bash", "BashExec", "Delete"];
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = engine.can_use_tool(tools[i % tools.len()]);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_policy_read_path_check_10000_under_1s() {
    let engine = make_policy_engine();
    let paths = [
        Path::new("src/lib.rs"),
        Path::new(".env"),
        Path::new("secret/key.pem"),
        Path::new("docs/readme.md"),
    ];
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = engine.can_read_path(paths[i % paths.len()]);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_policy_write_path_check_10000_under_1s() {
    let engine = make_policy_engine();
    let paths = [
        Path::new("src/main.rs"),
        Path::new(".git/config"),
        Path::new("locked/data.txt"),
        Path::new("docs/guide.md"),
    ];
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = engine.can_write_path(paths[i % paths.len()]);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_policy_mixed_checks_10000_under_1s() {
    let engine = make_policy_engine();
    let start = Instant::now();
    for i in 0..10_000 {
        match i % 3 {
            0 => {
                let _ = engine.can_use_tool("Read");
            }
            1 => {
                let _ = engine.can_read_path(Path::new("src/lib.rs"));
            }
            _ => {
                let _ = engine.can_write_path(Path::new("docs/a.md"));
            }
        }
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_policy_compile_100_under_1s() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash*".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let start = Instant::now();
    for _ in 0..100 {
        let _ = PolicyEngine::new(&policy).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_policy_empty_profile_10000_under_1s() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let start = Instant::now();
    for _ in 0..10_000 {
        assert!(engine.can_use_tool("anything").allowed);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Category 4: Glob matching throughput
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_glob_decide_str_10000_under_1s() {
    let globs = make_globs();
    let paths = [
        "src/lib.rs",
        "target/debug/bin",
        "tests/it.rs",
        "README.md",
        "build.log",
        "src/a/b/c.rs",
    ];
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = globs.decide_str(paths[i % paths.len()]);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_glob_decide_path_10000_under_1s() {
    let globs = make_globs();
    let paths: Vec<_> = ["src/lib.rs", "target/debug/bin", "tests/it.rs", "README.md"]
        .iter()
        .map(Path::new)
        .collect();
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = globs.decide_path(paths[i % paths.len()]);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_glob_compile_100_under_1s() {
    let start = Instant::now();
    for _ in 0..100 {
        let _ = IncludeExcludeGlobs::new(
            &["src/**".into(), "tests/**".into()],
            &["target/**".into(), "*.log".into()],
        )
        .unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_glob_many_patterns_compile_under_1s() {
    let includes: Vec<String> = (0..50).map(|i| format!("dir{i}/**")).collect();
    let excludes: Vec<String> = (0..50).map(|i| format!("dir{i}/tmp/**")).collect();
    let start = Instant::now();
    for _ in 0..50 {
        let _ = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_glob_deep_paths_10000_under_1s() {
    let globs = make_globs();
    let deep_path = "src/a/b/c/d/e/f/g/h/i/j/k/l/m/n.rs";
    let start = Instant::now();
    for _ in 0..10_000 {
        let _ = globs.decide_str(deep_path);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_glob_empty_rules_10000_under_1s() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    let start = Instant::now();
    for _ in 0..10_000 {
        assert!(globs.decide_str("any/path.txt").is_allowed());
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Category 5: Dialect detection throughput
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_dialect_detect_openai_10000_under_1s() {
    let detector = DialectDetector::new();
    let val = make_openai_json();
    let start = Instant::now();
    for _ in 0..10_000 {
        let r = detector.detect(&val);
        assert!(r.is_some());
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_dialect_detect_claude_10000_under_1s() {
    let detector = DialectDetector::new();
    let val = make_claude_json();
    let start = Instant::now();
    for _ in 0..10_000 {
        let r = detector.detect(&val);
        assert!(r.is_some());
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_dialect_detect_gemini_10000_under_1s() {
    let detector = DialectDetector::new();
    let val = make_gemini_json();
    let start = Instant::now();
    for _ in 0..10_000 {
        let r = detector.detect(&val);
        assert!(r.is_some());
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_dialect_detect_all_10000_under_1s() {
    let detector = DialectDetector::new();
    let val = make_openai_json();
    let start = Instant::now();
    for _ in 0..10_000 {
        let _ = detector.detect_all(&val);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_dialect_detect_mixed_10000_under_1s() {
    let detector = DialectDetector::new();
    let values = [make_openai_json(), make_claude_json(), make_gemini_json()];
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = detector.detect(&values[i % values.len()]);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_dialect_all_variants_enumeration() {
    let all = Dialect::all();
    assert!(all.len() >= 5);
    let start = Instant::now();
    for _ in 0..10_000 {
        for d in all {
            let _ = d.label();
            let _ = d.to_string();
        }
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Category 6: Large payload handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_large_work_order_serialize() {
    let large_content = "x".repeat(1_000_000);
    let wo = WorkOrderBuilder::new(large_content).build();
    let start = Instant::now();
    let json = serde_json::to_string(&wo).unwrap();
    let elapsed = start.elapsed();
    assert!(json.len() > 1_000_000);
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn perf_large_work_order_deserialize() {
    let large_content = "x".repeat(1_000_000);
    let wo = WorkOrderBuilder::new(large_content).build();
    let json = serde_json::to_string(&wo).unwrap();
    let start = Instant::now();
    let _: WorkOrder = serde_json::from_str(&json).unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn perf_large_receipt_hash() {
    let large_text = "y".repeat(500_000);
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: large_text },
            ext: None,
        })
        .build();
    let start = Instant::now();
    let hash = receipt_hash(&receipt).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(hash.len(), 64);
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn perf_large_json_canonical() {
    let big_map: BTreeMap<String, String> = (0..10_000)
        .map(|i| (format!("key_{i:05}"), format!("value_{i}")))
        .collect();
    let start = Instant::now();
    let json = canonical_json(&big_map).unwrap();
    let elapsed = start.elapsed();
    assert!(json.len() > 100_000);
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn perf_large_sha256() {
    let data = vec![0xABu8; 2_000_000];
    let start = Instant::now();
    let hex = sha256_hex(&data);
    let elapsed = start.elapsed();
    assert_eq!(hex.len(), 64);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_large_dialect_payload() {
    let big_messages: Vec<serde_json::Value> = (0..1000)
        .map(|i| json!({"role": "user", "content": format!("msg {i} {}", "z".repeat(1000))}))
        .collect();
    let val = json!({
        "model": "gpt-4",
        "messages": big_messages,
        "choices": [{"message": {"role": "assistant", "content": "ok"}}]
    });
    let detector = DialectDetector::new();
    let start = Instant::now();
    let result = detector.detect(&val);
    let elapsed = start.elapsed();
    assert!(result.is_some());
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Category 7: Many-event receipt
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_receipt_1000_events_serialize() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..1000 {
        builder = builder.add_trace_event(make_event(i));
    }
    let receipt = builder.build();
    let start = Instant::now();
    let json = serde_json::to_string(&receipt).unwrap();
    let elapsed = start.elapsed();
    assert!(json.len() > 10_000);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_1000_events_hash() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..1000 {
        builder = builder.add_trace_event(make_event(i));
    }
    let receipt = builder.build();
    let start = Instant::now();
    let hash = receipt_hash(&receipt).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(hash.len(), 64);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_1000_events_with_hash() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..1000 {
        builder = builder.add_trace_event(make_event(i));
    }
    let receipt = builder.build();
    let start = Instant::now();
    let hashed = receipt.with_hash().unwrap();
    let elapsed = start.elapsed();
    assert!(hashed.receipt_sha256.is_some());
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_mixed_events_serialize() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..500 {
        let event = match i % 5 {
            0 => AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("msg {i}"),
                },
                ext: None,
            },
            1 => AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "Read".into(),
                    tool_use_id: Some(format!("tu-{i}")),
                    parent_tool_use_id: None,
                    input: json!({"path": format!("file_{i}.rs")}),
                },
                ext: None,
            },
            2 => AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "Read".into(),
                    tool_use_id: Some(format!("tu-{i}")),
                    output: json!({"content": "data"}),
                    is_error: false,
                },
                ext: None,
            },
            3 => AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: format!("src/file_{i}.rs"),
                    summary: "modified".into(),
                },
                ext: None,
            },
            _ => AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("delta-{i}"),
                },
                ext: None,
            },
        };
        builder = builder.add_trace_event(event);
    }
    let receipt = builder.build();
    let start = Instant::now();
    let json = serde_json::to_string(&receipt).unwrap();
    let elapsed = start.elapsed();
    assert!(json.len() > 10_000);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_5000_events_build() {
    let start = Instant::now();
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..5000 {
        builder = builder.add_trace_event(make_event(i));
    }
    let receipt = builder.build();
    let elapsed = start.elapsed();
    assert_eq!(receipt.trace.len(), 5000);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_receipt_artifacts_1000() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..1000 {
        builder = builder.add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: format!("patches/p{i}.diff"),
        });
    }
    let receipt = builder.build();
    let start = Instant::now();
    let json = serde_json::to_string(&receipt).unwrap();
    let elapsed = start.elapsed();
    assert!(json.len() > 10_000);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Category 8: Concurrent workload simulation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_concurrent_serialize_many_work_orders() {
    let orders: Vec<_> = (0..500).map(make_work_order).collect();
    let start = Instant::now();
    let results: Vec<_> = orders
        .iter()
        .map(|wo| serde_json::to_string(wo).unwrap())
        .collect();
    let elapsed = start.elapsed();
    assert_eq!(results.len(), 500);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_concurrent_hash_many_receipts() {
    let receipts: Vec<_> = (0..500).map(make_receipt).collect();
    let start = Instant::now();
    let hashes: Vec<_> = receipts.iter().map(|r| receipt_hash(r).unwrap()).collect();
    let elapsed = start.elapsed();
    assert_eq!(hashes.len(), 500);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_concurrent_policy_and_glob() {
    let engine = make_policy_engine();
    let globs = make_globs();
    let start = Instant::now();
    for i in 0..5_000 {
        if i % 2 == 0 {
            let _ = engine.can_use_tool("Read");
        } else {
            let _ = globs.decide_str("src/lib.rs");
        }
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_concurrent_dialect_detection() {
    let detector = DialectDetector::new();
    let vals = [make_openai_json(), make_claude_json(), make_gemini_json()];
    let start = Instant::now();
    let results: Vec<_> = (0..3000)
        .map(|i| detector.detect(&vals[i % vals.len()]))
        .collect();
    let elapsed = start.elapsed();
    assert_eq!(results.len(), 3000);
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_interleaved_work_order_receipt() {
    let start = Instant::now();
    for i in 0..500 {
        let wo = make_work_order(i);
        let _ = serde_json::to_string(&wo).unwrap();
        let r = make_receipt(i);
        let _ = r.with_hash().unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn perf_full_pipeline_simulation() {
    let engine = make_policy_engine();
    let detector = DialectDetector::new();
    let globs = make_globs();
    let val = make_openai_json();
    let start = Instant::now();
    for i in 0..1_000 {
        let wo = make_work_order(i);
        let _ = serde_json::to_string(&wo).unwrap();
        let _ = engine.can_use_tool("Read");
        let _ = globs.decide_str("src/lib.rs");
        let _ = detector.detect(&val);
        let r = make_receipt(i);
        let _ = receipt_hash(&r).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Category 9: Memory usage patterns (no leaks in loops)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mem_work_order_create_drop_loop() {
    for i in 0..10_000 {
        let wo = make_work_order(i);
        let json = serde_json::to_string(&wo).unwrap();
        let _: WorkOrder = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn mem_receipt_create_hash_drop_loop() {
    for i in 0..5_000 {
        let r = make_receipt(i);
        let _ = r.with_hash().unwrap();
    }
}

#[test]
fn mem_policy_engine_create_drop_loop() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    for _ in 0..1_000 {
        let engine = PolicyEngine::new(&policy).unwrap();
        let _ = engine.can_use_tool("Read");
        let _ = engine.can_write_path(Path::new("src/a.rs"));
    }
}

#[test]
fn mem_glob_create_match_drop_loop() {
    for _ in 0..1_000 {
        let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["target/**".into()]).unwrap();
        let _ = globs.decide_str("src/lib.rs");
        let _ = globs.decide_str("target/debug/bin");
    }
}

#[test]
fn mem_dialect_detector_reuse() {
    let detector = DialectDetector::new();
    for _ in 0..10_000 {
        let val = make_openai_json();
        let _ = detector.detect(&val);
    }
}

#[test]
fn mem_event_creation_loop() {
    for i in 0..50_000 {
        let _ = make_event(i);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 10: Stress: rapid create/check/drop cycles
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stress_rapid_work_order_cycles() {
    let start = Instant::now();
    for i in 0..2_000 {
        let wo = make_work_order(i);
        let json = serde_json::to_string(&wo).unwrap();
        assert!(json.contains(&format!("Task {i}")));
        drop(wo);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn stress_rapid_receipt_hash_cycles() {
    let start = Instant::now();
    for i in 0..2_000 {
        let r = make_receipt(i);
        let hashed = r.with_hash().unwrap();
        assert!(hashed.receipt_sha256.is_some());
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn stress_rapid_policy_compile_check_cycles() {
    let policies: Vec<PolicyProfile> = (0..100)
        .map(|i| PolicyProfile {
            disallowed_tools: vec![format!("tool_{i}")],
            deny_write: vec![format!("dir_{i}/**")],
            ..PolicyProfile::default()
        })
        .collect();
    let start = Instant::now();
    for policy in &policies {
        let engine = PolicyEngine::new(policy).unwrap();
        for _ in 0..100 {
            let _ = engine.can_use_tool("Read");
            let _ = engine.can_write_path(Path::new("src/a.rs"));
        }
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn stress_rapid_glob_compile_match_cycles() {
    let start = Instant::now();
    for i in 0..500 {
        let globs =
            IncludeExcludeGlobs::new(&[format!("dir_{i}/**")], &[format!("dir_{i}/tmp/**")])
                .unwrap();
        for j in 0..20 {
            let _ = globs.decide_str(&format!("dir_{i}/file_{j}.rs"));
        }
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn stress_rapid_dialect_detection_cycles() {
    let detector = DialectDetector::new();
    let start = Instant::now();
    for i in 0..5_000 {
        let val = json!({
            "model": format!("model-{i}"),
            "messages": [{"role": "user", "content": format!("msg-{i}")}],
            "choices": [{"message": {"role": "assistant", "content": "ok"}}]
        });
        let r = detector.detect(&val);
        assert!(r.is_some());
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}

#[test]
fn stress_alternating_all_subsystems() {
    let engine = make_policy_engine();
    let globs = make_globs();
    let detector = DialectDetector::new();
    let start = Instant::now();
    for i in 0..1_000 {
        let wo = make_work_order(i);
        let _ = serde_json::to_string(&wo).unwrap();

        let _ = engine.can_use_tool("Read");
        let _ = engine.can_read_path(Path::new("src/lib.rs"));
        let _ = engine.can_write_path(Path::new("docs/a.md"));

        let _ = globs.decide_str("src/lib.rs");
        let _ = globs.decide_str("target/debug/bin");

        let val = make_openai_json();
        let _ = detector.detect(&val);

        let r = make_receipt(i);
        let _ = r.with_hash().unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 3.0, "took {elapsed:?}");
}

#[test]
fn stress_hash_determinism_under_load() {
    let receipt = make_receipt(42);
    let baseline = receipt_hash(&receipt).unwrap();
    for _ in 0..1_000 {
        let hash = receipt_hash(&receipt).unwrap();
        assert_eq!(hash, baseline, "hash diverged under load");
    }
}

#[test]
fn stress_contract_version_constant() {
    for _ in 0..10_000 {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }
}

#[test]
fn stress_receipt_hash_null_field_invariant() {
    for i in 0..500 {
        let receipt = make_receipt(i);
        let h1 = receipt_hash(&receipt).unwrap();
        let hashed = receipt.with_hash().unwrap();
        let h2 = receipt_hash(&hashed).unwrap();
        assert_eq!(h1, h2, "hash differs after with_hash at iteration {i}");
    }
}

#[test]
fn stress_verification_report_default_loop() {
    for _ in 0..10_000 {
        let v = VerificationReport::default();
        assert!(v.git_diff.is_none());
        assert!(!v.harness_ok);
    }
}
