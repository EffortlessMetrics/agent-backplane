#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deep comprehensive tests for the `abp-receipt` crate covering receipt
//! canonicalization, hashing, chain building, chain verification, builder
//! patterns, deterministic hashing, edge cases, auditing, diffing,
//! validation, serialization formats, and the in-memory store.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_core::ArtifactRef;
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_receipt::verify::{verify_receipt, ReceiptAuditor};
use abp_receipt::{
    canonicalize, compute_hash, diff_receipts, verify_hash, AgentEvent, AgentEventKind,
    ChainBuilder, ChainError, ExecutionMode, Outcome, Receipt, ReceiptBuilder, ReceiptChain,
    ReceiptValidator, UsageNormalized, VerificationReport, CONTRACT_VERSION,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn fixed_time() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn make_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build()
}

fn make_hashed_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .with_hash()
        .unwrap()
}

/// Creates a chain-compatible hashed receipt with a specific run_id and
/// a `started_at` offset by `offset_secs` from the fixed base time.
fn make_chain_receipt(backend: &str, offset_secs: i64) -> Receipt {
    let base = fixed_time();
    let start = base + chrono::Duration::seconds(offset_secs);
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(start)
        .with_hash()
        .unwrap()
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Receipt Hash Computation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn compute_hash_returns_64_hex_chars() {
    let r = make_receipt("mock");
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64, "SHA-256 hex should be 64 characters");
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn compute_hash_hex_is_lowercase() {
    let r = make_receipt("mock");
    let h = compute_hash(&r).unwrap();
    assert_eq!(h, h.to_lowercase());
}

#[test]
fn compute_hash_is_deterministic_same_receipt() {
    let r = make_receipt("mock");
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2, "Same receipt must produce same hash");
}

#[test]
fn compute_hash_is_deterministic_cloned_receipt() {
    let r = make_receipt("mock");
    let clone = r.clone();
    assert_eq!(compute_hash(&r).unwrap(), compute_hash(&clone).unwrap());
}

#[test]
fn compute_hash_differs_for_different_backends() {
    let r1 = make_receipt("backend-a");
    let r2 = make_receipt("backend-b");
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn compute_hash_differs_for_different_outcomes() {
    let r1 = ReceiptBuilder::new("m")
        .outcome(Outcome::Complete)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let r2 = ReceiptBuilder::new("m")
        .outcome(Outcome::Failed)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn compute_hash_differs_for_different_run_ids() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("m")
        .run_id(id1)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let r2 = ReceiptBuilder::new("m")
        .run_id(id2)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Self-referential prevention: receipt_sha256 set to null before hash
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn canonicalize_sets_receipt_sha256_to_null() {
    let mut r = make_receipt("mock");
    r.receipt_sha256 = Some("some-existing-hash".to_string());
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed["receipt_sha256"].is_null());
}

#[test]
fn hash_independent_of_stored_hash_value() {
    let r = make_receipt("mock");
    let h_none = compute_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("garbage".to_string());
    let h_garbage = compute_hash(&r2).unwrap();

    let mut r3 = r.clone();
    r3.receipt_sha256 = Some(h_none.clone());
    let h_with_correct = compute_hash(&r3).unwrap();

    assert_eq!(h_none, h_garbage, "Hash must be independent of stored hash");
    assert_eq!(
        h_none, h_with_correct,
        "Hash must be independent even when stored hash is the correct one"
    );
}

#[test]
fn canonicalize_always_nullifies_hash_field() {
    let mut r = make_receipt("mock");
    // Even with None, the field should appear as null
    r.receipt_sha256 = None;
    let json1 = canonicalize(&r).unwrap();

    r.receipt_sha256 = Some("abc123".into());
    let json2 = canonicalize(&r).unwrap();

    assert_eq!(json1, json2, "Canonical form ignores receipt_sha256 value");
}

#[test]
fn with_hash_produces_matching_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let recomputed = compute_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_deref(), Some(recomputed.as_str()));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. verify_hash
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn verify_hash_true_when_no_stored_hash() {
    let r = make_receipt("mock");
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_true_when_correct_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_false_when_tampered_hash() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("tampered_value".into());
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_false_when_content_tampered() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    // Tamper with the content while keeping the old hash
    r.backend.id = "hacked-backend".into();
    assert!(!verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Canonical JSON Serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn canonicalize_produces_valid_json() {
    let r = make_receipt("mock");
    let json = canonicalize(&r).unwrap();
    let _: serde_json::Value = serde_json::from_str(&json).unwrap();
}

#[test]
fn canonicalize_is_deterministic() {
    let r = make_receipt("mock");
    let json1 = canonicalize(&r).unwrap();
    let json2 = canonicalize(&r).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn canonicalize_has_no_pretty_printing() {
    let r = make_receipt("mock");
    let json = canonicalize(&r).unwrap();
    // Compact JSON should not have newlines (except inside string values)
    assert!(!json.contains('\n'), "Canonical JSON must be compact");
}

#[test]
fn canonicalize_contains_contract_version() {
    let r = make_receipt("mock");
    let json = canonicalize(&r).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn canonicalize_contains_backend_id() {
    let r = make_receipt("my-backend");
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("my-backend"));
}

#[test]
fn canonicalize_contains_outcome() {
    let r = make_receipt("mock");
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"complete\""));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Receipt Builder Patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builder_new_sets_backend_id() {
    let r = ReceiptBuilder::new("test-backend").build();
    assert_eq!(r.backend.id, "test-backend");
}

#[test]
fn builder_defaults_complete_outcome() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn builder_defaults_no_hash() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn builder_defaults_empty_trace() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.trace.is_empty());
}

#[test]
fn builder_defaults_empty_artifacts() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.artifacts.is_empty());
}

#[test]
fn builder_defaults_mapped_mode() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn builder_defaults_contract_version() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn builder_defaults_nil_work_order_id() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.meta.work_order_id, Uuid::nil());
}

#[test]
fn builder_outcome_sets_outcome() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn builder_outcome_partial() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn builder_backend_version() {
    let r = ReceiptBuilder::new("b").backend_version("1.2.3").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
}

