#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive performance regression test suite for Agent Backplane.
//!
//! Validates that performance-sensitive operations don't regress.
//! Uses `std::time::Instant` with generous bounds (no external benchmark frameworks).
//!
//! ```bash
//! cargo test --test perf_regression_comprehensive
//! ```

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    ExecutionLane, ExecutionMode, Outcome, PolicyProfile, Receipt, ReceiptBuilder,
    VerificationReport, WorkOrder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_error::{AbpError, ErrorCategory, ErrorCode};
use abp_glob::IncludeExcludeGlobs;
use abp_ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use abp_mapping::{known_rules, validate_mapping, Fidelity, MappingRegistry, MappingRule};
use abp_policy::PolicyEngine;
use abp_protocol::{parse_version, Envelope, JsonlCodec};
use abp_receipt::{compute_hash, diff_receipts, verify_hash};
use abp_stream::{
    event_kind_name, EventFilter, EventRecorder, EventStats, EventTransform, StreamAggregator,
    StreamBuffer, StreamMetrics, StreamPipelineBuilder,
};
use chrono::Utc;
use serde_json::json;

// ─── Helpers ────────────────────────────────────────────────────────────

fn make_work_order(i: usize) -> WorkOrder {
    WorkOrderBuilder::new(format!("Task {i}: refactor authentication module"))
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/workspace")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(1.0)
        .build()
}

fn make_receipt(i: usize) -> Receipt {
    ReceiptBuilder::new(format!("backend-{i}"))
        .outcome(Outcome::Complete)
        .backend_version("0.1.0")
        .adapter_version("0.1.0")
        .build()
}

fn make_receipt_with_trace(n: usize) -> Receipt {
    let mut builder = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .backend_version("0.1.0");
    for i in 0..n {
        builder = builder.add_trace_event(make_event(i));
    }
    builder.build()
}

