#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stress and performance tests for the Agent Backplane.
//!
//! Exercises high-volume, large-payload, and concurrent scenarios across
//! core contract types, IR, policy, protocol, receipt store, mapper,
//! config, and stream processing.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use abp_capability::{generate_report, negotiate};
use abp_config::{
    BackendEntry as ConfigBackendEntry, BackplaneConfig, merge_configs, parse_toml, validate_config,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder as CoreReceiptBuilder,
    SupportLevel, WorkOrderBuilder, WorkspaceMode, canonical_json, receipt_hash,
};
use abp_dialect::Dialect;
use abp_glob::IncludeExcludeGlobs;
use abp_mapper::{IrMapper, default_ir_mapper, supported_ir_pairs};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{ReceiptBuilder, ReceiptChain, compute_hash, diff_receipts, verify_hash};
use abp_receipt_store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_stream::{EventFilter, EventRecorder, EventStats, EventTransform, StreamPipelineBuilder};
use tokio::task::JoinSet;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_delta(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantDelta {
        text: text.to_string(),
    })
}

fn make_tool_call(name: &str, idx: usize) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(format!("tc-{idx}")),
        parent_tool_use_id: None,
        input: json!({"arg": idx}),
    })
}

fn make_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .build()
}

fn make_hashed_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap()
}

fn full_capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Native);
    m.insert(Capability::ToolBash, SupportLevel::Native);
    m.insert(Capability::ToolGlob, SupportLevel::Emulated);
    m.insert(Capability::ToolGrep, SupportLevel::Emulated);
    m
}

fn build_large_ir_conversation(n: usize) -> IrConversation {
    let mut msgs = Vec::with_capacity(n);
    for i in 0..n {
        let role = match i % 3 {
            0 => IrRole::User,
            1 => IrRole::Assistant,
            _ => IrRole::Tool,
        };
        msgs.push(IrMessage::text(role, format!("message-{i}")));
    }
    IrConversation::from_messages(msgs)
}

// =========================================================================
// 1. Large conversation IR (1000+ messages)
// =========================================================================

#[test]
fn stress_ir_conversation_1000_messages() {
    let conv = build_large_ir_conversation(1000);
    assert_eq!(conv.len(), 1000);
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_some());
}

#[test]
fn stress_ir_conversation_5000_messages() {
    let conv = build_large_ir_conversation(5000);
    assert_eq!(conv.len(), 5000);
    let user_msgs = conv.messages_by_role(IrRole::User);
    assert!(user_msgs.len() > 1600);
}

#[test]
fn stress_ir_conversation_serialize_1000() {
    let conv = build_large_ir_conversation(1000);
    let json = serde_json::to_string(&conv).unwrap();
    assert!(json.len() > 10_000);
    let roundtrip: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.len(), 1000);
}

#[test]
fn stress_ir_conversation_10000_messages() {
    let conv = build_large_ir_conversation(10_000);
    assert_eq!(conv.len(), 10_000);
    let tools = conv.messages_by_role(IrRole::Tool);
    assert!(tools.len() > 3000);
}

#[test]
fn stress_ir_conversation_with_tool_use_blocks() {
    let mut msgs = Vec::new();
    for i in 0..2000 {
        let block = IrContentBlock::ToolUse {
            id: format!("tu-{i}"),
            name: format!("tool_{}", i % 10),
            input: json!({"index": i}),
        };
        msgs.push(IrMessage::new(IrRole::Assistant, vec![block]));
    }
    let conv = IrConversation::from_messages(msgs);
    assert_eq!(conv.tool_calls().len(), 2000);
}

#[test]
fn stress_ir_conversation_text_content_extraction() {
    let conv = build_large_ir_conversation(1500);
    for msg in &conv.messages {
        let _ = msg.text_content();
    }
    let last = conv.last_message().unwrap();
    assert!(!last.text_content().is_empty());
}

#[test]
fn stress_ir_conversation_push_chain() {
    let mut conv = IrConversation::new();
    for i in 0..1000 {
        conv = conv.push(IrMessage::text(IrRole::User, format!("msg-{i}")));
    }
    assert_eq!(conv.len(), 1000);
}

