#![allow(clippy::all)]
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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Deep tests for receipt hashing, chain verification, and store operations.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, Capability, ExecutionMode, Outcome, SupportLevel,
    UsageNormalized, VerificationReport,
};
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore, StoreError};
use abp_receipt::{
    ChainError, ReceiptBuilder, ReceiptChain, ReceiptValidator, canonicalize, compute_hash,
    diff_receipts, verify_hash,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ══════════════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════════════

/// Build a receipt with a fixed timestamp to avoid non-determinism.
fn fixed_receipt(backend: &str, outcome: Outcome) -> abp_core::Receipt {
    let ts = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(ts)
        .finished_at(ts)
        .build()
}

/// Build a hashed receipt at a specific second offset for chain ordering.
fn chain_receipt(backend: &str, second: u32) -> abp_core::Receipt {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, second).unwrap();
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap()
}

// ══════════════════════════════════════════════════════════════════════
// 1. Receipt Hashing (15+ tests)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn hash_determinism_same_receipt() {
    let r = fixed_receipt("mock", Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    let h3 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn hash_determinism_cloned_receipt() {
    let r = fixed_receipt("mock", Outcome::Complete);
    let clone = r.clone();
    assert_eq!(compute_hash(&r).unwrap(), compute_hash(&clone).unwrap());
}

#[test]
fn hash_sensitivity_outcome_change() {
    let r1 = fixed_receipt("mock", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_backend_id_change() {
    let r1 = fixed_receipt("alpha", Outcome::Complete);
    let r2 = fixed_receipt("beta", Outcome::Complete);
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_usage_raw_change() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = ReceiptBuilder::new("x")
        .started_at(ts)
        .finished_at(ts)
        .usage_raw(serde_json::json!({"tokens": 10}))
        .build();
    let mut r2 = r1.clone();
    r2.usage_raw = serde_json::json!({"tokens": 11});
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_trace_change() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = ReceiptBuilder::new("x")
        .started_at(ts)
        .finished_at(ts)
        .build();
    let mut r2 = r1.clone();
    r2.trace.push(AgentEvent {
        ts,
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    });
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_mode_change() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = ReceiptBuilder::new("x")
        .started_at(ts)
        .finished_at(ts)
        .mode(ExecutionMode::Mapped)
        .build();
    let mut r2 = r1.clone();
    r2.mode = ExecutionMode::Passthrough;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_contract_version_change() {
    let r1 = fixed_receipt("x", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.meta.contract_version = "abp/v999".to_string();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_self_referential_prevention() {
    let r = fixed_receipt("mock", Outcome::Complete);
    let h_no_hash = compute_hash(&r).unwrap();

    let mut r_with_hash = r.clone();
    r_with_hash.receipt_sha256 = Some("deadbeefdeadbeef".into());
    let h_with_hash = compute_hash(&r_with_hash).unwrap();

    // Hash should be the same regardless of receipt_sha256 value
    assert_eq!(h_no_hash, h_with_hash);
}

#[test]
fn hash_self_referential_prevention_with_real_hash() {
    let r = fixed_receipt("mock", Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some(h1.clone());
    let h2 = compute_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn with_hash_idempotence() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let first_hash = r.receipt_sha256.clone().unwrap();

    // Compute hash again on the already-hashed receipt
    let second_hash = compute_hash(&r).unwrap();
    assert_eq!(first_hash, second_hash);
}

#[test]
fn canonical_json_btreemap_ordering_stable() {
    let r = fixed_receipt("mock", Outcome::Complete);
    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    let j3 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2);
    assert_eq!(j2, j3);
}

#[test]
fn hash_format_valid_sha256_hex() {
    let r = fixed_receipt("mock", Outcome::Complete);
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64, "SHA-256 hex must be 64 chars");
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "must be hex: {h}");
    assert!(
        h.chars().all(|c| !c.is_uppercase()),
        "must be lowercase hex: {h}"
    );
}

#[test]
fn hash_empty_backend_receipt() {
    let r = fixed_receipt("", Outcome::Complete);
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_all_fields_populated() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let ts_end = Utc.with_ymd_and_hms(2025, 6, 1, 12, 5, 0).unwrap();
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let r = ReceiptBuilder::new("full-backend")
        .outcome(Outcome::Partial)
        .backend_version("2.0.0")
        .adapter_version("1.0.0")
        .model("gpt-4")
        .dialect("openai")
        .started_at(ts)
        .finished_at(ts_end)
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .capabilities(caps)
        .mode(ExecutionMode::Passthrough)
        .usage_raw(serde_json::json!({"vendor_field": "value"}))
        .usage(UsageNormalized {
            input_tokens: Some(1000),
            output_tokens: Some(2000),
            cache_read_tokens: Some(500),
            cache_write_tokens: Some(100),
            request_units: Some(3),
            estimated_cost_usd: Some(0.05),
        })
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .verification(VerificationReport {
            git_diff: Some("diff --git a/f b/f".into()),
            git_status: Some("M f".into()),
            harness_ok: true,
        })
        .build();

    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    // Verify determinism
    assert_eq!(h, compute_hash(&r).unwrap());
}

#[test]
fn canonical_json_nullifies_existing_hash() {
    let mut r = fixed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("should_be_nulled".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
    assert!(!json.contains("should_be_nulled"));
}

#[test]
fn hash_sensitivity_verification_change() {
    let r1 = fixed_receipt("x", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.verification.harness_ok = true;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_sensitivity_artifact_change() {
    let r1 = fixed_receipt("x", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.artifacts.push(ArtifactRef {
        kind: "log".into(),
        path: "run.log".into(),
    });
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// ══════════════════════════════════════════════════════════════════════
// 2. Receipt Chain (15+ tests)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn chain_empty_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn chain_empty_latest_is_none() {
    let chain = ReceiptChain::new();
    assert!(chain.latest().is_none());
}

#[test]
fn chain_empty_verify_returns_error() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
}

#[test]
fn chain_empty_iter_yields_nothing() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.iter().count(), 0);
}

#[test]
fn chain_push_single_receipt() {
    let mut chain = ReceiptChain::new();
    chain.push(chain_receipt("mock", 0)).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
}

#[test]
fn chain_push_and_retrieve() {
    let mut chain = ReceiptChain::new();
    let r = chain_receipt("mock", 0);
    let expected_id = r.meta.run_id;
    chain.push(r).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, expected_id);
}

#[test]
fn chain_ordering_preserved() {
    let mut chain = ReceiptChain::new();
    chain.push(chain_receipt("first", 0)).unwrap();
    chain.push(chain_receipt("second", 1)).unwrap();
    chain.push(chain_receipt("third", 2)).unwrap();

    let backends: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(backends, vec!["first", "second", "third"]);
}

#[test]
fn chain_length_tracking() {
    let mut chain = ReceiptChain::new();
    assert_eq!(chain.len(), 0);
    chain.push(chain_receipt("a", 0)).unwrap();
    assert_eq!(chain.len(), 1);
    chain.push(chain_receipt("b", 1)).unwrap();
    assert_eq!(chain.len(), 2);
    chain.push(chain_receipt("c", 2)).unwrap();
    assert_eq!(chain.len(), 3);
}

#[test]
fn chain_latest_returns_most_recent() {
    let mut chain = ReceiptChain::new();
    chain.push(chain_receipt("first", 0)).unwrap();
    assert_eq!(chain.latest().unwrap().backend.id, "first");

    chain.push(chain_receipt("second", 1)).unwrap();
    assert_eq!(chain.latest().unwrap().backend.id, "second");

    chain.push(chain_receipt("third", 2)).unwrap();
    assert_eq!(chain.latest().unwrap().backend.id, "third");
}

#[test]
fn chain_iter_collects_all() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        chain
            .push(chain_receipt(&format!("b{i}"), i as u32))
            .unwrap();
    }
    let ids: Vec<_> = chain.iter().map(|r| r.backend.id.clone()).collect();
    assert_eq!(ids.len(), 5);
    assert_eq!(ids, vec!["b0", "b1", "b2", "b3", "b4"]);
}

#[test]
fn chain_into_iterator() {
    let mut chain = ReceiptChain::new();
    chain.push(chain_receipt("x", 0)).unwrap();
    chain.push(chain_receipt("y", 1)).unwrap();
    let count = (&chain).into_iter().count();
    assert_eq!(count, 2);
}

#[test]
fn chain_duplicate_receipt_rejected() {
    let mut chain = ReceiptChain::new();
    let id = Uuid::new_v4();
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    chain.push(r1).unwrap();
    assert_eq!(chain.push(r2), Err(ChainError::DuplicateId { id }));
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_rejects_out_of_order() {
    let mut chain = ReceiptChain::new();
    chain.push(chain_receipt("later", 10)).unwrap();
    let result = chain.push(chain_receipt("earlier", 5));
    assert!(matches!(result, Err(ChainError::BrokenLink { index: 1 })));
}

#[test]
fn chain_rejects_tampered_hash() {
    let mut chain = ReceiptChain::new();
    let mut r = chain_receipt("mock", 0);
    r.outcome = Outcome::Failed; // tamper after hashing
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { index: 0 })
    ));
}

