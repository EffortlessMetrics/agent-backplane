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
//! End-to-end tests for receipt canonicalization, hashing, and serialization determinism.

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, ExecutionMode, Outcome, Receipt, receipt_hash,
};
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_receipt::{ReceiptBuilder, canonicalize, compute_hash, verify_hash};
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Helpers ───────────────────────────────────────────────────────────

fn fixed_receipt() -> Receipt {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build()
}

fn receipt_with_backend(name: &str) -> Receipt {
    ReceiptBuilder::new(name).outcome(Outcome::Complete).build()
}

// ── a) Hashing determinism (10 tests) ────────────────────────────────

#[test]
fn hash_same_receipt_always_same_hash() {
    let r = fixed_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    let h3 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1, h3);
}

#[test]
fn hash_different_receipts_differ() {
    let r1 = ReceiptBuilder::new("backend-a")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("backend-b")
        .outcome(Outcome::Failed)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_excludes_receipt_sha256_field() {
    let r1 = fixed_receipt();
    let mut r2 = fixed_receipt();
    r2.receipt_sha256 = Some("some_previous_hash_value".into());
    let mut r3 = fixed_receipt();
    r3.receipt_sha256 = Some("completely_different_hash".into());

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    let h3 = receipt_hash(&r3).unwrap();
    assert_eq!(h1, h2, "hash must ignore receipt_sha256 field");
    assert_eq!(
        h2, h3,
        "hash must ignore receipt_sha256 field regardless of value"
    );
}

#[test]
fn hash_stable_across_serde_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap())
        .finished_at(Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 5).unwrap())
        .build();

    let h_before = compute_hash(&r).unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let h_after = compute_hash(&r2).unwrap();
    assert_eq!(
        h_before, h_after,
        "hash must survive serialization roundtrip"
    );
}

#[test]
fn hash_unchanged_when_receipt_sha256_metadata_changes() {
    let r = fixed_receipt();
    let h_no_hash = compute_hash(&r).unwrap();

    let mut r_with_hash = r.clone();
    r_with_hash.receipt_sha256 = Some(h_no_hash.clone());
    let h_with_hash = compute_hash(&r_with_hash).unwrap();
    assert_eq!(h_no_hash, h_with_hash);
}

#[test]
fn hash_btreemap_ordering_deterministic() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    // Build two receipts with BTreeMap ext fields in different insertion orders
    let mut ext1 = BTreeMap::new();
    ext1.insert("alpha".to_string(), serde_json::json!(1));
    ext1.insert("beta".to_string(), serde_json::json!(2));
    ext1.insert("gamma".to_string(), serde_json::json!(3));

    let mut ext2 = BTreeMap::new();
    ext2.insert("gamma".to_string(), serde_json::json!(3));
    ext2.insert("alpha".to_string(), serde_json::json!(1));
    ext2.insert("beta".to_string(), serde_json::json!(2));

    assert_eq!(ext1, ext2, "BTreeMaps with same content should be equal");

    let evt1 = AgentEvent {
        ts,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext1),
    };
    let evt2 = AgentEvent {
        ts,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext2),
    };

    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(evt1)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(evt2)
        .build();

    assert_eq!(
        compute_hash(&r1).unwrap(),
        compute_hash(&r2).unwrap(),
        "BTreeMap ordering must be deterministic"
    );
}

#[test]
fn hash_empty_fields_consistent() {
    let r1 = ReceiptBuilder::new("").build();
    let r2 = ReceiptBuilder::new("").build();
    // Different run_ids, so hashes will differ, but each hashes consistently
    let h1a = compute_hash(&r1).unwrap();
    let h1b = compute_hash(&r1).unwrap();
    assert_eq!(h1a, h1b, "empty-field receipt must hash consistently");
    assert_eq!(h1a.len(), 64);

    let h2a = compute_hash(&r2).unwrap();
    assert_eq!(h2a.len(), 64);
}

#[test]
fn hash_unicode_content_correct() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("バックエンド🚀")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .backend_version("版本 1.0 — «тест»")
        .build();

    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2, "unicode content must hash deterministically");
    assert_eq!(h1.len(), 64);
}

#[test]
fn hash_large_receipt_correct() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let mut builder = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts);

    for i in 0..1000 {
        builder = builder.add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta {
                text: format!("token_{i}_with_some_padding_text_here"),
            },
            ext: None,
        });
    }
    let r = builder.build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2, "large receipt must hash deterministically");
    assert_eq!(h1.len(), 64);
}

#[test]
fn hash_null_vs_none_receipt_sha256_consistent() {
    let r_none = fixed_receipt();
    assert!(r_none.receipt_sha256.is_none());

    // Manually create one via serde with explicit null
    let json = serde_json::to_string(&r_none).unwrap();
    let r_from_json: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(
        compute_hash(&r_none).unwrap(),
        compute_hash(&r_from_json).unwrap(),
        "None receipt_sha256 and deserialized null must produce same hash"
    );
}

// ── b) Canonical JSON (10 tests) ─────────────────────────────────────

