#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive receipt verification tests covering hash integrity, determinism,
//! sensitivity, self-referential prevention, chain validation, store operations,
//! diffing, export/import roundtrips, canonical JSON, and outcome variants.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    ContractError, ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport,
};
use abp_receipt::serde_formats;
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_receipt::verify::{ReceiptAuditor, verify_receipt};
use abp_receipt::{
    ChainBuilder, ChainError, ReceiptBuilder, ReceiptChain, ReceiptValidator, ValidationError,
    canonicalize, compute_hash, diff_receipts as receipt_diff_receipts, verify_hash,
};
use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend).outcome(outcome).build()
}

fn make_hashed_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .with_hash()
        .unwrap()
}

fn make_receipt_with_tokens(backend: &str, input: u64, output: u64) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .usage_tokens(input, output)
        .with_hash()
        .unwrap()
}

fn make_receipt_at(backend: &str, start_ms: i64, dur_ms: u64) -> Receipt {
    let start = Utc.timestamp_millis_opt(start_ms).unwrap();
    let dur = Duration::from_millis(dur_ms);
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(start)
        .duration(dur)
        .with_hash()
        .unwrap()
}

fn make_full_receipt() -> Receipt {
    let start = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(3),
        estimated_cost_usd: Some(0.015),
    };

    let event = AgentEvent {
        ts: start,
        kind: AgentEventKind::RunStarted {
            message: "starting".into(),
        },
        ext: None,
    };

    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "/tmp/output.patch".into(),
    };

    let verification = VerificationReport {
        git_diff: Some("diff --git a/foo b/foo".into()),
        git_status: Some("M foo".into()),
        harness_ok: true,
    };

    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .backend_version("2.1.0")
        .adapter_version("1.0.0")
        .model("gpt-4")
        .dialect("openai")
        .started_at(start)
        .duration(Duration::from_millis(5000))
        .work_order_id(Uuid::nil())
        .capabilities(caps)
        .mode(ExecutionMode::Passthrough)
        .usage(usage)
        .usage_raw(json!({"model": "gpt-4", "total_tokens": 1500}))
        .verification(verification)
        .events(vec![event])
        .add_artifact(artifact)
        .build()
}

// ===========================================================================
// 1. Hash integrity
// ===========================================================================

#[test]
fn hash_integrity_correct_hash_verifies() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn hash_integrity_tampered_hash_fails() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("deadbeef".into());
    assert!(!verify_hash(&r));
}

#[test]
fn hash_integrity_none_hash_passes() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn hash_integrity_empty_string_hash_fails() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some(String::new());
    assert!(!verify_hash(&r));
}

#[test]
fn hash_integrity_correct_length() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
}

#[test]
fn hash_integrity_hex_chars_only() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_integrity_lowercase_hex() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash, &hash.to_lowercase());
}

#[test]
fn hash_integrity_compute_hash_matches_stored() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let recomputed = compute_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &recomputed);
}

// ===========================================================================
// 2. Hash determinism
// ===========================================================================

