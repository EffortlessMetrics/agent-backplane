#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive receipt integrity tests covering hashing, canonicalization,
//! chain validation, metadata preservation, and serde roundtrips.

use std::collections::BTreeMap;

use abp_core::verify::{
    verify_chain, ChainBuilder, ChainEntry, ChainError, ChainVerification, ChainVerifier,
    ReceiptChain, ReceiptVerifier,
};
use abp_core::{
    receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability,
    ContractError, ExecutionMode, Outcome, Receipt, ReceiptBuilder, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport as CoreVerificationReport, CONTRACT_VERSION,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use uuid::Uuid;

// ─── Helpers ──────────────────────────────────────────────────────────

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_time(),
        kind,
        ext: None,
    }
}

fn make_event_at(kind: AgentEventKind, ts: DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .started_at(fixed_time())
        .finished_at(fixed_time() + Duration::milliseconds(100))
        .build()
}

fn receipt_with_trace(events: Vec<AgentEvent>) -> Receipt {
    let mut builder = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .started_at(fixed_time())
        .finished_at(fixed_time() + Duration::milliseconds(500));
    for e in events {
        builder = builder.add_trace_event(e);
    }
    builder.build()
}

fn receipt_with_backend(id: &str) -> Receipt {
    ReceiptBuilder::new(id)
        .outcome(Outcome::Complete)
        .started_at(fixed_time())
        .finished_at(fixed_time() + Duration::milliseconds(100))
        .build()
}

fn receipt_at(start: DateTime<Utc>, duration_ms: i64) -> Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(start + Duration::milliseconds(duration_ms))
        .build()
}

fn hashed_receipt() -> Receipt {
    minimal_receipt().with_hash().unwrap()
}

fn hashed_receipt_at(start: DateTime<Utc>, duration_ms: i64) -> Receipt {
    receipt_at(start, duration_ms).with_hash().unwrap()
}

// ─── 1. receipt_hash() sets receipt_sha256 to null before hashing ─────

#[test]
fn receipt_hash_nullifies_sha256_field() {
    let mut receipt = minimal_receipt();
    receipt.receipt_sha256 = Some("bogus_hash_value".to_string());
    let h1 = receipt_hash(&receipt).unwrap();

    receipt.receipt_sha256 = None;
    let h2 = receipt_hash(&receipt).unwrap();

    assert_eq!(
        h1, h2,
        "hash must be identical regardless of receipt_sha256 value"
    );
}

#[test]
fn receipt_hash_nullifies_even_valid_hash() {
    let receipt = hashed_receipt();
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[test]
fn receipt_hash_with_empty_string_sha256() {
    let mut receipt = minimal_receipt();
    receipt.receipt_sha256 = Some(String::new());
    let h1 = receipt_hash(&receipt).unwrap();

    receipt.receipt_sha256 = None;
    let h2 = receipt_hash(&receipt).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_with_very_long_sha256() {
    let mut receipt = minimal_receipt();
    receipt.receipt_sha256 = Some("a".repeat(10_000));
    let h1 = receipt_hash(&receipt).unwrap();

    receipt.receipt_sha256 = None;
    let h2 = receipt_hash(&receipt).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_is_valid_hex_string() {
    let hash = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex must be 64 characters");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_is_lowercase_hex() {
    let hash = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(hash, hash.to_lowercase());
}

// ─── 2. with_hash() produces a receipt with valid hash ──────────────

#[test]
fn with_hash_produces_some_hash() {
    let receipt = minimal_receipt().with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn with_hash_hash_matches_receipt_hash() {
    let receipt = minimal_receipt().with_hash().unwrap();
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.unwrap(), recomputed);
}

#[test]
fn with_hash_is_idempotent() {
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = r1.clone().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn with_hash_overwrites_bogus_hash() {
    let mut receipt = minimal_receipt();
    receipt.receipt_sha256 = Some("not_a_real_hash".into());
    let hashed = receipt.with_hash().unwrap();
    assert_ne!(hashed.receipt_sha256.as_deref(), Some("not_a_real_hash"));
    let recomputed = receipt_hash(&hashed).unwrap();
    assert_eq!(hashed.receipt_sha256.unwrap(), recomputed);
}

#[test]
fn with_hash_on_builder_receipt() {
    let receipt = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.unwrap(), recomputed);
}

#[test]
fn with_hash_preserves_all_fields() {
    let original = receipt_with_trace(vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ]);
    let hashed = original.clone().with_hash().unwrap();

    assert_eq!(original.meta.run_id, hashed.meta.run_id);
    assert_eq!(original.backend.id, hashed.backend.id);
    assert_eq!(original.trace.len(), hashed.trace.len());
    assert_eq!(original.outcome, hashed.outcome);
}

// ─── 3. Deterministic hashing ────────────────────────────────────────

#[test]
fn hash_deterministic_same_receipt() {
    let receipt = minimal_receipt();
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_deterministic_cloned_receipt() {
    let receipt = minimal_receipt();
    let cloned = receipt.clone();
    assert_eq!(
        receipt_hash(&receipt).unwrap(),
        receipt_hash(&cloned).unwrap()
    );
}

#[test]
fn hash_deterministic_after_serde_roundtrip() {
    let receipt = minimal_receipt();
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(
        receipt_hash(&receipt).unwrap(),
        receipt_hash(&deserialized).unwrap()
    );
}

#[test]
fn hash_deterministic_with_trace_events() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let r1 = receipt_with_trace(events.clone());
    // Verify the hash function itself is deterministic on the same receipt
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_deterministic_100_iterations() {
    let receipt = minimal_receipt();
    let first = receipt_hash(&receipt).unwrap();
    for _ in 0..100 {
        assert_eq!(first, receipt_hash(&receipt).unwrap());
    }
}

#[test]
fn hash_deterministic_with_capabilities() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .capabilities(caps)
        .started_at(fixed_time())
        .finished_at(fixed_time() + Duration::milliseconds(50))
        .build();

    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

// ─── 4. Field changes produce different hashes ──────────────────────

#[test]
fn different_backend_id_different_hash() {
    let r1 = receipt_with_backend("alpha");
    let mut r2_copy = r1.clone();
    r2_copy.backend.id = "beta".into();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2_copy).unwrap());
}

#[test]
fn different_outcome_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_outcome_partial_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Partial;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_contract_version_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.meta.contract_version = "abp/v99.99".into();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_work_order_id_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.meta.work_order_id = Uuid::new_v4();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_run_id_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.meta.run_id = Uuid::new_v4();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_started_at_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.meta.started_at = r1.meta.started_at + Duration::seconds(1);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_finished_at_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.meta.finished_at = r1.meta.finished_at + Duration::seconds(1);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_duration_ms_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.meta.duration_ms = r1.meta.duration_ms + 1;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_mode_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.mode = ExecutionMode::Passthrough;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_usage_raw_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.usage_raw = json!({"tokens": 42});
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_usage_input_tokens_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.usage.input_tokens = Some(1000);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_usage_output_tokens_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.usage.output_tokens = Some(500);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_trace_events_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.trace.push(make_event(AgentEventKind::RunStarted {
        message: "hello".into(),
    }));
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_trace_event_content_different_hash() {
    let r1 = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    })]);
    let mut r2 = r1.clone();
    r2.trace[0] = make_event(AgentEventKind::AssistantMessage {
        text: "world".into(),
    });
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_artifacts_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.artifacts.push(ArtifactRef {
        kind: "file".into(),
        path: "/tmp/out.txt".into(),
    });
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_verification_git_diff_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.verification.git_diff = Some("diff --git a/foo".into());
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_verification_harness_ok_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.verification.harness_ok = !r1.verification.harness_ok;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_backend_version_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.backend.backend_version = Some("2.0.0".into());
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_adapter_version_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.backend.adapter_version = Some("1.5.0".into());
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_capabilities_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.capabilities
        .insert(Capability::ToolUse, SupportLevel::Native);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_cache_read_tokens_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.usage.cache_read_tokens = Some(100);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_cache_write_tokens_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.usage.cache_write_tokens = Some(200);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_estimated_cost_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.usage.estimated_cost_usd = Some(0.05);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_verification_git_status_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = r1.clone();
    r2.verification.git_status = Some("M foo.rs".into());
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ─── 5. Canonical JSON serialization (BTreeMap ordering) ────────────

