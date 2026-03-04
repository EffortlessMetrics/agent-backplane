// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deep tests for receipt canonicalization and hashing.

use abp_core::{ArtifactRef, Capability, SupportLevel};
use abp_receipt::verify::{ReceiptAuditor, verify_receipt};
use abp_receipt::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, ExecutionMode, Outcome, Receipt, ReceiptBuilder,
    ReceiptChain, canonicalize, compute_hash, verify_hash,
};
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

// ── Helpers ────────────────────────────────────────────────────────

/// Build a deterministic receipt with fixed IDs and timestamps.
fn deterministic_receipt() -> Receipt {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    ReceiptBuilder::new("test-backend")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .build()
}

fn deterministic_receipt_hashed() -> Receipt {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    ReceiptBuilder::new("test-backend")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap()
}

/// Fixed timestamp helper.
fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap()
}

/// Build a deterministic receipt with all fixed fields for field-independence tests.
fn baseline_receipt() -> Receipt {
    ReceiptBuilder::new("baseline")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .build()
}

// =========================================================================
// 1. Basic hash computation (receipt_hash sets receipt_sha256 to null)
// =========================================================================

#[test]
fn hash_is_64_hex_chars() {
    let r = deterministic_receipt();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_is_lowercase_hex() {
    let r = deterministic_receipt();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h, h.to_lowercase());
}

#[test]
fn hash_of_receipt_without_stored_hash() {
    let r = deterministic_receipt();
    assert!(r.receipt_sha256.is_none());
    let h = compute_hash(&r).unwrap();
    assert!(!h.is_empty());
}

// =========================================================================
// 2. with_hash() method (produces deterministic output)
// =========================================================================

#[test]
fn with_hash_sets_valid_sha256() {
    let r = deterministic_receipt_hashed();
    assert!(r.receipt_sha256.is_some());
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn with_hash_matches_recomputed() {
    let r = deterministic_receipt_hashed();
    let stored = r.receipt_sha256.as_ref().unwrap();
    let recomputed = compute_hash(&r).unwrap();
    assert_eq!(*stored, recomputed);
}

#[test]
fn with_hash_passes_verification() {
    let r = deterministic_receipt_hashed();
    assert!(verify_hash(&r));
}

#[test]
fn with_hash_builder_method_equivalent_to_manual() {
    let ts = fixed_ts();
    let r_manual = {
        let mut r = ReceiptBuilder::new("test-backend")
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(ts)
            .finished_at(ts)
            .build();
        r.receipt_sha256 = Some(compute_hash(&r).unwrap());
        r
    };
    let r_builder = ReceiptBuilder::new("test-backend")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert_eq!(r_manual.receipt_sha256, r_builder.receipt_sha256);
}

#[test]
fn with_hash_on_core_receipt_matches_abp_receipt() {
    let ts = fixed_ts();
    let mut core_receipt = ReceiptBuilder::new("test-backend")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    // Use the core with_hash method
    core_receipt = core_receipt.with_hash().unwrap();
    // Compare against abp-receipt compute_hash
    let recomputed = compute_hash(&core_receipt).unwrap();
    assert_eq!(
        core_receipt.receipt_sha256.as_deref(),
        Some(recomputed.as_str())
    );
}

// =========================================================================
// 3. Hash stability (same receipt always produces same hash)
// =========================================================================

#[test]
fn hash_determinism_identical_receipts() {
    let r = deterministic_receipt();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_determinism_across_clones() {
    let r1 = deterministic_receipt();
    let r2 = r1.clone();
    assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_determinism_multiple_iterations() {
    let r = deterministic_receipt();
    let hashes: Vec<String> = (0..100).map(|_| compute_hash(&r).unwrap()).collect();
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn hash_stability_after_clone_and_modify_back() {
    let r1 = deterministic_receipt();
    let h1 = compute_hash(&r1).unwrap();
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    r2.outcome = Outcome::Complete; // revert
    let h2 = compute_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

// =========================================================================
// 4. Hash uniqueness (different receipts produce different hashes)
// =========================================================================

#[test]
fn hash_uniqueness_different_backends() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("backend-a")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    let r2 = ReceiptBuilder::new("backend-b")
        .run_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_uniqueness_different_outcomes() {
    let r1 = deterministic_receipt();
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_uniqueness_different_run_ids() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::from_u128(1))
        .started_at(ts)
        .finished_at(ts)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::from_u128(2))
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn many_unique_receipts_all_produce_unique_hashes() {
    let ts = fixed_ts();
    let mut hashes = BTreeMap::new();
    for i in 0u128..50 {
        let r = ReceiptBuilder::new(format!("backend-{i}"))
            .run_id(Uuid::from_u128(i))
            .work_order_id(Uuid::nil())
            .started_at(ts)
            .finished_at(ts)
            .build();
        let h = compute_hash(&r).unwrap();
        hashes.insert(h, i);
    }
    assert_eq!(hashes.len(), 50, "expected 50 unique hashes");
}

// =========================================================================
// 5. Field independence (changing any field changes the hash)
// =========================================================================

#[test]
fn field_independence_backend_id() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.backend.id = "other-backend".into();
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_backend_version() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.backend.backend_version = Some("1.0.0".into());
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_adapter_version() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.backend.adapter_version = Some("2.0.0".into());
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_work_order_id() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.meta.work_order_id = Uuid::from_u128(99);
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_contract_version() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.meta.contract_version = "abp/v999".into();
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_execution_mode() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.mode = ExecutionMode::Passthrough;
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_outcome() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.outcome = Outcome::Failed;
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_usage_input_tokens() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.usage.input_tokens = Some(1000);
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_usage_output_tokens() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.usage.output_tokens = Some(500);
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_duration_ms() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.meta.duration_ms = 42_000;
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_verification_harness_ok() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.verification.harness_ok = true;
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_verification_git_diff() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.verification.git_diff = Some("diff --git a/foo".into());
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_trace_addition() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.trace.push(AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    });
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_artifacts() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    });
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_usage_raw() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.usage_raw = serde_json::json!({"tokens": 100});
    assert_ne!(h1, compute_hash(&r).unwrap());
}

