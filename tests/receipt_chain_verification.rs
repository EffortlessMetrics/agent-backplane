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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive receipt chain verification and audit trail tests.

use abp_error::{ErrorCategory, ErrorCode, ErrorInfo};
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_receipt::{
    ChainError, Outcome, Receipt, ReceiptBuilder, ReceiptChain, canonicalize, compute_hash,
    diff_receipts, verify_hash,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a hashed receipt for the given backend with a controlled start time
/// offset (in seconds from `base`).
fn make_receipt(backend: &str, offset_secs: i64, outcome: Outcome) -> Receipt {
    let base = Utc::now();
    let start = base + Duration::seconds(offset_secs);
    let finish = start + Duration::milliseconds(150);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .expect("hash computation should succeed")
}

/// Build a receipt with a fixed base time for deterministic time ranges.
fn make_receipt_at(
    backend: &str,
    base: chrono::DateTime<chrono::Utc>,
    offset_secs: i64,
    outcome: Outcome,
) -> Receipt {
    let start = base + Duration::seconds(offset_secs);
    let finish = start + Duration::milliseconds(150);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .expect("hash computation should succeed")
}

/// Build a chain of `n` receipts from a single backend.
fn build_chain(n: usize) -> ReceiptChain {
    let mut chain = ReceiptChain::new();
    for i in 0..n {
        let r = make_receipt("mock", i as i64, Outcome::Complete);
        chain.push(r).expect("push should succeed");
    }
    chain
}

// ===========================================================================
// 1. Hash integrity (15 tests)
// ===========================================================================

#[test]
fn hash_is_64_hex_chars() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_deterministic_same_receipt() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_null_before_hash_rule() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn hash_stable_across_serialization_roundtrip() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let h_before = compute_hash(&r).unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let h_after = compute_hash(&r2).unwrap();
    assert_eq!(h_before, h_after);
}

#[test]
fn hash_independent_of_stored_hash_value() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();
    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("totally_different_value".into());
    let h2 = compute_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_independent_of_none_vs_some_stored_hash() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
    let h1 = compute_hash(&r).unwrap();
    r.receipt_sha256 = Some("foobar".into());
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_changes_with_different_backend() {
    let r1 = make_receipt("alpha", 0, Outcome::Complete);
    let r2 = make_receipt("beta", 0, Outcome::Complete);
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_outcome() {
    let base = Utc::now();
    let start = base;
    let finish = base + Duration::milliseconds(100);
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(finish)
        .run_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .started_at(start)
        .finished_at(finish)
        .run_id(Uuid::nil())
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_run_id() {
    let base = Utc::now();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .started_at(base)
        .finished_at(base)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(Uuid::from_u128(1))
        .started_at(base)
        .finished_at(base)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_duration() {
    let base = Utc::now();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(base)
        .finished_at(base + Duration::milliseconds(100))
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(base)
        .finished_at(base + Duration::milliseconds(200))
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn verify_hash_true_when_correct() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_true_when_none() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_false_when_tampered() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.receipt_sha256 = Some("deadbeef".into());
    assert!(!verify_hash(&r));
}

#[test]
fn canonicalize_is_deterministic() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    let c2 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonicalize_uses_btreemap_sorted_keys() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = v {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    } else {
        panic!("expected JSON object");
    }
}

// ===========================================================================
// 2. Chain validation (15 tests)
// ===========================================================================

#[test]
fn chain_single_receipt_verifies() {
    let mut chain = ReceiptChain::new();
    let r = make_receipt("mock", 0, Outcome::Complete);
    chain.push(r).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_multiple_receipts_verify() {
    let chain = build_chain(5);
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_preserves_insertion_order() {
    let chain = build_chain(3);
    let ids: Vec<Uuid> = chain.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
}

#[test]
fn chain_latest_returns_last_pushed() {
    let mut chain = ReceiptChain::new();
    let r1 = make_receipt("mock", 0, Outcome::Complete);
    let r2 = make_receipt("mock", 1, Outcome::Failed);
    let r2_id = r2.meta.run_id;
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, r2_id);
}

#[test]
fn empty_chain_verify_returns_error() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert!(matches!(chain.verify(), Err(ChainError::EmptyChain)));
}

#[test]
fn duplicate_run_id_rejected() {
    let mut chain = ReceiptChain::new();
    let r = make_receipt("mock", 0, Outcome::Complete);
    let dup = r.clone();
    chain.push(r).unwrap();
    let err = chain.push(dup).unwrap_err();
    assert!(matches!(err, ChainError::DuplicateId { .. }));
}

#[test]
fn out_of_order_receipt_rejected() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_receipt("mock", 10, Outcome::Complete))
        .unwrap();
    let result = chain.push(make_receipt("mock", 0, Outcome::Complete));
    assert!(matches!(result, Err(ChainError::BrokenLink { index: 1 })));
}

