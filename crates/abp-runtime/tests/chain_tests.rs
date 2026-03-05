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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_core::{
    BackendIdentity, ExecutionMode, Outcome, Receipt, RunMetadata, UsageNormalized,
    VerificationReport, CONTRACT_VERSION,
};
use abp_runtime::store::ReceiptStore;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

fn sample_receipt_at(run_id: Uuid, start_min: i64, end_min: i64) -> Receipt {
    let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let started_at = base + chrono::Duration::minutes(start_min);
    let finished_at = base + chrono::Duration::minutes(end_min);
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at,
            finished_at,
            duration_ms: ((end_min - start_min) * 60_000) as u64,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        capabilities: Default::default(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
    .with_hash()
    .expect("hash receipt")
}

#[test]
fn empty_store_produces_valid_chain() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 0);
    assert!(chain.invalid_hashes.is_empty());
    assert!(chain.gaps.is_empty());
}

#[test]
fn single_receipt_produces_valid_chain() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    store
        .save(&sample_receipt_at(Uuid::new_v4(), 0, 5))
        .unwrap();

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 1);
    assert!(chain.invalid_hashes.is_empty());
    assert!(chain.gaps.is_empty());
}

#[test]
fn multiple_chronological_receipts_are_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    store
        .save(&sample_receipt_at(Uuid::new_v4(), 0, 5))
        .unwrap();
    store
        .save(&sample_receipt_at(Uuid::new_v4(), 10, 15))
        .unwrap();
    store
        .save(&sample_receipt_at(Uuid::new_v4(), 20, 25))
        .unwrap();

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 3);
    assert!(chain.invalid_hashes.is_empty());
    assert_eq!(chain.gaps.len(), 2);
}

#[test]
fn tampered_hash_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let good = sample_receipt_at(Uuid::new_v4(), 0, 5);
    store.save(&good).unwrap();

    let mut bad = sample_receipt_at(Uuid::new_v4(), 10, 15);
    let bad_id = bad.meta.run_id;
    bad.receipt_sha256 = Some("deadbeef".into());
    // Write directly so the tampered hash is persisted.
    let path = dir.path().join(format!("{bad_id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&bad).unwrap()).unwrap();

    let chain = store.verify_chain().unwrap();
    assert!(!chain.is_valid);
    assert_eq!(chain.valid_count, 1);
    assert_eq!(chain.invalid_hashes, vec![bad_id]);
}

#[test]
fn chain_reports_correct_valid_count() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    for i in 0..5 {
        let start = i * 10;
        store
            .save(&sample_receipt_at(Uuid::new_v4(), start, start + 5))
            .unwrap();
    }

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 5);
    assert_eq!(chain.gaps.len(), 4);
}