#[test]
fn chain_accepts_receipt_without_hash() {
    let mut chain = ReceiptChain::new();
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .started_at(ts)
        .finished_at(ts)
        .build(); // no hash
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_verify_single_valid() {
    let mut chain = ReceiptChain::new();
    chain.push(chain_receipt("mock", 0)).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_verify_multiple_valid() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        chain
            .push(chain_receipt(&format!("b{i}"), i as u32))
            .unwrap();
    }
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_with_many_receipts() {
    let mut chain = ReceiptChain::new();
    for i in 0..150u32 {
        let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
            + chrono::Duration::seconds(i64::from(i));
        let r = ReceiptBuilder::new(format!("backend-{i}"))
            .outcome(Outcome::Complete)
            .started_at(ts)
            .finished_at(ts)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 150);
    assert!(chain.verify().is_ok());
    assert_eq!(chain.latest().unwrap().backend.id, "backend-149");
}

#[test]
fn chain_same_timestamp_allowed() {
    let mut chain = ReceiptChain::new();
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = ReceiptBuilder::new("a")
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.len(), 2);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_error_display_all_variants() {
    assert_eq!(ChainError::EmptyChain.to_string(), "chain is empty");
    assert_eq!(
        ChainError::HashMismatch { index: 5 }.to_string(),
        "hash mismatch at chain index 5"
    );
    assert_eq!(
        ChainError::BrokenLink { index: 3 }.to_string(),
        "broken link at chain index 3"
    );
    let id = Uuid::nil();
    let msg = ChainError::DuplicateId { id }.to_string();
    assert!(msg.contains("duplicate"));
    assert!(msg.contains(&id.to_string()));
}

// ══════════════════════════════════════════════════════════════════════
// 3. Receipt Store (10+ tests)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn store_new_is_empty() {
    let store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

#[test]
fn store_and_retrieve_by_run_id() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let id = r.meta.run_id;
    store.store(r).unwrap();
    let got = store.get(id).unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
    assert_eq!(got.meta.run_id, id);
}

