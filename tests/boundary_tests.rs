// SPDX-License-Identifier: MIT OR Apache-2.0
//! Boundary and edge-case tests for ABP contract types, protocol, policy, and globs.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, MinSupport, Outcome, PolicyProfile, ReceiptBuilder,
    RuntimeConfig, WorkOrderBuilder,
};
use abp_glob::IncludeExcludeGlobs;
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::json;

// ── helpers ──────────────────────────────────────────────────────────

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_at(ts: DateTime<Utc>, kind: AgentEventKind) -> AgentEvent {
    AgentEvent { ts, kind, ext: None }
}

// ── 1. Empty string task ─────────────────────────────────────────────

#[test]
fn empty_string_task_builds_and_serialises() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.task, "");
}

// ── 2. 1 MB task string ─────────────────────────────────────────────

#[test]
fn one_mb_task_string_does_not_oom() {
    let big = "x".repeat(1_000_000);
    let wo = WorkOrderBuilder::new(&big).build();
    assert_eq!(wo.task.len(), 1_000_000);
    // Ensure it can be serialised and deserialised.
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.task.len(), 1_000_000);
}

// ── 3. Zero-length work order ID ────────────────────────────────────

#[test]
fn zero_length_work_order_id_in_receipt() {
    // A nil UUID is accepted by the builder.
    let receipt = ReceiptBuilder::new("mock")
        .work_order_id(uuid::Uuid::nil())
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

// ── 4. Work order with 1000 context entries ─────────────────────────

#[test]
fn work_order_with_1000_context_entries() {
    let snippets: Vec<ContextSnippet> = (0..1000)
        .map(|i| ContextSnippet {
            name: format!("snippet_{i}"),
            content: format!("content of snippet {i}"),
        })
        .collect();
    let ctx = ContextPacket {
        files: (0..1000).map(|i| format!("file_{i}.rs")).collect(),
        snippets,
    };
    let wo = WorkOrderBuilder::new("many context entries")
        .context(ctx)
        .build();
    assert_eq!(wo.context.files.len(), 1000);
    assert_eq!(wo.context.snippets.len(), 1000);
    // Round-trip through JSON.
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.context.files.len(), 1000);
}

// ── 5. Receipt with 10 000 events hashes correctly ──────────────────

#[test]
fn receipt_with_10000_events_hashes_correctly() {
    let mut builder = ReceiptBuilder::new("bulk-backend").outcome(Outcome::Complete);
    for i in 0..10_000 {
        builder = builder.add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: format!("delta {i}"),
        }));
    }
    let receipt = builder.build().with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    // Hash must be deterministic — recompute and compare.
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(*hash, recomputed);
}

// ── 6. Very long tool names (10 KB) don't panic ─────────────────────

#[test]
fn very_long_tool_name_does_not_panic() {
    let long_name = "T".repeat(10_000);
    let event = make_event(AgentEventKind::ToolCall {
        tool_name: long_name.clone(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    });
    let json = serde_json::to_string(&event).unwrap();
    let round: AgentEvent = serde_json::from_str(&json).unwrap();
    match &round.kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name.len(), 10_000),
        other => panic!("unexpected variant: {other:?}"),
    }
}

// ── 7. Deeply nested JSON vendor config (100 levels) ────────────────

#[test]
fn deeply_nested_vendor_config_parses() {
    // Build a 100-level nested JSON object: {"a": {"a": { ... }}}
    let mut val = json!("leaf");
    for _ in 0..100 {
        val = json!({ "a": val });
    }
    let mut vendor = BTreeMap::new();
    vendor.insert("deep".to_string(), val);
    let config = RuntimeConfig {
        vendor,
        ..RuntimeConfig::default()
    };
    let wo = WorkOrderBuilder::new("nested config").config(config).build();
    let json_str = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json_str).unwrap();
    // Walk down 100 levels.
    let mut cursor = &round.config.vendor["deep"];
    for _ in 0..100 {
        cursor = &cursor["a"];
    }
    assert_eq!(cursor, &json!("leaf"));
}

// ── 8. Work order with 1000 capability requirements ─────────────────

#[test]
fn work_order_with_1000_capability_requirements() {
    let capabilities = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
    ];
    let reqs = CapabilityRequirements {
        required: (0..1000)
            .map(|i| CapabilityRequirement {
                capability: capabilities[i % capabilities.len()].clone(),
                min_support: if i % 2 == 0 {
                    MinSupport::Native
                } else {
                    MinSupport::Emulated
                },
            })
            .collect(),
    };
    let wo = WorkOrderBuilder::new("many caps")
        .requirements(reqs)
        .build();
    assert_eq!(wo.requirements.required.len(), 1000);
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.requirements.required.len(), 1000);
}

// ── 9. Policy with 10 000 glob patterns compiles ────────────────────