#[test]
fn canonical_keys_sorted() {
    let r = fixed_receipt();
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = v {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "canonical JSON keys must be sorted");
    } else {
        panic!("canonical JSON must be an object");
    }
}

#[test]
fn canonical_no_extra_whitespace() {
    let r = fixed_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'), "no newlines in canonical form");
    assert!(!json.contains("  "), "no double spaces in canonical form");
    // Ensure it's a single line of compact JSON
    assert!(json.starts_with('{'));
    assert!(json.ends_with('}'));
}

#[test]
fn canonical_numbers_consistent() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .usage_tokens(12345, 67890)
        .build();

    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2, "numbers must serialize consistently");
    assert!(j1.contains("12345"), "input_tokens should appear as 12345");
    assert!(j1.contains("67890"), "output_tokens should appear as 67890");
}

#[test]
fn canonical_null_handling_consistent() {
    let r = fixed_receipt();
    let json = canonicalize(&r).unwrap();

    // receipt_sha256 must always be null in canonical form
    assert!(json.contains("\"receipt_sha256\":null"));

    // Canonical form with an existing hash also has null
    let mut r2 = fixed_receipt();
    r2.receipt_sha256 = Some("abc123".into());
    let json2 = canonicalize(&r2).unwrap();
    assert!(json2.contains("\"receipt_sha256\":null"));
    assert_eq!(json, json2);
}

#[test]
fn canonical_empty_arrays_handled() {
    let r = fixed_receipt();
    let json = canonicalize(&r).unwrap();
    // trace and artifacts should be empty arrays
    assert!(json.contains("\"trace\":[]"));
    assert!(json.contains("\"artifacts\":[]"));
}

#[test]
fn canonical_empty_objects_handled() {
    let r = fixed_receipt();
    let json = canonicalize(&r).unwrap();
    // usage_raw defaults to {}
    assert!(json.contains("\"usage_raw\":{}"));
}

#[test]
fn canonical_produces_valid_json() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .usage_tokens(100, 200)
        .add_event(AgentEvent {
            ts: Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
            kind: AgentEventKind::RunStarted {
                message: "hello".into(),
            },
            ext: None,
        })
        .build();
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("canonical JSON must be valid JSON");
    assert!(parsed.is_object());
}

#[test]
fn canonical_outcome_serialized_snake_case() {
    let r_complete = ReceiptBuilder::new("x")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .build();
    let r_failed = ReceiptBuilder::new("x")
        .outcome(Outcome::Failed)
        .run_id(Uuid::nil())
        .build();
    let r_partial = ReceiptBuilder::new("x")
        .outcome(Outcome::Partial)
        .run_id(Uuid::nil())
        .build();

    assert!(
        canonicalize(&r_complete)
            .unwrap()
            .contains("\"outcome\":\"complete\"")
    );
    assert!(
        canonicalize(&r_failed)
            .unwrap()
            .contains("\"outcome\":\"failed\"")
    );
    assert!(
        canonicalize(&r_partial)
            .unwrap()
            .contains("\"outcome\":\"partial\"")
    );
}

#[test]
fn canonical_mode_serialized_snake_case() {
    let r_mapped = ReceiptBuilder::new("x")
        .mode(ExecutionMode::Mapped)
        .run_id(Uuid::nil())
        .build();
    let r_passthrough = ReceiptBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .run_id(Uuid::nil())
        .build();

    let j_mapped = canonicalize(&r_mapped).unwrap();
    let j_passthrough = canonicalize(&r_passthrough).unwrap();
    assert!(j_mapped.contains("\"mode\":\"mapped\""));
    assert!(j_passthrough.contains("\"mode\":\"passthrough\""));
}

#[test]
fn canonical_deterministic_across_multiple_calls() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(1, 2)
        .usage_raw(serde_json::json!({"model": "gpt-4", "extra": true}))
        .backend_version("1.0")
        .add_event(AgentEvent {
            ts: Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        })
        .build();

    let results: Vec<String> = (0..10).map(|_| canonicalize(&r).unwrap()).collect();
    for (i, json) in results.iter().enumerate().skip(1) {
        assert_eq!(&results[0], json, "call {i} differs from call 0");
    }
}

// ── c) Receipt builder/lifecycle (10 tests) ──────────────────────────

#[test]
fn with_hash_returns_receipt_with_hash_filled() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn original_receipt_unchanged_after_with_hash() {
    let original = fixed_receipt();
    assert!(original.receipt_sha256.is_none());

    let hashed = original.clone().with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    // Original should still have no hash (it was cloned before with_hash)
    assert!(original.receipt_sha256.is_none());
}

#[test]
fn double_hash_idempotent() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = r.clone().with_hash().unwrap();
    let hash1 = h1.receipt_sha256.clone().unwrap();

    // Hash again — the hash should be the same because receipt_sha256 is
    // nullified during canonicalization
    let h2 = h1.with_hash().unwrap();
    let hash2 = h2.receipt_sha256.clone().unwrap();
    assert_eq!(hash1, hash2, "double hashing must be idempotent");
}