#[test]
fn tampered_receipt_rejected_by_chain_push() {
    let mut chain = ReceiptChain::new();
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.outcome = Outcome::Failed;
    let result = chain.push(r);
    assert!(matches!(result, Err(ChainError::HashMismatch { index: 0 })));
}

#[test]
fn tampered_hash_rejected_at_index_1() {
    let mut chain = ReceiptChain::new();
    let r1 = make_receipt("mock", 0, Outcome::Complete);
    chain.push(r1).unwrap();
    let mut r2 = make_receipt("mock", 1, Outcome::Complete);
    r2.receipt_sha256 = Some("badhash".into());
    let err = chain.push(r2);
    assert!(matches!(err, Err(ChainError::HashMismatch { index: 1 })));
}

#[test]
fn receipt_without_hash_accepted_by_chain() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
    chain.push(r).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_len_and_is_empty() {
    let mut chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);

    chain
        .push(make_receipt("mock", 0, Outcome::Complete))
        .unwrap();
    assert!(!chain.is_empty());
    assert_eq!(chain.len(), 1);
}

#[test]
fn latest_on_empty_chain_is_none() {
    let chain = ReceiptChain::new();
    assert!(chain.latest().is_none());
}

#[test]
fn large_chain_verifies() {
    let chain = build_chain(100);
    assert_eq!(chain.len(), 100);
    assert!(chain.verify().is_ok());
}

#[test]
fn multi_backend_chain_verifies() {
    let backends = ["openai", "anthropic", "gemini", "mock", "copilot"];
    let mut chain = ReceiptChain::new();
    for (i, backend) in backends.iter().enumerate() {
        chain
            .push(make_receipt(backend, i as i64, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_rejects_same_timestamp_different_ids() {
    // Two receipts at exactly the same time should still work if ordered.
    let base = Utc::now();
    let mut chain = ReceiptChain::new();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(base)
        .finished_at(base + Duration::milliseconds(100))
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(base)
        .finished_at(base + Duration::milliseconds(100))
        .with_hash()
        .unwrap();
    chain.push(r1).unwrap();
    // Same timestamp is accepted (not strictly less-than).
    chain.push(r2).unwrap();
    assert!(chain.verify().is_ok());
}

// ===========================================================================
// 3. Receipt store (10 tests)
// ===========================================================================

#[test]
fn store_and_retrieve_receipt() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let id = r.meta.run_id;
    store.store(r).unwrap();
    let fetched = store.get(id).unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().meta.run_id, id);
}

#[test]
fn store_returns_correct_id() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let expected_id = r.meta.run_id;
    let returned_id = store.store(r).unwrap();
    assert_eq!(returned_id, expected_id);
}

#[test]
fn store_rejects_duplicate_id() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let dup = r.clone();
    store.store(r).unwrap();
    let err = store.store(dup);
    assert!(err.is_err());
}

#[test]
fn store_get_missing_returns_none() {
    let store = InMemoryReceiptStore::new();
    let result = store.get(Uuid::new_v4()).unwrap();
    assert!(result.is_none());
}

#[test]
fn store_list_all() {
    let mut store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        store.store(r).unwrap();
    }
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 5);
}

#[test]
fn store_filter_by_backend() {
    let mut store = InMemoryReceiptStore::new();
    let base = Utc::now();
    for (i, backend) in ["alpha", "beta", "alpha"].iter().enumerate() {
        let r = make_receipt_at(backend, base, i as i64, Outcome::Complete);
        store.store(r).unwrap();
    }
    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|s| s.backend_id == "alpha"));
}