#[test]
fn hash_determinism_same_receipt_same_hash() {
    let r = make_receipt("mock", Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_determinism_clone_same_hash() {
    let r = make_receipt("mock", Outcome::Complete);
    let clone = r.clone();
    assert_eq!(compute_hash(&r).unwrap(), compute_hash(&clone).unwrap());
}

#[test]
fn hash_determinism_builder_with_fixed_id() {
    let id = Uuid::nil();
    let start = Utc.timestamp_millis_opt(1_000_000).unwrap();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(id)
        .work_order_id(id)
        .started_at(start)
        .duration(Duration::from_millis(100))
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(id)
        .work_order_id(id)
        .started_at(start)
        .duration(Duration::from_millis(100))
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_determinism_canonicalize_stable() {
    let r = make_receipt("mock", Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    let c2 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn hash_determinism_hundred_iterations() {
    let r = make_receipt("mock", Outcome::Complete);
    let base = compute_hash(&r).unwrap();
    for _ in 0..100 {
        assert_eq!(compute_hash(&r).unwrap(), base);
    }
}

// ===========================================================================
// 3. Hash sensitivity (any field change → different hash)
// ===========================================================================

#[test]
fn hash_sensitivity_backend_id() {
    let a = make_receipt("alpha", Outcome::Complete);
    let b = make_receipt("beta", Outcome::Complete);
    assert_ne!(compute_hash(&a).unwrap(), compute_hash(&b).unwrap());
}

#[test]
fn hash_sensitivity_outcome_complete_vs_failed() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.outcome = Outcome::Failed;
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_outcome_complete_vs_partial() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.outcome = Outcome::Partial;
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_outcome_failed_vs_partial() {
    let r = make_receipt("mock", Outcome::Failed);
    let mut r2 = r.clone();
    r2.outcome = Outcome::Partial;
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_backend_version() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.backend.backend_version = Some("2.0".into());
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_adapter_version() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.backend.adapter_version = Some("1.0".into());
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_contract_version() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.meta.contract_version = "abp/v999".into();
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_work_order_id() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.meta.work_order_id = Uuid::new_v4();
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_run_id() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.meta.run_id = Uuid::new_v4();
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_duration_ms() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.meta.duration_ms = 999_999;
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_usage_raw() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.usage_raw = json!({"extra": true});
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_input_tokens() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.usage.input_tokens = Some(42);
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_output_tokens() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.usage.output_tokens = Some(42);
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_verification_harness() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.verification.harness_ok = !r.verification.harness_ok;
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_verification_git_diff() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.verification.git_diff = Some("changed".into());
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_trace_addition() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "warn".into(),
        },
        ext: None,
    });
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_artifact_addition() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.artifacts.push(ArtifactRef {
        kind: "log".into(),
        path: "/tmp/log.txt".into(),
    });
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_mode_change() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.mode = ExecutionMode::Passthrough;
    // Only differs if the default was different
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    // If mode changed, hash should differ; if same, they're equal
    if r.mode != r2.mode {
        assert_ne!(h1, h2);
    }
}

#[test]
fn hash_sensitivity_capabilities() {
    let r = make_receipt("mock", Outcome::Complete);
    let mut r2 = r.clone();
    r2.capabilities
        .insert(Capability::Streaming, SupportLevel::Native);
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

// ===========================================================================
// 4. Self-referential prevention
// ===========================================================================

#[test]
fn self_ref_hash_field_excluded_from_canonical() {
    let mut r = make_receipt("mock", Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    r.receipt_sha256 = Some("anything".into());
    let c2 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2, "receipt_sha256 must be nulled before hashing");
}

#[test]
fn self_ref_hash_with_none_and_some_produce_same_canonical() {
    let r1 = make_receipt("mock", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("ffffffffffffffffffffffffffffffffffff".into());
    assert_eq!(canonicalize(&r1).unwrap(), canonicalize(&r2).unwrap());
}

#[test]
fn self_ref_hash_compute_ignores_existing_hash() {
    let mut r = make_receipt("mock", Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();
    r.receipt_sha256 = Some("bogus_hash".into());
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn self_ref_canonical_json_has_null_receipt_sha256() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let canonical = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&canonical).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn self_ref_with_hash_then_verify_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    // Compute again independently
    let h = compute_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &h);
}

// ===========================================================================
// 5. Receipt chain validation
// ===========================================================================

#[test]
fn chain_empty_chain_errors() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
}

#[test]
fn chain_single_receipt_verifies() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_multiple_receipts_verify() {
    let mut chain = ReceiptChain::new();
    for _ in 0..5 {
        chain
            .push(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    assert!(chain.verify().is_ok());
    assert_eq!(chain.len(), 5);
}

#[test]
fn chain_duplicate_id_rejected() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let mut chain = ReceiptChain::new();
    chain.push(r.clone()).unwrap();
    let err = chain.push(r).unwrap_err();
    matches!(err, ChainError::DuplicateId { .. });
}

#[test]
fn chain_tampered_hash_detected() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let mut chain = ReceiptChain::new();
    let err = chain.push(r).unwrap_err();
    assert!(matches!(err, ChainError::HashMismatch { index: 0 }));
}

#[test]
fn chain_verify_chain_comprehensive() {
    let mut chain = ReceiptChain::new();
    for _ in 0..3 {
        chain
            .push(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn chain_sequence_numbers_contiguous() {
    let mut chain = ReceiptChain::new();
    for _ in 0..4 {
        chain
            .push(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    for i in 0..4 {
        assert_eq!(chain.sequence_at(i), Some(i as u64));
    }
}

#[test]
fn chain_parent_hash_linkage() {
    let mut chain = ReceiptChain::new();
    let r1 = make_hashed_receipt("mock", Outcome::Complete);
    let hash1 = r1.receipt_sha256.clone();
    chain.push(r1).unwrap();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();

    assert!(chain.parent_hash_at(0).is_none());
    assert_eq!(chain.parent_hash_at(1).map(String::from), hash1,);
}

#[test]
fn chain_detect_tampering_clean() {
    let mut chain = ReceiptChain::new();
    for _ in 0..3 {
        chain
            .push(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    assert!(chain.detect_tampering().is_empty());
}

#[test]
fn chain_find_gaps_none() {
    let mut chain = ReceiptChain::new();
    for _ in 0..3 {
        chain
            .push(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn chain_summary_counts() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    chain
        .push(make_hashed_receipt("mock", Outcome::Failed))
        .unwrap();
    chain
        .push(make_hashed_receipt("mock", Outcome::Partial))
        .unwrap();
    let summary = chain.chain_summary();
    assert_eq!(summary.total_receipts, 3);
    assert_eq!(summary.complete_count, 1);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.partial_count, 1);
    assert!(summary.all_hashes_valid);
}

#[test]
fn chain_summary_multiple_backends() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("alpha", Outcome::Complete))
        .unwrap();
    chain
        .push(make_hashed_receipt("beta", Outcome::Complete))
        .unwrap();
    let summary = chain.chain_summary();
    assert_eq!(summary.backends.len(), 2);
}

#[test]
fn chain_latest_returns_last() {
    let mut chain = ReceiptChain::new();
    let r1 = make_hashed_receipt("first", Outcome::Complete);
    let r2 = make_hashed_receipt("second", Outcome::Complete);
    let expected_id = r2.meta.run_id;
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, expected_id);
}

#[test]
fn chain_get_by_index() {
    let mut chain = ReceiptChain::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id;
    chain.push(r).unwrap();
    assert_eq!(chain.get(0).unwrap().meta.run_id, id);
    assert!(chain.get(1).is_none());
}

#[test]
fn chain_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
}

#[test]
fn chain_builder_constructs_valid_chain() {
    let chain = ChainBuilder::new()
        .append(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap()
        .append(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap()
        .build();
    assert_eq!(chain.len(), 2);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_builder_with_sequence_gap() {
    let chain = ChainBuilder::new()
        .append_with_sequence(make_hashed_receipt("mock", Outcome::Complete), 0)
        .unwrap()
        .append_with_sequence(make_hashed_receipt("mock", Outcome::Complete), 5)
        .unwrap()
        .build();
    let gaps = chain.find_gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].expected, 1);
    assert_eq!(gaps[0].actual, 5);
}

#[test]
fn chain_builder_skip_validation() {
    // Should accept unhashed receipt without error
    let r = make_receipt("mock", Outcome::Complete);
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r)
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_iter() {
    let mut chain = ReceiptChain::new();
    for _ in 0..3 {
        chain
            .push(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.iter().count(), 3);
}

#[test]
fn chain_as_slice() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    assert_eq!(chain.as_slice().len(), 1);
}

// ===========================================================================
// 6. Receipt store operations
// ===========================================================================

#[test]
fn store_add_and_get() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id;
    store.store(r).unwrap();
    let got = store.get(id).unwrap();
    assert!(got.is_some());
    assert_eq!(got.unwrap().meta.run_id, id);
}

#[test]
fn store_get_nonexistent() {
    let store = InMemoryReceiptStore::new();
    assert!(store.get(Uuid::new_v4()).unwrap().is_none());
}

#[test]
fn store_duplicate_rejected() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    store.store(r.clone()).unwrap();
    assert!(store.store(r).is_err());
}

#[test]
fn store_list_all() {
    let mut store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 5);
}

#[test]
fn store_filter_by_backend() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("alpha", Outcome::Complete))
        .unwrap();
    store
        .store(make_hashed_receipt("beta", Outcome::Complete))
        .unwrap();
    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "alpha");
}

#[test]
fn store_filter_by_outcome() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    store
        .store(make_hashed_receipt("mock", Outcome::Failed))
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[test]
fn store_len_and_empty() {
    let mut store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    store
        .store(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    assert!(!store.is_empty());
    assert_eq!(store.len(), 1);
}

// ===========================================================================
// 7. Receipt diff
// ===========================================================================

#[test]
fn diff_identical_receipts_empty() {
    let r = make_receipt("mock", Outcome::Complete);
    let d = receipt_diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);
}

#[test]
fn diff_outcome_change_detected() {
    let a = make_receipt("mock", Outcome::Complete);
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let d = receipt_diff_receipts(&a, &b);
    assert!(!d.is_empty());
    assert!(d.changes.iter().any(|f| f.field == "outcome"));
}

#[test]
fn diff_backend_id_change_detected() {
    let a = make_receipt("alpha", Outcome::Complete);
    let mut b = a.clone();
    b.backend.id = "beta".into();
    let d = receipt_diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|f| f.field == "backend.id"));
}