fn make_event(i: usize) -> AgentEvent {
    let kind = match i % 7 {
        0 => AgentEventKind::RunStarted {
            message: format!("start-{i}"),
        },
        1 => AgentEventKind::AssistantMessage {
            text: format!("msg-{i}"),
        },
        2 => AgentEventKind::AssistantDelta {
            text: format!("delta-{i}"),
        },
        3 => AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some(format!("tu-{i}")),
            parent_tool_use_id: None,
            input: json!({"path": format!("src/file_{i}.rs")}),
        },
        4 => AgentEventKind::ToolResult {
            tool_name: "Read".into(),
            tool_use_id: Some(format!("tu-{i}")),
            output: json!({"content": "some data"}),
            is_error: false,
        },
        5 => AgentEventKind::FileChanged {
            path: format!("src/file_{i}.rs"),
            summary: "updated".into(),
        },
        _ => AgentEventKind::RunCompleted {
            message: format!("done-{i}"),
        },
    };
    AgentEvent {
        ts: Utc::now(),
        kind,
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

fn make_ir_conversation(n: usize) -> IrConversation {
    let mut msgs = Vec::with_capacity(n);
    for i in 0..n {
        let msg = match i % 3 {
            0 => IrMessage::text(IrRole::System, format!("System instruction {i}")),
            1 => IrMessage::text(IrRole::User, format!("User query {i}")),
            _ => IrMessage::text(IrRole::Assistant, format!("Assistant response {i}")),
        };
        msgs.push(msg);
    }
    IrConversation::from_messages(msgs)
}

fn make_ir_tools(n: usize) -> Vec<IrToolDefinition> {
    (0..n)
        .map(|i| IrToolDefinition {
            name: format!("tool_{i}"),
            description: format!("A tool for testing {i}"),
            parameters: json!({"type": "object", "properties": {"arg": {"type": "string"}}}),
        })
        .collect()
}

fn make_jsonl_line(i: usize) -> String {
    let envelope = Envelope::Event {
        ref_id: format!("ref-{i}"),
        event: make_event(i),
    };
    JsonlCodec::encode(&envelope).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════
// Category 1: Canonical JSON serialization speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_canonical_json_work_order_1000() {
    let orders: Vec<_> = (0..1000).map(make_work_order).collect();
    let start = Instant::now();
    for wo in &orders {
        let _ = canonical_json(wo).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_canonical_json_receipt_1000() {
    let receipts: Vec<_> = (0..1000).map(make_receipt).collect();
    let start = Instant::now();
    for r in &receipts {
        let _ = canonical_json(r).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_canonical_json_large_btreemap() {
    let big: BTreeMap<String, String> = (0..10_000)
        .map(|i| (format!("key_{i:05}"), format!("value_{i}")))
        .collect();
    let start = Instant::now();
    let json = canonical_json(&big).unwrap();
    assert!(json.len() > 100_000);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_canonical_json_deterministic() {
    let wo = make_work_order(42);
    let baseline = canonical_json(&wo).unwrap();
    let start = Instant::now();
    for _ in 0..500 {
        let json = canonical_json(&wo).unwrap();
        assert_eq!(json, baseline);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_canonical_json_nested_values() {
    let nested = json!({
        "a": {"b": {"c": {"d": {"e": "deep"}}}},
        "arr": [1, 2, 3, {"x": [4, 5]}],
        "z": null
    });
    let start = Instant::now();
    for _ in 0..5_000 {
        let _ = canonical_json(&nested).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 2: Receipt hash computation speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_receipt_hash_1000() {
    let receipts: Vec<_> = (0..1000).map(make_receipt).collect();
    let start = Instant::now();
    for r in &receipts {
        let h = receipt_hash(r).unwrap();
        assert_eq!(h.len(), 64);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_receipt_with_hash_1000() {
    let start = Instant::now();
    for i in 0..1000 {
        let r = make_receipt(i);
        let hashed = r.with_hash().unwrap();
        assert!(hashed.receipt_sha256.is_some());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_receipt_hash_large_trace() {
    let receipt = make_receipt_with_trace(500);
    let start = Instant::now();
    for _ in 0..100 {
        let _ = receipt_hash(&receipt).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_receipt_hash_determinism_under_load() {
    let receipt = make_receipt(99);
    let baseline = receipt_hash(&receipt).unwrap();
    let start = Instant::now();
    for _ in 0..2_000 {
        assert_eq!(receipt_hash(&receipt).unwrap(), baseline);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_receipt_hash_null_field_invariant() {
    let start = Instant::now();
    for i in 0..500 {
        let receipt = make_receipt(i);
        let h1 = receipt_hash(&receipt).unwrap();
        let hashed = receipt.with_hash().unwrap();
        let h2 = receipt_hash(&hashed).unwrap();
        assert_eq!(h1, h2);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_sha256_hex_10000() {
    let data = b"deterministic hashing benchmark input data for perf test";
    let start = Instant::now();
    for _ in 0..10_000 {
        let h = sha256_hex(data);
        assert_eq!(h.len(), 64);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_compute_hash_verify_hash_round_trip() {
    let start = Instant::now();
    for i in 0..500 {
        let r = make_receipt(i);
        let h = compute_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
        let hashed = r.with_hash().unwrap();
        assert!(verify_hash(&hashed));
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 3: JSONL parsing throughput
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_jsonl_encode_1000() {
    let events: Vec<_> = (0..1000)
        .map(|i| Envelope::Event {
            ref_id: format!("ref-{i}"),
            event: make_event(i),
        })
        .collect();
    let start = Instant::now();
    for e in &events {
        let _ = JsonlCodec::encode(e).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_jsonl_decode_1000() {
    let lines: Vec<_> = (0..1000).map(|i| make_jsonl_line(i)).collect();
    let start = Instant::now();
    for line in &lines {
        let _ = JsonlCodec::decode(line).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_jsonl_roundtrip_1000() {
    let start = Instant::now();
    for i in 0..1000 {
        let env = Envelope::Event {
            ref_id: format!("ref-{i}"),
            event: make_event(i),
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let _ = JsonlCodec::decode(&encoded).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_jsonl_decode_stream_batch() {
    let blob: String = (0..500).map(|i| make_jsonl_line(i)).collect::<String>();
    let start = Instant::now();
    let reader = std::io::Cursor::new(blob.as_bytes());
    let count = JsonlCodec::decode_stream(reader)
        .filter(|r| r.is_ok())
        .count();
    assert_eq!(count, 500);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_jsonl_encode_hello_envelope() {
    use abp_core::{BackendIdentity, CapabilityManifest};
    let hello = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        },
        CapabilityManifest::default(),
    );
    let start = Instant::now();
    for _ in 0..5_000 {
        let _ = JsonlCodec::encode(&hello).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_jsonl_mixed_envelope_types() {
    let wo = make_work_order(0);
    let receipt = make_receipt(0);
    let start = Instant::now();
    for i in 0..1_000 {
        let env = match i % 3 {
            0 => Envelope::Run {
                id: format!("run-{i}"),
                work_order: wo.clone(),
            },
            1 => Envelope::Event {
                ref_id: format!("ref-{i}"),
                event: make_event(i),
            },
            _ => Envelope::Final {
                ref_id: format!("ref-{i}"),
                receipt: receipt.clone(),
            },
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let _ = JsonlCodec::decode(&encoded).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 4: IR normalization speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_ir_normalize_small_conversation() {
    use abp_ir::normalize::normalize;
    let conv = make_ir_conversation(10);
    let start = Instant::now();
    for _ in 0..2_000 {
        let _ = normalize(&conv);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_normalize_large_conversation() {
    use abp_ir::normalize::normalize;
    let conv = make_ir_conversation(200);
    let start = Instant::now();
    for _ in 0..200 {
        let _ = normalize(&conv);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_dedup_system() {
    use abp_ir::normalize::dedup_system;
    let mut msgs = vec![];
    for i in 0..50 {
        msgs.push(IrMessage::text(IrRole::System, format!("sys {i}")));
    }
    msgs.push(IrMessage::text(IrRole::User, "hello"));
    let conv = IrConversation::from_messages(msgs);
    let start = Instant::now();
    for _ in 0..2_000 {
        let _ = dedup_system(&conv);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_trim_text() {
    use abp_ir::normalize::trim_text;
    let conv = make_ir_conversation(50);
    let start = Instant::now();
    for _ in 0..2_000 {
        let _ = trim_text(&conv);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_merge_adjacent_text() {
    use abp_ir::normalize::merge_adjacent_text;
    let msgs: Vec<_> = (0..100)
        .map(|i| IrMessage::text(IrRole::User, format!("chunk {i}")))
        .collect();
    let conv = IrConversation::from_messages(msgs);
    let start = Instant::now();
    for _ in 0..500 {
        let _ = merge_adjacent_text(&conv);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_strip_empty() {
    use abp_ir::normalize::strip_empty;
    let mut msgs: Vec<IrMessage> = (0..100)
        .map(|i| IrMessage::text(IrRole::User, format!("msg {i}")))
        .collect();
    for i in (0..100).step_by(3) {
        msgs[i] = IrMessage::new(IrRole::User, vec![]);
    }
    let conv = IrConversation::from_messages(msgs);
    let start = Instant::now();
    for _ in 0..2_000 {
        let _ = strip_empty(&conv);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_strip_metadata() {
    use abp_ir::normalize::strip_metadata;
    let mut msgs = vec![];
    for i in 0..50 {
        let mut msg = IrMessage::text(IrRole::User, format!("msg {i}"));
        msg.metadata.insert("key".into(), json!("value"));
        msgs.push(msg);
    }
    let conv = IrConversation::from_messages(msgs);
    let start = Instant::now();
    for _ in 0..2_000 {
        let _ = strip_metadata(&conv, &[]);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_extract_system() {
    use abp_ir::normalize::extract_system;
    let conv = make_ir_conversation(50);
    let start = Instant::now();
    for _ in 0..2_000 {
        let _ = extract_system(&conv);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_sort_tools() {
    use abp_ir::normalize::sort_tools;
    let tools = make_ir_tools(100);
    let start = Instant::now();
    for _ in 0..2_000 {
        let mut t = tools.clone();
        sort_tools(&mut t);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_lowering_openai() {
    use abp_ir::lower::lower_to_openai;
    let conv = make_ir_conversation(20);
    let tools = make_ir_tools(5);
    let start = Instant::now();
    for _ in 0..1_000 {
        let _ = lower_to_openai(&conv, &tools);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_lowering_claude() {
    use abp_ir::lower::lower_to_claude;
    let conv = make_ir_conversation(20);
    let tools = make_ir_tools(5);
    let start = Instant::now();
    for _ in 0..1_000 {
        let _ = lower_to_claude(&conv, &tools);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_lowering_gemini() {
    use abp_ir::lower::lower_to_gemini;
    let conv = make_ir_conversation(20);
    let tools = make_ir_tools(5);
    let start = Instant::now();
    for _ in 0..1_000 {
        let _ = lower_to_gemini(&conv, &tools);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_lowering_all_dialects() {
    use abp_ir::lower::{lower_to_claude, lower_to_gemini, lower_to_openai};
    let conv = make_ir_conversation(10);
    let tools = make_ir_tools(3);
    let start = Instant::now();
    for _ in 0..500 {
        let _ = lower_to_openai(&conv, &tools);
        let _ = lower_to_claude(&conv, &tools);
        let _ = lower_to_gemini(&conv, &tools);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 5: Mapper lookup speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_mapper_lookup_known_rules() {
    use abp_mapping::features;
    let reg = known_rules();
    let feature_names = [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ];
    let start = Instant::now();
    for _ in 0..5_000 {
        for d in Dialect::all() {
            for f in &feature_names {
                let _ = reg.lookup(*d, Dialect::OpenAi, f);
            }
        }
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_mapper_registry_insert_lookup() {
    let start = Instant::now();
    for _ in 0..200 {
        let mut reg = MappingRegistry::new();
        for i in 0..50 {
            reg.insert(MappingRule {
                source_dialect: Dialect::OpenAi,
                target_dialect: Dialect::Claude,
                feature: format!("feature_{i}"),
                fidelity: Fidelity::Lossless,
            });
        }
        for i in 0..50 {
            let _ = reg.lookup(Dialect::OpenAi, Dialect::Claude, &format!("feature_{i}"));
        }
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_mapper_validate_mapping() {
    use abp_mapping::features;
    let reg = known_rules();
    let feats: Vec<String> = [features::TOOL_USE, features::STREAMING, features::THINKING]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let start = Instant::now();
    for _ in 0..500 {
        let _ = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_mapper_rank_targets() {
    use abp_mapping::features;
    let reg = known_rules();
    let feat_refs = [features::TOOL_USE, features::STREAMING, features::THINKING];
    let start = Instant::now();
    for _ in 0..5_000 {
        let _ = reg.rank_targets(Dialect::OpenAi, &feat_refs);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_mapper_supported_ir_pairs() {
    let start = Instant::now();
    for _ in 0..10_000 {
        let pairs = supported_ir_pairs();
        assert!(!pairs.is_empty());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_mapper_default_ir_mapper_creation() {
    let pairs = supported_ir_pairs();
    let start = Instant::now();
    for _ in 0..500 {
        for (src, tgt) in &pairs {
            let _ = default_ir_mapper(*src, *tgt);
        }
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_mapper_identity_passthrough() {
    use abp_mapper::IdentityMapper;
    use abp_mapper::{DialectRequest, Mapper};
    let mapper = IdentityMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
    };
    let start = Instant::now();
    for _ in 0..10_000 {
        let _ = mapper.map_request(&req).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 6: Policy evaluation speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_policy_tool_check_10000() {
    let engine = make_policy_engine();
    let tools = [
        "Read", "Write", "Grep", "Bash", "BashExec", "Delete", "Unknown",
    ];
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = engine.can_use_tool(tools[i % tools.len()]);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_policy_read_path_check_10000() {
    let engine = make_policy_engine();
    let paths = [
        Path::new("src/lib.rs"),
        Path::new(".env"),
        Path::new("secret/key.pem"),
        Path::new("docs/readme.md"),
        Path::new("src/deep/nested/file.rs"),
    ];
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = engine.can_read_path(paths[i % paths.len()]);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_policy_write_path_check_10000() {
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
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_policy_mixed_checks_10000() {
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
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_policy_compile_100() {
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
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_policy_empty_profile_fast() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    let start = Instant::now();
    for _ in 0..10_000 {
        assert!(engine.can_use_tool("anything").allowed);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_policy_complex_glob_patterns() {
    let policy = PolicyProfile {
        allowed_tools: (0..20).map(|i| format!("tool_{i}*")).collect(),
        disallowed_tools: (0..10).map(|i| format!("bad_{i}*")).collect(),
        deny_read: (0..20).map(|i| format!("secret_{i}/**")).collect(),
        deny_write: (0..20).map(|i| format!("locked_{i}/**")).collect(),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let start = Instant::now();
    for i in 0..5_000 {
        let _ = engine.can_use_tool(&format!("tool_{}", i % 30));
        let _ = engine.can_read_path(Path::new(&format!("secret_{}/file.txt", i % 25)));
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 7: BTreeMap O(n²) guard
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_btreemap_insert_10000_no_quadratic() {
    let start = Instant::now();
    let mut map = BTreeMap::new();
    for i in 0..10_000 {
        map.insert(format!("key_{i:05}"), format!("value_{i}"));
    }
    assert_eq!(map.len(), 10_000);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_btreemap_lookup_10000_no_quadratic() {
    let map: BTreeMap<String, String> = (0..10_000)
        .map(|i| (format!("key_{i:05}"), format!("value_{i}")))
        .collect();
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = map.get(&format!("key_{i:05}"));
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_btreemap_serialize_10000() {
    let map: BTreeMap<String, String> = (0..10_000)
        .map(|i| (format!("key_{i:05}"), format!("value_{i}")))
        .collect();
    let start = Instant::now();
    let json = serde_json::to_string(&map).unwrap();
    assert!(json.len() > 100_000);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_btreemap_iteration_no_quadratic() {
    let map: BTreeMap<String, String> = (0..10_000)
        .map(|i| (format!("key_{i:05}"), format!("value_{i}")))
        .collect();
    let start = Instant::now();
    let mut count = 0;
    for (k, v) in &map {
        count += k.len() + v.len();
    }
    assert!(count > 0);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_btreemap_ordered_keys_deterministic_serialization() {
    let map: BTreeMap<String, String> = (0..1_000)
        .map(|i| (format!("key_{i:05}"), format!("val_{i}")))
        .collect();
    let start = Instant::now();
    let json1 = serde_json::to_string(&map).unwrap();
    for _ in 0..100 {
        let json2 = serde_json::to_string(&map).unwrap();
        assert_eq!(json1, json2);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 8: Stream processing throughput
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_stream_buffer_push_drain() {
    let mut buffer = StreamBuffer::new(1000);
    let start = Instant::now();
    for i in 0..5_000 {
        buffer.push(make_event(i));
        if buffer.is_full() {
            let _ = buffer.drain();
        }
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_filter_10000() {
    let filter = EventFilter::by_kind("assistant_message");
    let events: Vec<_> = (0..10_000).map(|i| make_event(i)).collect();
    let start = Instant::now();
    let matched: usize = events.iter().filter(|e| filter.matches(e)).count();
    assert!(matched > 0);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_filter_errors_only() {
    let filter = EventFilter::errors_only();
    let events: Vec<_> = (0..10_000).map(|i| make_event(i)).collect();
    let start = Instant::now();
    for e in &events {
        let _ = filter.matches(e);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_recorder_10000() {
    let recorder = EventRecorder::new();
    let start = Instant::now();
    for i in 0..10_000 {
        recorder.record(&make_event(i));
    }
    let events = recorder.events();
    assert_eq!(events.len(), 10_000);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_stats_10000() {
    let stats = EventStats::new();
    let start = Instant::now();
    for i in 0..10_000 {
        stats.observe(&make_event(i));
    }
    assert_eq!(stats.total_events(), 10_000);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_transform_identity_10000() {
    let transform = EventTransform::identity();
    let events: Vec<_> = (0..10_000).map(|i| make_event(i)).collect();
    let start = Instant::now();
    for e in events {
        let _ = transform.apply(e);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_aggregator_500_events() {
    let mut agg = StreamAggregator::new();
    let start = Instant::now();
    for i in 0..500 {
        agg.push(&make_event(i));
    }
    let summary = agg.to_summary();
    assert!(summary.total_events >= 500);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_metrics_recording() {
    let mut metrics = StreamMetrics::new();
    let start = Instant::now();
    for i in 0..10_000 {
        metrics.record_event(&make_event(i));
    }
    let summary = metrics.summary();
    assert_eq!(summary.event_count, 10_000);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_event_kind_name_10000() {
    let events: Vec<_> = (0..10_000).map(|i| make_event(i)).collect();
    let start = Instant::now();
    for e in &events {
        let name = event_kind_name(&e.kind);
        assert!(!name.is_empty());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_pipeline_build_process() {
    let start = Instant::now();
    for _ in 0..500 {
        let pipeline = StreamPipelineBuilder::new()
            .filter(EventFilter::exclude_errors())
            .record()
            .build();
        for j in 0..20 {
            pipeline.process(make_event(j));
        }
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 9: Memory allocation patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mem_work_order_create_drop_10000() {
    for i in 0..10_000 {
        let wo = make_work_order(i);
        let json = serde_json::to_string(&wo).unwrap();
        let _: WorkOrder = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn mem_receipt_create_hash_drop_5000() {
    for i in 0..5_000 {
        let r = make_receipt(i);
        let _ = r.with_hash().unwrap();
    }
}

#[test]
fn mem_policy_engine_create_drop_1000() {
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
fn mem_glob_create_match_drop_1000() {
    for _ in 0..1_000 {
        let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["target/**".into()]).unwrap();
        let _ = globs.decide_str("src/lib.rs");
        let _ = globs.decide_str("target/debug/bin");
    }
}

#[test]
fn mem_event_creation_loop_50000() {
    for i in 0..50_000 {
        let _ = make_event(i);
    }
}

#[test]
fn mem_ir_conversation_create_drop() {
    for _ in 0..1_000 {
        let conv = make_ir_conversation(100);
        assert!(!conv.is_empty());
    }
}

#[test]
fn mem_stream_recorder_clear_cycle() {
    let recorder = EventRecorder::new();
    for cycle in 0..100 {
        for j in 0..100 {
            recorder.record(&make_event(cycle * 100 + j));
        }
        recorder.clear();
        assert!(recorder.is_empty());
    }
}

#[test]
fn mem_mapping_registry_rebuild_cycle() {
    for _ in 0..100 {
        let mut reg = MappingRegistry::new();
        for i in 0..50 {
            reg.insert(MappingRule {
                source_dialect: Dialect::OpenAi,
                target_dialect: Dialect::Claude,
                feature: format!("f_{i}"),
                fidelity: Fidelity::Lossless,
            });
        }
        assert_eq!(reg.len(), 50);
    }
}

#[test]
fn mem_error_on_invalid_glob_no_leak() {
    for _ in 0..5_000 {
        let result = IncludeExcludeGlobs::new(&["[invalid".into()], &[]);
        assert!(result.is_err());
    }
}

#[test]
fn mem_jsonl_decode_error_no_leak() {
    for _ in 0..10_000 {
        let result = JsonlCodec::decode("not valid json at all");
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 10: Dialect detection speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_dialect_detect_openai() {
    let detector = DialectDetector::new();
    let val = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}],
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "temperature": 0.7
    });
    let start = Instant::now();
    for _ in 0..10_000 {
        assert!(detector.detect(&val).is_some());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_dialect_detect_claude() {
    let detector = DialectDetector::new();
    let val = json!({
        "type": "message",
        "model": "claude-3",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}],
        "stop_reason": "end_turn"
    });
    let start = Instant::now();
    for _ in 0..10_000 {
        assert!(detector.detect(&val).is_some());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_dialect_detect_gemini() {
    let detector = DialectDetector::new();
    let val = json!({
        "contents": [{"parts": [{"text": "hello"}]}],
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}],
        "generationConfig": {"temperature": 0.5}
    });
    let start = Instant::now();
    for _ in 0..10_000 {
        assert!(detector.detect(&val).is_some());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_dialect_detect_all_variants() {
    let detector = DialectDetector::new();
    let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let start = Instant::now();
    for _ in 0..5_000 {
        let _ = detector.detect_all(&val);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 11: ErrorCode operations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_error_code_as_str_all_variants() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];
    let start = Instant::now();
    for _ in 0..10_000 {
        for code in &codes {
            let s = code.as_str();
            assert!(s.contains('_') || s == "internal");
        }
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_error_code_category_10000() {
    let codes = [
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::IrInvalid,
        ErrorCode::Internal,
    ];
    let start = Instant::now();
    for i in 0..10_000 {
        let cat = codes[i % codes.len()].category();
        let _ = cat.to_string();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_error_code_is_retryable_10000() {
    let codes = [
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
    ];
    let start = Instant::now();
    for i in 0..10_000 {
        let _ = codes[i % codes.len()].is_retryable();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_abp_error_creation_10000() {
    let start = Instant::now();
    for i in 0..10_000 {
        let err = AbpError::new(ErrorCode::BackendTimeout, format!("timeout {i}"))
            .with_context("attempt", i);
        let _ = err.category();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 12: Glob matching throughput
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_glob_decide_str_10000() {
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
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_glob_compile_many_patterns() {
    let includes: Vec<String> = (0..50).map(|i| format!("dir{i}/**")).collect();
    let excludes: Vec<String> = (0..50).map(|i| format!("dir{i}/tmp/**")).collect();
    let start = Instant::now();
    for _ in 0..50 {
        let _ = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_glob_deep_paths() {
    let globs = make_globs();
    let deep = "src/a/b/c/d/e/f/g/h/i/j/k/l/m/n.rs";
    let start = Instant::now();
    for _ in 0..10_000 {
        let _ = globs.decide_str(deep);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_glob_empty_rules_fast() {
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    let start = Instant::now();
    for _ in 0..10_000 {
        assert!(globs.decide_str("any/path.txt").is_allowed());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 13: Receipt diff and chain speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_receipt_diff_1000_pairs() {
    let r1 = make_receipt(0);
    let r2 = make_receipt(1);
    let start = Instant::now();
    for _ in 0..1_000 {
        let diff = diff_receipts(&r1, &r2);
        let _ = diff.len();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_receipt_diff_identical() {
    let r = make_receipt(42);
    let start = Instant::now();
    for _ in 0..2_000 {
        let diff = diff_receipts(&r, &r);
        assert!(diff.is_empty());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 14: Version parsing speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_version_parsing_10000() {
    let start = Instant::now();
    for _ in 0..10_000 {
        let parsed = parse_version("abp/v0.1");
        assert_eq!(parsed, Some((0, 1)));
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_version_parsing_invalid() {
    let start = Instant::now();
    for _ in 0..10_000 {
        assert!(parse_version("not-a-version").is_none());
        assert!(parse_version("").is_none());
        assert!(parse_version("abp/v").is_none());
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 15: Builder speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_work_order_builder_1000() {
    let start = Instant::now();
    for i in 0..1_000 {
        let _ = WorkOrderBuilder::new(format!("task-{i}"))
            .lane(ExecutionLane::WorkspaceFirst)
            .root("/tmp")
            .model("gpt-4")
            .max_turns(5)
            .max_budget_usd(2.0)
            .build();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_receipt_builder_1000() {
    let start = Instant::now();
    for i in 0..1_000 {
        let _ = ReceiptBuilder::new(format!("mock-{i}"))
            .outcome(Outcome::Complete)
            .mode(ExecutionMode::Mapped)
            .backend_version("1.0.0")
            .build();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_receipt_builder_with_artifacts() {
    let start = Instant::now();
    for i in 0..200 {
        let mut builder = ReceiptBuilder::new(format!("mock-{i}")).outcome(Outcome::Complete);
        for j in 0..50 {
            builder = builder.add_artifact(ArtifactRef {
                kind: "patch".into(),
                path: format!("patches/p{j}.diff"),
            });
        }
        let _ = builder.build();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 16: Serialization roundtrip speed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_work_order_serde_roundtrip_1000() {
    let start = Instant::now();
    for i in 0..1_000 {
        let wo = make_work_order(i);
        let json = serde_json::to_string(&wo).unwrap();
        let _: WorkOrder = serde_json::from_str(&json).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_receipt_serde_roundtrip_1000() {
    let start = Instant::now();
    for i in 0..1_000 {
        let r = make_receipt(i);
        let json = serde_json::to_string(&r).unwrap();
        let _: Receipt = serde_json::from_str(&json).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_event_serde_roundtrip_5000() {
    let start = Instant::now();
    for i in 0..5_000 {
        let e = make_event(i);
        let json = serde_json::to_string(&e).unwrap();
        let _: AgentEvent = serde_json::from_str(&json).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_large_payload_serialize() {
    let big = "x".repeat(1_000_000);
    let wo = WorkOrderBuilder::new(big).build();
    let start = Instant::now();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.len() > 1_000_000);
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 17: Combined pipeline simulations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn perf_full_pipeline_simulation() {
    let engine = make_policy_engine();
    let detector = DialectDetector::new();
    let globs = make_globs();
    let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let start = Instant::now();
    for i in 0..500 {
        let wo = make_work_order(i);
        let _ = serde_json::to_string(&wo).unwrap();
        let _ = engine.can_use_tool("Read");
        let _ = globs.decide_str("src/lib.rs");
        let _ = detector.detect(&val);
        let r = make_receipt(i);
        let _ = receipt_hash(&r).unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_interleaved_subsystems() {
    let engine = make_policy_engine();
    let globs = make_globs();
    let reg = known_rules();
    let start = Instant::now();
    for i in 0..1_000 {
        let wo = make_work_order(i);
        let _ = canonical_json(&wo).unwrap();
        let _ = engine.can_use_tool("Read");
        let _ = engine.can_read_path(Path::new("src/lib.rs"));
        let _ = globs.decide_str("src/lib.rs");
        let _ = reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use");
        let r = make_receipt(i);
        let _ = r.with_hash().unwrap();
    }
    assert!(
        start.elapsed().as_secs_f64() < 3.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_ir_to_lowering_pipeline() {
    use abp_ir::lower::{lower_to_claude, lower_to_gemini, lower_to_openai};
    use abp_ir::normalize::normalize;
    let conv = make_ir_conversation(20);
    let tools = make_ir_tools(5);
    let start = Instant::now();
    for _ in 0..200 {
        let normed = normalize(&conv);
        let _ = lower_to_openai(&normed, &tools);
        let _ = lower_to_claude(&normed, &tools);
        let _ = lower_to_gemini(&normed, &tools);
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn perf_stream_pipeline_full_cycle() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .record()
        .build();
    let start = Instant::now();
    for i in 0..5_000 {
        pipeline.process(make_event(i));
    }
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "took {:?}",
        start.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Category 18: Correctness-under-speed sanity checks
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sanity_contract_version() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn sanity_error_code_as_str_is_snake_case() {
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(ErrorCode::IrInvalid.as_str(), "ir_invalid");
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
}

#[test]
fn sanity_receipt_hash_excludes_sha_field() {
    let r1 = make_receipt(0);
    let h1 = receipt_hash(&r1).unwrap();
    let r2 = r1.with_hash().unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2, "hash must be self-referential-proof");
}

#[test]
fn sanity_canonical_json_ordered_keys() {
    let m: BTreeMap<String, i32> = [("z", 1), ("a", 2), ("m", 3)]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let json = canonical_json(&m).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let keys: Vec<_> = parsed.as_object().unwrap().keys().collect();
    assert_eq!(keys, vec!["a", "m", "z"]);
}

#[test]
fn sanity_policy_denies_disallowed() {
    let engine = make_policy_engine();
    assert!(!engine.can_use_tool("BashExec").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn sanity_ir_conversation_methods() {
    let conv = make_ir_conversation(10);
    assert_eq!(conv.len(), 10);
    assert!(!conv.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn sanity_dialect_all_has_entries() {
    assert!(Dialect::all().len() >= 5);
}

#[test]
fn sanity_glob_allowed_and_denied() {
    let globs = make_globs();
    assert!(globs.decide_str("src/lib.rs").is_allowed());
    assert!(!globs.decide_str("target/debug/bin").is_allowed());
}

#[test]
fn sanity_envelope_roundtrip() {
    let env = Envelope::Event {
        ref_id: "test-ref".into(),
        event: make_event(0),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(&encoded).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "test-ref"),
        _ => panic!("expected Event envelope"),
    }
}

#[test]
fn sanity_verify_hash_true_after_with_hash() {
    let r = make_receipt(0).with_hash().unwrap();
    assert!(verify_hash(&r));
}
