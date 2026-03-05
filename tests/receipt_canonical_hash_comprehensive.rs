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

//! Comprehensive tests for receipt canonical hashing.
//!
//! Validates deterministic SHA-256 hashing of ABP receipts, the self-referential
//! prevention gotcha (`receipt_sha256` set to `null` before hashing), field
//! sensitivity, canonical JSON properties, chain integrity, and edge cases.

use std::collections::BTreeMap;

use abp_core::chain::ReceiptChain;
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    BackendIdentity, Capability, CapabilityManifest, ExecutionMode, Outcome, Receipt,
    ReceiptBuilder, RunMetadata, SupportLevel, UsageNormalized, VerificationReport,
    CONTRACT_VERSION,
};
use abp_receipt::{canonicalize, compute_hash, verify_hash};
use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
}

fn fixed_ts_later() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap()
}

/// A fully-deterministic minimal receipt (nil UUIDs, fixed timestamps).
fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "mock".into(),
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

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_ts(),
        kind,
        ext: None,
    }
}

fn populated_receipt() -> Receipt {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    Receipt {
        meta: RunMetadata {
            run_id: Uuid::from_u128(1),
            work_order_id: Uuid::from_u128(2),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts_later(),
            duration_ms: 60_000,
        },
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.5.0".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Passthrough,
        usage_raw: serde_json::json!({"prompt_tokens": 100, "completion_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.003),
        },
        trace: vec![
            make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
            make_event(AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/f.rs".into()),
            git_status: Some("M f.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ===========================================================================
// 1. Basic Hashing (~15 tests)
// ===========================================================================

#[test]
fn basic_with_hash_produces_non_empty_hash() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(!r.receipt_sha256.as_ref().unwrap().is_empty());
}

#[test]
fn basic_hash_is_valid_hex() {
    let r = minimal_receipt().with_hash().unwrap();
    let h = r.receipt_sha256.unwrap();
    assert!(
        h.chars().all(|c| c.is_ascii_hexdigit()),
        "hash contains non-hex chars: {h}"
    );
}

#[test]
fn basic_hash_length_is_64_chars() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h.len(), 64, "SHA-256 hex must be 64 chars");
}

#[test]
fn basic_same_receipt_same_hash() {
    let r = minimal_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn basic_deterministic_across_reconstructions() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let h2 = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn basic_different_receipts_different_hashes() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let h2 = receipt_hash(&populated_receipt()).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn basic_hash_survives_serde_roundtrip() {
    let r = minimal_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn basic_hash_is_lowercase_hex() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h, h.to_lowercase());
}

#[test]
fn basic_receipt_hash_matches_compute_hash() {
    let r = minimal_receipt();
    assert_eq!(receipt_hash(&r).unwrap(), compute_hash(&r).unwrap());
}

#[test]
fn basic_with_hash_via_builder() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn basic_builder_hash_matches_direct_hash() {
    let mut r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .work_order_id(Uuid::nil())
        .build();
    r1.meta.run_id = Uuid::nil();
    let h1 = receipt_hash(&r1).unwrap();

    let mut r2 = r1.clone();
    r2 = r2.with_hash().unwrap();
    assert_eq!(r2.receipt_sha256.as_ref().unwrap(), &h1);
}

#[test]
fn basic_populated_receipt_hash_is_deterministic() {
    let h1 = receipt_hash(&populated_receipt()).unwrap();
    let h2 = receipt_hash(&populated_receipt()).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn basic_hash_not_all_zeros() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_ne!(h, "0".repeat(64));
}

#[test]
fn basic_hash_not_all_fs() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_ne!(h, "f".repeat(64));
}

#[test]
fn basic_verify_hash_returns_true_for_correct_hash() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(verify_hash(&r));
}

// ===========================================================================
// 2. Self-Referential Prevention (~10 tests)
// ===========================================================================

#[test]
fn selfref_none_vs_some_same_hash() {
    let mut r1 = minimal_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("fake_hash_value".into());
    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn selfref_different_stored_hashes_same_computed_hash() {
    let mut ra = minimal_receipt();
    ra.receipt_sha256 = Some("aaaa".into());
    let mut rb = minimal_receipt();
    rb.receipt_sha256 = Some("zzzz".into());
    assert_eq!(receipt_hash(&ra).unwrap(), receipt_hash(&rb).unwrap());
}

#[test]
fn selfref_rehash_is_stable() {
    let r = minimal_receipt().with_hash().unwrap();
    let h1 = r.receipt_sha256.clone().unwrap();
    let r2 = r.with_hash().unwrap();
    let h2 = r2.receipt_sha256.unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn selfref_triple_rehash_stable() {
    let r = minimal_receipt()
        .with_hash()
        .unwrap()
        .with_hash()
        .unwrap()
        .with_hash()
        .unwrap();
    let expected = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(r.receipt_sha256.unwrap(), expected);
}

#[test]
fn selfref_hash_of_hashed_equals_hash_of_unhashed() {
    let unhashed = minimal_receipt();
    let hashed = minimal_receipt().with_hash().unwrap();
    assert_eq!(
        receipt_hash(&unhashed).unwrap(),
        receipt_hash(&hashed).unwrap()
    );
}

#[test]
fn selfref_canonicalize_nulls_receipt_sha256() {
    let mut r = minimal_receipt();
    r.receipt_sha256 = Some("should_be_nulled".into());
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn selfref_canonical_json_same_regardless_of_stored_hash() {
    let mut r1 = minimal_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("anything".into());
    assert_eq!(canonicalize(&r1).unwrap(), canonicalize(&r2).unwrap());
}

#[test]
fn selfref_verify_hash_detects_tampered_hash() {
    let mut r = minimal_receipt().with_hash().unwrap();
    r.receipt_sha256 = Some("tampered_value".into());
    assert!(!verify_hash(&r));
}

#[test]
fn selfref_verify_hash_passes_no_hash() {
    let r = minimal_receipt();
    assert!(verify_hash(&r));
}

#[test]
fn selfref_populated_receipt_rehash_stable() {
    let r1 = populated_receipt().with_hash().unwrap();
    let r2 = r1.clone().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

// ===========================================================================
// 3. Field Sensitivity (~15 tests)
// ===========================================================================

#[test]
fn field_changing_contract_version_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.meta.contract_version = "abp/v0.2".into();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn field_changing_outcome_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.outcome = Outcome::Failed;
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn field_changing_outcome_partial_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.outcome = Outcome::Partial;
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_changing_duration_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.meta.duration_ms = 9999;
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_changing_backend_id_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.backend.id = "other-backend".into();
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_changing_run_id_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.meta.run_id = Uuid::from_u128(42);
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_changing_work_order_id_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.meta.work_order_id = Uuid::from_u128(99);
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_adding_usage_tokens_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.usage.input_tokens = Some(500);
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_adding_output_tokens_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.usage.output_tokens = Some(200);
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_changing_execution_mode_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.mode = ExecutionMode::Passthrough;
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_adding_trace_event_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.trace.push(make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    }));
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_adding_artifact_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.artifacts.push(ArtifactRef {
        kind: "log".into(),
        path: "run.log".into(),
    });
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_changing_verification_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.verification.harness_ok = true;
    r2.verification.git_diff = Some("diff".into());
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

#[test]
fn field_btreemap_capabilities_deterministic() {
    // Insert capabilities in different orders → same hash because BTreeMap sorts.
    let mut r1 = minimal_receipt();
    r1.capabilities
        .insert(Capability::ToolRead, SupportLevel::Native);
    r1.capabilities
        .insert(Capability::Streaming, SupportLevel::Emulated);

    let mut r2 = minimal_receipt();
    r2.capabilities
        .insert(Capability::Streaming, SupportLevel::Emulated);
    r2.capabilities
        .insert(Capability::ToolRead, SupportLevel::Native);

    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn field_changing_backend_version_changes_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let mut r2 = minimal_receipt();
    r2.backend.backend_version = Some("2.0.0".into());
    assert_ne!(h1, receipt_hash(&r2).unwrap());
}

// ===========================================================================
// 4. Canonical JSON (~10 tests)
// ===========================================================================

#[test]
fn canonical_btreemap_field_ordering() {
    let json = canonical_json(&minimal_receipt()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    // The top-level object should have sorted keys.
    if let serde_json::Value::Object(map) = &v {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "canonical JSON keys should be sorted");
    } else {
        panic!("expected top-level object");
    }
}

#[test]
fn canonical_no_whitespace_variation() {
    let json = canonicalize(&minimal_receipt()).unwrap();
    // Compact JSON has no newlines or leading spaces.
    assert!(!json.contains('\n'));
    assert!(!json.starts_with(' '));
}

#[test]
fn canonical_compact_no_pretty_printing() {
    let json = canonicalize(&minimal_receipt()).unwrap();
    // Compact serialization has no indentation.
    assert!(!json.contains("  "));
}

#[test]
fn canonical_json_deterministic_multiple_calls() {
    let j1 = canonicalize(&minimal_receipt()).unwrap();
    let j2 = canonicalize(&minimal_receipt()).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonical_json_populated_deterministic() {
    let j1 = canonicalize(&populated_receipt()).unwrap();
    let j2 = canonicalize(&populated_receipt()).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonical_unicode_handling() {
    let mut r = minimal_receipt();
    r.backend.id = "backend-日本語".into();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn canonical_null_vs_absent_receipt_sha256() {
    // Both None and Some(...) become null in canonical form.
    let mut r1 = minimal_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("anything".into());
    let j1 = canonicalize(&r1).unwrap();
    let j2 = canonicalize(&r2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonical_usage_raw_key_order() {
    // JSON object keys in usage_raw should be sorted.
    let mut r = minimal_receipt();
    r.usage_raw = serde_json::json!({"z_key": 1, "a_key": 2, "m_key": 3});
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let Some(serde_json::Value::Object(map)) = v.get("usage_raw") {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }
}

#[test]
fn canonical_sha256_hex_matches_manual() {
    let data = b"hello world";
    let h = sha256_hex(data);
    assert_eq!(h.len(), 64);
    // Known SHA-256 of "hello world".
    assert_eq!(
        h,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn canonical_receipt_hash_uses_sha256_of_canonical_json() {
    let r = minimal_receipt();
    let json = canonicalize(&r).unwrap();
    let expected = sha256_hex(json.as_bytes());
    let actual = receipt_hash(&r).unwrap();
    assert_eq!(actual, expected);
}

// ===========================================================================
// 5. Chain Integrity (~15 tests)
// ===========================================================================

/// Helper to build a receipt chain of length `n`, each with a valid hash.
fn build_chain(n: usize) -> Vec<Receipt> {
    (0..n)
        .map(|i| {
            let mut r = minimal_receipt();
            r.meta.run_id = Uuid::from_u128(i as u128 + 100);
            r.meta.duration_ms = i as u64;
            r.with_hash().unwrap()
        })
        .collect()
}

#[test]
fn chain_single_receipt() {
    let mut chain = ReceiptChain::new();
    let r = minimal_receipt().with_hash().unwrap();
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
    chain.verify().unwrap();
}

#[test]
fn chain_multiple_receipts() {
    let mut chain = ReceiptChain::new();
    for r in build_chain(5) {
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 5);
    chain.verify().unwrap();
}

#[test]
fn chain_empty_verify_fails() {
    let chain = ReceiptChain::new();
    assert!(chain.verify().is_err());
}

#[test]
fn chain_tampered_hash_detected() {
    let mut chain = ReceiptChain::new();
    let mut r = minimal_receipt().with_hash().unwrap();
    r.receipt_sha256 = Some("tampered".into());
    let result = chain.push(r);
    assert!(result.is_err());
}

#[test]
fn chain_tampered_field_after_hash_detected() {
    let mut chain = ReceiptChain::new();
    let mut r = minimal_receipt().with_hash().unwrap();
    // Tamper with a field after hashing.
    r.outcome = Outcome::Failed;
    let result = chain.push(r);
    assert!(result.is_err());
}

#[test]
fn chain_duplicate_id_rejected() {
    let mut chain = ReceiptChain::new();
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = minimal_receipt().with_hash().unwrap();
    // Both have Uuid::nil()
    chain.push(r1).unwrap();
    let result = chain.push(r2);
    assert!(result.is_err());
}

#[test]
fn chain_unique_ids_accepted() {
    let mut chain = ReceiptChain::new();
    for r in build_chain(3) {
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 3);
}

#[test]
fn chain_prev_hash_linking() {
    // Simulate a chain where each receipt stores the previous receipt's hash.
    let receipts = build_chain(4);
    let mut prev_hash: Option<String> = None;
    for r in &receipts {
        if let Some(ref ph) = prev_hash {
            // Verify the chain is ordered by checking each receipt has a valid hash
            // that differs from the previous.
            assert_ne!(r.receipt_sha256.as_ref().unwrap(), ph);
        }
        prev_hash = r.receipt_sha256.clone();
    }
}

#[test]
fn chain_verification_with_all_valid() {
    let mut chain = ReceiptChain::new();
    for r in build_chain(10) {
        chain.push(r).unwrap();
    }
    chain.verify().unwrap();
}

#[test]
fn chain_is_empty_initially() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn chain_last_returns_last_receipt() {
    let mut chain = ReceiptChain::new();
    let receipts = build_chain(3);
    let expected_id = receipts.last().unwrap().meta.run_id;
    for r in receipts {
        chain.push(r).unwrap();
    }
    assert_eq!(chain.last().unwrap().meta.run_id, expected_id);
}

#[test]
fn chain_find_by_id() {
    let mut chain = ReceiptChain::new();
    let receipts = build_chain(5);
    let target_id = receipts[2].meta.run_id;
    for r in receipts {
        chain.push(r).unwrap();
    }
    assert!(chain.find_by_id(&target_id).is_some());
    assert!(chain.find_by_id(&Uuid::nil()).is_none());
}

#[test]
fn chain_no_hash_receipt_accepted() {
    // A receipt with receipt_sha256 = None is allowed in the chain
    // (the chain only rejects mismatched hashes).
    let mut chain = ReceiptChain::new();
    let r = minimal_receipt(); // no hash
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_mixed_hashed_and_unhashed() {
    let mut chain = ReceiptChain::new();
    let mut r1 = minimal_receipt();
    r1.meta.run_id = Uuid::from_u128(1);
    chain.push(r1).unwrap(); // no hash

    let mut r2 = minimal_receipt();
    r2.meta.run_id = Uuid::from_u128(2);
    let r2 = r2.with_hash().unwrap();
    chain.push(r2).unwrap(); // with hash

    assert_eq!(chain.len(), 2);
    chain.verify().unwrap();
}

#[test]
fn chain_success_rate() {
    let mut chain = ReceiptChain::new();
    let mut r1 = minimal_receipt();
    r1.meta.run_id = Uuid::from_u128(1);
    r1.outcome = Outcome::Complete;
    chain.push(r1.with_hash().unwrap()).unwrap();

    let mut r2 = minimal_receipt();
    r2.meta.run_id = Uuid::from_u128(2);
    r2.outcome = Outcome::Failed;
    chain.push(r2.with_hash().unwrap()).unwrap();

    assert!((chain.success_rate() - 0.5).abs() < f64::EPSILON);
}

// ===========================================================================
// 6. Edge Cases (~10 tests)
// ===========================================================================

#[test]
fn edge_all_fields_populated_hashes_correctly() {
    let r = populated_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn edge_minimal_fields_hashes_correctly() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn edge_large_ext_data() {
    let mut r = minimal_receipt();
    let mut ext = BTreeMap::new();
    for i in 0..100 {
        ext.insert(
            format!("key_{i:04}"),
            serde_json::Value::String("x".repeat(1000)),
        );
    }
    r.trace.push(AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "big ext".into(),
        },
        ext: Some(ext),
    });
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    // Deterministic even with large data.
    assert_eq!(h, receipt_hash(&r).unwrap());
}

#[test]
fn edge_unicode_in_backend_id() {
    let mut r = minimal_receipt();
    r.backend.id = "бэкенд-κόσμε-世界".into();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert_eq!(h, receipt_hash(&r).unwrap());
}

#[test]
fn edge_unicode_in_trace_events() {
    let mut r = minimal_receipt();
    r.trace.push(make_event(AgentEventKind::AssistantMessage {
        text: "こんにちは世界 🌍 مرحبا".into(),
    }));
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert_eq!(h, receipt_hash(&r).unwrap());
}

#[test]
fn edge_empty_strings_in_fields() {
    let mut r = minimal_receipt();
    r.backend.id = String::new();
    r.meta.contract_version = String::new();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_many_trace_events() {
    let mut r = minimal_receipt();
    for i in 0..500 {
        r.trace.push(make_event(AgentEventKind::AssistantDelta {
            text: format!("token_{i}"),
        }));
    }
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert_eq!(h, receipt_hash(&r).unwrap());
}

#[test]
fn edge_usage_raw_complex_nested_json() {
    let mut r = minimal_receipt();
    r.usage_raw = serde_json::json!({
        "nested": {
            "deeply": {
                "value": [1, 2, 3],
                "flag": true
            }
        },
        "array": [null, "str", 42, {"k": "v"}]
    });
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert_eq!(h, receipt_hash(&r).unwrap());
}

#[test]
fn edge_special_chars_in_strings() {
    let mut r = minimal_receipt();
    r.backend.id = "back\\end\t\"quoted\"\nnewline".into();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert_eq!(h, receipt_hash(&r).unwrap());
}

#[test]
fn edge_max_uuid_values() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::max();
    r.meta.work_order_id = Uuid::max();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert_eq!(h, receipt_hash(&r).unwrap());
}

#[test]
fn edge_serde_roundtrip_preserves_hash_validity() {
    let r = populated_receipt().with_hash().unwrap();
    let json = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn edge_all_outcome_variants_produce_distinct_hashes() {
    let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    let mut hashes = std::collections::HashSet::new();
    for outcome in &outcomes {
        let mut r = minimal_receipt();
        r.outcome = outcome.clone();
        hashes.insert(receipt_hash(&r).unwrap());
    }
    assert_eq!(hashes.len(), 3, "each outcome should produce a unique hash");
}

#[test]
fn edge_all_execution_modes_produce_distinct_hashes() {
    let modes = [ExecutionMode::Mapped, ExecutionMode::Passthrough];
    let mut hashes = std::collections::HashSet::new();
    for mode in &modes {
        let mut r = minimal_receipt();
        r.mode = *mode;
        hashes.insert(receipt_hash(&r).unwrap());
    }
    assert_eq!(hashes.len(), 2);
}

#[test]
fn edge_verify_hash_after_clone() {
    let r = populated_receipt().with_hash().unwrap();
    let cloned = r.clone();
    assert!(verify_hash(&cloned));
    assert_eq!(r.receipt_sha256, cloned.receipt_sha256);
}