#[test]
fn receipt_status_complete() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn receipt_status_failed() {
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_status_partial() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn receipt_error_sets_failed_and_adds_trace() {
    let r = ReceiptBuilder::new("mock")
        .error("something went wrong")
        .build();
    assert_eq!(r.outcome, Outcome::Failed);
    assert_eq!(r.trace.len(), 1);
    match &r.trace[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert_eq!(message, "something went wrong");
        }
        other => panic!("expected Error event, got {other:?}"),
    }
}

#[test]
fn receipt_timestamps_monotonic() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 5, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .started_at(t1)
        .finished_at(t2)
        .build();
    assert!(
        r.meta.finished_at >= r.meta.started_at,
        "finished_at must be >= started_at"
    );
    assert_eq!(r.meta.duration_ms, 300_000); // 5 minutes
}

#[test]
fn receipt_contract_version_correct() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.meta.contract_version, abp_core::CONTRACT_VERSION);
    assert_eq!(r.meta.contract_version, "abp/v0.1");
}

#[test]
fn receipt_builder_with_hash_produces_verifiable_receipt() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(500, 1000)
        .backend_version("2.0")
        .model("gpt-4o")
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r), "receipt built with with_hash must verify");
}

// ── d) Receipt store (5 tests) ───────────────────────────────────────

#[test]
fn store_and_retrieve_receipt() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let id = r.meta.run_id;
    store.store(r).unwrap();

    let retrieved = store.get(id).unwrap().unwrap();
    assert_eq!(retrieved.backend.id, "mock");
    assert_eq!(retrieved.outcome, Outcome::Complete);
    assert!(retrieved.receipt_sha256.is_some());
}

#[test]
fn store_retrieve_nonexistent_returns_none() {
    let store = InMemoryReceiptStore::new();
    let result = store.get(Uuid::new_v4()).unwrap();
    assert!(result.is_none());
}

#[test]
fn store_multiple_receipts() {
    let mut store = InMemoryReceiptStore::new();
    let mut ids = Vec::new();

    for name in ["alpha", "beta", "gamma", "delta", "epsilon"] {
        let r = receipt_with_backend(name);
        ids.push(r.meta.run_id);
        store.store(r).unwrap();
    }

    assert_eq!(store.len(), 5);

    for (i, id) in ids.iter().enumerate() {
        let r = store.get(*id).unwrap().unwrap();
        assert_eq!(
            r.backend.id,
            ["alpha", "beta", "gamma", "delta", "epsilon"][i]
        );
    }
}

#[test]
fn store_query_by_backend_id() {
    let mut store = InMemoryReceiptStore::new();

    for _ in 0..3 {
        store.store(receipt_with_backend("target")).unwrap();
    }
    for _ in 0..2 {
        store.store(receipt_with_backend("other")).unwrap();
    }

    let filter = ReceiptFilter {
        backend_id: Some("target".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 3);
    for r in &results {
        assert_eq!(r.backend_id, "target");
    }
}

#[test]
fn store_duplicate_rejected() {
    use abp_receipt::store::StoreError;

    let mut store = InMemoryReceiptStore::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a").run_id(id).build();
    let r2 = ReceiptBuilder::new("b").run_id(id).build();

    store.store(r1).unwrap();
    let err = store.store(r2).unwrap_err();
    assert!(matches!(err, StoreError::DuplicateId(_)));
}

// ── Additional edge-case tests ───────────────────────────────────────

#[test]
fn verify_hash_detects_tampered_backend_id() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.backend.id = "tampered".into();
    assert!(
        !verify_hash(&r),
        "tampered backend.id must fail verification"
    );
}

#[test]
fn verify_hash_detects_tampered_outcome() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r), "tampered outcome must fail verification");
}

#[test]
fn verify_hash_passes_when_no_hash_present() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
    assert!(
        verify_hash(&r),
        "no hash means verification passes vacuously"
    );
}

#[test]
fn canonical_json_identical_for_cloned_receipt() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(42, 84)
        .backend_version("1.0")
        .build();
    let cloned = r.clone();
    assert_eq!(
        canonicalize(&r).unwrap(),
        canonicalize(&cloned).unwrap(),
        "cloned receipt must produce identical canonical JSON"
    );
}

#[test]
fn hash_with_artifacts() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();

    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2, "receipt with artifacts must hash deterministically");

    // Verify different artifacts produce different hash
    let r2 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_artifact(ArtifactRef {
            kind: "diff".into(),
            path: "other.diff".into(),
        })
        .build();
    assert_ne!(
        compute_hash(&r).unwrap(),
        compute_hash(&r2).unwrap(),
        "different artifacts must produce different hash"
    );
}

#[test]
fn receipt_hash_and_compute_hash_agree() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(100, 200)
        .build();

    let h_core = receipt_hash(&r).unwrap();
    let h_receipt = compute_hash(&r).unwrap();
    assert_eq!(
        h_core, h_receipt,
        "abp_core::receipt_hash and abp_receipt::compute_hash must agree"
    );
}