// =========================================================================
// 2. Large work order (many context items)
// =========================================================================

#[test]
fn stress_work_order_500_context_files() {
    let files: Vec<String> = (0..500).map(|i| format!("src/file_{i}.rs")).collect();
    let ctx = ContextPacket {
        files,
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("task with many files")
        .context(ctx)
        .build();
    assert_eq!(wo.context.files.len(), 500);
}

#[test]
fn stress_work_order_1000_snippets() {
    let snippets: Vec<ContextSnippet> = (0..1000)
        .map(|i| ContextSnippet {
            name: format!("snippet-{i}"),
            content: format!("content of snippet {i} with some text padding for size"),
        })
        .collect();
    let ctx = ContextPacket {
        files: vec![],
        snippets,
    };
    let wo = WorkOrderBuilder::new("task with many snippets")
        .context(ctx)
        .build();
    assert_eq!(wo.context.snippets.len(), 1000);
}

#[test]
fn stress_work_order_large_snippet_content() {
    let big = "x".repeat(100_000);
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "big".into(),
            content: big.clone(),
        }],
    };
    let wo = WorkOrderBuilder::new("task with big snippet")
        .context(ctx)
        .build();
    assert_eq!(wo.context.snippets[0].content.len(), 100_000);
}

#[test]
fn stress_work_order_serialize_large_context() {
    let snippets: Vec<ContextSnippet> = (0..500)
        .map(|i| ContextSnippet {
            name: format!("s-{i}"),
            content: "a".repeat(1000),
        })
        .collect();
    let ctx = ContextPacket {
        files: (0..200).map(|i| format!("f/{i}.rs")).collect(),
        snippets,
    };
    let wo = WorkOrderBuilder::new("big").context(ctx).build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.len() > 500_000);
    let rt: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.context.snippets.len(), 500);
}

#[test]
fn stress_work_order_many_include_exclude_globs() {
    let includes: Vec<String> = (0..100).map(|i| format!("src/mod_{i}/**")).collect();
    let excludes: Vec<String> = (0..100)
        .map(|i| format!("src/mod_{i}/generated/**"))
        .collect();
    let wo = WorkOrderBuilder::new("glob task")
        .include(includes)
        .exclude(excludes)
        .build();
    assert_eq!(wo.workspace.include.len(), 100);
    assert_eq!(wo.workspace.exclude.len(), 100);
}

#[test]
fn stress_work_order_deep_vendor_config() {
    let mut vendor = BTreeMap::new();
    for i in 0..200 {
        vendor.insert(
            format!("key_{i}"),
            json!({"nested": {"depth": i, "data": "x".repeat(100)}}),
        );
    }
    let mut config = abp_core::RuntimeConfig::default();
    config.vendor = vendor;
    let wo = WorkOrderBuilder::new("vendor config stress")
        .config(config)
        .build();
    assert_eq!(wo.config.vendor.len(), 200);
}

// =========================================================================
// 3. Receipt store with 10000+ receipts
// =========================================================================

#[tokio::test]
async fn stress_receipt_store_10000_receipts() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..10_000 {
        let r = make_receipt("bench-backend");
        store.store(&r).await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 10_000);
}

#[tokio::test]
async fn stress_receipt_store_15000_receipts() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..15_000 {
        let r = make_receipt("bulk");
        store.store(&r).await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 15_000);
}

#[tokio::test]
async fn stress_receipt_store_get_after_bulk_insert() {
    let store = InMemoryReceiptStore::new();
    let mut ids = Vec::new();
    for _ in 0..5000 {
        let r = make_receipt("lookup");
        ids.push(r.meta.run_id.to_string());
        store.store(&r).await.unwrap();
    }
    // Spot-check lookups
    for id in ids.iter().take(100) {
        let found = store.get(id).await.unwrap();
        assert!(found.is_some());
    }
}

#[tokio::test]
async fn stress_receipt_store_filter_by_outcome() {
    let store = InMemoryReceiptStore::new();
    for i in 0..2000 {
        let outcome = if i % 3 == 0 {
            Outcome::Failed
        } else {
            Outcome::Complete
        };
        let r = ReceiptBuilder::new("filter-test").outcome(outcome).build();
        store.store(&r).await.unwrap();
    }
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let failed = store.list(filter).await.unwrap();
    assert!(failed.len() >= 600);
}