#[test]
fn capabilities_btreemap_ordering_deterministic() {
    let mut caps1 = BTreeMap::new();
    caps1.insert(Capability::ToolUse, SupportLevel::Native);
    caps1.insert(Capability::Streaming, SupportLevel::Emulated);
    caps1.insert(Capability::Vision, SupportLevel::Unsupported);

    // Insert in different order
    let mut caps2 = BTreeMap::new();
    caps2.insert(Capability::Vision, SupportLevel::Unsupported);
    caps2.insert(Capability::ToolUse, SupportLevel::Native);
    caps2.insert(Capability::Streaming, SupportLevel::Emulated);

    let j1 = serde_json::to_string(&caps1).unwrap();
    let j2 = serde_json::to_string(&caps2).unwrap();
    assert_eq!(j1, j2, "BTreeMap produces deterministic key ordering");
}

#[test]
fn receipt_json_key_order_is_stable() {
    let receipt = minimal_receipt();
    let j1 = serde_json::to_string(&receipt).unwrap();
    let j2 = serde_json::to_string(&receipt).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn receipt_canonical_json_ignores_receipt_sha256() {
    let mut r1 = minimal_receipt();
    r1.receipt_sha256 = Some("aaa".into());
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("zzz".into());

    let hash1 = receipt_hash(&r1).unwrap();
    let hash2 = receipt_hash(&r2).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn receipt_canonical_json_null_vs_none_sha256() {
    let r1 = minimal_receipt();
    let mut v = serde_json::to_value(&r1).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    let canonical = serde_json::to_string(&v).unwrap();
    assert!(canonical.contains("\"receipt_sha256\":null"));
}

#[test]
fn receipt_value_canonicalization_sets_null() {
    let mut receipt = minimal_receipt();
    receipt.receipt_sha256 = Some("test123".into());
    let mut v = serde_json::to_value(&receipt).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
    assert!(!json.contains("test123"));
}

#[test]
fn receipt_with_ext_field_canonical() {
    let mut ext = BTreeMap::new();
    ext.insert("z_field".to_string(), json!("last"));
    ext.insert("a_field".to_string(), json!("first"));
    let event = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let j1 = serde_json::to_string(&event).unwrap();
    let j2 = serde_json::to_string(&event).unwrap();
    assert_eq!(j1, j2);
    // BTreeMap keys are alphabetical
    let a_pos = j1.find("a_field").unwrap();
    let z_pos = j1.find("z_field").unwrap();
    assert!(
        a_pos < z_pos,
        "BTreeMap should order a_field before z_field"
    );
}

#[test]
fn receipt_hash_uses_json_not_binary() {
    let receipt = minimal_receipt();
    let hash = receipt_hash(&receipt).unwrap();
    // Verify we can reproduce the hash using manual JSON canonicalization
    let mut v = serde_json::to_value(&receipt).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    let json = serde_json::to_string(&v).unwrap();
    let manual_hash = sha256_hex(json.as_bytes());
    assert_eq!(hash, manual_hash);
}

// ─── 6. Chain validation ─────────────────────────────────────────────

#[test]
fn empty_chain_is_valid() {
    let chain = ChainBuilder::new().build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn single_receipt_chain_valid() {
    let r = hashed_receipt();
    let chain = ChainBuilder::new().push(r).build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 1);
}

#[test]
fn chain_with_valid_parent_reference() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0, 100);
    let parent_id = r1.meta.run_id;
    let r2 = hashed_receipt_at(t0 + Duration::milliseconds(200), 100);
    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(r2, parent_id)
        .build();
    let result = verify_chain(&chain);
    assert!(result.valid);
}