#[test]
fn field_independence_capabilities() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("baseline")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    let h1 = compute_hash(&r1).unwrap();

    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r2 = ReceiptBuilder::new("baseline")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .capabilities(caps)
        .build();
    assert_ne!(h1, compute_hash(&r2).unwrap());
}

// =========================================================================
// 6. BTreeMap ordering (keys always sorted, canonical JSON)
// =========================================================================

#[test]
fn field_ordering_canonical_json_has_sorted_keys() {
    let r = deterministic_receipt();
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = parsed {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    } else {
        panic!("expected JSON object");
    }
}

#[test]
fn field_ordering_usage_raw_btreemap_sorted() {
    let mut raw = serde_json::Map::new();
    raw.insert("zebra".into(), serde_json::json!(1));
    raw.insert("apple".into(), serde_json::json!(2));
    raw.insert("mango".into(), serde_json::json!(3));

    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::Value::Object(raw))
        .build();

    let json = canonicalize(&r).unwrap();
    let apple_pos = json.find("\"apple\"").unwrap();
    let mango_pos = json.find("\"mango\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(apple_pos < mango_pos);
    assert!(mango_pos < zebra_pos);
}

#[test]
fn field_ordering_nested_object_keys_sorted() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::json!({"z": {"c": 1, "a": 2, "b": 3}}))
        .build();
    let json = canonicalize(&r).unwrap();
    let a_pos = json.find("\"a\":2").unwrap();
    let b_pos = json.find("\"b\":3").unwrap();
    let c_pos = json.find("\"c\":1").unwrap();
    assert!(a_pos < b_pos);
    assert!(b_pos < c_pos);
}