#[test]
fn policy_with_10000_glob_patterns_compiles() {
    let deny_write: Vec<String> = (0..10_000)
        .map(|i| format!("**/deny_{i}/**"))
        .collect();
    let policy = PolicyProfile {
        deny_write,
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("should compile 10k globs");
    // Spot-check a match and a non-match.
    assert!(!engine.can_write_path(Path::new("deny_42/file.txt")).allowed);
    assert!(engine.can_write_path(Path::new("allowed/file.txt")).allowed);
}

// ── 10. JSONL line at buffer boundary ───────────────────────────────

#[test]
fn jsonl_line_at_buffer_boundary() {
    // Craft a JSONL line whose total byte length (with newline) is exactly 8192.
    let base = r#"{"t":"fatal","ref_id":null,"error":""#;
    let suffix = r#""}"#;
    let newline_len = 1;
    let pad_len = 8192 - base.len() - suffix.len() - newline_len;
    let padded_error = "E".repeat(pad_len);
    let line = format!("{base}{padded_error}{suffix}");
    assert_eq!(line.len() + 1, 8192); // +1 for the newline decode_stream will see

    let input = format!("{line}\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
    match &envelopes[0] {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), pad_len),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ── 11. Unicode boundary: exact N bytes of multi-byte chars ─────────

#[test]
fn unicode_exact_byte_boundary() {
    // U+00E9 (é) is 2 bytes in UTF-8, U+1F600 (😀) is 4 bytes.
    let two_byte = "é".repeat(500); // 1000 bytes
    assert_eq!(two_byte.len(), 1000);
    let four_byte = "😀".repeat(250); // 1000 bytes
    assert_eq!(four_byte.len(), 1000);

    let wo = WorkOrderBuilder::new(&two_byte).build();
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.task, two_byte);

    let wo2 = WorkOrderBuilder::new(&four_byte).build();
    let json2 = serde_json::to_string(&wo2).unwrap();
    let round2: abp_core::WorkOrder = serde_json::from_str(&json2).unwrap();
    assert_eq!(round2.task, four_byte);
}

// ── 12. Timestamp at epoch (1970-01-01) ─────────────────────────────

#[test]
fn timestamp_at_epoch_is_valid() {
    let epoch = Utc.timestamp_opt(0, 0).single().unwrap();
    let receipt = ReceiptBuilder::new("epoch-backend")
        .started_at(epoch)
        .finished_at(epoch)
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert_eq!(receipt.meta.started_at, epoch);
    assert!(receipt.receipt_sha256.is_some());
    // Round-trip through JSON.
    let json = serde_json::to_string(&receipt).unwrap();
    let round: abp_core::Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(round.meta.started_at, epoch);
}

// ── 13. Timestamp far future (year 9999) ────────────────────────────

#[test]
fn timestamp_far_future_is_valid() {
    let future = Utc.with_ymd_and_hms(9999, 12, 31, 23, 59, 59).unwrap();
    let event = make_event_at(
        future,
        AgentEventKind::RunStarted {
            message: "far future".into(),
        },
    );
    let receipt = ReceiptBuilder::new("future-backend")
        .started_at(future)
        .finished_at(future)
        .outcome(Outcome::Complete)
        .add_trace_event(event)
        .build()
        .with_hash()
        .unwrap();
    assert_eq!(receipt.meta.started_at, future);
    let json = serde_json::to_string(&receipt).unwrap();
    let round: abp_core::Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(round.meta.started_at, future);
}

// ── 14. Negative numeric values in vendor config ────────────────────

#[test]
fn negative_numeric_values_in_vendor_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert("temperature".to_string(), json!(-1.5));
    vendor.insert("offset".to_string(), json!(-9999));
    vendor.insert("min_float".to_string(), json!(f64::MIN));
    let config = RuntimeConfig {
        vendor,
        ..RuntimeConfig::default()
    };
    let wo = WorkOrderBuilder::new("negatives").config(config).build();
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.config.vendor["temperature"], json!(-1.5));
    assert_eq!(round.config.vendor["offset"], json!(-9999));
}

// ── 15. Boolean values where strings expected ───────────────────────

#[test]
fn boolean_in_vendor_config_where_strings_expected() {
    let mut vendor = BTreeMap::new();
    vendor.insert("flag".to_string(), json!(true));
    vendor.insert("off".to_string(), json!(false));
    let config = RuntimeConfig {
        vendor,
        ..RuntimeConfig::default()
    };
    let wo = WorkOrderBuilder::new("booleans").config(config).build();
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(round.config.vendor["flag"], json!(true));
    assert_eq!(round.config.vendor["off"], json!(false));
}

// ── 16. Empty context packet round-trips ────────────────────────────

#[test]
fn empty_context_packet_round_trips() {
    let wo = WorkOrderBuilder::new("empty ctx")
        .context(ContextPacket::default())
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let round: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(round.context.files.is_empty());
    assert!(round.context.snippets.is_empty());
}

// ── 17. Glob with empty pattern lists ───────────────────────────────

#[test]
fn glob_empty_patterns_allows_everything() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert!(g.decide_str("anything/at/all").is_allowed());
    assert!(g.decide_str("").is_allowed());
}

// ── 18. Receipt hash determinism with identical data ────────────────

#[test]
fn receipt_hash_deterministic_across_calls() {
    let epoch = Utc.timestamp_opt(0, 0).single().unwrap();
    let receipt = ReceiptBuilder::new("det")
        .work_order_id(uuid::Uuid::nil())
        .started_at(epoch)
        .finished_at(epoch)
        .outcome(Outcome::Failed)
        .build();
    let h1 = abp_core::receipt_hash(&receipt).unwrap();
    let h2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2, "same receipt data must produce identical hashes");
}

// ── 19. JSONL decode_stream skips blank lines ───────────────────────

#[test]
fn jsonl_decode_stream_skips_blank_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"a\"}\n\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"b\"}\n\n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

// ── 20. Envelope round-trip with large work order ───────────────────

#[test]
fn envelope_round_trip_large_work_order() {
    let wo = WorkOrderBuilder::new("x".repeat(100_000)).build();
    let envelope = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    assert!(encoded.ends_with('\n'));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-1");
            assert_eq!(work_order.task.len(), 100_000);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}