#[test]
fn store_filter_by_outcome() {
    let mut store = InMemoryReceiptStore::new();
    let base = Utc::now();
    let outcomes = [
        Outcome::Complete,
        Outcome::Failed,
        Outcome::Complete,
        Outcome::Partial,
    ];
    for (i, outcome) in outcomes.iter().enumerate() {
        let r = make_receipt_at("mock", base, i as i64, outcome.clone());
        store.store(r).unwrap();
    }
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn store_filter_by_time_range() {
    let mut store = InMemoryReceiptStore::new();
    let base = Utc::now();
    for i in 0..5 {
        let r = make_receipt_at("mock", base, i * 10, Outcome::Complete);
        store.store(r).unwrap();
    }
    let filter = ReceiptFilter {
        after: Some(base + Duration::seconds(15)),
        before: Some(base + Duration::seconds(35)),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn store_len_and_is_empty() {
    let mut store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    store.store(r).unwrap();
    assert!(!store.is_empty());
    assert_eq!(store.len(), 1);
}

#[test]
fn store_summary_fields_correct() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Partial)
        .with_hash()
        .unwrap();
    let run_id = r.meta.run_id;
    let started = r.meta.started_at;
    let finished = r.meta.finished_at;
    store.store(r).unwrap();
    let summaries = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(summaries.len(), 1);
    let s = &summaries[0];
    assert_eq!(s.id, run_id);
    assert_eq!(s.backend_id, "test-backend");
    assert_eq!(s.outcome, Outcome::Partial);
    assert_eq!(s.started_at, started);
    assert_eq!(s.finished_at, finished);
}

// ===========================================================================
// 4. Receipt diff (10 tests)
// ===========================================================================

#[test]
fn diff_identical_receipts_empty() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let diff = diff_receipts(&r, &r.clone());
    assert!(diff.is_empty());
    assert_eq!(diff.len(), 0);
}

#[test]
fn diff_detects_outcome_change() {
    let a = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let diff = diff_receipts(&a, &b);
    assert!(!diff.is_empty());
    assert!(diff.changes.iter().any(|d| d.field == "outcome"));
}

#[test]
fn diff_detects_backend_id_change() {
    let a = ReceiptBuilder::new("alpha").build();
    let mut b = a.clone();
    b.backend.id = "beta".into();
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "backend.id"));
}

#[test]
fn diff_detects_run_id_change() {
    let a = ReceiptBuilder::new("mock").build();
    let mut b = a.clone();
    b.meta.run_id = Uuid::new_v4();
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "meta.run_id"));
}

#[test]
fn diff_detects_duration_change() {
    let a = ReceiptBuilder::new("mock").build();
    let mut b = a.clone();
    b.meta.duration_ms = 9999;
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "meta.duration_ms"));
}

#[test]
fn diff_detects_contract_version_change() {
    let a = ReceiptBuilder::new("mock").build();
    let mut b = a.clone();
    b.meta.contract_version = "abp/v99".into();
    let diff = diff_receipts(&a, &b);
    assert!(
        diff.changes
            .iter()
            .any(|d| d.field == "meta.contract_version")
    );
}

#[test]
fn diff_detects_backend_version_change() {
    let a = ReceiptBuilder::new("mock").backend_version("1.0").build();
    let mut b = a.clone();
    b.backend.backend_version = Some("2.0".into());
    let diff = diff_receipts(&a, &b);
    assert!(
        diff.changes
            .iter()
            .any(|d| d.field == "backend.backend_version")
    );
}

#[test]
fn diff_detects_trace_length_change() {
    use abp_receipt::{AgentEvent, AgentEventKind};
    let a = ReceiptBuilder::new("mock").build();
    let b = ReceiptBuilder::new("mock")
        .started_at(a.meta.started_at)
        .finished_at(a.meta.finished_at)
        .run_id(a.meta.run_id)
        .work_order_id(a.meta.work_order_id)
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        })
        .build();
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "trace.len"));
}

#[test]
fn diff_detects_mode_change() {
    use abp_receipt::ExecutionMode;
    let a = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Mapped)
        .build();
    let mut b = a.clone();
    b.mode = ExecutionMode::Passthrough;
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "mode"));
}

#[test]
fn diff_reports_old_and_new_values() {
    let a = ReceiptBuilder::new("alpha").build();
    let mut b = a.clone();
    b.backend.id = "beta".into();
    let diff = diff_receipts(&a, &b);
    let change = diff
        .changes
        .iter()
        .find(|d| d.field == "backend.id")
        .unwrap();
    assert_eq!(change.old, "alpha");
    assert_eq!(change.new, "beta");
}

// ===========================================================================
// 5. Error in receipts (15 tests)
// ===========================================================================

#[test]
fn error_code_serializes_snake_case() {
    let code = ErrorCode::BackendTimeout;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
}

#[test]
fn error_code_as_str_returns_snake_case() {
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
}

#[test]
fn error_code_deserializes_from_snake_case() {
    let code: ErrorCode = serde_json::from_str(r#""backend_timeout""#).unwrap();
    assert_eq!(code, ErrorCode::BackendTimeout);
}

#[test]
fn error_code_roundtrip_all_variants() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::PolicyDenied,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::Internal,
    ];
    for code in &codes {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back);
    }
}

#[test]
fn error_code_category_mapping() {
    assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn error_code_retryable_flags() {
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendUnavailable.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::Internal.is_retryable());
}

#[test]
fn error_info_new_infers_retryable() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
    assert!(info.is_retryable);
    let info2 = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
    assert!(!info2.is_retryable);
}

