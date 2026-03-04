// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deep tests for receipt canonicalization and hashing.

use abp_receipt::verify::{ReceiptAuditor, verify_receipt};
use abp_receipt::{
    AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, ReceiptChain, canonicalize,
    compute_hash, verify_hash,
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

// =========================================================================
// 1. Hash determinism (same receipt → same hash)
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

// =========================================================================
// 2. Hash uniqueness (different receipts → different hashes)
// =========================================================================

#[test]
fn hash_uniqueness_different_backends() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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

// =========================================================================
// 3. Self-referential prevention (receipt_sha256 nullified before hashing)
// =========================================================================

#[test]
fn self_referential_prevention_hash_field_ignored() {
    let r1 = deterministic_receipt();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("some_previous_hash_value".into());
    // Hashing must produce the same result regardless of existing receipt_sha256
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

// =========================================================================
// 4. with_hash() correctness
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
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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

// =========================================================================
// 5. Field ordering (BTreeMap determinism)
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
    // Insert keys in reverse alphabetical order
    let mut raw = serde_json::Map::new();
    raw.insert("zebra".into(), serde_json::json!(1));
    raw.insert("apple".into(), serde_json::json!(2));
    raw.insert("mango".into(), serde_json::json!(3));

    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::Value::Object(raw))
        .build();

    let json = canonicalize(&r).unwrap();
    // In canonical JSON, keys within usage_raw should be sorted
    let apple_pos = json.find("\"apple\"").unwrap();
    let mango_pos = json.find("\"mango\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(apple_pos < mango_pos);
    assert!(mango_pos < zebra_pos);
}

// =========================================================================
// 6. Usage raw preservation (arbitrary JSON survives hashing)
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
        .usage_raw(complex.clone())
        .with_hash()
        .unwrap();
    // Model gets merged into usage_raw by builder; verify nested survives
    assert_eq!(r.usage_raw["nested"]["a"], serde_json::json!([1, 2, 3]));
    assert!(r.usage_raw["nested"]["b"].is_null());
    assert!(verify_hash(&r));
}

#[test]
fn usage_raw_empty_object_hashes_deterministically() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
// 7. Timestamp handling
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

// =========================================================================
// 8. Model field effects on hash
// =========================================================================

#[test]
fn model_field_present_vs_absent_different_hash() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
// 9. Error receipts (Failed outcome) hash correctly
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
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
// 10. Event count (trace length) affects hash
// =========================================================================

#[test]
fn event_count_zero_vs_one_different_hash() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
// 11. Batch verification (ReceiptAuditor.audit_batch with mixed valid/invalid)
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
    bad.meta.contract_version = "wrong/v0".into(); // invalidate
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
    tampered.outcome = Outcome::Failed; // tamper after hashing
    let report = auditor.audit_batch(&[tampered]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
    assert!(report.issues.iter().any(|i| i.description.contains("hash")));
}

// =========================================================================
// 12. Chain integrity (sequential receipts verify as chain)
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
    r.backend.id = "tampered".into(); // tamper after hashing
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

// =========================================================================
// 13. Canonical JSON (sorted keys, no extra whitespace)
// =========================================================================

#[test]
fn canonical_json_no_newlines_or_indentation() {
    let r = deterministic_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
    assert!(!json.contains("  ")); // no double-space indentation
}

#[test]
fn canonical_json_top_level_keys_sorted() {
    let r = deterministic_receipt();
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = parsed.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn canonical_json_receipt_sha256_always_null() {
    let r = deterministic_receipt_hashed();
    assert!(r.receipt_sha256.is_some());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

#[test]
fn canonical_json_is_valid_json() {
    let r = ReceiptBuilder::new("mock")
        .model("gpt-4")
        .usage_raw(serde_json::json!({"deep": {"nested": [1, 2]}}))
        .add_event(AgentEvent {
            ts: Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
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
// 14. Empty fields (empty strings, zero counts, null optional fields)
// =========================================================================

#[test]
fn empty_backend_id_hashes_stably() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
fn null_optional_fields_hash_correctly() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    // backend_version, adapter_version, receipt_sha256 are all None
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
    assert!(r.receipt_sha256.is_none());
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(verify_hash(&r));
}

#[test]
fn zero_duration_hashes_correctly() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
fn empty_trace_and_artifacts_hash_correctly() {
    let r = deterministic_receipt();
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn null_usage_tokens_hash_correctly() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(verify_hash(&r)); // no hash set, passes trivially

    let r_hashed = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r_hashed));
}

// =========================================================================
// Extra: verify_receipt integration with canonicalization
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
fn partial_outcome_hashes_differently_from_complete_and_failed() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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

    // All three must be distinct
    let mut set = std::collections::HashSet::new();
    set.insert(h_complete);
    set.insert(h_partial);
    set.insert(h_failed);
    assert_eq!(set.len(), 3);
}

#[test]
fn many_unique_receipts_all_produce_unique_hashes() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
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