#[tokio::test]
async fn stress_receipt_store_delete_half() {
    let store = InMemoryReceiptStore::new();
    let mut ids = Vec::new();
    for _ in 0..2000 {
        let r = make_receipt("del");
        ids.push(r.meta.run_id.to_string());
        store.store(&r).await.unwrap();
    }
    for id in ids.iter().take(1000) {
        store.delete(id).await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 1000);
}

#[tokio::test]
async fn stress_receipt_store_pagination() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..500 {
        store.store(&make_receipt("page")).await.unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(50),
        offset: Some(100),
        ..Default::default()
    };
    let page = store.list(filter).await.unwrap();
    assert!(page.len() <= 50);
}

// =========================================================================
// 4. Event stream with 50000+ events
// =========================================================================

#[test]
fn stress_event_recorder_50000_events() {
    let recorder = EventRecorder::new();
    for i in 0..50_000 {
        recorder.record(&make_delta(&format!("tok-{i}")));
    }
    assert_eq!(recorder.len(), 50_000);
}

#[test]
fn stress_event_stats_50000_events() {
    let stats = EventStats::new();
    for i in 0..50_000 {
        if i % 2 == 0 {
            stats.observe(&make_delta("d"));
        } else {
            stats.observe(&make_tool_call("tool", i));
        }
    }
    assert_eq!(stats.total_events(), 50_000);
}

#[test]
fn stress_event_filter_50000_events() {
    let filter = EventFilter::by_kind("assistant_delta");
    let events: Vec<AgentEvent> = (0..50_000)
        .map(|i| {
            if i % 3 == 0 {
                make_delta("d")
            } else {
                make_tool_call("t", i)
            }
        })
        .collect();
    let matched: usize = events.iter().filter(|e| filter.matches(e)).count();
    assert!(matched > 16_000);
}

#[test]
fn stress_event_transform_50000_events() {
    let transform = EventTransform::map_text(|s| s.to_uppercase());
    for i in 0..50_000 {
        let ev = make_delta(&format!("hello-{i}"));
        let transformed = transform.apply(ev);
        if let AgentEventKind::AssistantDelta { text } = &transformed.kind {
            assert!(text.starts_with("HELLO-"));
        }
    }
}

#[test]
fn stress_event_recorder_clear_cycles_large() {
    let recorder = EventRecorder::new();
    for cycle in 0..100 {
        for i in 0..500 {
            recorder.record(&make_delta(&format!("c{cycle}-{i}")));
        }
        recorder.clear();
        assert!(recorder.is_empty());
    }
}

#[test]
fn stress_event_stream_pipeline_50000() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::identity())
        .build();
    for i in 0..50_000 {
        let ev = make_delta(&format!("p-{i}"));
        let result = pipeline.process(ev);
        assert!(result.is_some());
    }
}

#[test]
fn stress_event_mixed_kinds_100000() {
    let stats = EventStats::new();
    for i in 0..100_000 {
        let ev = match i % 5 {
            0 => make_delta("d"),
            1 => make_tool_call("bash", i),
            2 => make_event(AgentEventKind::FileChanged {
                path: format!("src/{i}.rs"),
                summary: "changed".into(),
            }),
            3 => make_event(AgentEventKind::Warning {
                message: format!("warn-{i}"),
            }),
            _ => make_event(AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: Some(0),
                output_preview: None,
            }),
        };
        stats.observe(&ev);
    }
    assert_eq!(stats.total_events(), 100_000);
}

// =========================================================================
// 5. Policy evaluation on complex glob patterns
// =========================================================================