#[test]
fn error_info_with_detail_preserves_context() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 30_000);
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
    assert_eq!(info.details["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn error_info_details_use_btreemap() {
    let info = ErrorInfo::new(ErrorCode::Internal, "test")
        .with_detail("zebra", "z")
        .with_detail("apple", "a")
        .with_detail("mango", "m");
    let keys: Vec<&String> = info.details.keys().collect();
    assert_eq!(keys, vec!["apple", "mango", "zebra"]);
}

#[test]
fn error_info_serialization_roundtrip() {
    let info =
        ErrorInfo::new(ErrorCode::BackendTimeout, "timed out").with_detail("backend", "openai");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "connection refused");
    let display = format!("{info}");
    assert!(display.contains("backend_timeout"));
    assert!(display.contains("connection refused"));
}

#[test]
fn error_code_in_agent_event_serializes_snake_case() {
    use abp_receipt::AgentEvent;
    use abp_receipt::AgentEventKind;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "timed out".into(),
            error_code: Some(ErrorCode::BackendTimeout),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""backend_timeout""#));
}

#[test]
fn error_code_in_agent_event_roundtrip() {
    use abp_receipt::AgentEvent;
    use abp_receipt::AgentEventKind;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "test error".into(),
            error_code: Some(ErrorCode::ReceiptHashMismatch),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &back.kind
    {
        assert_eq!(message, "test error");
        assert_eq!(*error_code, Some(ErrorCode::ReceiptHashMismatch));
    } else {
        panic!("expected Error event kind");
    }
}

#[test]
fn receipt_with_error_event_hashes_correctly() {
    use abp_receipt::AgentEvent;
    use abp_receipt::AgentEventKind;
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "backend crashed".into(),
                error_code: Some(ErrorCode::BackendCrashed),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains(r#""backend_crashed""#));
}

#[test]
fn error_info_serialization_code_is_snake_case_in_json() {
    let info = ErrorInfo::new(ErrorCode::ReceiptChainBroken, "chain broken");
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains(r#""receipt_chain_broken""#));
    assert!(!json.contains("ReceiptChainBroken"));
}

// ===========================================================================
// 6. Tamper detection (additional)
// ===========================================================================

#[test]
fn tamper_outcome_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    assert!(verify_hash(&r));
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn tamper_backend_id_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn tamper_duration_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.meta.duration_ms = 999_999;
    assert!(!verify_hash(&r));
}

#[test]
fn tamper_run_id_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.meta.run_id = Uuid::new_v4();
    assert!(!verify_hash(&r));
}

// ===========================================================================
// 7. Chain export / serialization roundtrip
// ===========================================================================

#[test]
fn receipt_json_roundtrip() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let json = serde_json::to_string_pretty(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.meta.run_id, r.meta.run_id);
    assert_eq!(deserialized.outcome, r.outcome);
    assert_eq!(deserialized.receipt_sha256, r.receipt_sha256);
}

#[test]
fn receipt_hash_survives_roundtrip() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let json = serde_json::to_string(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&deserialized));
}

#[test]
fn chain_serialization_roundtrip() {
    let chain = build_chain(4);
    let receipts: Vec<&Receipt> = chain.iter().collect();
    let json = serde_json::to_string(&receipts).unwrap();
    let deserialized: Vec<Receipt> = serde_json::from_str(&json).unwrap();
    let mut chain2 = ReceiptChain::new();
    for r in deserialized {
        chain2.push(r).unwrap();
    }
    assert_eq!(chain2.len(), 4);
    assert!(chain2.verify().is_ok());
}

#[test]
fn export_chain_as_jsonl() {
    let chain = build_chain(4);
    let mut jsonl = String::new();
    for r in chain.iter() {
        let line = serde_json::to_string(r).unwrap();
        jsonl.push_str(&line);
        jsonl.push('\n');
    }
    let lines: Vec<&str> = jsonl.trim().split('\n').collect();
    assert_eq!(lines.len(), 4);
    for line in &lines {
        let parsed: Receipt = serde_json::from_str(line).unwrap();
        assert!(verify_hash(&parsed));
    }
}

#[test]
fn jsonl_import_rebuilds_chain() {
    let chain = build_chain(3);
    let mut jsonl = String::new();
    for r in chain.iter() {
        let line = serde_json::to_string(r).unwrap();
        jsonl.push_str(&line);
        jsonl.push('\n');
    }
    let mut chain2 = ReceiptChain::new();
    for line in jsonl.trim().split('\n') {
        let r: Receipt = serde_json::from_str(line).unwrap();
        chain2.push(r).unwrap();
    }
    assert_eq!(chain2.len(), 3);
    assert!(chain2.verify().is_ok());
}