#[test]
fn store_returns_id_on_insert() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("mock").build();
    let expected_id = r.meta.run_id;
    let returned_id = store.store(r).unwrap();
    assert_eq!(returned_id, expected_id);
}

#[test]
fn store_list_all() {
    let mut store = InMemoryReceiptStore::new();
    store.store(ReceiptBuilder::new("a").build()).unwrap();
    store.store(ReceiptBuilder::new("b").build()).unwrap();
    store.store(ReceiptBuilder::new("c").build()).unwrap();
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn store_missing_receipt_returns_none() {
    let store = InMemoryReceiptStore::new();
    let result = store.get(Uuid::new_v4()).unwrap();
    assert!(result.is_none());
}

#[test]
fn store_rejects_duplicate_id() {
    let mut store = InMemoryReceiptStore::new();
    let id = Uuid::new_v4();
    store
        .store(ReceiptBuilder::new("a").run_id(id).build())
        .unwrap();
    let result = store.store(ReceiptBuilder::new("b").run_id(id).build());
    assert!(matches!(result, Err(StoreError::DuplicateId(dup_id)) if dup_id == id));
}

#[test]
fn store_len_tracks_insertions() {
    let mut store = InMemoryReceiptStore::new();
    assert_eq!(store.len(), 0);
    store.store(ReceiptBuilder::new("a").build()).unwrap();
    assert_eq!(store.len(), 1);
    store.store(ReceiptBuilder::new("b").build()).unwrap();
    assert_eq!(store.len(), 2);
}

#[test]
fn store_filter_by_backend_id() {
    let mut store = InMemoryReceiptStore::new();
    store.store(ReceiptBuilder::new("alpha").build()).unwrap();
    store.store(ReceiptBuilder::new("beta").build()).unwrap();
    store.store(ReceiptBuilder::new("alpha").build()).unwrap();

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
    store
        .store(ReceiptBuilder::new("a").outcome(Outcome::Complete).build())
        .unwrap();
    store
        .store(ReceiptBuilder::new("b").outcome(Outcome::Failed).build())
        .unwrap();
    store
        .store(ReceiptBuilder::new("c").outcome(Outcome::Partial).build())
        .unwrap();
    store
        .store(ReceiptBuilder::new("d").outcome(Outcome::Failed).build())
        .unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|s| s.outcome == Outcome::Failed));
}