#[test]
fn builder_adapter_version() {
    let r = ReceiptBuilder::new("b").adapter_version("0.5.0").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5.0"));
}

#[test]
fn builder_backend_alias() {
    let r = ReceiptBuilder::new("x").backend("y").build();
    assert_eq!(r.backend.id, "y");
}

#[test]
fn builder_backend_id_overrides() {
    let r = ReceiptBuilder::new("x").backend_id("z").build();
    assert_eq!(r.backend.id, "z");
}

#[test]
fn builder_model_in_usage_raw() {
    let r = ReceiptBuilder::new("b").model("gpt-4").build();
    assert_eq!(r.usage_raw["model"], "gpt-4");
}

#[test]
fn builder_dialect_in_usage_raw() {
    let r = ReceiptBuilder::new("b").dialect("openai").build();
    assert_eq!(r.usage_raw["dialect"], "openai");
}

#[test]
fn builder_model_and_dialect_coexist() {
    let r = ReceiptBuilder::new("b")
        .model("claude-3")
        .dialect("anthropic")
        .build();
    assert_eq!(r.usage_raw["model"], "claude-3");
    assert_eq!(r.usage_raw["dialect"], "anthropic");
}

#[test]
fn builder_started_at_and_finished_at() {
    let start = Utc.with_ymd_and_hms(2024, 6, 1, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 6, 1, 10, 0, 5).unwrap();
    let r = ReceiptBuilder::new("b")
        .started_at(start)
        .finished_at(end)
        .build();
    assert_eq!(r.meta.started_at, start);
    assert_eq!(r.meta.finished_at, end);
    assert_eq!(r.meta.duration_ms, 5000);
}

#[test]
fn builder_duration_adjusts_finished_at() {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("b")
        .started_at(start)
        .duration(Duration::from_millis(3500))
        .build();
    assert_eq!(r.meta.duration_ms, 3500);
}

#[test]
fn builder_work_order_id() {
    let wo_id = Uuid::new_v4();
    let r = ReceiptBuilder::new("b").work_order_id(wo_id).build();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[test]
fn builder_run_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("b").run_id(id).build();
    assert_eq!(r.meta.run_id, id);
}

#[test]
fn builder_mode_passthrough() {
    let r = ReceiptBuilder::new("b")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn builder_usage_tokens() {
    let r = ReceiptBuilder::new("b").usage_tokens(100, 200).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(200));
}

#[test]
fn builder_usage_raw_arbitrary_json() {
    let raw = serde_json::json!({"custom_field": 42, "nested": {"a": true}});
    let r = ReceiptBuilder::new("b").usage_raw(raw.clone()).build();
    assert_eq!(r.usage_raw["custom_field"], 42);
    assert_eq!(r.usage_raw["nested"]["a"], true);
}

#[test]
fn builder_add_event() {
    let evt = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b").add_event(evt).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn builder_add_trace_event_alias() {
    let evt = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b").add_trace_event(evt).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn builder_events_replaces_trace() {
    let e1 = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "a".into(),
        },
        ext: None,
    };
    let e2 = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunCompleted {
            message: "b".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b")
        .add_event(e1)
        .events(vec![e2])
        .build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn builder_add_artifact() {
    let art = ArtifactRef {
        kind: "patch".into(),
        path: "out.patch".into(),
    };
    let r = ReceiptBuilder::new("b").add_artifact(art).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn builder_error_sets_failed_outcome_and_adds_error_event() {
    let r = ReceiptBuilder::new("b").error("something broke").build();
    assert_eq!(r.outcome, Outcome::Failed);
    assert_eq!(r.trace.len(), 1);
    match &r.trace[0].kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "something broke");
            assert!(error_code.is_none());
        }
        _ => panic!("Expected Error event"),
    }
}

#[test]
fn builder_verification_report() {
    let report = VerificationReport {
        git_diff: Some("diff content".into()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("b").verification(report).build();
    assert!(r.verification.harness_ok);
    assert_eq!(r.verification.git_diff.as_deref(), Some("diff content"));
}

#[test]
fn builder_with_hash_produces_hashed_receipt() {
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn builder_chaining_all_methods() {
    let id = Uuid::new_v4();
    let wo = Uuid::new_v4();
    let r = ReceiptBuilder::new("full")
        .backend_version("v1")
        .adapter_version("a1")
        .model("gpt-4")
        .dialect("openai")
        .outcome(Outcome::Partial)
        .work_order_id(wo)
        .run_id(id)
        .started_at(fixed_time())
        .finished_at(fixed_time() + chrono::Duration::seconds(10))
        .mode(ExecutionMode::Passthrough)
        .usage_tokens(50, 75)
        .build();
    assert_eq!(r.backend.id, "full");
    assert_eq!(r.meta.run_id, id);
    assert_eq!(r.meta.work_order_id, wo);
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.meta.duration_ms, 10000);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Receipt Chain Building
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_new_is_empty() {
    let c = ReceiptChain::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn chain_push_single_receipt() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("mock", 0)).unwrap();
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
}

#[test]
fn chain_push_multiple_receipts_in_order() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();
    c.push(make_chain_receipt("c", 2)).unwrap();
    assert_eq!(c.len(), 3);
}

#[test]
fn chain_latest_returns_last() {
    let mut c = ReceiptChain::new();
    let r = make_chain_receipt("last", 0);
    let id = r.meta.run_id;
    c.push(r).unwrap();
    assert_eq!(c.latest().unwrap().meta.run_id, id);
}

#[test]
fn chain_get_by_index() {
    let mut c = ReceiptChain::new();
    let r0 = make_chain_receipt("a", 0);
    let id0 = r0.meta.run_id;
    c.push(r0).unwrap();
    let r1 = make_chain_receipt("b", 1);
    let id1 = r1.meta.run_id;
    c.push(r1).unwrap();

    assert_eq!(c.get(0).unwrap().meta.run_id, id0);
    assert_eq!(c.get(1).unwrap().meta.run_id, id1);
    assert!(c.get(2).is_none());
}

#[test]
fn chain_sequence_at() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();
    assert_eq!(c.sequence_at(0), Some(0));
    assert_eq!(c.sequence_at(1), Some(1));
    assert_eq!(c.sequence_at(2), None);
}

