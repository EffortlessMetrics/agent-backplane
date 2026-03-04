// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive end-to-end tests for receipt chain integrity, hashing determinism,
//! canonical JSON ordering, and receipt round-trip fidelity.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_backend_core::Backend;
use abp_backend_mock::MockBackend;
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, SupportLevel, VerificationReport, WorkOrderBuilder,
};
use abp_error::ErrorCode;
use abp_receipt::{
    ReceiptBuilder, ReceiptChain, ReceiptValidator, canonicalize, compute_hash, diff_receipts,
    store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore},
    verify::{ReceiptAuditor, verify_receipt},
    verify_hash,
};
use chrono::{Duration as ChronoDuration, Utc};
use uuid::Uuid;

// ───────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────

fn fixed_receipt(backend: &str) -> Receipt {
    let start = Utc::now();
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .build()
}

fn fixed_receipt_with_hash(backend: &str) -> Receipt {
    let start = Utc::now();
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .with_hash()
        .unwrap()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_chain_receipt(backend: &str, offset_secs: i64) -> Receipt {
    let base = Utc::now() + ChronoDuration::seconds(offset_secs);
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .run_id(Uuid::new_v4())
        .started_at(base)
        .finished_at(base + ChronoDuration::milliseconds(100))
        .with_hash()
        .unwrap()
}

// ═══════════════════════════════════════════════════════════════════
// 1. Receipt hashing determinism
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t01_same_receipt_produces_same_hash() {
    let r = fixed_receipt("mock");
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn t02_hash_is_64_hex_chars() {
    let r = fixed_receipt("mock");
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn t03_different_backend_produces_different_hash() {
    let start = Utc::now();
    let run_id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("alpha")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .build();
    let r2 = ReceiptBuilder::new("beta")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn t04_different_outcome_produces_different_hash() {
    let start = Utc::now();
    let run_id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Failed)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn t05_different_run_id_produces_different_hash() {
    let start = Utc::now();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn t06_hash_determinism_across_100_calls() {
    let r = fixed_receipt("determinism-check");
    let reference = compute_hash(&r).unwrap();
    for _ in 0..100 {
        assert_eq!(compute_hash(&r).unwrap(), reference);
    }
}

#[test]
fn t07_abp_core_receipt_hash_matches_abp_receipt_compute_hash() {
    let r = fixed_receipt("mock");
    let core_hash = abp_core::receipt_hash(&r).unwrap();
    let receipt_hash = compute_hash(&r).unwrap();
    assert_eq!(core_hash, receipt_hash);
}

// ═══════════════════════════════════════════════════════════════════
// 2. Receipt chain validation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t08_chain_single_receipt_valid() {
    let mut chain = ReceiptChain::new();
    let r = fixed_receipt_with_hash("mock");
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(chain.verify().is_ok());
}

#[test]
fn t09_chain_multiple_receipts_ordered() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        let r = make_chain_receipt("mock", i * 10);
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

#[test]
fn t10_chain_rejects_duplicate_run_id() {
    let mut chain = ReceiptChain::new();
    let run_id = Uuid::new_v4();
    let start = Utc::now();

    let r1 = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start + ChronoDuration::seconds(1))
        .finished_at(start + ChronoDuration::seconds(1))
        .with_hash()
        .unwrap();

    chain.push(r1).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, abp_receipt::ChainError::DuplicateId { .. }));
}

#[test]
fn t11_chain_rejects_out_of_order_timestamps() {
    let mut chain = ReceiptChain::new();
    let now = Utc::now();

    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(now + ChronoDuration::seconds(10))
        .finished_at(now + ChronoDuration::seconds(10))
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(now)
        .finished_at(now)
        .with_hash()
        .unwrap();

    chain.push(r1).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, abp_receipt::ChainError::BrokenLink { .. }));
}

#[test]
fn t12_chain_rejects_tampered_hash() {
    let mut chain = ReceiptChain::new();
    let mut r = fixed_receipt_with_hash("mock");
    r.receipt_sha256 = Some("deadbeef".repeat(8));
    let err = chain.push(r).unwrap_err();
    assert!(matches!(err, abp_receipt::ChainError::HashMismatch { .. }));
}

#[test]
fn t13_empty_chain_verify_returns_error() {
    let chain = ReceiptChain::new();
    let err = chain.verify().unwrap_err();
    assert!(matches!(err, abp_receipt::ChainError::EmptyChain));
}