#[test]
fn stress_policy_500_disallowed_tools() {
    let tools: Vec<String> = (0..500).map(|i| format!("DangerousTool_{i}")).collect();
    let policy = PolicyProfile {
        disallowed_tools: tools,
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..500 {
        let d = engine.can_use_tool(&format!("DangerousTool_{i}"));
        assert!(!d.allowed);
    }
    assert!(engine.can_use_tool("SafeTool").allowed);
}

#[test]
fn stress_policy_500_deny_write_paths() {
    let paths: Vec<String> = (0..500).map(|i| format!("secrets/vault_{i}/**")).collect();
    let policy = PolicyProfile {
        deny_write: paths,
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..500 {
        let d = engine.can_write_path(Path::new(&format!("secrets/vault_{i}/key.pem")));
        assert!(!d.allowed);
    }
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn stress_policy_complex_read_patterns() {
    let deny_read: Vec<String> = (0..200).map(|i| format!("**/sensitive_{i}/**")).collect();
    let policy = PolicyProfile {
        deny_read,
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..200 {
        let d = engine.can_read_path(Path::new(&format!("data/sensitive_{i}/secret.txt")));
        assert!(!d.allowed);
    }
}

#[test]
fn stress_policy_mixed_allow_deny_500() {
    let allowed: Vec<String> = (0..250).map(|i| format!("Tool_{i}")).collect();
    let disallowed: Vec<String> = (200..250).map(|i| format!("Tool_{i}")).collect();
    let policy = PolicyProfile {
        allowed_tools: allowed,
        disallowed_tools: disallowed,
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Deny list wins for 200..250
    for i in 200..250 {
        assert!(!engine.can_use_tool(&format!("Tool_{i}")).allowed);
    }
    // Allowed for 0..200
    for i in 0..200 {
        assert!(engine.can_use_tool(&format!("Tool_{i}")).allowed);
    }
}

#[test]
fn stress_policy_eval_10000_checks() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash*".into(), "Exec*".into()],
        deny_read: vec!["**/.env*".into(), "**/secret/**".into()],
        deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..10_000 {
        let _ = engine.can_use_tool(&format!("Tool_{i}"));
        let _ = engine.can_read_path(Path::new(&format!("src/mod_{i}/file.rs")));
        let _ = engine.can_write_path(Path::new(&format!("data/out_{i}.json")));
    }
}

#[test]
fn stress_policy_wildcard_glob_patterns() {
    let patterns: Vec<String> = (0..300).map(|i| format!("**/dir_{i}/**/*.tmp")).collect();
    let policy = PolicyProfile {
        deny_write: patterns,
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    for i in 0..300 {
        let d = engine.can_write_path(Path::new(&format!("root/dir_{i}/sub/file.tmp")));
        assert!(!d.allowed);
    }
    assert!(
        engine
            .can_write_path(Path::new("root/other/file.rs"))
            .allowed
    );
}

// =========================================================================
// 6. Concurrent receipt hashing
// =========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_receipt_hashing_200() {
    let mut set = JoinSet::new();
    for i in 0..200 {
        set.spawn(async move {
            let r = ReceiptBuilder::new(format!("backend-{i}"))
                .outcome(Outcome::Complete)
                .build();
            let h = compute_hash(&r).unwrap();
            assert_eq!(h.len(), 64);
            h
        });
    }
    let mut hashes = Vec::new();
    while let Some(result) = set.join_next().await {
        hashes.push(result.unwrap());
    }
    assert_eq!(hashes.len(), 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_receipt_hashing_1000() {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut set = JoinSet::new();
    for _ in 0..1000 {
        let counter = counter.clone();
        set.spawn(async move {
            let r = make_receipt("concurrent");
            let _ = compute_hash(&r).unwrap();
            counter.fetch_add(1, Ordering::Relaxed);
        });
    }
    while let Some(result) = set.join_next().await {
        result.unwrap();
    }
    assert_eq!(counter.load(Ordering::Relaxed), 1000);
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_verify_hash_500() {
    let mut set = JoinSet::new();
    for _ in 0..500 {
        set.spawn(async move {
            let r = make_hashed_receipt("verify");
            assert!(verify_hash(&r));
        });
    }
    while let Some(result) = set.join_next().await {
        result.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_receipt_store_insert() {
    let store = Arc::new(InMemoryReceiptStore::new());
    let mut set = JoinSet::new();
    for _ in 0..5000 {
        let store = store.clone();
        set.spawn(async move {
            let r = make_receipt("concurrent-store");
            store.store(&r).await.unwrap();
        });
    }
    while let Some(result) = set.join_next().await {
        result.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 5000);
}

// =========================================================================
// 7. Large JSONL parsing
// =========================================================================

#[test]
fn stress_jsonl_encode_decode_5000_envelopes() {
    let mut buffer = Vec::new();
    for i in 0..5000 {
        let env = Envelope::Fatal {
            ref_id: Some(format!("run-{i}")),
            error: format!("error-{i}"),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        buffer.extend_from_slice(line.as_bytes());
    }
    let reader = BufReader::new(buffer.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 5000);
}

#[test]
fn stress_jsonl_hello_envelopes_1000() {
    for i in 0..1000 {
        let env = Envelope::hello(
            BackendIdentity {
                id: format!("sidecar-{i}"),
                backend_version: Some("1.0".into()),
                adapter_version: None,
            },
            full_capability_manifest(),
        );
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }
}

#[test]
fn stress_jsonl_event_envelopes_10000() {
    let mut buffer = Vec::new();
    for i in 0..10_000 {
        let env = Envelope::Event {
            ref_id: format!("run-{}", i / 100),
            event: make_delta(&format!("token-{i}")),
        };
        let line = JsonlCodec::encode(&env).unwrap();
        buffer.extend_from_slice(line.as_bytes());
    }
    let reader = BufReader::new(buffer.as_slice());
    let count = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .len();
    assert_eq!(count, 10_000);
}

#[test]
fn stress_jsonl_run_envelopes_1000() {
    for i in 0..1000 {
        let wo = WorkOrderBuilder::new(format!("task-{i}")).build();
        let env = Envelope::Run {
            id: format!("run-{i}"),
            work_order: wo,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Run { .. }));
    }
}

#[test]
fn stress_jsonl_large_payload_roundtrip() {
    let big_text = "x".repeat(50_000);
    let ev = make_event(AgentEventKind::AssistantMessage { text: big_text });
    let env = Envelope::Event {
        ref_id: "big-run".into(),
        event: ev,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.len() > 50_000);
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text.len(), 50_000);
        } else {
            panic!("unexpected event kind");
        }
    } else {
        panic!("unexpected envelope type");
    }
}

// =========================================================================
// 8. Mapper with all dialect pairs
// =========================================================================

#[test]
fn stress_mapper_all_supported_pairs() {
    let pairs = supported_ir_pairs();
    assert!(pairs.len() >= 24); // 6 identity + 18 cross
    for (from, to) in &pairs {
        let mapper = default_ir_mapper(*from, *to);
        assert!(mapper.is_some(), "no mapper for {:?} -> {:?}", from, to);
    }
}

#[test]
fn stress_mapper_identity_1000_conversations() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    for i in 0..1000 {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are a helper."))
            .push(IrMessage::text(IrRole::User, format!("Question {i}")));
        let result = mapper.map_request(Dialect::OpenAi, Dialect::OpenAi, &conv);
        assert!(result.is_ok());
    }
}

#[test]
fn stress_mapper_cross_dialect_all_pairs_simple_conversation() {
    let pairs = supported_ir_pairs();
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "System prompt."))
        .push(IrMessage::text(IrRole::User, "Hello, how are you?"))
        .push(IrMessage::text(IrRole::Assistant, "I am fine."));
    for (from, to) in &pairs {
        let mapper = default_ir_mapper(*from, *to).unwrap();
        let result = mapper.map_request(*from, *to, &conv);
        assert!(
            result.is_ok(),
            "map_request failed for {:?}->{:?}: {:?}",
            from,
            to,
            result.err()
        );
    }
}

#[test]
fn stress_mapper_large_conversation_all_pairs() {
    let pairs = supported_ir_pairs();
    let conv = build_large_ir_conversation(100);
    for (from, to) in &pairs {
        let mapper = default_ir_mapper(*from, *to).unwrap();
        let _ = mapper.map_request(*from, *to, &conv);
    }
}

#[test]
fn stress_mapper_response_mapping_all_pairs() {
    let pairs = supported_ir_pairs();
    let conv =
        IrConversation::new().push(IrMessage::text(IrRole::Assistant, "Here is the answer."));
    for (from, to) in &pairs {
        let mapper = default_ir_mapper(*from, *to).unwrap();
        let _ = mapper.map_response(*from, *to, &conv);
    }
}

#[test]
fn stress_mapper_repeated_mapping_1000() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there"));
    for _ in 0..1000 {
        let _ = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &conv);
    }
}

// =========================================================================
// 9. Large config files
// =========================================================================

#[test]
fn stress_config_100_backends() {
    let mut backends = BTreeMap::new();
    for i in 0..100 {
        backends.insert(
            format!("backend_{i}"),
            ConfigBackendEntry::Sidecar {
                command: format!("node hosts/sidecar_{i}/index.js"),
                args: vec!["--port".into(), format!("{}", 3000 + i)],
                timeout_secs: Some(300),
            },
        );
    }
    let config = BackplaneConfig {
        backends,
        ..BackplaneConfig::default()
    };
    let warnings = validate_config(&config).unwrap();
    // Should not error, may have warnings
    let _ = warnings;
    assert_eq!(config.backends.len(), 100);
}

#[test]
fn stress_config_serialize_roundtrip_large() {
    let mut backends = BTreeMap::new();
    for i in 0..50 {
        backends.insert(
            format!("be_{i}"),
            ConfigBackendEntry::Sidecar {
                command: format!("python hosts/py_{i}/main.py"),
                args: vec![],
                timeout_secs: Some(600),
            },
        );
    }
    let config = BackplaneConfig {
        default_backend: Some("be_0".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        backends,
        ..BackplaneConfig::default()
    };
    let toml_str = toml::to_string(&config).unwrap();
    let parsed = parse_toml(&toml_str).unwrap();
    assert_eq!(parsed.backends.len(), 50);
}

#[test]
fn stress_config_merge_many_overlays() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..BackplaneConfig::default()
    };
    let mut current = base;
    for i in 0..50 {
        let overlay = BackplaneConfig {
            log_level: Some(if i % 2 == 0 {
                "debug".into()
            } else {
                "info".into()
            }),
            ..BackplaneConfig::default()
        };
        current = merge_configs(current, overlay);
    }
    assert!(current.log_level.is_some());
}