#[test]
fn chain_parent_hash_at() {
    let mut c = ReceiptChain::new();
    let r0 = make_chain_receipt("a", 0);
    let hash0 = r0.receipt_sha256.clone();
    c.push(r0).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();

    assert!(c.parent_hash_at(0).is_none()); // first has no parent
    assert_eq!(c.parent_hash_at(1), hash0.as_deref());
}

#[test]
fn chain_iter() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();
    let count = c.iter().count();
    assert_eq!(count, 2);
}

#[test]
fn chain_into_iter() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    let count = (&c).into_iter().count();
    assert_eq!(count, 1);
}

#[test]
fn chain_as_slice() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    assert_eq!(c.as_slice().len(), 1);
}

#[test]
fn chain_rejects_duplicate_run_id() {
    let mut c = ReceiptChain::new();
    let r = make_chain_receipt("a", 0);
    let dup = r.clone();
    c.push(r).unwrap();
    let err = c.push(dup).unwrap_err();
    assert!(matches!(err, ChainError::DuplicateId { .. }));
}

#[test]
fn chain_rejects_out_of_order_timestamps() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 10)).unwrap();
    let err = c.push(make_chain_receipt("b", 0)).unwrap_err();
    assert!(matches!(err, ChainError::BrokenLink { index: 1 }));
}

#[test]
fn chain_rejects_tampered_hash() {
    let mut c = ReceiptChain::new();
    let mut r = make_chain_receipt("a", 0);
    r.receipt_sha256 = Some("bad_hash".into());
    let err = c.push(r).unwrap_err();
    assert!(matches!(err, ChainError::HashMismatch { index: 0 }));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Chain Verification — Valid Chains
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_verify_single_receipt() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    assert!(c.verify().is_ok());
}

#[test]
fn chain_verify_multiple_receipts() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();
    c.push(make_chain_receipt("c", 2)).unwrap();
    assert!(c.verify().is_ok());
}

#[test]
fn chain_verify_chain_comprehensive() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();
    assert!(c.verify_chain().is_ok());
}

#[test]
fn chain_find_gaps_empty_on_contiguous() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();
    assert!(c.find_gaps().is_empty());
}

#[test]
fn chain_detect_tampering_clean_chain() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();
    assert!(c.detect_tampering().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Chain Verification Failure Cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_verify_empty_fails() {
    let c = ReceiptChain::new();
    let err = c.verify().unwrap_err();
    assert!(matches!(err, ChainError::EmptyChain));
}

#[test]
fn chain_verify_chain_empty_fails() {
    let c = ReceiptChain::new();
    let err = c.verify_chain().unwrap_err();
    assert!(matches!(err, ChainError::EmptyChain));
}

#[test]
fn chain_detect_tampering_hash_mismatch() {
    // Build a chain with skip_validation, then tamper with a hash
    let r0 = make_chain_receipt("a", 0);
    let mut r1 = make_chain_receipt("b", 1);
    r1.receipt_sha256 = Some("definitely_not_right".into());

    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r0)
        .unwrap()
        .append(r1)
        .unwrap()
        .build();

    let evidence = chain.detect_tampering();
    assert!(!evidence.is_empty());
    assert!(evidence.iter().any(|e| e.index == 1));
}

#[test]
fn chain_builder_skip_validation_allows_bad_hash() {
    let mut r = make_chain_receipt("a", 0);
    r.receipt_sha256 = Some("broken".into());

    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r)
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_with_validation_rejects_bad_hash() {
    let mut r = make_chain_receipt("a", 0);
    r.receipt_sha256 = Some("broken".into());

    let result = ChainBuilder::new().append(r);
    assert!(result.is_err());
}

#[test]
fn chain_builder_append_with_sequence_creates_gaps() {
    let r0 = make_chain_receipt("a", 0);
    let r1 = make_chain_receipt("b", 1);

    let chain = ChainBuilder::new()
        .append_with_sequence(r0, 0)
        .unwrap()
        .append_with_sequence(r1, 5)
        .unwrap()
        .build();

    let gaps = chain.find_gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].expected, 1);
    assert_eq!(gaps[0].actual, 5);
}

#[test]
fn chain_verify_chain_detects_sequence_gap() {
    let r0 = make_chain_receipt("a", 0);
    let r1 = make_chain_receipt("b", 1);

    let chain = ChainBuilder::new()
        .append_with_sequence(r0, 0)
        .unwrap()
        .append_with_sequence(r1, 3)
        .unwrap()
        .build();

    let err = chain.verify_chain().unwrap_err();
    assert!(matches!(
        err,
        ChainError::SequenceGap {
            expected: 1,
            actual: 3
        }
    ));
}

#[test]
fn chain_builder_duplicate_id_with_validation() {
    let r = make_chain_receipt("a", 0);
    let dup = r.clone();

    let result = ChainBuilder::new().append(r).unwrap().append(dup);
    assert!(result.is_err());
}