#[test]
fn t14_chain_latest_returns_last_pushed() {
    let mut chain = ReceiptChain::new();
    let r1 = make_chain_receipt("first", 0);
    let r2 = make_chain_receipt("second", 10);
    let r2_id = r2.meta.run_id;

    chain.push(r1).unwrap();
    chain.push(r2).unwrap();

    assert_eq!(chain.latest().unwrap().meta.run_id, r2_id);
}

#[test]
fn t15_chain_iter_yields_in_order() {
    let mut chain = ReceiptChain::new();
    let mut ids = Vec::new();
    for i in 0..3 {
        let r = make_chain_receipt("mock", i * 10);
        ids.push(r.meta.run_id);
        chain.push(r).unwrap();
    }
    let iterated: Vec<Uuid> = chain.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids, iterated);
}

// ═══════════════════════════════════════════════════════════════════
// 3. Canonical JSON ordering (BTreeMap determinism)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t16_canonical_json_is_deterministic() {
    let r = fixed_receipt("mock");
    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn t17_canonical_json_keys_are_sorted() {
    let r = fixed_receipt("mock");
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = parsed {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(
            keys, sorted,
            "top-level keys should be alphabetically sorted"
        );
    } else {
        panic!("expected JSON object");
    }
}

#[test]
fn t18_btreemap_capabilities_sorted_in_json() {
    let mut caps = CapabilityManifest::new();
    // Insert in reverse order — BTreeMap sorts by key
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .capabilities(caps)
        .build();

    let json = canonicalize(&r).unwrap();
    let streaming_pos = json.find("streaming").unwrap();
    let tool_read_pos = json.find("tool_read").unwrap();
    let tool_write_pos = json.find("tool_write").unwrap();
    assert!(streaming_pos < tool_read_pos);
    assert!(tool_read_pos < tool_write_pos);
}

#[test]
fn t19_canonical_json_vendor_data_sorted() {
    let mut vendor = serde_json::Map::new();
    vendor.insert("zebra".into(), serde_json::json!(1));
    vendor.insert("alpha".into(), serde_json::json!(2));
    let raw = serde_json::Value::Object(vendor);

    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .usage_raw(raw)
        .build();

    let json = canonicalize(&r).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(alpha_pos < zebra_pos);
}

#[test]
fn t20_canonical_json_forces_receipt_sha256_null() {
    let mut r = fixed_receipt("mock");
    r.receipt_sha256 = Some("some_hash_value".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

// ═══════════════════════════════════════════════════════════════════
// 4. Hash self-exclusion gotcha
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t21_hash_ignores_existing_receipt_sha256() {
    let r = fixed_receipt("mock");
    let h1 = compute_hash(&r).unwrap();

    let mut r_with_hash = r.clone();
    r_with_hash.receipt_sha256 = Some(h1.clone());
    let h2 = compute_hash(&r_with_hash).unwrap();

    assert_eq!(
        h1, h2,
        "hash must be identical regardless of stored receipt_sha256"
    );
}

#[test]
fn t22_receipt_sha256_none_before_with_hash() {
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn t23_with_hash_populates_receipt_sha256() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn t24_verify_hash_passes_for_correct_hash() {
    let r = fixed_receipt_with_hash("mock");
    assert!(verify_hash(&r));
}

#[test]
fn t25_verify_hash_fails_for_tampered_hash() {
    let mut r = fixed_receipt_with_hash("mock");
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(!verify_hash(&r));
}

#[test]
fn t26_verify_hash_passes_for_none_hash() {
    let r = fixed_receipt("mock");
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn t27_hash_self_exclusion_with_different_stored_values() {
    let r = fixed_receipt("mock");
    let h_none = compute_hash(&r).unwrap();

    let mut r1 = r.clone();
    r1.receipt_sha256 = Some("aaaa".into());
    let h_aaaa = compute_hash(&r1).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("bbbb".into());
    let h_bbbb = compute_hash(&r2).unwrap();

    assert_eq!(h_none, h_aaaa);
    assert_eq!(h_none, h_bbbb);
}

// ═══════════════════════════════════════════════════════════════════
// 5. Receipt round-trip (serialize → deserialize → re-hash)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t28_json_roundtrip_preserves_hash() {
    let r = fixed_receipt_with_hash("mock");
    let original_hash = r.receipt_sha256.clone().unwrap();

    let json = serde_json::to_string_pretty(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(
        deserialized.receipt_sha256.as_ref().unwrap(),
        &original_hash
    );
    let recomputed = compute_hash(&deserialized).unwrap();
    assert_eq!(recomputed, original_hash);
}

#[test]
fn t29_compact_bytes_roundtrip_preserves_hash() {
    let r = fixed_receipt_with_hash("mock");
    let original_hash = r.receipt_sha256.clone().unwrap();

    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let deserialized = abp_receipt::serde_formats::from_bytes(&bytes).unwrap();

    let recomputed = compute_hash(&deserialized).unwrap();
    assert_eq!(recomputed, original_hash);
}

#[test]
fn t30_serde_formats_json_roundtrip() {
    let r = fixed_receipt_with_hash("mock");
    let original_hash = r.receipt_sha256.clone().unwrap();

    let json_str = abp_receipt::serde_formats::to_json(&r).unwrap();
    let deserialized = abp_receipt::serde_formats::from_json(&json_str).unwrap();

    let recomputed = compute_hash(&deserialized).unwrap();
    assert_eq!(recomputed, original_hash);
}

#[test]
fn t31_roundtrip_with_trace_events() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .add_event(make_event(AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        }))
        .add_event(make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "/tmp/test.txt"}),
        }))
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.trace.len(), 2);
    assert!(verify_hash(&rt));
}