#[test]
fn stress_config_parse_toml_repeated() {
    let toml_str = r#"
default_backend = "mock"
log_level = "info"
workspace_dir = "/tmp/ws"

[backends.mock]
type = "mock"

[backends.node]
type = "sidecar"
command = "node hosts/node/index.js"
"#;
    for _ in 0..1000 {
        let config = parse_toml(toml_str).unwrap();
        assert_eq!(config.backends.len(), 2);
    }
}

// =========================================================================
// 10. Memory usage patterns
// =========================================================================

#[test]
fn stress_receipt_chain_500() {
    let mut chain = ReceiptChain::new();
    for _ in 0..500 {
        let r = make_hashed_receipt("chain");
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 500);
}

#[test]
fn stress_receipt_diff_many_pairs() {
    let receipts: Vec<Receipt> = (0..200).map(|_| make_receipt("diff")).collect();
    for i in 0..199 {
        let diff = diff_receipts(&receipts[i], &receipts[i + 1]);
        // Different run IDs should produce diffs
        assert!(!diff.changes.is_empty());
    }
}

#[test]
fn stress_canonical_json_1000_receipts() {
    for _ in 0..1000 {
        let r = make_receipt("canon");
        let json = canonical_json(&r).unwrap();
        assert!(!json.is_empty());
    }
}