#[test]
fn chain_builder_duplicate_id_with_append_with_sequence() {
    let r = make_chain_receipt("a", 0);
    let dup = r.clone();

    let result = ChainBuilder::new()
        .append_with_sequence(r, 0)
        .unwrap()
        .append_with_sequence(dup, 1);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Deterministic Hashing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn deterministic_hash_100_iterations() {
    let r = make_receipt("determinism-test");
    let expected = compute_hash(&r).unwrap();
    for _ in 0..100 {
        assert_eq!(compute_hash(&r).unwrap(), expected);
    }
}

#[test]
fn deterministic_hash_after_clone() {
    let r = make_receipt("clone-test");
    let h1 = compute_hash(&r).unwrap();
    let clone = r.clone();
    let h2 = compute_hash(&clone).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn deterministic_hash_after_serialization_roundtrip() {
    let r = make_hashed_receipt("serde-test");
    let json = serde_json::to_string(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(
        compute_hash(&r).unwrap(),
        compute_hash(&deserialized).unwrap()
    );
}

#[test]
fn deterministic_canonical_json() {
    let r = make_receipt("canonical-test");
    let json1 = canonicalize(&r).unwrap();
    let json2 = canonicalize(&r).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn hash_with_usage_tokens_deterministic() {
    let r = ReceiptBuilder::new("b")
        .usage_tokens(500, 1000)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Chain Summary
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_summary_empty_chain() {
    let c = ReceiptChain::new();
    let s = c.chain_summary();
    assert_eq!(s.total_receipts, 0);
    assert_eq!(s.complete_count, 0);
    assert_eq!(s.failed_count, 0);
    assert_eq!(s.partial_count, 0);
    assert!(s.backends.is_empty());
    assert!(s.first_started_at.is_none());
    assert!(s.last_finished_at.is_none());
    assert!(s.all_hashes_valid);
}

#[test]
fn chain_summary_counts_outcomes() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();

    let r_failed = ReceiptBuilder::new("b")
        .outcome(Outcome::Failed)
        .started_at(fixed_time() + chrono::Duration::seconds(1))
        .finished_at(fixed_time() + chrono::Duration::seconds(1))
        .with_hash()
        .unwrap();
    c.push(r_failed).unwrap();

    let r_partial = ReceiptBuilder::new("c")
        .outcome(Outcome::Partial)
        .started_at(fixed_time() + chrono::Duration::seconds(2))
        .finished_at(fixed_time() + chrono::Duration::seconds(2))
        .with_hash()
        .unwrap();
    c.push(r_partial).unwrap();

    let s = c.chain_summary();
    assert_eq!(s.total_receipts, 3);
    assert_eq!(s.complete_count, 1);
    assert_eq!(s.failed_count, 1);
    assert_eq!(s.partial_count, 1);
}

#[test]
fn chain_summary_aggregates_tokens() {
    let mut c = ReceiptChain::new();

    let r1 = ReceiptBuilder::new("a")
        .usage_tokens(100, 200)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .with_hash()
        .unwrap();
    c.push(r1).unwrap();

    let r2 = ReceiptBuilder::new("b")
        .usage_tokens(300, 400)
        .started_at(fixed_time() + chrono::Duration::seconds(1))
        .finished_at(fixed_time() + chrono::Duration::seconds(1))
        .with_hash()
        .unwrap();
    c.push(r2).unwrap();

    let s = c.chain_summary();
    assert_eq!(s.total_input_tokens, 400);
    assert_eq!(s.total_output_tokens, 600);
}

#[test]
fn chain_summary_collects_backends() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("alpha", 0)).unwrap();
    c.push(make_chain_receipt("beta", 1)).unwrap();
    c.push(make_chain_receipt("alpha", 2)).unwrap();

    let s = c.chain_summary();
    assert_eq!(s.backends.len(), 2);
    assert!(s.backends.contains(&"alpha".to_string()));
    assert!(s.backends.contains(&"beta".to_string()));
}

#[test]
fn chain_summary_duration_aggregation() {
    let mut c = ReceiptChain::new();

    let r1 = ReceiptBuilder::new("a")
        .started_at(fixed_time())
        .finished_at(fixed_time() + chrono::Duration::seconds(5))
        .with_hash()
        .unwrap();
    c.push(r1).unwrap();

    let r2 = ReceiptBuilder::new("b")
        .started_at(fixed_time() + chrono::Duration::seconds(10))
        .finished_at(fixed_time() + chrono::Duration::seconds(13))
        .with_hash()
        .unwrap();
    c.push(r2).unwrap();

    let s = c.chain_summary();
    assert_eq!(s.total_duration_ms, 8000);
}

#[test]
fn chain_summary_timestamps() {
    let mut c = ReceiptChain::new();

    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 1, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 1, 1, 2, 0, 0).unwrap();
    let t4 = Utc.with_ymd_and_hms(2025, 1, 1, 3, 0, 0).unwrap();

    let r1 = ReceiptBuilder::new("a")
        .started_at(t1)
        .finished_at(t2)
        .with_hash()
        .unwrap();
    c.push(r1).unwrap();

    let r2 = ReceiptBuilder::new("b")
        .started_at(t3)
        .finished_at(t4)
        .with_hash()
        .unwrap();
    c.push(r2).unwrap();

    let s = c.chain_summary();
    assert_eq!(s.first_started_at, Some(t1));
    assert_eq!(s.last_finished_at, Some(t4));
}

#[test]
fn chain_summary_all_hashes_valid() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    assert!(c.chain_summary().all_hashes_valid);
}

#[test]
fn chain_summary_gap_count() {
    let r0 = make_chain_receipt("a", 0);
    let r1 = make_chain_receipt("b", 1);

    let chain = ChainBuilder::new()
        .append_with_sequence(r0, 0)
        .unwrap()
        .append_with_sequence(r1, 5)
        .unwrap()
        .build();

    assert_eq!(chain.chain_summary().gap_count, 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Chain Serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_serialize_roundtrip() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();
    c.push(make_chain_receipt("b", 1)).unwrap();

    let json = serde_json::to_string(&c).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.len(), 2);
}

#[test]
fn chain_serializes_as_array() {
    let mut c = ReceiptChain::new();
    c.push(make_chain_receipt("a", 0)).unwrap();

    let json = serde_json::to_string(&c).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.is_array());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Verification (verify module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn verify_receipt_valid() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let result = verify_receipt(&r);
    assert!(result.is_verified());
    assert!(result.issues.is_empty());
}

#[test]
fn verify_receipt_no_hash_still_valid() {
    let r = make_receipt("mock");
    let result = verify_receipt(&r);
    assert!(result.hash_valid);
}

#[test]
fn verify_receipt_tampered_hash() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("bad".into());
    let result = verify_receipt(&r);
    assert!(!result.hash_valid);
    assert!(!result.is_verified());
}

#[test]
fn verify_receipt_wrong_contract_version() {
    let mut r = make_receipt("mock");
    r.meta.contract_version = "wrong/v99".into();
    let result = verify_receipt(&r);
    assert!(!result.contract_valid);
    assert!(!result.is_verified());
}