#[test]
fn t32_roundtrip_preserves_contract_version() {
    let r = fixed_receipt_with_hash("mock");
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn t33_roundtrip_preserves_artifacts() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.diff".into(),
        })
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.artifacts.len(), 1);
    assert_eq!(rt.artifacts[0].kind, "patch");
    assert!(verify_hash(&rt));
}

// ═══════════════════════════════════════════════════════════════════
// 6. Receipt with all event types
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t34_receipt_with_tool_call_event() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .add_event(make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"command": "ls"}),
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn t35_receipt_with_tool_result_event() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .add_event(make_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tc-1".into()),
            output: serde_json::json!({"stdout": "file.txt"}),
            is_error: false,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn t36_receipt_with_text_event() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .add_event(make_event(AgentEventKind::AssistantMessage {
            text: "Done!".into(),
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn t37_receipt_with_error_event() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Failed)
        .add_event(make_event(AgentEventKind::Error {
            message: "something went wrong".into(),
            error_code: None,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn t38_receipt_with_all_event_kinds() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .add_event(make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }))
        .add_event(make_event(AgentEventKind::AssistantDelta {
            text: "tok".into(),
        }))
        .add_event(make_event(AgentEventKind::AssistantMessage {
            text: "token".into(),
        }))
        .add_event(make_event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        }))
        .add_event(make_event(AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            output: serde_json::json!("contents"),
            is_error: false,
        }))
        .add_event(make_event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        }))
        .add_event(make_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        }))
        .add_event(make_event(AgentEventKind::Warning {
            message: "deprecated API".into(),
        }))
        .add_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .with_hash()
        .unwrap();

    assert_eq!(r.trace.len(), 9);
    assert!(verify_hash(&r));
}

#[test]
fn t39_receipt_with_delta_streaming_events() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .add_event(make_event(AgentEventKind::AssistantDelta {
            text: "Hel".into(),
        }))
        .add_event(make_event(AgentEventKind::AssistantDelta {
            text: "lo ".into(),
        }))
        .add_event(make_event(AgentEventKind::AssistantDelta {
            text: "world".into(),
        }))
        .with_hash()
        .unwrap();

    assert_eq!(r.trace.len(), 3);
    assert!(verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════
// 7. Receipt timestamp ordering
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t40_finished_at_equals_started_at_valid() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .with_hash()
        .unwrap();

    let result = verify_receipt(&r);
    assert!(result.is_verified());
}

#[test]
fn t41_finished_after_started_valid() {
    let start = Utc::now();
    let finish = start + ChronoDuration::milliseconds(500);
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .unwrap();

    let result = verify_receipt(&r);
    assert!(result.is_verified());
    assert_eq!(r.meta.duration_ms, 500);
}

#[test]
fn t42_duration_ms_consistent_with_timestamps() {
    let start = Utc::now();
    let finish = start + ChronoDuration::seconds(3);
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .unwrap();

    assert_eq!(r.meta.duration_ms, 3000);
}