#[test]
fn field_ordering_meta_keys_sorted() {
    let r = deterministic_receipt();
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let meta = parsed["meta"].as_object().unwrap();
    let keys: Vec<&String> = meta.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

// =========================================================================
// 7. Null receipt_sha256 in hash input (self-referential prevention)
// =========================================================================

#[test]
fn self_referential_prevention_hash_field_ignored() {
    let r1 = deterministic_receipt();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("some_previous_hash_value".into());
    assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn self_referential_prevention_canonical_json_always_null() {
    let mut r = deterministic_receipt();
    r.receipt_sha256 = Some("deadbeef".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
    assert!(!json.contains("deadbeef"));
}

#[test]
fn self_referential_prevention_hash_unaffected_by_stored_hash() {
    let r = deterministic_receipt();
    let h1 = compute_hash(&r).unwrap();

    let mut r_with_hash = r.clone();
    r_with_hash.receipt_sha256 = Some(h1.clone());
    let h2 = compute_hash(&r_with_hash).unwrap();

    let mut r_with_garbage = r;
    r_with_garbage.receipt_sha256 = Some("garbage".into());
    let h3 = compute_hash(&r_with_garbage).unwrap();

    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn self_referential_none_vs_some_same_hash() {
    let mut r1 = deterministic_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = deterministic_receipt();
    r2.receipt_sha256 = Some("anything".into());
    assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn canonicalize_always_produces_null_receipt_sha256_in_json() {
    // Even when the field is None (serialized as null)
    let r_none = deterministic_receipt();
    let json_none = canonicalize(&r_none).unwrap();
    assert!(json_none.contains("\"receipt_sha256\":null"));

    // When the field is Some
    let r_some = deterministic_receipt_hashed();
    let json_some = canonicalize(&r_some).unwrap();
    assert!(json_some.contains("\"receipt_sha256\":null"));
}

// =========================================================================
// 8. Receipt chain verification (parent_receipt_id linkage)
// =========================================================================

#[test]
fn chain_sequential_receipts_verify() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, i as u32, 0).unwrap();
        let r = ReceiptBuilder::new("mock")
            .started_at(ts)
            .finished_at(ts)
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_rejects_tampered_receipt() {
    let mut chain = ReceiptChain::new();
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.backend.id = "tampered".into();
    assert!(chain.push(r).is_err());
}

#[test]
fn chain_rejects_out_of_order() {
    let mut chain = ReceiptChain::new();
    let ts_later = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let ts_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("mock")
                .started_at(ts_later)
                .finished_at(ts_later)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .started_at(ts_earlier)
        .finished_at(ts_earlier)
        .with_hash()
        .unwrap();
    assert!(chain.push(r2).is_err());
}

#[test]
fn chain_single_receipt_valid() {
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.len(), 1);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_empty_verify_returns_error() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.len(), 0);
    // An empty chain is not considered valid for verification
    assert!(chain.verify().is_err());
}

// =========================================================================
// 9. Serde roundtrip (serialize → deserialize → hash matches)
// =========================================================================

#[test]
fn serde_roundtrip_hash_preserved() {
    let r = deterministic_receipt_hashed();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn serde_roundtrip_hash_still_verifies() {
    let r = deterministic_receipt_hashed();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn serde_roundtrip_recomputed_hash_matches() {
    let r = deterministic_receipt_hashed();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let h_original = compute_hash(&r).unwrap();
    let h_roundtrip = compute_hash(&r2).unwrap();
    assert_eq!(h_original, h_roundtrip);
}

#[test]
fn serde_roundtrip_with_trace_events() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "hello world".into(),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
    assert_eq!(r2.trace.len(), 2);
}

#[test]
fn serde_roundtrip_with_ext_field() {
    let ts = fixed_ts();
    let mut ext = BTreeMap::new();
    ext.insert("custom_key".to_string(), serde_json::json!("custom_value"));
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: Some(ext),
        })
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn serde_roundtrip_canonical_form_unchanged() {
    let r = deterministic_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let c1 = canonicalize(&r).unwrap();
    let c2 = canonicalize(&r2).unwrap();
    assert_eq!(c1, c2);
}

// =========================================================================
// 10. Contract version in receipts
// =========================================================================

#[test]
fn receipt_has_correct_contract_version() {
    let r = deterministic_receipt();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn contract_version_is_abp_v01() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_appears_in_canonical_json() {
    let r = deterministic_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn different_contract_version_changes_hash() {
    let mut r = baseline_receipt();
    let h1 = compute_hash(&r).unwrap();
    r.meta.contract_version = "abp/v0.2".into();
    let h2 = compute_hash(&r).unwrap();
    assert_ne!(h1, h2);
}

// =========================================================================
// 11. Timestamp handling (UTC, format consistency)
// =========================================================================

#[test]
fn timestamp_different_started_at_different_hash() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t1)
        .finished_at(t1)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t2)
        .finished_at(t2)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn timestamp_different_finished_at_different_hash() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 10).unwrap();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t1)
        .finished_at(t2)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t1)
        .finished_at(t3)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn timestamp_duration_affects_hash() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .duration(Duration::from_secs(5))
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .duration(Duration::from_secs(10))
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn timestamp_utc_serialized_in_canonical_json() {
    let r = deterministic_receipt();
    let json = canonicalize(&r).unwrap();
    // Should contain the UTC timestamp for 2025-06-01T12:00:00
    assert!(json.contains("2025-06-01"));
}