#[test]
fn verify_receipt_timestamps_finished_before_started() {
    let start = fixed_time();
    let end = start - chrono::Duration::seconds(10);
    let mut r = make_receipt("mock");
    r.meta.started_at = start;
    r.meta.finished_at = end;
    let result = verify_receipt(&r);
    assert!(!result.timestamps_valid);
}

#[test]
fn verify_receipt_duration_mismatch() {
    let mut r = make_receipt("mock");
    r.meta.duration_ms = 99999;
    let result = verify_receipt(&r);
    assert!(!result.timestamps_valid);
}

#[test]
fn verify_receipt_outcome_failed_without_error_events() {
    let evt = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Failed)
        .add_event(evt)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let result = verify_receipt(&r);
    assert!(!result.outcome_consistent);
}

#[test]
fn verify_receipt_outcome_complete_with_error_events() {
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Complete)
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        })
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let result = verify_receipt(&r);
    assert!(!result.outcome_consistent);
}

#[test]
fn verification_result_display_verified() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let result = verify_receipt(&r);
    assert!(result.to_string().contains("verified"));
}

#[test]
fn verification_result_display_failed() {
    let mut r = make_receipt("mock");
    r.receipt_sha256 = Some("bad".into());
    let result = verify_receipt(&r);
    assert!(result.to_string().contains("failed"));
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Auditor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn auditor_clean_batch() {
    let auditor = ReceiptAuditor::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let report = auditor.audit_batch(&[r]);
    assert!(report.is_clean());
    assert_eq!(report.total, 1);
    assert_eq!(report.valid, 1);
    assert_eq!(report.invalid, 0);
}

#[test]
fn auditor_empty_batch() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[]);
    assert!(report.is_clean());
    assert_eq!(report.total, 0);
}

#[test]
fn auditor_detects_invalid_receipt() {
    let auditor = ReceiptAuditor::new();
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("tampered".into());
    let report = auditor.audit_batch(&[r]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
}

#[test]
fn auditor_detects_duplicate_run_ids() {
    let auditor = ReceiptAuditor::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(fixed_time() + chrono::Duration::seconds(1))
        .finished_at(fixed_time() + chrono::Duration::seconds(1))
        .build();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(!report.is_clean());
    assert!(report
        .issues
        .iter()
        .any(|i| i.description.contains("duplicate run_id")));
}

#[test]
fn auditor_report_display() {
    let auditor = ReceiptAuditor::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let report = auditor.audit_batch(&[r]);
    let s = report.to_string();
    assert!(s.contains("AuditReport"));
    assert!(s.contains("total: 1"));
}

#[test]
fn audit_issue_display_with_index_and_run_id() {
    let issue = abp_receipt::verify::AuditIssue {
        receipt_index: Some(0),
        run_id: Some("abc".into()),
        description: "test issue".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("#0"));
    assert!(s.contains("abc"));
    assert!(s.contains("test issue"));
}

#[test]
fn audit_issue_display_no_index_no_run_id() {
    let issue = abp_receipt::verify::AuditIssue {
        receipt_index: None,
        run_id: None,
        description: "orphan issue".into(),
    };
    assert_eq!(issue.to_string(), "orphan issue");
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Validation (ReceiptValidator)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validator_valid_receipt() {
    let v = ReceiptValidator::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(v.validate(&r).is_ok());
}

#[test]
fn validator_wrong_contract_version() {
    let v = ReceiptValidator::new();
    let mut r = make_receipt("mock");
    r.meta.contract_version = "bad".into();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.contract_version"));
}

#[test]
fn validator_bad_hash() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("wrong".into());
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "receipt_sha256"));
}

#[test]
fn validator_empty_backend_id() {
    let v = ReceiptValidator::new();
    let r = ReceiptBuilder::new("").build();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "backend.id"));
}

#[test]
fn validator_finished_before_started() {
    let v = ReceiptValidator::new();
    let mut r = make_receipt("mock");
    r.meta.finished_at = r.meta.started_at - chrono::Duration::seconds(1);
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.finished_at"));
}

#[test]
fn validator_duration_mismatch() {
    let v = ReceiptValidator::new();
    let mut r = make_receipt("mock");
    r.meta.duration_ms = 999999;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.duration_ms"));
}

#[test]
fn validator_collects_all_errors() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("").build();
    r.meta.contract_version = "wrong".into();
    r.meta.duration_ms = 9999;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.len() >= 3);
}

#[test]
fn validation_error_display() {
    let err = abp_receipt::ValidationError {
        field: "test_field".into(),
        message: "bad value".into(),
    };
    let s = err.to_string();
    assert!(s.contains("test_field"));
    assert!(s.contains("bad value"));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Diffing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn diff_identical_receipts_is_empty() {
    let r = make_receipt("mock");
    let diff = diff_receipts(&r, &r.clone());
    assert!(diff.is_empty());
    assert_eq!(diff.len(), 0);
}

#[test]
fn diff_detects_backend_change() {
    let a = make_receipt("old");
    let mut b = a.clone();
    b.backend.id = "new".into();
    let diff = diff_receipts(&a, &b);
    assert!(!diff.is_empty());
    assert!(diff.changes.iter().any(|d| d.field == "backend.id"));
}

#[test]
fn diff_detects_outcome_change() {
    let a = make_receipt("mock");
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "outcome"));
}

#[test]
fn diff_detects_mode_change() {
    let a = make_receipt("mock");
    let mut b = a.clone();
    b.mode = ExecutionMode::Passthrough;
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "mode"));
}

#[test]
fn diff_detects_trace_length_change() {
    let a = make_receipt("mock");
    let mut b = a.clone();
    b.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "hi".into(),
        },
        ext: None,
    });
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "trace.len"));
}

#[test]
fn diff_detects_artifacts_length_change() {
    let a = make_receipt("mock");
    let mut b = a.clone();
    b.artifacts.push(ArtifactRef {
        kind: "log".into(),
        path: "out.log".into(),
    });
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "artifacts.len"));
}