#[test]
fn t43_chain_enforces_monotonic_start_times() {
    let mut chain = ReceiptChain::new();
    let now = Utc::now();

    for i in 0..10i64 {
        let start = now + ChronoDuration::seconds(i);
        let r = ReceiptBuilder::new("mock")
            .run_id(Uuid::new_v4())
            .started_at(start)
            .finished_at(start + ChronoDuration::milliseconds(50))
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }

    assert_eq!(chain.len(), 10);
    assert!(chain.verify().is_ok());
}

#[test]
fn t44_validator_catches_duration_mismatch() {
    let start = Utc::now();
    let finish = start + ChronoDuration::seconds(5);
    let mut r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(finish)
        .build();

    // Tamper with duration
    r.receipt_sha256 = None;
    r.meta.duration_ms = 999;

    let validator = ReceiptValidator::new();
    let err = validator.validate(&r).unwrap_err();
    assert!(err.iter().any(|e| e.field == "meta.duration_ms"));
}

// ═══════════════════════════════════════════════════════════════════
// 8. Receipt with capabilities
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t45_capabilities_survive_roundtrip() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Unsupported);

    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .capabilities(caps)
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(rt.capabilities.len(), 4);
    assert!(rt.capabilities.contains_key(&Capability::Streaming));
    assert!(rt.capabilities.contains_key(&Capability::ToolRead));
    assert!(rt.capabilities.contains_key(&Capability::ToolWrite));
    assert!(rt.capabilities.contains_key(&Capability::ExtendedThinking));
    assert!(verify_hash(&rt));
}

#[test]
fn t46_capabilities_affect_hash() {
    let start = Utc::now();
    let run_id = Uuid::new_v4();

    let r1 = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .build();

    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);

    let r2 = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .capabilities(caps)
        .build();

    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn t47_empty_capabilities_are_valid() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .capabilities(CapabilityManifest::new())
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert!(r.capabilities.is_empty());
}

#[test]
fn t48_many_capabilities_roundtrip() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolEdit, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    caps.insert(Capability::ToolGlob, SupportLevel::Emulated);
    caps.insert(Capability::ToolGrep, SupportLevel::Emulated);
    caps.insert(Capability::McpClient, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ImageInput, SupportLevel::Unsupported);

    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .capabilities(caps.clone())
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.capabilities.len(), caps.len());
    assert!(verify_hash(&rt));
}

// ═══════════════════════════════════════════════════════════════════
// 9. Mock backend receipt flow
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t49_mock_backend_produces_hashed_receipt() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("test task").build();
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();

    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt));

    // Drain events
    while rx.try_recv().is_ok() {}
}

#[tokio::test]
async fn t50_mock_backend_receipt_passes_verification() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("verify me").build();
    let run_id = Uuid::new_v4();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();

    let result = verify_receipt(&receipt);
    assert!(result.is_verified(), "issues: {:?}", result.issues);
}

#[tokio::test]
async fn t51_mock_backend_receipt_has_correct_run_id() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("check run id").build();
    let run_id = Uuid::new_v4();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn t52_mock_backend_receipt_has_contract_version() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("check contract").build();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn t53_mock_backend_receipt_has_trace_events() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("check trace").build();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(
        !receipt.trace.is_empty(),
        "mock backend should emit trace events"
    );
}