#[test]
fn chain_with_missing_parent_reference() {
    let r1 = hashed_receipt_at(fixed_time(), 100);
    let fake_parent = Uuid::new_v4();
    let r2 = hashed_receipt_at(fixed_time() + Duration::milliseconds(200), 100);
    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(r2, fake_parent)
        .build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ChainError::MissingParent { .. })));
}

#[test]
fn chain_out_of_order_detected() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0 + Duration::seconds(10), 100);
    let r2 = hashed_receipt_at(t0, 100);
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ChainError::OutOfOrder { .. })));
}

#[test]
fn chain_duplicate_run_id_detected() {
    let r1 = hashed_receipt_at(fixed_time(), 100);
    let mut r2 = hashed_receipt_at(fixed_time() + Duration::milliseconds(200), 100);
    r2.meta.run_id = r1.meta.run_id;
    // Re-hash after modifying run_id
    let r2 = r2.with_hash().unwrap();
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ChainError::DuplicateId { .. })));
}

#[test]
fn chain_version_mismatch_detected() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0, 100);
    let mut r2 = receipt_at(t0 + Duration::milliseconds(200), 100);
    r2.meta.contract_version = "abp/v99.0".into();
    let r2 = r2.with_hash().unwrap();
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ChainError::ContractVersionMismatch { .. })));
}

#[test]
fn chain_broken_hash_detected() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0, 100);
    let mut r2 = hashed_receipt_at(t0 + Duration::milliseconds(200), 100);
    r2.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ChainError::BrokenHash { .. })));
}

#[test]
fn chain_total_events_counted() {
    let t0 = fixed_time();
    let r1 = receipt_with_trace(vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "d".into(),
        }),
    ])
    .with_hash()
    .unwrap();
    let mut r2 = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    })]);
    r2.meta.started_at = t0 + Duration::seconds(1);
    r2.meta.finished_at = t0 + Duration::seconds(2);
    let r2 = r2.with_hash().unwrap();

    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert_eq!(result.total_events, 3);
}

#[test]
fn chain_total_duration_accumulated() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0, 100);
    let r2 = hashed_receipt_at(t0 + Duration::milliseconds(200), 300);
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert_eq!(result.total_duration_ms, 400);
}

#[test]
fn chain_three_levels_deep() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0, 100);
    let r1_id = r1.meta.run_id;
    let r2 = hashed_receipt_at(t0 + Duration::milliseconds(200), 100);
    let r2_id = r2.meta.run_id;
    let r3 = hashed_receipt_at(t0 + Duration::milliseconds(400), 100);
    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(r2, r1_id)
        .push_child(r3, r2_id)
        .build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 3);
}

#[test]
fn chain_multiple_children_same_parent() {
    let t0 = fixed_time();
    let parent = hashed_receipt_at(t0, 100);
    let pid = parent.meta.run_id;
    let c1 = hashed_receipt_at(t0 + Duration::milliseconds(200), 100);
    let c2 = hashed_receipt_at(t0 + Duration::milliseconds(400), 100);
    let chain = ChainBuilder::new()
        .push(parent)
        .push_child(c1, pid)
        .push_child(c2, pid)
        .build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 3);
}

#[test]
fn chain_no_hash_receipts_still_valid() {
    let t0 = fixed_time();
    let r1 = receipt_at(t0, 100);
    let r2 = receipt_at(t0 + Duration::milliseconds(200), 100);
    assert!(r1.receipt_sha256.is_none());
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    // No hash = no hash check, no broken hash error
    assert!(result.valid);
}

// ─── 7. Receipt metadata preservation ─────────────────────────────

#[test]
fn metadata_contract_version_matches() {
    let receipt = minimal_receipt();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn metadata_run_id_is_non_nil() {
    let receipt = minimal_receipt();
    assert_ne!(receipt.meta.run_id, Uuid::nil());
}

#[test]
fn metadata_work_order_id_preserved() {
    let wo_id = Uuid::new_v4();
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .work_order_id(wo_id)
        .build();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[test]
fn metadata_backend_identity_preserved() {
    let receipt = ReceiptBuilder::new("my-backend")
        .backend_version("1.0.0")
        .adapter_version("0.5.0")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.backend.id, "my-backend");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("1.0.0"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("0.5.0"));
}

#[test]
fn metadata_capabilities_preserved() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .capabilities(caps.clone())
        .build();
    assert_eq!(receipt.capabilities.len(), caps.len());
}