#[test]
fn store_filter_by_time_range() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let mut store = InMemoryReceiptStore::new();
    store
        .store(
            ReceiptBuilder::new("early")
                .started_at(t1)
                .finished_at(t1)
                .build(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("mid")
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("late")
                .started_at(t3)
                .finished_at(t3)
                .build(),
        )
        .unwrap();

    let filter = ReceiptFilter {
        after: Some(Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()),
        before: Some(Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "mid");
}

#[test]
fn store_filter_combined() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();

    let mut store = InMemoryReceiptStore::new();
    store
        .store(
            ReceiptBuilder::new("alpha")
                .outcome(Outcome::Complete)
                .started_at(t1)
                .finished_at(t1)
                .build(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("alpha")
                .outcome(Outcome::Failed)
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("beta")
                .outcome(Outcome::Failed)
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();

    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "alpha");
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[test]
fn store_many_receipts() {
    let mut store = InMemoryReceiptStore::new();
    for i in 0..200 {
        store
            .store(ReceiptBuilder::new(format!("b{i}")).build())
            .unwrap();
    }
    assert_eq!(store.len(), 200);
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 200);
}

#[test]
fn store_receipt_summary_fields() {
    let ts = Utc.with_ymd_and_hms(2025, 3, 15, 10, 0, 0).unwrap();
    let ts_end = Utc.with_ymd_and_hms(2025, 3, 15, 10, 5, 0).unwrap();
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("test-be")
        .outcome(Outcome::Partial)
        .started_at(ts)
        .finished_at(ts_end)
        .build();
    let id = r.meta.run_id;
    store.store(r).unwrap();

    let summaries = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(summaries.len(), 1);
    let s = &summaries[0];
    assert_eq!(s.id, id);
    assert_eq!(s.backend_id, "test-be");
    assert_eq!(s.outcome, Outcome::Partial);
    assert_eq!(s.started_at, ts);
    assert_eq!(s.finished_at, ts_end);
}

#[test]
fn store_filter_no_match_returns_empty() {
    let mut store = InMemoryReceiptStore::new();
    store.store(ReceiptBuilder::new("a").build()).unwrap();
    let filter = ReceiptFilter {
        backend_id: Some("nonexistent".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert!(results.is_empty());
}

// ══════════════════════════════════════════════════════════════════════
// 4. Receipt Integrity (10+ tests)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn verify_hash_after_json_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .model("gpt-4")
        .usage_tokens(100, 200)
        .with_hash()
        .unwrap();

    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();

    assert!(verify_hash(&r2));
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn verify_hash_after_bytes_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_bytes(&bytes).unwrap();

    assert!(verify_hash(&r2));
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn verify_hash_passes_without_stored_hash() {
    let r = fixed_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_fails_with_garbage_hash() {
    let mut r = fixed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("not_a_valid_hash".into());
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_after_tampering() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.backend.id = "tampered".into();
    assert!(!verify_hash(&r));
}

#[test]
fn receipt_with_complex_usage_raw() {
    let complex_usage = serde_json::json!({
        "model": "gpt-4-turbo",
        "tokens": {
            "prompt": 1500,
            "completion": 2000,
            "total": 3500
        },
        "costs": [
            {"type": "input", "amount": 0.015},
            {"type": "output", "amount": 0.060}
        ],
        "metadata": {
            "region": "us-east-1",
            "cache_hit": true,
            "latency_ms": 1234
        }
    });

    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("openai")
        .usage_raw(complex_usage.clone())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.usage_raw["model"], "gpt-4-turbo");
    assert_eq!(r.usage_raw["tokens"]["total"], 3500);

    // Roundtrip preserves complex usage_raw
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(r.usage_raw, r2.usage_raw);
    assert!(verify_hash(&r2));
}

#[test]
fn receipt_with_verification_data() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .started_at(ts)
        .finished_at(ts)
        .verification(VerificationReport {
            git_diff: Some("--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new".into()),
            git_status: Some("M file".into()),
            harness_ok: true,
        })
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.verification.git_status.as_deref(), Some("M file"));
    assert!(r.verification.harness_ok);
}

#[test]
fn receipt_outcome_complete() {
    let r = fixed_receipt("mock", Outcome::Complete);
    let h = compute_hash(&r).unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_outcome_partial() {
    let r = fixed_receipt("mock", Outcome::Partial);
    let h = compute_hash(&r).unwrap();
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_outcome_failed() {
    let r = fixed_receipt("mock", Outcome::Failed);
    let h = compute_hash(&r).unwrap();
    assert_eq!(r.outcome, Outcome::Failed);
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_all_outcomes_produce_different_hashes() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let id = Uuid::nil();
    let make = |outcome| {
        ReceiptBuilder::new("mock")
            .run_id(id)
            .work_order_id(id)
            .started_at(ts)
            .finished_at(ts)
            .outcome(outcome)
            .build()
    };
    let h_complete = compute_hash(&make(Outcome::Complete)).unwrap();
    let h_partial = compute_hash(&make(Outcome::Partial)).unwrap();
    let h_failed = compute_hash(&make(Outcome::Failed)).unwrap();

    assert_ne!(h_complete, h_partial);
    assert_ne!(h_complete, h_failed);
    assert_ne!(h_partial, h_failed);
}

#[test]
fn receipt_metadata_preserved_through_store() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let ts_end = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 5).unwrap();
    let wo_id = Uuid::new_v4();

    let r = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Partial)
        .backend_version("3.0")
        .adapter_version("1.5")
        .work_order_id(wo_id)
        .started_at(ts)
        .finished_at(ts_end)
        .usage_tokens(500, 1000)
        .with_hash()
        .unwrap();

    let mut store = InMemoryReceiptStore::new();
    let id = store.store(r).unwrap();
    let got = store.get(id).unwrap().unwrap();

    assert_eq!(got.backend.id, "test-backend");
    assert_eq!(got.backend.backend_version.as_deref(), Some("3.0"));
    assert_eq!(got.backend.adapter_version.as_deref(), Some("1.5"));
    assert_eq!(got.outcome, Outcome::Partial);
    assert_eq!(got.meta.work_order_id, wo_id);
    assert_eq!(got.meta.started_at, ts);
    assert_eq!(got.meta.finished_at, ts_end);
    assert_eq!(got.meta.duration_ms, 5000);
    assert_eq!(got.usage.input_tokens, Some(500));
    assert_eq!(got.usage.output_tokens, Some(1000));
    assert!(verify_hash(got));
}