#[test]
fn stress_receipt_with_large_trace() {
    let events: Vec<AgentEvent> = (0..5000).map(|i| make_delta(&format!("t-{i}"))).collect();
    let r = ReceiptBuilder::new("big-trace")
        .outcome(Outcome::Complete)
        .events(events)
        .build();
    assert_eq!(r.trace.len(), 5000);
    let hash = compute_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn stress_work_order_creation_and_drop_10000() {
    for i in 0..10_000 {
        let wo = WorkOrderBuilder::new(format!("ephemeral-{i}")).build();
        assert!(!wo.task.is_empty());
    }
}

#[test]
fn stress_receipt_serde_roundtrip_1000() {
    for _ in 0..1000 {
        let r = make_hashed_receipt("serde");
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.receipt_sha256, r.receipt_sha256);
    }
}

#[test]
fn stress_glob_compile_500_patterns() {
    let includes: Vec<String> = (0..500).map(|i| format!("src/mod_{i}/**/*.rs")).collect();
    let excludes: Vec<String> = (0..500).map(|i| format!("src/mod_{i}/test_*")).collect();
    let globs = IncludeExcludeGlobs::new(&includes, &excludes).unwrap();
    for i in 0..500 {
        assert!(
            globs
                .decide_str(&format!("src/mod_{i}/lib.rs"))
                .is_allowed()
        );
    }
}