#[test]
fn metadata_mode_preserved() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn metadata_mode_default_is_mapped() {
    let receipt = minimal_receipt();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[test]
fn metadata_usage_preserved() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: Some(10),
        cache_write_tokens: Some(5),
        request_units: Some(1),
        estimated_cost_usd: Some(0.01),
    };
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .usage(usage.clone())
        .build();
    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(50));
    assert_eq!(receipt.usage.cache_read_tokens, Some(10));
    assert_eq!(receipt.usage.cache_write_tokens, Some(5));
    assert_eq!(receipt.usage.request_units, Some(1));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.01));
}

#[test]
fn metadata_usage_raw_preserved() {
    let raw = json!({"prompt_tokens": 150, "completion_tokens": 75});
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .usage_raw(raw.clone())
        .build();
    assert_eq!(receipt.usage_raw, raw);
}

#[test]
fn metadata_artifacts_preserved() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .add_artifact(ArtifactRef {
            kind: "file".into(),
            path: "/tmp/result.txt".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "/tmp/fix.patch".into(),
        })
        .build();
    assert_eq!(receipt.artifacts.len(), 2);
    assert_eq!(receipt.artifacts[0].kind, "file");
    assert_eq!(receipt.artifacts[1].path, "/tmp/fix.patch");
}

#[test]
fn metadata_verification_preserved() {
    let v = CoreVerificationReport {
        git_diff: Some("diff text".into()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .verification(v)
        .build();
    assert_eq!(receipt.verification.git_diff.as_deref(), Some("diff text"));
    assert!(receipt.verification.harness_ok);
}

// ─── 8. Receipt timestamps ──────────────────────────────────────────

#[test]
fn timestamps_started_before_finished() {
    let receipt = minimal_receipt();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[test]
fn timestamps_duration_computed_correctly() {
    let start = fixed_time();
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(start + Duration::milliseconds(42))
        .build();
    assert_eq!(receipt.meta.duration_ms, 42);
}

#[test]
fn timestamps_zero_duration() {
    let t = fixed_time();
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .started_at(t)
        .finished_at(t)
        .build();
    assert_eq!(receipt.meta.duration_ms, 0);
}

#[test]
fn timestamps_large_duration() {
    let start = fixed_time();
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(start + Duration::hours(24))
        .build();
    assert_eq!(receipt.meta.duration_ms, 24 * 60 * 60 * 1000);
}

#[test]
fn timestamps_preserved_after_hash() {
    let start = fixed_time();
    let end = start + Duration::milliseconds(250);
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(end)
        .build()
        .with_hash()
        .unwrap();
    assert_eq!(receipt.meta.started_at, start);
    assert_eq!(receipt.meta.finished_at, end);
    assert_eq!(receipt.meta.duration_ms, 250);
}

#[test]
fn timestamps_serde_roundtrip() {
    let receipt = minimal_receipt();
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt.meta.started_at, deserialized.meta.started_at);
    assert_eq!(receipt.meta.finished_at, deserialized.meta.finished_at);
    assert_eq!(receipt.meta.duration_ms, deserialized.meta.duration_ms);
}

// ─── 9. Receipt event counting ───────────────────────────────────────

#[test]
fn event_count_zero() {
    let receipt = minimal_receipt();
    assert_eq!(receipt.trace.len(), 0);
}

#[test]
fn event_count_one() {
    let receipt = receipt_with_trace(vec![make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    })]);
    assert_eq!(receipt.trace.len(), 1);
}

#[test]
fn event_count_multiple() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "foo.rs"}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("t1".into()),
            output: json!("content"),
            is_error: false,
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "d".into(),
        }),
    ];
    let receipt = receipt_with_trace(events);
    assert_eq!(receipt.trace.len(), 5);
}

#[test]
fn event_count_preserved_after_hash() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "d".into(),
        }),
    ];
    let receipt = receipt_with_trace(events).with_hash().unwrap();
    assert_eq!(receipt.trace.len(), 2);
}

#[test]
fn event_types_preserved() {
    let events = vec![
        make_event(AgentEventKind::Warning {
            message: "watch out".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "bad".into(),
            error_code: None,
        }),
        make_event(AgentEventKind::FileChanged {
            path: "foo.rs".into(),
            summary: "modified".into(),
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: None,
        }),
    ];
    let receipt = receipt_with_trace(events);
    assert!(matches!(
        receipt.trace[0].kind,
        AgentEventKind::Warning { .. }
    ));
    assert!(matches!(
        receipt.trace[1].kind,
        AgentEventKind::Error { .. }
    ));
    assert!(matches!(
        receipt.trace[2].kind,
        AgentEventKind::FileChanged { .. }
    ));
    assert!(matches!(
        receipt.trace[3].kind,
        AgentEventKind::CommandExecuted { .. }
    ));
}

// ─── 10. Empty receipts ──────────────────────────────────────────────

#[test]
fn empty_receipt_hashes_successfully() {
    let receipt = minimal_receipt();
    assert!(receipt.trace.is_empty());
    assert!(receipt.artifacts.is_empty());
    assert!(receipt.capabilities.is_empty());
    let hash = receipt_hash(&receipt).unwrap();
    assert!(!hash.is_empty());
}