#[test]
fn receipt_validator_accepts_clean_receipt() {
    let v = ReceiptValidator::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(v.validate(&r).is_ok());
}

#[test]
fn receipt_diff_no_changes_on_clone() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(100, 200)
        .with_hash()
        .unwrap();
    let d = diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
}

#[test]
fn receipt_diff_detects_multiple_changes() {
    let r1 = fixed_receipt("alpha", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.backend.id = "beta".into();
    r2.outcome = Outcome::Failed;
    r2.verification.harness_ok = true;

    let d = diff_receipts(&r1, &r2);
    assert!(d.len() >= 3);
    assert!(d.changes.iter().any(|c| c.field == "backend.id"));
    assert!(d.changes.iter().any(|c| c.field == "outcome"));
    assert!(d.changes.iter().any(|c| c.field == "verification"));
}

#[test]
fn receipt_builder_duration_helper() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("x")
        .started_at(ts)
        .duration(Duration::from_secs(30))
        .build();
    assert_eq!(r.meta.duration_ms, 30_000);
}

#[test]
fn receipt_builder_error_sets_failed() {
    let r = ReceiptBuilder::new("x")
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
fn chain_and_store_integration() {
    // Build receipts, push into chain, store in store, verify everything.
    let mut chain = ReceiptChain::new();
    let mut store = InMemoryReceiptStore::new();

    for i in 0..10u32 {
        let r = chain_receipt(&format!("backend-{i}"), i);
        let id = r.meta.run_id;
        chain.push(r.clone()).unwrap();
        store.store(r).unwrap();

        // Verify store has it
        let stored = store.get(id).unwrap().unwrap();
        assert!(verify_hash(stored));
    }

    assert_eq!(chain.len(), 10);
    assert!(chain.verify().is_ok());
    assert_eq!(store.len(), 10);

    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 10);
}