#[test]
fn diff_multiple_changes() {
    let a = make_receipt("mock", Outcome::Complete);
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    b.backend.id = "other".into();
    b.usage.input_tokens = Some(999);
    let d = receipt_diff_receipts(&a, &b);
    assert!(d.len() >= 3);
}

#[test]
fn diff_usage_token_change() {
    let a = make_receipt("mock", Outcome::Complete);
    let mut b = a.clone();
    b.usage.output_tokens = Some(42);
    let d = receipt_diff_receipts(&a, &b);
    assert!(!d.is_empty());
}

#[test]
fn diff_verification_change() {
    let a = make_receipt("mock", Outcome::Complete);
    let mut b = a.clone();
    b.verification.harness_ok = !a.verification.harness_ok;
    let d = receipt_diff_receipts(&a, &b);
    assert!(!d.is_empty());
}

// ===========================================================================
// 8. Export/import roundtrip (serde_formats)
// ===========================================================================

#[test]
fn serde_json_roundtrip() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let json = serde_formats::to_json(&r).unwrap();
    let r2 = serde_formats::from_json(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert!(verify_hash(&r2));
}

#[test]
fn serde_bytes_roundtrip() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let bytes = serde_formats::to_bytes(&r).unwrap();
    let r2 = serde_formats::from_bytes(&bytes).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert!(verify_hash(&r2));
}