#[test]
fn empty_receipt_with_hash_succeeds() {
    let receipt = minimal_receipt().with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn empty_receipt_serializes() {
    let receipt = minimal_receipt();
    let json = serde_json::to_string(&receipt).unwrap();
    assert!(!json.is_empty());
}

#[test]
fn empty_receipt_deserializes() {
    let receipt = minimal_receipt();
    let json = serde_json::to_string(&receipt).unwrap();
    let _: Receipt = serde_json::from_str(&json).unwrap();
}

#[test]
fn empty_receipt_default_usage() {
    let receipt = minimal_receipt();
    assert!(receipt.usage.input_tokens.is_none());
    assert!(receipt.usage.output_tokens.is_none());
    assert!(receipt.usage.cache_read_tokens.is_none());
    assert!(receipt.usage.cache_write_tokens.is_none());
    assert!(receipt.usage.request_units.is_none());
    assert!(receipt.usage.estimated_cost_usd.is_none());
}

#[test]
fn empty_receipt_default_verification() {
    let receipt = minimal_receipt();
    assert!(receipt.verification.git_diff.is_none());
    assert!(receipt.verification.git_status.is_none());
    assert!(!receipt.verification.harness_ok);
}

#[test]
fn empty_receipt_no_sha256() {
    let receipt = minimal_receipt();
    assert!(receipt.receipt_sha256.is_none());
}

// ─── 11. Receipts with max events ────────────────────────────────────

#[test]
fn receipt_with_100_events() {
    let events: Vec<AgentEvent> = (0..100)
        .map(|i| {
            make_event(AgentEventKind::AssistantDelta {
                text: format!("chunk_{i}"),
            })
        })
        .collect();
    let receipt = receipt_with_trace(events);
    assert_eq!(receipt.trace.len(), 100);
    let hash = receipt_hash(&receipt).unwrap();
    assert!(!hash.is_empty());
}

#[test]
fn receipt_with_1000_events_hashes() {
    let events: Vec<AgentEvent> = (0..1000)
        .map(|i| {
            make_event(AgentEventKind::AssistantDelta {
                text: format!("tok_{i}"),
            })
        })
        .collect();
    let receipt = receipt_with_trace(events);
    assert_eq!(receipt.trace.len(), 1000);
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn receipt_with_many_events_deterministic() {
    let events: Vec<AgentEvent> = (0..500)
        .map(|i| {
            make_event(AgentEventKind::AssistantDelta {
                text: format!("t_{i}"),
            })
        })
        .collect();
    let receipt = receipt_with_trace(events);
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_many_tool_calls() {
    let events: Vec<AgentEvent> = (0..50)
        .flat_map(|i| {
            vec![
                make_event(AgentEventKind::ToolCall {
                    tool_name: format!("tool_{i}"),
                    tool_use_id: Some(format!("call_{i}")),
                    parent_tool_use_id: None,
                    input: json!({"arg": i}),
                }),
                make_event(AgentEventKind::ToolResult {
                    tool_name: format!("tool_{i}"),
                    tool_use_id: Some(format!("result_{i}")),
                    output: json!({"result": i * 2}),
                    is_error: false,
                }),
            ]
        })
        .collect();
    let receipt = receipt_with_trace(events);
    assert_eq!(receipt.trace.len(), 100);
    let hashed = receipt.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
}

#[test]
fn receipt_with_many_artifacts() {
    let mut builder = ReceiptBuilder::new("test").outcome(Outcome::Complete);
    for i in 0..50 {
        builder = builder.add_artifact(ArtifactRef {
            kind: "file".into(),
            path: format!("/tmp/output_{i}.txt"),
        });
    }
    let receipt = builder.build();
    assert_eq!(receipt.artifacts.len(), 50);
    let hash = receipt_hash(&receipt).unwrap();
    assert!(!hash.is_empty());
}

// ─── 12. Receipt comparison ──────────────────────────────────────────

#[test]
fn receipt_outcome_equality() {
    assert_eq!(Outcome::Complete, Outcome::Complete);
    assert_eq!(Outcome::Failed, Outcome::Failed);
    assert_eq!(Outcome::Partial, Outcome::Partial);
    assert_ne!(Outcome::Complete, Outcome::Failed);
    assert_ne!(Outcome::Complete, Outcome::Partial);
    assert_ne!(Outcome::Failed, Outcome::Partial);
}

#[test]
fn receipt_execution_mode_equality() {
    assert_eq!(ExecutionMode::Mapped, ExecutionMode::Mapped);
    assert_eq!(ExecutionMode::Passthrough, ExecutionMode::Passthrough);
    assert_ne!(ExecutionMode::Mapped, ExecutionMode::Passthrough);
}

#[test]
fn receipt_hash_differs_between_receipts() {
    let r1 = hashed_receipt();
    let r2 = hashed_receipt();
    // Different run_ids → different hashes
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn receipt_same_data_same_hash() {
    let receipt = minimal_receipt();
    let cloned = receipt.clone();
    let h1 = receipt.with_hash().unwrap();
    let h2 = cloned.with_hash().unwrap();
    assert_eq!(h1.receipt_sha256, h2.receipt_sha256);
}

#[test]
fn receipts_with_different_trace_different_hash() {
    let base = minimal_receipt();
    let mut with_trace = base.clone();
    with_trace.trace.push(make_event(AgentEventKind::Warning {
        message: "danger".into(),
    }));
    assert_ne!(
        receipt_hash(&base).unwrap(),
        receipt_hash(&with_trace).unwrap()
    );
}

#[test]
fn receipt_sha256_none_vs_some_comparison() {
    let r1 = minimal_receipt();
    let r2 = r1.clone().with_hash().unwrap();
    assert!(r1.receipt_sha256.is_none());
    assert!(r2.receipt_sha256.is_some());
}

// ─── 13. Serde roundtrip preserving hash ─────────────────────────────

#[test]
fn serde_roundtrip_preserves_hash() {
    let receipt = hashed_receipt();
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt.receipt_sha256, deserialized.receipt_sha256);
}

#[test]
fn serde_roundtrip_hash_still_valid() {
    let receipt = hashed_receipt();
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    let recomputed = receipt_hash(&deserialized).unwrap();
    assert_eq!(deserialized.receipt_sha256.unwrap(), recomputed);
}

#[test]
fn serde_roundtrip_no_hash() {
    let receipt = minimal_receipt();
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert!(deserialized.receipt_sha256.is_none());
}

#[test]
fn serde_roundtrip_with_trace() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "result text".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("wf_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "out.txt", "content": "data"}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "write_file".into(),
            tool_use_id: Some("wf_1".into()),
            output: json!("ok"),
            is_error: false,
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let receipt = receipt_with_trace(events).with_hash().unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt.trace.len(), deserialized.trace.len());
    assert_eq!(receipt.receipt_sha256, deserialized.receipt_sha256);
    let recomputed = receipt_hash(&deserialized).unwrap();
    assert_eq!(deserialized.receipt_sha256.unwrap(), recomputed);
}

#[test]
fn serde_roundtrip_preserves_all_fields() {
    let receipt = ReceiptBuilder::new("my-backend")
        .outcome(Outcome::Partial)
        .mode(ExecutionMode::Passthrough)
        .backend_version("3.0")
        .adapter_version("1.2")
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: Some(2),
            estimated_cost_usd: Some(0.03),
        })
        .usage_raw(json!({"raw": true}))
        .verification(CoreVerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M".into()),
            harness_ok: true,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "/out.patch".into(),
        })
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hi".into(),
        }))
        .build()
        .with_hash()
        .unwrap();

    let json = serde_json::to_string_pretty(&receipt).unwrap();
    let de: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(receipt.meta.run_id, de.meta.run_id);
    assert_eq!(receipt.meta.work_order_id, de.meta.work_order_id);
    assert_eq!(receipt.meta.contract_version, de.meta.contract_version);
    assert_eq!(receipt.meta.started_at, de.meta.started_at);
    assert_eq!(receipt.meta.finished_at, de.meta.finished_at);
    assert_eq!(receipt.meta.duration_ms, de.meta.duration_ms);
    assert_eq!(receipt.backend.id, de.backend.id);
    assert_eq!(receipt.backend.backend_version, de.backend.backend_version);
    assert_eq!(receipt.backend.adapter_version, de.backend.adapter_version);
    assert_eq!(receipt.mode, de.mode);
    assert_eq!(receipt.outcome, de.outcome);
    assert_eq!(receipt.usage.input_tokens, de.usage.input_tokens);
    assert_eq!(receipt.usage.output_tokens, de.usage.output_tokens);
    assert_eq!(receipt.usage_raw, de.usage_raw);
    assert_eq!(receipt.artifacts.len(), de.artifacts.len());
    assert_eq!(receipt.trace.len(), de.trace.len());
    assert_eq!(receipt.verification.harness_ok, de.verification.harness_ok);
    assert_eq!(receipt.receipt_sha256, de.receipt_sha256);
}