#[tokio::test]
async fn t54_mock_backend_receipt_outcome_is_complete() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("check outcome").build();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t55_mock_backend_receipt_in_chain() {
    let backend = MockBackend;
    let mut chain = ReceiptChain::new();

    for _ in 0..3 {
        let wo = WorkOrderBuilder::new("chain test").build();
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        chain.push(receipt).unwrap();
    }

    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// 10. Receipt error codes
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t56_error_receipt_with_error_code_hashes_correctly() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Failed)
        .add_event(make_event(AgentEventKind::Error {
            message: "backend crashed".into(),
            error_code: Some(ErrorCode::BackendCrashed),
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn t57_error_receipt_with_none_error_code() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Failed)
        .add_event(make_event(AgentEventKind::Error {
            message: "unknown error".into(),
            error_code: None,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn t58_different_error_codes_produce_different_hashes() {
    let start = Utc::now();
    let run_id = Uuid::new_v4();

    let r1 = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Failed)
        .add_event(AgentEvent {
            ts: start,
            kind: AgentEventKind::Error {
                message: "err".into(),
                error_code: Some(ErrorCode::BackendTimeout),
            },
            ext: None,
        })
        .build();

    let r2 = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Failed)
        .add_event(AgentEvent {
            ts: start,
            kind: AgentEventKind::Error {
                message: "err".into(),
                error_code: Some(ErrorCode::BackendCrashed),
            },
            ext: None,
        })
        .build();

    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn t59_error_receipt_builder_shorthand() {
    let r = ReceiptBuilder::new("mock")
        .error("something broke")
        .with_hash()
        .unwrap();

    assert_eq!(r.outcome, Outcome::Failed);
    assert!(r.trace.iter().any(|e| matches!(
        &e.kind,
        AgentEventKind::Error { message, .. } if message == "something broke"
    )));
    assert!(verify_hash(&r));
}

#[test]
fn t60_error_receipt_roundtrip_preserves_error_code() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Failed)
        .add_event(AgentEvent {
            ts: start,
            kind: AgentEventKind::Error {
                message: "auth failed".into(),
                error_code: Some(ErrorCode::BackendAuthFailed),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();

    let err_event = rt
        .trace
        .iter()
        .find(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        .unwrap();

    if let AgentEventKind::Error { error_code, .. } = &err_event.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendAuthFailed));
    } else {
        panic!("expected error event");
    }
    assert!(verify_hash(&rt));
}

// ═══════════════════════════════════════════════════════════════════
// Additional coverage: validator, auditor, diff, store, edge cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn t61_validator_accepts_valid_receipt() {
    let r = fixed_receipt_with_hash("mock");
    let v = ReceiptValidator::new();
    assert!(v.validate(&r).is_ok());
}

#[test]
fn t62_validator_rejects_wrong_contract_version() {
    let mut r = fixed_receipt("mock");
    r.meta.contract_version = "abp/v99".into();
    let v = ReceiptValidator::new();
    let err = v.validate(&r).unwrap_err();
    assert!(err.iter().any(|e| e.field == "meta.contract_version"));
}

#[test]
fn t63_validator_rejects_tampered_hash() {
    let mut r = fixed_receipt_with_hash("mock");
    r.receipt_sha256 = Some("bad_hash".into());
    let v = ReceiptValidator::new();
    let err = v.validate(&r).unwrap_err();
    assert!(err.iter().any(|e| e.field == "receipt_sha256"));
}

#[test]
fn t64_auditor_clean_batch() {
    let auditor = ReceiptAuditor::new();
    let receipts: Vec<Receipt> = (0..5).map(|i| make_chain_receipt("mock", i * 10)).collect();
    let report = auditor.audit_batch(&receipts);
    assert!(report.is_clean(), "issues: {:?}", report.issues);
    assert_eq!(report.total, 5);
    assert_eq!(report.valid, 5);
}

#[test]
fn t65_auditor_detects_invalid_receipt() {
    let auditor = ReceiptAuditor::new();
    let mut r = fixed_receipt_with_hash("mock");
    r.receipt_sha256 = Some("tampered".into());
    let report = auditor.audit_batch(&[r]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
}

#[test]
fn t66_diff_identical_receipts_is_empty() {
    let r = fixed_receipt("mock");
    let d = diff_receipts(&r, &r);
    assert!(d.is_empty());
}

#[test]
fn t67_diff_detects_outcome_change() {
    let r1 = fixed_receipt("mock");
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    let d = diff_receipts(&r1, &r2);
    assert!(!d.is_empty());
    assert!(d.changes.iter().any(|c| c.field == "outcome"));
}

#[test]
fn t68_store_and_retrieve_receipt() {
    let mut store = InMemoryReceiptStore::new();
    let r = fixed_receipt_with_hash("mock");
    let id = r.meta.run_id;
    store.store(r).unwrap();

    let retrieved = store.get(id).unwrap().unwrap();
    assert!(verify_hash(retrieved));
}

#[test]
fn t69_store_rejects_duplicate() {
    let mut store = InMemoryReceiptStore::new();
    let r = fixed_receipt_with_hash("mock");
    let r2 = r.clone();
    store.store(r).unwrap();
    assert!(store.store(r2).is_err());
}

#[test]
fn t70_store_filter_by_backend() {
    let mut store = InMemoryReceiptStore::new();

    let now = Utc::now();
    let r1 = ReceiptBuilder::new("alpha")
        .run_id(Uuid::new_v4())
        .started_at(now)
        .finished_at(now)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("beta")
        .run_id(Uuid::new_v4())
        .started_at(now)
        .finished_at(now)
        .with_hash()
        .unwrap();

    store.store(r1).unwrap();
    store.store(r2).unwrap();

    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "alpha");
}

#[test]
fn t71_execution_mode_default_is_mapped() {
    let r = fixed_receipt("mock");
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn t72_execution_mode_passthrough_hashes_differently() {
    let start = Utc::now();
    let run_id = Uuid::new_v4();

    let r_mapped = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .mode(ExecutionMode::Mapped)
        .build();

    let r_passthrough = ReceiptBuilder::new("mock")
        .run_id(run_id)
        .started_at(start)
        .finished_at(start)
        .mode(ExecutionMode::Passthrough)
        .build();

    assert_ne!(
        compute_hash(&r_mapped).unwrap(),
        compute_hash(&r_passthrough).unwrap()
    );
}

#[test]
fn t73_receipt_with_usage_tokens() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .usage_tokens(1000, 500)
        .with_hash()
        .unwrap();

    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.output_tokens, Some(500));
    assert!(verify_hash(&r));
}

#[test]
fn t74_receipt_with_model_and_dialect() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .model("gpt-4o")
        .dialect("openai")
        .with_hash()
        .unwrap();

    let raw = &r.usage_raw;
    assert_eq!(raw["model"], "gpt-4o");
    assert_eq!(raw["dialect"], "openai");
    assert!(verify_hash(&r));
}