#[test]
fn timestamp_subsecond_precision_affects_hash() {
    use chrono::NaiveDateTime;
    let t1 = chrono::DateTime::<Utc>::from_naive_utc_and_offset(
        NaiveDateTime::parse_from_str("2025-06-01 12:00:00.000", "%Y-%m-%d %H:%M:%S%.f").unwrap(),
        Utc,
    );
    let t2 = chrono::DateTime::<Utc>::from_naive_utc_and_offset(
        NaiveDateTime::parse_from_str("2025-06-01 12:00:00.001", "%Y-%m-%d %H:%M:%S%.f").unwrap(),
        Utc,
    );
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t1)
        .finished_at(t1)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t2)
        .finished_at(t2)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// =========================================================================
// 12. Edge cases
// =========================================================================

#[test]
fn edge_empty_backend_id_hashes_stably() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn edge_unicode_backend_id() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("バックエンド-日本語")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    let json = canonicalize(&r).unwrap();
    // Unicode should survive canonicalization
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.to_string().contains("バックエンド"));
}

#[test]
fn edge_unicode_in_trace_events() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "Héllo wörld 🌍 日本語テスト".into(),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_emoji_in_usage_raw() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::json!({"note": "🚀 launch"}))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_very_large_trace() {
    let ts = fixed_ts();
    let mut builder = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts);
    for i in 0..200 {
        builder = builder.add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
            ext: None,
        });
    }
    let r = builder.with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 200);
}

#[test]
fn edge_large_usage_raw() {
    let ts = fixed_ts();
    let mut big_obj = serde_json::Map::new();
    for i in 0..100 {
        big_obj.insert(format!("key_{i:04}"), serde_json::json!(i));
    }
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::Value::Object(big_obj))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_minimal_receipt() {
    let r = ReceiptBuilder::new("x").build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_null_optional_fields_hash_correctly() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
    assert!(r.receipt_sha256.is_none());
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(verify_hash(&r));
}

#[test]
fn edge_zero_duration_hashes_correctly() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert_eq!(r.meta.duration_ms, 0);
    assert!(verify_hash(&r));
}

#[test]
fn edge_empty_trace_and_artifacts_hash_correctly() {
    let r = deterministic_receipt();
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_null_usage_tokens_hash_correctly() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn edge_very_long_string_field() {
    let ts = fixed_ts();
    let long_str = "a".repeat(10_000);
    let r = ReceiptBuilder::new(&long_str)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_special_json_chars_in_strings() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("back\\end\"with/special\nchars\ttab")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    let json = canonicalize(&r).unwrap();
    let _: serde_json::Value = serde_json::from_str(&json).unwrap();
}

// =========================================================================
// Extra: Canonical JSON structural tests
// =========================================================================

#[test]
fn canonical_json_no_newlines_or_indentation() {
    let r = deterministic_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
    assert!(!json.contains("  "));
}

#[test]
fn canonical_json_is_valid_json() {
    let r = ReceiptBuilder::new("mock")
        .model("gpt-4")
        .usage_raw(serde_json::json!({"deep": {"nested": [1, 2]}}))
        .add_event(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    let json = canonicalize(&r).unwrap();
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&json);
    assert!(parsed.is_ok());
}

#[test]
fn canonical_json_hash_matches_direct_sha256() {
    use sha2::{Digest, Sha256};

    let r = deterministic_receipt();
    let json = canonicalize(&r).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    let manual_hash = format!("{:x}", hasher.finalize());
    let lib_hash = compute_hash(&r).unwrap();
    assert_eq!(manual_hash, lib_hash);
}

// =========================================================================
// Extra: Usage raw preservation
// =========================================================================

#[test]
fn usage_raw_nested_json_preserved() {
    let complex = serde_json::json!({
        "model": "gpt-4",
        "nested": { "a": [1, 2, 3], "b": null },
        "flag": true,
        "count": 42
    });
    let r = ReceiptBuilder::new("mock")
        .usage_raw(complex)
        .with_hash()
        .unwrap();
    assert_eq!(r.usage_raw["nested"]["a"], serde_json::json!([1, 2, 3]));
    assert!(r.usage_raw["nested"]["b"].is_null());
    assert!(verify_hash(&r));
}

#[test]
fn usage_raw_empty_object_hashes_deterministically() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::json!({}))
        .build();
    let r2 = r1.clone();
    assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn usage_raw_different_values_produce_different_hashes() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::json!({"tokens": 100}))
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::json!({"tokens": 200}))
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// =========================================================================
// Extra: Model/dialect field effects
// =========================================================================

#[test]
fn model_field_present_vs_absent_different_hash() {
    let ts = fixed_ts();
    let r_no_model = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    let r_with_model = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .model("gpt-4")
        .build();
    assert_ne!(
        compute_hash(&r_no_model).unwrap(),
        compute_hash(&r_with_model).unwrap()
    );
}