#[test]
fn serde_roundtrip_value_level() {
    let receipt = hashed_receipt();
    let value = serde_json::to_value(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_value(value).unwrap();
    assert_eq!(receipt.receipt_sha256, deserialized.receipt_sha256);
}

#[test]
fn serde_roundtrip_pretty_vs_compact() {
    let receipt = hashed_receipt();
    let compact = serde_json::to_string(&receipt).unwrap();
    let pretty = serde_json::to_string_pretty(&receipt).unwrap();
    let from_compact: Receipt = serde_json::from_str(&compact).unwrap();
    let from_pretty: Receipt = serde_json::from_str(&pretty).unwrap();
    assert_eq!(from_compact.receipt_sha256, from_pretty.receipt_sha256);
    assert_eq!(
        receipt_hash(&from_compact).unwrap(),
        receipt_hash(&from_pretty).unwrap()
    );
}

#[test]
fn serde_roundtrip_chain() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0, 100);
    let pid = r1.meta.run_id;
    let r2 = hashed_receipt_at(t0 + Duration::milliseconds(200), 100);
    let chain = ChainBuilder::new().push(r1).push_child(r2, pid).build();

    let json = serde_json::to_string(&chain).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(chain.len(), deserialized.len());

    let result = verify_chain(&deserialized);
    assert!(result.valid);
}

// ─── 14. ReceiptVerifier individual checks ──────────────────────────

#[test]
fn verifier_passes_valid_receipt() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::new_v4())
        .started_at(fixed_time())
        .finished_at(fixed_time() + Duration::milliseconds(100))
        .build()
        .with_hash()
        .unwrap();
    let verifier = ReceiptVerifier::new();
    let report = verifier.verify(&receipt);
    assert!(report.passed);
}

#[test]
fn verifier_detects_tampered_hash() {
    let mut receipt = hashed_receipt();
    receipt.receipt_sha256 = Some("deadbeef".repeat(8));
    let verifier = ReceiptVerifier::new();
    let report = verifier.verify(&receipt);
    assert!(!report.passed);
    assert!(report
        .checks
        .iter()
        .any(|c| c.name == "hash_integrity" && !c.passed));
}