#[test]
fn serde_json_preserves_all_fields() {
    let r = make_full_receipt();
    let hashed = r.clone();
    let json = serde_formats::to_json(&hashed).unwrap();
    let r2 = serde_formats::from_json(&json).unwrap();
    assert_eq!(r2.backend.id, "test-backend");
    assert_eq!(r2.outcome, Outcome::Complete);
    assert_eq!(r2.usage.input_tokens, Some(1000));
    assert_eq!(r2.usage.output_tokens, Some(500));
    assert!(r2.verification.harness_ok);
    assert_eq!(r2.trace.len(), 1);
    assert_eq!(r2.artifacts.len(), 1);
}

#[test]
fn serde_json_roundtrip_preserves_hash_validity() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn serde_json_roundtrip_minimal() {
    let r = ReceiptBuilder::new("x").build();
    let json = serde_formats::to_json(&r).unwrap();
    let r2 = serde_formats::from_json(&json).unwrap();
    assert_eq!(r2.backend.id, "x");
}

// ===========================================================================
// 9. Canonical JSON format
// ===========================================================================

#[test]
fn canonical_json_is_valid_json() {
    let r = make_receipt("mock", Outcome::Complete);
    let c = canonicalize(&r).unwrap();
    let _: serde_json::Value = serde_json::from_str(&c).unwrap();
}

#[test]
fn canonical_json_receipt_sha256_is_null() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let c = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&c).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn canonical_json_no_pretty_print() {
    let r = make_receipt("mock", Outcome::Complete);
    let c = canonicalize(&r).unwrap();
    assert!(!c.contains('\n'), "canonical JSON should be compact");
}