#[test]
fn stress_glob_eval_10000_paths() {
    let globs = IncludeExcludeGlobs::new(
        &["src/**".into(), "tests/**".into()],
        &["**/generated/**".into()],
    )
    .unwrap();
    for i in 0..10_000 {
        let _ = globs.decide_str(&format!("src/module_{}/file_{}.rs", i / 100, i % 100));
    }
}

#[test]
fn stress_ir_usage_merge_many() {
    let mut usage = IrUsage::from_io(0, 0);
    for i in 0..10_000 {
        let delta = IrUsage::from_io(i as u64, (i * 2) as u64);
        usage = usage.merge(delta);
    }
    assert!(usage.input_tokens > 0);
    assert!(usage.total_tokens > 0);
}

#[test]
fn stress_ir_tool_definitions_1000() {
    let tools: Vec<IrToolDefinition> = (0..1000)
        .map(|i| IrToolDefinition {
            name: format!("tool_{i}"),
            description: format!("Tool number {i} for testing"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "arg": {"type": "string"}
                }
            }),
        })
        .collect();
    let json = serde_json::to_string(&tools).unwrap();
    let rt: Vec<IrToolDefinition> = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.len(), 1000);
}

#[test]
fn stress_envelope_serde_roundtrip_2000() {
    for i in 0..2000 {
        let env = match i % 4 {
            0 => Envelope::hello(
                BackendIdentity {
                    id: format!("be-{i}"),
                    backend_version: None,
                    adapter_version: None,
                },
                CapabilityManifest::new(),
            ),
            1 => Envelope::Run {
                id: format!("run-{i}"),
                work_order: WorkOrderBuilder::new(format!("t-{i}")).build(),
            },
            2 => Envelope::Event {
                ref_id: format!("run-{i}"),
                event: make_delta("tok"),
            },
            _ => Envelope::Fatal {
                ref_id: Some(format!("run-{i}")),
                error: "oops".into(),
                error_code: None,
            },
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let _ = JsonlCodec::decode(line.trim()).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_concurrent_receipt_store_mixed_ops() {
    let store = Arc::new(InMemoryReceiptStore::new());
    let mut set = JoinSet::new();
    // Insert 2000 receipts concurrently
    for _ in 0..2000 {
        let store = store.clone();
        set.spawn(async move {
            let r = make_receipt("mixed");
            let id = r.meta.run_id.to_string();
            store.store(&r).await.unwrap();
            id
        });
    }
    let mut ids = Vec::new();
    while let Some(result) = set.join_next().await {
        ids.push(result.unwrap());
    }
    // Concurrent reads
    let mut set2 = JoinSet::new();
    for id in ids.iter().take(500).cloned() {
        let store = store.clone();
        set2.spawn(async move {
            let found = store.get(&id).await.unwrap();
            assert!(found.is_some());
        });
    }
    while let Some(result) = set2.join_next().await {
        result.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 2000);
}

#[test]
fn stress_hash_determinism_same_receipt() {
    let r = make_receipt("deterministic");
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    let h3 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn stress_receipt_builder_many_events() {
    let mut builder = ReceiptBuilder::new("builder-stress").outcome(Outcome::Complete);
    for i in 0..5000 {
        builder = builder.add_event(make_delta(&format!("ev-{i}")));
    }
    let r = builder.build();
    assert_eq!(r.trace.len(), 5000);
}