#[test]
fn verifier_passes_no_hash() {
    let receipt = minimal_receipt();
    let verifier = ReceiptVerifier::new();
    let report = verifier.verify(&receipt);
    // No hash means hash check is skipped (passes)
    assert!(report
        .checks
        .iter()
        .any(|c| c.name == "hash_integrity" && c.passed));
}

#[test]
fn verifier_checks_contract_version() {
    let receipt = minimal_receipt();
    let verifier = ReceiptVerifier::new();
    let report = verifier.verify(&receipt);
    assert!(report
        .checks
        .iter()
        .any(|c| c.name == "contract_version" && c.passed));
}

#[test]
fn verifier_checks_timestamps() {
    let receipt = minimal_receipt();
    let verifier = ReceiptVerifier::new();
    let report = verifier.verify(&receipt);
    assert!(report
        .checks
        .iter()
        .any(|c| c.name == "timestamps" && c.passed));
}

// ─── 15. Additional hash edge cases ─────────────────────────────────

#[test]
fn sha256_hex_basic() {
    let hash = sha256_hex(b"hello");
    assert_eq!(hash.len(), 64);
    assert_eq!(
        hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha256_hex_empty_input() {
    let hash = sha256_hex(b"");
    assert_eq!(hash.len(), 64);
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_hex_deterministic() {
    let h1 = sha256_hex(b"test data");
    let h2 = sha256_hex(b"test data");
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_different_from_raw_json_hash() {
    // receipt_hash canonicalizes (sets receipt_sha256 to null)
    // so it should differ from just hashing the raw JSON when hash is present
    let receipt = hashed_receipt();
    let raw_json = serde_json::to_string(&receipt).unwrap();
    let raw_hash = sha256_hex(raw_json.as_bytes());
    let canonical_hash = receipt_hash(&receipt).unwrap();
    assert_ne!(raw_hash, canonical_hash);
}

#[test]
fn receipt_hash_matches_raw_json_hash_when_no_hash() {
    // When receipt_sha256 is None, serializing gives null for that field,
    // same as canonical form
    let receipt = minimal_receipt();
    let mut v = serde_json::to_value(&receipt).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    let canonical_json = serde_json::to_string(&v).unwrap();
    let manual = sha256_hex(canonical_json.as_bytes());
    let from_fn = receipt_hash(&receipt).unwrap();
    assert_eq!(manual, from_fn);
}

// ─── 16. ChainVerifier (simple verify) ──────────────────────────────

#[test]
fn chain_verifier_empty_chain() {
    let chain: Vec<Receipt> = vec![];
    let report = ChainVerifier::verify_chain(&chain);
    assert!(report.all_valid);
    assert_eq!(report.receipt_count, 0);
}

#[test]
fn chain_verifier_single_valid() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::new_v4())
        .started_at(fixed_time())
        .finished_at(fixed_time() + Duration::milliseconds(100))
        .build()
        .with_hash()
        .unwrap();
    let report = ChainVerifier::verify_chain(&[receipt]);
    assert!(report.all_valid);
}

#[test]
fn chain_verifier_ordered_chain() {
    let t0 = fixed_time();
    let wo = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .work_order_id(wo)
        .started_at(t0)
        .finished_at(t0 + Duration::milliseconds(100))
        .build()
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .work_order_id(wo)
        .started_at(t0 + Duration::milliseconds(200))
        .finished_at(t0 + Duration::milliseconds(300))
        .build()
        .with_hash()
        .unwrap();
    let r3 = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .work_order_id(wo)
        .started_at(t0 + Duration::milliseconds(400))
        .finished_at(t0 + Duration::milliseconds(500))
        .build()
        .with_hash()
        .unwrap();
    let report = ChainVerifier::verify_chain(&[r1, r2, r3]);
    assert!(report.all_valid);
}

#[test]
fn chain_verifier_out_of_order() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0 + Duration::seconds(5), 100);
    let r2 = hashed_receipt_at(t0, 100);
    let report = ChainVerifier::verify_chain(&[r1, r2]);
    assert!(!report.all_valid);
}

// ─── 17. All event kind variants in trace ────────────────────────────

#[test]
fn hash_with_run_started_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_run_completed_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_assistant_delta_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_assistant_message_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_tool_call_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tc1".into()),
        parent_tool_use_id: None,
        input: json!({}),
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_tool_result_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tr1".into()),
        output: json!("data"),
        is_error: false,
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_file_changed_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added function".into(),
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_command_executed_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok".into()),
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_warning_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::Warning {
        message: "low disk space".into(),
    })]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn hash_with_error_event() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::Error {
        message: "crash".into(),
        error_code: None,
    })]);
    assert!(receipt_hash(&r).is_ok());
}

// ─── 18. Extension fields ────────────────────────────────────────────

#[test]
fn event_ext_field_included_in_hash() {
    let r1 = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    })]);
    let mut r2 = r1.clone();
    let mut ext = BTreeMap::new();
    ext.insert("vendor_key".to_string(), json!("vendor_value"));
    r2.trace[0].ext = Some(ext);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn event_ext_none_vs_empty_map() {
    let mut r1 = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    })]);
    r1.trace[0].ext = None;
    let mut r2 = r1.clone();
    r2.trace[0].ext = Some(BTreeMap::new());
    // ext is skip_serializing_if = "Option::is_none", so None skips the field
    // Some(empty map) would serialize as {} — these differ
    // But both hash results are still valid
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    // They may or may not differ depending on serde behavior, just check both are valid
    assert_eq!(h1.len(), 64);
    assert_eq!(h2.len(), 64);
}