#[test]
fn canonical_json_deterministic_key_order() {
    let r = make_receipt("mock", Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    let c2 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonical_json_contains_contract_version() {
    let r = make_receipt("mock", Outcome::Complete);
    let c = canonicalize(&r).unwrap();
    assert!(c.contains(CONTRACT_VERSION));
}

#[test]
fn canonical_json_contains_backend_id() {
    let r = make_receipt("my-backend", Outcome::Complete);
    let c = canonicalize(&r).unwrap();
    assert!(c.contains("my-backend"));
}

// ===========================================================================
// 10. Receipt with all optional fields populated
// ===========================================================================

#[test]
fn full_receipt_hashes_correctly() {
    let r = make_full_receipt();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn full_receipt_with_hash_verifies() {
    let mut r = make_full_receipt();
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    assert!(verify_hash(&r));
}

#[test]
fn full_receipt_validates() {
    let mut r = make_full_receipt();
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    let result = verify_receipt(&r);
    assert!(result.hash_valid);
    assert!(result.contract_valid);
    assert!(result.timestamps_valid);
}

#[test]
fn full_receipt_roundtrips_json() {
    let mut r = make_full_receipt();
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn full_receipt_usage_fields() {
    let r = make_full_receipt();
    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.output_tokens, Some(500));
    assert_eq!(r.usage.cache_read_tokens, Some(200));
    assert_eq!(r.usage.cache_write_tokens, Some(100));
    assert_eq!(r.usage.request_units, Some(3));
    assert!(r.usage.estimated_cost_usd.is_some());
}

#[test]
fn full_receipt_backend_fields() {
    let r = make_full_receipt();
    assert_eq!(r.backend.id, "test-backend");
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.1.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("1.0.0"));
}

#[test]
fn full_receipt_verification_fields() {
    let r = make_full_receipt();
    assert!(r.verification.harness_ok);
    assert!(r.verification.git_diff.is_some());
    assert!(r.verification.git_status.is_some());
}

// ===========================================================================
// 11. Receipt with minimal fields
// ===========================================================================

#[test]
fn minimal_receipt_builds() {
    let r = ReceiptBuilder::new("x").build();
    assert_eq!(r.backend.id, "x");
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn minimal_receipt_hashes() {
    let r = ReceiptBuilder::new("x").build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn minimal_receipt_with_hash_verifies() {
    let r = ReceiptBuilder::new("x").with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn minimal_receipt_has_default_outcome() {
    let r = ReceiptBuilder::new("x").build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn minimal_receipt_has_contract_version() {
    let r = ReceiptBuilder::new("x").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn minimal_receipt_empty_trace() {
    let r = ReceiptBuilder::new("x").build();
    assert!(r.trace.is_empty());
}

#[test]
fn minimal_receipt_empty_artifacts() {
    let r = ReceiptBuilder::new("x").build();
    assert!(r.artifacts.is_empty());
}

#[test]
fn minimal_receipt_empty_capabilities() {
    let r = ReceiptBuilder::new("x").build();
    assert!(r.capabilities.is_empty());
}

// ===========================================================================
// 12. Receipt with each outcome type
// ===========================================================================

#[test]
fn outcome_complete_hashes_and_verifies() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    assert!(verify_hash(&r));
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn outcome_failed_hashes_and_verifies() {
    let r = make_hashed_receipt("mock", Outcome::Failed);
    assert!(verify_hash(&r));
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn outcome_partial_hashes_and_verifies() {
    let r = make_hashed_receipt("mock", Outcome::Partial);
    assert!(verify_hash(&r));
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn outcome_each_produces_unique_hash() {
    let c = make_receipt("mock", Outcome::Complete);
    let f = make_receipt("mock", Outcome::Failed);
    let p = make_receipt("mock", Outcome::Partial);
    // All three are from the same builder (different run_ids though)
    // but outcome differs — each should hash differently
    // (run_ids also differ so hashes are unique regardless)
    let hc = compute_hash(&c).unwrap();
    let hf = compute_hash(&f).unwrap();
    let hp = compute_hash(&p).unwrap();
    assert_ne!(hc, hf);
    assert_ne!(hc, hp);
    assert_ne!(hf, hp);
}

// ===========================================================================
// 13. Verification result checks
// ===========================================================================

#[test]
fn verify_receipt_valid_receipt() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let result = verify_receipt(&r);
    assert!(result.is_verified());
    assert!(result.issues.is_empty());
}

#[test]
fn verify_receipt_bad_hash() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("bad".into());
    let result = verify_receipt(&r);
    assert!(!result.hash_valid);
    assert!(!result.is_verified());
}

#[test]
fn verify_receipt_bad_contract_version() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.meta.contract_version = "wrong".into();
    let result = verify_receipt(&r);
    assert!(!result.contract_valid);
}

#[test]
fn verify_receipt_inconsistent_duration() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 999_999;
    let result = verify_receipt(&r);
    assert!(!result.timestamps_valid);
}

#[test]
fn verify_receipt_complete_with_error_event() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        })
        .build();
    let result = verify_receipt(&r);
    assert!(!result.outcome_consistent);
}

// ===========================================================================
// 14. Validator
// ===========================================================================

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
fn validator_bad_contract_version() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.meta.contract_version = "wrong/v0".into();
    let err = v.validate(&r).unwrap_err();
    assert!(err.iter().any(|e| e.field == "meta.contract_version"));
}

#[test]
fn validator_empty_backend_id() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").build();
    r.backend.id = String::new();
    let err = v.validate(&r).unwrap_err();
    assert!(err.iter().any(|e| e.field == "backend.id"));
}