#[test]
fn model_field_different_values_different_hash() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .model("gpt-4")
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .model("claude-3")
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// =========================================================================
// Extra: Error receipts
// =========================================================================

#[test]
fn error_receipt_hashes_and_verifies() {
    let r = ReceiptBuilder::new("mock")
        .error("something went wrong")
        .with_hash()
        .unwrap();
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(verify_hash(&r));
}

#[test]
fn error_receipt_hash_differs_from_success() {
    let ts = fixed_ts();
    let r_ok = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .outcome(Outcome::Complete)
        .build();
    let r_err = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .outcome(Outcome::Failed)
        .build();
    assert_ne!(compute_hash(&r_ok).unwrap(), compute_hash(&r_err).unwrap());
}

#[test]
fn error_receipt_with_tool_result_is_error_flag() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .outcome(Outcome::Failed)
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::ToolResult {
                tool_name: "exec".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!("error output"),
                is_error: true,
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::Error {
                message: "tool failed".into(),
                error_code: None,
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

// =========================================================================
// Extra: Event count affects hash
// =========================================================================

#[test]
fn event_count_zero_vs_one_different_hash() {
    let ts = fixed_ts();
    let r_empty = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    let r_one = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .build();
    assert_ne!(
        compute_hash(&r_empty).unwrap(),
        compute_hash(&r_one).unwrap()
    );
}

#[test]
fn event_count_different_counts_different_hashes() {
    let ts = fixed_ts();
    let make_event = |msg: &str| AgentEvent {
        ts,
        kind: AgentEventKind::AssistantDelta {
            text: msg.to_string(),
        },
        ext: None,
    };
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(make_event("a"))
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(make_event("a"))
        .add_event(make_event("b"))
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// =========================================================================
// Extra: Batch verification
// =========================================================================

#[test]
fn batch_verification_all_valid() {
    let auditor = ReceiptAuditor::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap();
    let r1 = ReceiptBuilder::new("a")
        .started_at(t1)
        .duration(Duration::from_secs(5))
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .started_at(t2)
        .duration(Duration::from_secs(5))
        .with_hash()
        .unwrap();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(report.is_clean());
    assert_eq!(report.valid, 2);
    assert_eq!(report.invalid, 0);
}

#[test]
fn batch_verification_mixed_valid_invalid() {
    let auditor = ReceiptAuditor::new();
    let good = ReceiptBuilder::new("good")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let mut bad = ReceiptBuilder::new("bad")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    bad.meta.contract_version = "wrong/v0".into();
    let report = auditor.audit_batch(&[good, bad]);
    assert!(!report.is_clean());
    assert_eq!(report.valid, 1);
    assert_eq!(report.invalid, 1);
}

#[test]
fn batch_verification_tampered_hash_detected() {
    let auditor = ReceiptAuditor::new();
    let mut tampered = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    tampered.outcome = Outcome::Failed;
    let report = auditor.audit_batch(&[tampered]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
    assert!(report.issues.iter().any(|i| i.description.contains("hash")));
}

// =========================================================================
// Extra: verify_receipt integration
// =========================================================================

#[test]
fn verify_receipt_passes_for_properly_hashed_receipt() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let result = verify_receipt(&r);
    assert!(result.is_verified());
    assert!(result.hash_valid);
}

#[test]
fn verify_receipt_detects_field_tampering() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.backend.id = "tampered-backend".into();
    let result = verify_receipt(&r);
    assert!(!result.hash_valid);
    assert!(!result.is_verified());
}

#[test]
fn verify_receipt_no_hash_is_valid() {
    let r = deterministic_receipt();
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_detects_tampered_hash_string() {
    let mut r = deterministic_receipt_hashed();
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(!verify_hash(&r));
}

#[test]
fn partial_outcome_hashes_differently_from_complete_and_failed() {
    let ts = fixed_ts();
    let make = |outcome: Outcome| {
        ReceiptBuilder::new("mock")
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(ts)
            .finished_at(ts)
            .outcome(outcome)
            .build()
    };
    let h_complete = compute_hash(&make(Outcome::Complete)).unwrap();
    let h_partial = compute_hash(&make(Outcome::Partial)).unwrap();
    let h_failed = compute_hash(&make(Outcome::Failed)).unwrap();

    let mut set = std::collections::HashSet::new();
    set.insert(h_complete);
    set.insert(h_partial);
    set.insert(h_failed);
    assert_eq!(set.len(), 3);
}