#[test]
fn t75_receipt_with_ext_data() {
    let start = Utc::now();
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"role": "assistant", "content": "hi"}),
    );

    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .add_event(AgentEvent {
            ts: start,
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        })
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert!(rt.trace[0].ext.is_some());
    assert!(verify_hash(&rt));
}

#[test]
fn t76_chain_with_ten_receipts_is_valid() {
    let mut chain = ReceiptChain::new();
    let base = Utc::now();
    for i in 0..10i64 {
        let start = base + ChronoDuration::seconds(i);
        let r = ReceiptBuilder::new("mock")
            .run_id(Uuid::new_v4())
            .started_at(start)
            .finished_at(start + ChronoDuration::milliseconds(50))
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert!(chain.verify().is_ok());
    assert_eq!(chain.len(), 10);
}

#[test]
fn t77_receipt_with_verification_report() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .verification(VerificationReport {
            git_diff: Some("diff --git a/foo b/foo\n+bar".into()),
            git_status: Some("M foo".into()),
            harness_ok: true,
        })
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(
        rt.verification.git_diff.as_deref(),
        Some("diff --git a/foo b/foo\n+bar")
    );
    assert!(verify_hash(&rt));
}

#[test]
fn t78_partial_outcome_receipt() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .outcome(Outcome::Partial)
        .with_hash()
        .unwrap();

    assert_eq!(r.outcome, Outcome::Partial);
    assert!(verify_hash(&r));
}

#[test]
fn t79_receipt_with_backend_version_info() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("sidecar:node")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .finished_at(start)
        .backend_version("2.1.0")
        .adapter_version("0.3.0")
        .with_hash()
        .unwrap();

    assert_eq!(r.backend.id, "sidecar:node");
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.1.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.3.0"));
    assert!(verify_hash(&r));
}

#[test]
fn t80_duration_builder_sets_finished_at() {
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .started_at(start)
        .duration(Duration::from_millis(2500))
        .build();

    assert_eq!(r.meta.duration_ms, 2500);
    assert_eq!(
        (r.meta.finished_at - r.meta.started_at).num_milliseconds(),
        2500
    );
}

#[test]
fn t81_work_order_id_survives_roundtrip() {
    let wo_id = Uuid::new_v4();
    let start = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::new_v4())
        .work_order_id(wo_id)
        .started_at(start)
        .finished_at(start)
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let rt: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.meta.work_order_id, wo_id);
    assert!(verify_hash(&rt));
}

#[test]
fn t82_core_with_hash_method_works() {
    let r = abp_core::ReceiptBuilder::new("core-mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn t83_error_code_serialization_roundtrip() {
    let start = Utc::now();
    let event = AgentEvent {
        ts: start,
        kind: AgentEventKind::Error {
            message: "rate limited".into(),
            error_code: Some(ErrorCode::BackendRateLimited),
        },
        ext: None,
    };

    let json = serde_json::to_string(&event).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendRateLimited));
    } else {
        panic!("expected error event kind");
    }
}