#[test]
fn validator_bad_hash() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").build();
    r.receipt_sha256 = Some("wrong".into());
    let err = v.validate(&r).unwrap_err();
    assert!(err.iter().any(|e| e.field == "receipt_sha256"));
}

// ===========================================================================
// 15. Auditor batch
// ===========================================================================

#[test]
fn auditor_clean_batch() {
    let auditor = ReceiptAuditor::new();
    let receipts: Vec<Receipt> = (0..3)
        .map(|_| make_hashed_receipt("mock", Outcome::Complete))
        .collect();
    let report = auditor.audit_batch(&receipts);
    assert!(report.is_clean());
    assert_eq!(report.total, 3);
    assert_eq!(report.valid, 3);
    assert_eq!(report.invalid, 0);
}

#[test]
fn auditor_detects_invalid() {
    let auditor = ReceiptAuditor::new();
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("tampered".into());
    let report = auditor.audit_batch(&[r]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
}

#[test]
fn auditor_empty_batch() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[]);
    assert!(report.is_clean());
    assert_eq!(report.total, 0);
}

// ===========================================================================
// 16. Edge cases
// ===========================================================================

#[test]
fn edge_unicode_in_trace_event() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "こんにちは 🌍 émoji".into(),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_very_long_backend_id() {
    let long_id = "x".repeat(10_000);
    let r = ReceiptBuilder::new(&long_id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.backend.id, long_id);
}

#[test]
fn edge_large_trace() {
    let events: Vec<AgentEvent> = (0..100)
        .map(|i| AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("chunk-{i}"),
            },
            ext: None,
        })
        .collect();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .events(events)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 100);
}

#[test]
fn edge_many_artifacts() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..50 {
        builder = builder.add_artifact(ArtifactRef {
            kind: "file".into(),
            path: format!("/output/{i}.txt"),
        });
    }
    let r = builder.with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.artifacts.len(), 50);
}

#[test]
fn edge_special_chars_in_fields() {
    let r = ReceiptBuilder::new("mock/<script>alert('xss')</script>")
        .outcome(Outcome::Complete)
        .backend_version("v1.0 \"quoted\"")
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_null_json_in_usage_raw() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_raw(json!(null))
        .build();
    // Should still compute hash without panicking
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_nested_json_in_usage_raw() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_raw(json!({"deeply": {"nested": {"value": [1, 2, 3]}}}))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_ext_field_in_event() {
    let mut ext = BTreeMap::new();
    ext.insert("custom".to_string(), json!("data"));
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: Some(ext),
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_zero_duration() {
    let start = Utc.timestamp_millis_opt(1_000_000).unwrap();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start)
        .duration(Duration::from_millis(0))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn edge_max_token_values() {
    let usage = UsageNormalized {
        input_tokens: Some(u64::MAX),
        output_tokens: Some(u64::MAX),
        cache_read_tokens: Some(u64::MAX),
        cache_write_tokens: Some(u64::MAX),
        request_units: Some(u64::MAX),
        estimated_cost_usd: Some(f64::MAX),
    };
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage(usage)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn edge_error_builder_method() {
    let r = ReceiptBuilder::new("mock").error("something broke").build();
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(r.trace.iter().any(|e| matches!(
        &e.kind,
        AgentEventKind::Error { message, .. } if message == "something broke"
    )));
}