#[test]
fn diff_detects_usage_raw_change() {
    let a = make_receipt("mock");
    let mut b = a.clone();
    b.usage_raw = serde_json::json!({"new_key": 42});
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "usage_raw"));
}

#[test]
fn diff_detects_contract_version_change() {
    let a = make_receipt("mock");
    let mut b = a.clone();
    b.meta.contract_version = "different".into();
    let diff = diff_receipts(&a, &b);
    assert!(diff
        .changes
        .iter()
        .any(|d| d.field == "meta.contract_version"));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Serde Formats
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn serde_format_to_json_roundtrip() {
    let r = make_hashed_receipt("mock");
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let back = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(back.meta.run_id, r.meta.run_id);
    assert_eq!(back.receipt_sha256, r.receipt_sha256);
}

#[test]
fn serde_format_to_bytes_roundtrip() {
    let r = make_hashed_receipt("mock");
    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let back = abp_receipt::serde_formats::from_bytes(&bytes).unwrap();
    assert_eq!(back.meta.run_id, r.meta.run_id);
}

#[test]
fn serde_format_to_json_pretty() {
    let r = make_receipt("mock");
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    assert!(json.contains('\n'), "Pretty JSON should have newlines");
}

#[test]
fn serde_format_to_bytes_compact() {
    let r = make_receipt("mock");
    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(!s.contains('\n'), "Compact bytes should have no newlines");
}

#[test]
fn serde_format_json_preserves_hash() {
    let r = make_hashed_receipt("mock");
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let back = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert!(verify_hash(&back));
}

#[test]
fn serde_format_from_json_invalid_returns_error() {
    let result = abp_receipt::serde_formats::from_json("not json");
    assert!(result.is_err());
}

#[test]
fn serde_format_from_bytes_invalid_returns_error() {
    let result = abp_receipt::serde_formats::from_bytes(b"not json");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 17. In-Memory Store
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn store_new_is_empty() {
    let store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

#[test]
fn store_insert_and_retrieve() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("mock");
    let id = r.meta.run_id;
    store.store(r).unwrap();
    assert_eq!(store.len(), 1);

    let fetched = store.get(id).unwrap().unwrap();
    assert_eq!(fetched.meta.run_id, id);
}

#[test]
fn store_duplicate_id_returns_error() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("mock");
    let dup = r.clone();
    store.store(r).unwrap();
    let err = store.store(dup);
    assert!(err.is_err());
}

#[test]
fn store_get_nonexistent_returns_none() {
    let store = InMemoryReceiptStore::new();
    let result = store.get(Uuid::new_v4()).unwrap();
    assert!(result.is_none());
}

#[test]
fn store_list_all() {
    let mut store = InMemoryReceiptStore::new();
    store.store(make_hashed_receipt("a")).unwrap();
    store.store(make_hashed_receipt("b")).unwrap();
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn store_filter_by_backend() {
    let mut store = InMemoryReceiptStore::new();
    store.store(make_hashed_receipt("alpha")).unwrap();
    store.store(make_hashed_receipt("beta")).unwrap();
    let filtered = store
        .list(&ReceiptFilter {
            backend_id: Some("alpha".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].backend_id, "alpha");
}

#[test]
fn store_filter_by_outcome() {
    let mut store = InMemoryReceiptStore::new();
    store.store(make_hashed_receipt("a")).unwrap();

    let failed = ReceiptBuilder::new("b")
        .outcome(Outcome::Failed)
        .with_hash()
        .unwrap();
    store.store(failed).unwrap();

    let filtered = store
        .list(&ReceiptFilter {
            outcome: Some(Outcome::Failed),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].outcome, Outcome::Failed);
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Edge Cases: Empty and Minimal Receipts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_backend_id_hashes_ok() {
    let r = ReceiptBuilder::new("").build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn empty_backend_id_canonicalizes() {
    let r = ReceiptBuilder::new("").build();
    let json = canonicalize(&r).unwrap();
    assert!(json.contains(r#""id":"""#));
}

#[test]
fn minimal_receipt_hashes() {
    let r = ReceiptBuilder::new("x").build();
    assert!(compute_hash(&r).is_ok());
}

#[test]
fn minimal_receipt_validates_with_builder_hash() {
    let r = ReceiptBuilder::new("x").with_hash().unwrap();
    assert!(verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Edge Cases: Unicode Content
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unicode_backend_id_hashes() {
    let r = ReceiptBuilder::new("后端-バックエンド-🚀")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn unicode_backend_id_deterministic() {
    let r = ReceiptBuilder::new("日本語テスト")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn unicode_in_trace_events_hashes() {
    let r = ReceiptBuilder::new("b")
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::AssistantMessage {
                text: "Привет мир! 你好世界! مرحبا بالعالم".into(),
            },
            ext: None,
        })
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn unicode_in_verification_report() {
    let r = ReceiptBuilder::new("b")
        .verification(VerificationReport {
            git_diff: Some("--- a/файл.rs\n+++ b/файл.rs".into()),
            git_status: Some("M 文件.rs".into()),
            harness_ok: true,
        })
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    assert!(compute_hash(&r).is_ok());
}

#[test]
fn emoji_in_backend_id() {
    let r = ReceiptBuilder::new("🤖-agent").with_hash().unwrap();
    assert!(verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Edge Cases: Large Receipts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn large_trace_hashes() {
    let mut builder = ReceiptBuilder::new("big")
        .started_at(fixed_time())
        .finished_at(fixed_time());
    for i in 0..100 {
        builder = builder.add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("chunk-{i}"),
            },
            ext: None,
        });
    }
    let r = builder.build();
    assert!(compute_hash(&r).is_ok());
    assert_eq!(r.trace.len(), 100);
}

#[test]
fn large_usage_raw_hashes() {
    let mut map = serde_json::Map::new();
    for i in 0..200 {
        map.insert(format!("key_{i}"), serde_json::Value::Number(i.into()));
    }
    let r = ReceiptBuilder::new("b")
        .usage_raw(serde_json::Value::Object(map))
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn large_artifact_list_hashes() {
    let mut builder = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time());
    for i in 0..50 {
        builder = builder.add_artifact(ArtifactRef {
            kind: "log".into(),
            path: format!("artifacts/output_{i}.log"),
        });
    }
    let r = builder.build();
    assert_eq!(r.artifacts.len(), 50);
    assert!(compute_hash(&r).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 21. ChainError Display
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_error_display_hash_mismatch() {
    let e = ChainError::HashMismatch { index: 3 };
    assert_eq!(e.to_string(), "hash mismatch at chain index 3");
}

#[test]
fn chain_error_display_broken_link() {
    let e = ChainError::BrokenLink { index: 5 };
    assert_eq!(e.to_string(), "broken link at chain index 5");
}

#[test]
fn chain_error_display_empty_chain() {
    let e = ChainError::EmptyChain;
    assert_eq!(e.to_string(), "chain is empty");
}

#[test]
fn chain_error_display_duplicate_id() {
    let id = Uuid::nil();
    let e = ChainError::DuplicateId { id };
    assert!(e.to_string().contains("duplicate receipt id"));
}

#[test]
fn chain_error_display_parent_mismatch() {
    let e = ChainError::ParentMismatch { index: 2 };
    assert_eq!(e.to_string(), "parent hash mismatch at chain index 2");
}

#[test]
fn chain_error_display_sequence_gap() {
    let e = ChainError::SequenceGap {
        expected: 5,
        actual: 10,
    };
    assert_eq!(e.to_string(), "sequence gap: expected 5, found 10");
}

// ═══════════════════════════════════════════════════════════════════════
// 22. TamperEvidence and TamperKind Display
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn tamper_kind_hash_mismatch_display() {
    let tk = abp_receipt::TamperKind::HashMismatch {
        stored: "abc".into(),
        computed: "def".into(),
    };
    let s = tk.to_string();
    assert!(s.contains("abc"));
    assert!(s.contains("def"));
}

#[test]
fn tamper_kind_parent_link_broken_display() {
    let tk = abp_receipt::TamperKind::ParentLinkBroken {
        expected: Some("aaa".into()),
        actual: Some("bbb".into()),
    };
    let s = tk.to_string();
    assert!(s.contains("parent link broken"));
}

#[test]
fn tamper_evidence_display() {
    let te = abp_receipt::TamperEvidence {
        index: 1,
        sequence: 2,
        kind: abp_receipt::TamperKind::HashMismatch {
            stored: "x".into(),
            computed: "y".into(),
        },
    };
    let s = te.to_string();
    assert!(s.contains("index=1"));
    assert!(s.contains("seq=2"));
}

// ═══════════════════════════════════════════════════════════════════════
// 23. ChainGap Display
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_gap_display() {
    let gap = abp_receipt::ChainGap {
        expected: 3,
        actual: 7,
        after_index: 2,
    };
    let s = gap.to_string();
    assert!(s.contains("gap after index 2"));
    assert!(s.contains("expected seq 3"));
    assert!(s.contains("found 7"));
}

// ═══════════════════════════════════════════════════════════════════════
// 24. ChainBuilder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_builder_default() {
    let chain = ChainBuilder::default().build();
    assert!(chain.is_empty());
}

#[test]
fn chain_builder_new_is_empty() {
    let chain = ChainBuilder::new().build();
    assert!(chain.is_empty());
}

#[test]
fn chain_builder_append_valid() {
    let r = make_chain_receipt("a", 0);
    let chain = ChainBuilder::new().append(r).unwrap().build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_multiple_appends() {
    let chain = ChainBuilder::new()
        .append(make_chain_receipt("a", 0))
        .unwrap()
        .append(make_chain_receipt("b", 1))
        .unwrap()
        .append(make_chain_receipt("c", 2))
        .unwrap()
        .build();
    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_builder_skip_validation_allows_out_of_order() {
    let r0 = make_chain_receipt("a", 10);
    let r1 = make_chain_receipt("b", 0);

    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r0)
        .unwrap()
        .append(r1)
        .unwrap()
        .build();
    assert_eq!(chain.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 25. Receipt Serde Roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_serde_roundtrip_preserves_all_fields() {
    let r = ReceiptBuilder::new("serde-test")
        .backend_version("v1")
        .adapter_version("a1")
        .model("gpt-4")
        .dialect("openai")
        .outcome(Outcome::Complete)
        .usage_tokens(100, 200)
        .mode(ExecutionMode::Passthrough)
        .started_at(fixed_time())
        .finished_at(fixed_time() + chrono::Duration::seconds(5))
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(back.backend.id, "serde-test");
    assert_eq!(back.backend.backend_version.as_deref(), Some("v1"));
    assert_eq!(back.backend.adapter_version.as_deref(), Some("a1"));
    assert_eq!(back.outcome, Outcome::Complete);
    assert_eq!(back.mode, ExecutionMode::Passthrough);
    assert_eq!(back.usage.input_tokens, Some(100));
    assert_eq!(back.usage.output_tokens, Some(200));
    assert_eq!(back.receipt_sha256, r.receipt_sha256);
    assert!(verify_hash(&back));
}

#[test]
fn receipt_serde_roundtrip_with_trace() {
    let r = ReceiptBuilder::new("t")
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp/test"}),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu1".into()),
                output: serde_json::json!("file contents"),
                is_error: false,
            },
            ext: None,
        })
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();

    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.trace.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// 26. Additional Hash Edge Cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hash_changes_when_trace_event_added() {
    let r1 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::Warning {
                message: "warn".into(),
            },
            ext: None,
        })
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_when_duration_changes() {
    let r1 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time() + chrono::Duration::seconds(1))
        .run_id(Uuid::nil())
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_usage_tokens() {
    let r1 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .usage_tokens(1, 1)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_mode() {
    let r1 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .mode(ExecutionMode::Mapped)
        .build();
    let r2 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_artifact() {
    let r1 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "file.patch".into(),
        })
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_verification_report() {
    let r1 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .run_id(Uuid::nil())
        .verification(VerificationReport {
            git_diff: Some("changed".into()),
            git_status: None,
            harness_ok: true,
        })
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════
// 27. Multiple Tamper Detection in a Single Chain
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_tampering_multiple_entries() {
    let r0 = make_chain_receipt("a", 0);
    let mut r1 = make_chain_receipt("b", 1);
    r1.receipt_sha256 = Some("bad1".into());
    let mut r2 = make_chain_receipt("c", 2);
    r2.receipt_sha256 = Some("bad2".into());

    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r0)
        .unwrap()
        .append(r1)
        .unwrap()
        .append(r2)
        .unwrap()
        .build();

    let evidence = chain.detect_tampering();
    // At least 2 hash mismatches
    let hash_mismatches: Vec<_> = evidence
        .iter()
        .filter(|e| matches!(e.kind, abp_receipt::TamperKind::HashMismatch { .. }))
        .collect();
    assert!(hash_mismatches.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 28. Chain with receipts without hashes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_push_receipt_without_hash() {
    let mut c = ReceiptChain::new();
    let r = make_receipt("no-hash");
    c.push(r).unwrap();
    assert_eq!(c.len(), 1);
    assert!(c.verify().is_ok());
}

#[test]
fn chain_mixed_hashed_and_unhashed() {
    let mut c = ReceiptChain::new();
    let r1 = make_receipt("unhashed"); // No hash
    c.push(r1).unwrap();

    let r2 = ReceiptBuilder::new("hashed")
        .started_at(fixed_time() + chrono::Duration::seconds(1))
        .finished_at(fixed_time() + chrono::Duration::seconds(1))
        .with_hash()
        .unwrap();
    c.push(r2).unwrap();
    assert_eq!(c.len(), 2);
    assert!(c.verify().is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 29. Builder special values
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builder_zero_duration() {
    let r = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .duration(Duration::ZERO)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn builder_large_duration() {
    let r = ReceiptBuilder::new("b")
        .started_at(fixed_time())
        .duration(Duration::from_secs(86400))
        .build();
    assert_eq!(r.meta.duration_ms, 86400 * 1000);
}

#[test]
fn builder_usage_custom_struct() {
    let usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(2000),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: Some(3),
        estimated_cost_usd: Some(0.05),
    };
    let r = ReceiptBuilder::new("b").usage(usage).build();
    assert_eq!(r.usage.cache_read_tokens, Some(50));
    assert_eq!(r.usage.cache_write_tokens, Some(25));
    assert_eq!(r.usage.request_units, Some(3));
    assert!((r.usage.estimated_cost_usd.unwrap() - 0.05).abs() < f64::EPSILON);
}

// ═══════════════════════════════════════════════════════════════════════
// 30. AgentEvent edge cases in trace
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn trace_with_file_changed_event() {
    let r = ReceiptBuilder::new("b")
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Added function".into(),
            },
            ext: None,
        })
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    assert_eq!(r.trace.len(), 1);
    assert!(compute_hash(&r).is_ok());
}

#[test]
fn trace_with_command_executed_event() {
    let r = ReceiptBuilder::new("b")
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("All tests passed".into()),
            },
            ext: None,
        })
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn trace_with_ext_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "vendor_specific".into(),
        serde_json::json!({"key": "value"}),
    );

    let r = ReceiptBuilder::new("b")
        .add_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext),
        })
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();

    assert!(compute_hash(&r).is_ok());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("vendor_specific"));
}

// ═══════════════════════════════════════════════════════════════════════
// 31. Additional chain and hash interaction tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_parent_hash_links_correctly() {
    let mut c = ReceiptChain::new();
    let r0 = make_chain_receipt("a", 0);
    let h0 = r0.receipt_sha256.clone();
    c.push(r0).unwrap();

    let r1 = make_chain_receipt("b", 1);
    let h1 = r1.receipt_sha256.clone();
    c.push(r1).unwrap();

    let r2 = make_chain_receipt("c", 2);
    c.push(r2).unwrap();

    assert!(c.parent_hash_at(0).is_none());
    assert_eq!(c.parent_hash_at(1), h0.as_deref());
    assert_eq!(c.parent_hash_at(2), h1.as_deref());
}

#[test]
fn chain_verify_chain_passes_for_well_formed_chain() {
    let mut c = ReceiptChain::new();
    for i in 0..5 {
        c.push(make_chain_receipt("b", i)).unwrap();
    }
    assert!(c.verify_chain().is_ok());
}

#[test]
fn chain_ten_receipts() {
    let mut c = ReceiptChain::new();
    for i in 0..10 {
        c.push(make_chain_receipt("b", i)).unwrap();
    }
    assert_eq!(c.len(), 10);
    assert!(c.verify().is_ok());
    assert!(c.verify_chain().is_ok());
    assert!(c.find_gaps().is_empty());
    assert!(c.detect_tampering().is_empty());
}

#[test]
fn chain_summary_with_mixed_hashed_unhashed() {
    let mut c = ReceiptChain::new();
    let r1 = make_receipt("a"); // no hash
    c.push(r1).unwrap();

    let r2 = ReceiptBuilder::new("b")
        .started_at(fixed_time() + chrono::Duration::seconds(1))
        .finished_at(fixed_time() + chrono::Duration::seconds(1))
        .with_hash()
        .unwrap();
    c.push(r2).unwrap();

    let s = c.chain_summary();
    assert_eq!(s.total_receipts, 2);
    assert!(s.all_hashes_valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 32. Special Characters in Strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn special_chars_in_backend_id() {
    for name in &[
        "back/end",
        "back\\end",
        "back\"end",
        "back\tend",
        "back\nend",
    ] {
        let r = ReceiptBuilder::new(*name)
            .started_at(fixed_time())
            .finished_at(fixed_time())
            .build();
        assert!(compute_hash(&r).is_ok());
    }
}

#[test]
fn very_long_backend_id() {
    let long_name = "x".repeat(10_000);
    let r = ReceiptBuilder::new(long_name)
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    assert!(compute_hash(&r).is_ok());
}

#[test]
fn null_bytes_in_model_name() {
    let r = ReceiptBuilder::new("b")
        .model("model\0name")
        .started_at(fixed_time())
        .finished_at(fixed_time())
        .build();
    assert!(compute_hash(&r).is_ok());
}