// ─── 19. Builder edge cases ──────────────────────────────────────────

#[test]
fn builder_default_outcome() {
    // Default outcome for builder — check it's some valid variant
    let receipt = ReceiptBuilder::new("test").build();
    // Just verify it serializes (all Outcome variants are valid)
    let json = serde_json::to_string(&receipt).unwrap();
    assert!(json.contains("outcome"));
}

#[test]
fn builder_with_hash_method() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.unwrap(), recomputed);
}

#[test]
fn builder_multiple_trace_events() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "x".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "ok".into(),
        }))
        .build();
    assert_eq!(receipt.trace.len(), 3);
}

#[test]
fn builder_chained_calls() {
    let wo_id = Uuid::new_v4();
    let t0 = fixed_time();
    let receipt = ReceiptBuilder::new("chain-test")
        .outcome(Outcome::Complete)
        .work_order_id(wo_id)
        .started_at(t0)
        .finished_at(t0 + Duration::milliseconds(200))
        .mode(ExecutionMode::Passthrough)
        .backend_version("1.0")
        .adapter_version("2.0")
        .usage_raw(json!({"test": 1}))
        .usage(UsageNormalized {
            input_tokens: Some(10),
            output_tokens: Some(5),
            ..Default::default()
        })
        .verification(CoreVerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: true,
        })
        .add_artifact(ArtifactRef {
            kind: "file".into(),
            path: "out.txt".into(),
        })
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }))
        .with_hash()
        .unwrap();

    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert!(receipt.receipt_sha256.is_some());
}

// ─── 20. Large chain validation ──────────────────────────────────────

#[test]
fn chain_10_receipts_valid() {
    let t0 = fixed_time();
    let mut builder = ChainBuilder::new();
    for i in 0..10 {
        let r = hashed_receipt_at(t0 + Duration::seconds(i), 100);
        builder = builder.push(r);
    }
    let chain = builder.build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 10);
}

#[test]
fn chain_linear_parent_links() {
    let t0 = fixed_time();
    let r0 = hashed_receipt_at(t0, 100);
    let mut builder = ChainBuilder::new().push(r0.clone());
    let mut prev_id = r0.meta.run_id;
    for i in 1..5 {
        let r = hashed_receipt_at(t0 + Duration::seconds(i), 100);
        let this_id = r.meta.run_id;
        builder = builder.push_child(r, prev_id);
        prev_id = this_id;
    }
    let chain = builder.build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 5);
}

// ─── 21. Miscellaneous ──────────────────────────────────────────────

#[test]
fn contract_version_is_correct() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn receipt_builder_new_receipt_has_no_hash() {
    let receipt = ReceiptBuilder::new("test").build();
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn receipt_with_all_outcomes() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let receipt = ReceiptBuilder::new("test")
            .outcome(outcome)
            .build()
            .with_hash()
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
    }
}

#[test]
fn receipt_with_all_execution_modes() {
    for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .mode(mode)
            .build()
            .with_hash()
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
    }
}

#[test]
fn receipt_hash_error_type() {
    // receipt_hash should return Ok for valid receipts
    let result = receipt_hash(&minimal_receipt());
    assert!(result.is_ok());
}

#[test]
fn multiple_with_hash_calls_stable() {
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = r1.clone().with_hash().unwrap();
    let r3 = r2.clone().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r2.receipt_sha256, r3.receipt_sha256);
}

#[test]
fn receipt_hash_len_always_64() {
    for i in 0..20 {
        let mut r = minimal_receipt();
        r.meta.duration_ms = i;
        let h = receipt_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
    }
}

#[test]
fn chain_serde_roundtrip_preserves_validity() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0, 50);
    let r2 = hashed_receipt_at(t0 + Duration::milliseconds(100), 75);
    let chain = ChainBuilder::new().push(r1).push(r2).build();

    let json = serde_json::to_string(&chain).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();

    let before = verify_chain(&chain);
    let after = verify_chain(&deserialized);
    assert_eq!(before.valid, after.valid);
    assert_eq!(before.chain_length, after.chain_length);
    assert_eq!(before.total_events, after.total_events);
}

#[test]
fn receipt_with_unicode_content_hashes() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "こんにちは世界 🌍 émojis".into(),
    })]);
    let hash = receipt_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn receipt_with_special_chars_hashes() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "line1\nline2\ttab \"quotes\" \\backslash".into(),
    })]);
    let hash = receipt_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn receipt_with_empty_strings_hashes() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::AssistantMessage {
        text: "".into(),
    })]);
    let hash = receipt_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn receipt_with_null_json_values_hashes() {
    let r = receipt_with_trace(vec![make_event(AgentEventKind::ToolCall {
        tool_name: "test".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::Value::Null,
    })]);
    let hash = receipt_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn chain_verification_report_fields() {
    let t0 = fixed_time();
    let r1 = hashed_receipt_at(t0, 100);
    let r2 = hashed_receipt_at(t0 + Duration::milliseconds(200), 200);
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 2);
    assert_eq!(result.total_duration_ms, 300);
    assert!(result.errors.is_empty());
}
