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

//! Comprehensive integration tests for `abp-receipt-store`.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use uuid::Uuid;

use abp_core::{Outcome, Receipt};
use abp_receipt_store::{
    FileReceiptStore, InMemoryReceiptStore, ReceiptFilter, ReceiptIndex, ReceiptStore, StoreError,
    validate_chain,
};

// ── Helpers ────────────────────────────────────────────────────────

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    Receipt {
        meta: abp_core::RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::nil(),
            contract_version: abp_core::CONTRACT_VERSION.to_string(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 0,
        },
        backend: abp_core::BackendIdentity {
            id: backend.to_string(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: abp_core::CapabilityManifest::new(),
        mode: abp_core::ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: abp_core::UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: abp_core::VerificationReport::default(),
        outcome,
        receipt_sha256: None,
    }
}

fn make_receipt_at(backend: &str, outcome: Outcome, ts: chrono::DateTime<Utc>) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.meta.started_at = ts;
    r.meta.finished_at = ts;
    r
}

fn make_receipt_with_id(backend: &str, outcome: Outcome, id: Uuid) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.meta.run_id = id;
    r
}

fn make_hashed_receipt(backend: &str, outcome: Outcome) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.receipt_sha256 = Some(abp_core::receipt_hash(&r).unwrap());
    r
}

fn make_hashed_receipt_at(backend: &str, outcome: Outcome, ts: chrono::DateTime<Utc>) -> Receipt {
    let mut r = make_receipt_at(backend, outcome, ts);
    r.receipt_sha256 = Some(abp_core::receipt_hash(&r).unwrap());
    r
}

fn file_store(dir: &tempfile::TempDir) -> FileReceiptStore {
    FileReceiptStore::new(dir.path().join("receipts.jsonl"))
}

// ── Generic store test harness ─────────────────────────────────────
// Runs the same assertion against any ReceiptStore implementation.

async fn assert_store_and_get(store: &dyn ReceiptStore) {
    let r = make_receipt("generic", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "generic");
    assert_eq!(got.outcome, Outcome::Complete);
}

// ════════════════════════════════════════════════════════════════════
//  1. Store construction
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn construct_in_memory_store() {
    let store = InMemoryReceiptStore::new();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn construct_in_memory_store_default() {
    let store = InMemoryReceiptStore::default();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn construct_file_store_no_file_yet() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn construct_file_store_creates_file_on_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    assert!(!path.exists());
    let store = FileReceiptStore::new(&path);
    store
        .store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    assert!(path.exists());
}

// ════════════════════════════════════════════════════════════════════
//  2. Insert receipt — verify persistence
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_insert_persists() {
    let store = InMemoryReceiptStore::new();
    assert_store_and_get(&store).await;
}

#[tokio::test]
async fn file_insert_persists() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert_store_and_get(&store).await;
}

#[tokio::test]
async fn file_insert_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let r = make_receipt("persistent", Outcome::Complete);
    let id = r.meta.run_id.to_string();

    {
        let store = FileReceiptStore::new(&path);
        store.store(&r).await.unwrap();
    }

    let store2 = FileReceiptStore::new(&path);
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "persistent");
}

#[tokio::test]
async fn memory_insert_multiple_unique() {
    let store = InMemoryReceiptStore::new();
    for i in 0..10 {
        store
            .store(&make_receipt(&format!("b{i}"), Outcome::Complete))
            .await
            .unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 10);
}

#[tokio::test]
async fn file_insert_multiple_unique() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    for i in 0..10 {
        store
            .store(&make_receipt(&format!("b{i}"), Outcome::Complete))
            .await
            .unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 10);
}

// ════════════════════════════════════════════════════════════════════
//  3. Query by ID
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_get_by_id() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.get(&id).await.unwrap().is_some());
}

#[tokio::test]
async fn memory_get_missing_returns_none() {
    let store = InMemoryReceiptStore::new();
    assert!(
        store
            .get(&Uuid::new_v4().to_string())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn file_get_by_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.get(&id).await.unwrap().is_some());
}

#[tokio::test]
async fn file_get_missing_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert!(
        store
            .get(&Uuid::new_v4().to_string())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn memory_get_preserves_all_fields() {
    let store = InMemoryReceiptStore::new();
    let mut r = make_receipt("backend-x", Outcome::Partial);
    r.meta.duration_ms = 42;
    r.meta.work_order_id = Uuid::new_v4();
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.meta.duration_ms, 42);
    assert_eq!(got.meta.work_order_id, r.meta.work_order_id);
    assert_eq!(got.outcome, Outcome::Partial);
}

// ════════════════════════════════════════════════════════════════════
//  4. Query by time range
// ════════════════════════════════════════════════════════════════════

fn setup_time_receipts() -> (Receipt, Receipt, Receipt) {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
    (
        make_receipt_at("early", Outcome::Complete, t1),
        make_receipt_at("mid", Outcome::Complete, t2),
        make_receipt_at("late", Outcome::Complete, t3),
    )
}

#[tokio::test]
async fn memory_time_range_includes_middle() {
    let store = InMemoryReceiptStore::new();
    let (r1, r2, r3) = setup_time_receipts();
    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.store(&r3).await.unwrap();

    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "mid");
}

#[tokio::test]
async fn memory_time_range_includes_all() {
    let store = InMemoryReceiptStore::new();
    let (r1, r2, r3) = setup_time_receipts();
    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.store(&r3).await.unwrap();

    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn memory_time_range_excludes_all() {
    let store = InMemoryReceiptStore::new();
    let (r1, r2, r3) = setup_time_receipts();
    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.store(&r3).await.unwrap();

    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2030, 12, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn memory_time_range_boundary_inclusive() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("exact", Outcome::Complete, ts))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        time_range: Some((ts, ts)),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn file_time_range_query() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let (r1, r2, r3) = setup_time_receipts();
    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.store(&r3).await.unwrap();

    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "mid");
}

// ════════════════════════════════════════════════════════════════════
//  5. Query by backend
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_filter_backend_single_match() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("beta", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        backend: Some("alpha".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
}

#[tokio::test]
async fn memory_filter_backend_multiple_matches() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("same", Outcome::Complete))
            .await
            .unwrap();
    }
    store
        .store(&make_receipt("other", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        backend: Some("same".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 5);
}

#[tokio::test]
async fn memory_filter_backend_no_match() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        backend: Some("nonexistent".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn file_filter_backend() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    store
        .store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("beta", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        backend: Some("beta".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "beta");
}

// ════════════════════════════════════════════════════════════════════
//  6. Query by outcome
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_filter_outcome_complete() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    store
        .store(&make_receipt("c", Outcome::Partial))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Complete);
}

#[tokio::test]
async fn memory_filter_outcome_failed() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn memory_filter_outcome_partial() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Partial))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Partial),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Partial);
}

#[tokio::test]
async fn file_filter_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    store
        .store(&make_receipt("c", Outcome::Partial))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Partial),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Partial);
}

#[tokio::test]
async fn memory_filter_combined_backend_and_outcome() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("alpha", Outcome::Failed))
        .await
        .unwrap();
    store
        .store(&make_receipt("beta", Outcome::Failed))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        backend: Some("alpha".into()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn memory_filter_combined_time_and_outcome() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("a", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("b", Outcome::Failed, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("c", Outcome::Failed, t3))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        time_range: Some((
            Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "b");
}

#[tokio::test]
async fn memory_filter_combined_all_three() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("target", Outcome::Complete, ts))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("target", Outcome::Failed, ts))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("other", Outcome::Complete, ts))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        backend: Some("target".into()),
        outcome: Some(Outcome::Complete),
        time_range: Some((
            Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 12, 31, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "target");
    assert_eq!(results[0].outcome, Outcome::Complete);
}

// ════════════════════════════════════════════════════════════════════
//  7. Pagination
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_pagination_limit() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..10 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(5),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 5);
}

#[tokio::test]
async fn memory_pagination_offset() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..10 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        offset: Some(7),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 3);
}

#[tokio::test]
async fn memory_pagination_limit_and_offset() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..20 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(5),
        offset: Some(3),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 5);
}

#[tokio::test]
async fn memory_pagination_offset_beyond_end() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        offset: Some(100),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn memory_pagination_limit_zero() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        limit: Some(0),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn memory_pagination_limit_exceeds_total() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..3 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(100),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 3);
}

#[tokio::test]
async fn file_pagination_limit_and_offset() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    for _ in 0..15 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(5),
        offset: Some(5),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 5);
}

#[tokio::test]
async fn memory_pagination_with_filter() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..10 {
        store
            .store(&make_receipt("target", Outcome::Complete))
            .await
            .unwrap();
    }
    for _ in 0..5 {
        store
            .store(&make_receipt("other", Outcome::Failed))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        backend: Some("target".into()),
        limit: Some(3),
        offset: Some(2),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 3);
    for r in &results {
        assert_eq!(r.backend.id, "target");
    }
}

// ════════════════════════════════════════════════════════════════════
//  8. Ordering (receipts in file store are in insertion order)
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn file_preserves_insertion_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);

    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    store
        .store(&make_receipt_at("first", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("second", Outcome::Complete, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("third", Outcome::Complete, t3))
        .await
        .unwrap();

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].backend.id, "first");
    assert_eq!(all[1].backend.id, "second");
    assert_eq!(all[2].backend.id, "third");
}

#[tokio::test]
async fn file_list_timestamps_non_decreasing() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);

    for month in 1..=6 {
        let ts = Utc.with_ymd_and_hms(2025, month, 1, 0, 0, 0).unwrap();
        store
            .store(&make_receipt_at("b", Outcome::Complete, ts))
            .await
            .unwrap();
    }

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    for pair in all.windows(2) {
        assert!(pair[0].meta.started_at <= pair[1].meta.started_at);
    }
}

// ════════════════════════════════════════════════════════════════════
//  9. Hash verification — stored receipts maintain valid hash
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_hashed_receipt_roundtrip() {
    let store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert!(got.receipt_sha256.is_some());
    let recomputed = abp_core::receipt_hash(&got).unwrap();
    assert_eq!(got.receipt_sha256.as_ref().unwrap(), &recomputed);
}

#[tokio::test]
async fn file_hashed_receipt_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert!(got.receipt_sha256.is_some());
    let recomputed = abp_core::receipt_hash(&got).unwrap();
    assert_eq!(got.receipt_sha256.as_ref().unwrap(), &recomputed);
}

#[tokio::test]
async fn file_hashed_receipt_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    let original_hash = r.receipt_sha256.clone().unwrap();

    {
        let store = FileReceiptStore::new(&path);
        store.store(&r).await.unwrap();
    }

    let store2 = FileReceiptStore::new(&path);
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.receipt_sha256.as_ref().unwrap(), &original_hash);
    let recomputed = abp_core::receipt_hash(&got).unwrap();
    assert_eq!(recomputed, original_hash);
}

#[tokio::test]
async fn memory_unhashed_receipt_has_none() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert!(got.receipt_sha256.is_none());
}

#[test]
fn chain_validates_hashed_receipts_after_store() {
    let r1 = make_hashed_receipt("a", Outcome::Complete);
    let r2 = make_hashed_receipt("b", Outcome::Failed);
    let result = validate_chain(&[r1, r2]);
    assert!(result.valid);
}

// ════════════════════════════════════════════════════════════════════
//  10. Duplicate handling
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_duplicate_returns_error() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    store.store(&r).await.unwrap();
    let err = store.store(&r).await.unwrap_err();
    assert!(matches!(err, StoreError::DuplicateId(_)));
}

#[tokio::test]
async fn memory_duplicate_error_contains_id() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let err = store.store(&r).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains(&id));
}

#[tokio::test]
async fn file_duplicate_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("mock", Outcome::Complete);
    store.store(&r).await.unwrap();
    let err = store.store(&r).await.unwrap_err();
    assert!(matches!(err, StoreError::DuplicateId(_)));
}

#[tokio::test]
async fn memory_duplicate_does_not_modify_count() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    store.store(&r).await.unwrap();
    let _ = store.store(&r).await;
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn memory_reinsert_after_delete_succeeds() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    store.delete(&id).await.unwrap();
    // Re-insert the same receipt after deletion should succeed.
    store.store(&r).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

// ════════════════════════════════════════════════════════════════════
//  11. Delete / purge
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_delete_existing_returns_true() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.delete(&id).await.unwrap());
}

#[tokio::test]
async fn memory_delete_missing_returns_false() {
    let store = InMemoryReceiptStore::new();
    assert!(!store.delete(&Uuid::new_v4().to_string()).await.unwrap());
}

#[tokio::test]
async fn memory_delete_removes_from_list() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    store.delete(&id).await.unwrap();
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn memory_delete_one_of_many() {
    let store = InMemoryReceiptStore::new();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Complete);
    let r3 = make_receipt("c", Outcome::Complete);
    let id2 = r2.meta.run_id.to_string();
    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.store(&r3).await.unwrap();

    store.delete(&id2).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 2);
    assert!(store.get(&id2).await.unwrap().is_none());
}

#[tokio::test]
async fn file_delete_existing_returns_true() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.delete(&id).await.unwrap());
}

#[tokio::test]
async fn file_delete_missing_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert!(!store.delete(&Uuid::new_v4().to_string()).await.unwrap());
}

#[tokio::test]
async fn file_delete_persists_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();

    {
        let store = FileReceiptStore::new(&path);
        store.store(&r).await.unwrap();
        store.delete(&id).await.unwrap();
    }

    let store2 = FileReceiptStore::new(&path);
    assert!(store2.get(&id).await.unwrap().is_none());
    assert_eq!(store2.count().await.unwrap(), 0);
}

#[tokio::test]
async fn memory_purge_all_by_iterating() {
    let store = InMemoryReceiptStore::new();
    let mut ids = Vec::new();
    for _ in 0..5 {
        let r = make_receipt("x", Outcome::Complete);
        ids.push(r.meta.run_id.to_string());
        store.store(&r).await.unwrap();
    }
    for id in &ids {
        store.delete(id).await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 0);
}

// ════════════════════════════════════════════════════════════════════
//  12. Statistics — count, success rate approximation
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn memory_count_empty() {
    let store = InMemoryReceiptStore::new();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn memory_count_reflects_inserts() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..7 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 7);
}

#[tokio::test]
async fn memory_count_reflects_deletes() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("x", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    store.delete(&id).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn memory_success_rate_via_filters() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..7 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    for _ in 0..3 {
        store
            .store(&make_receipt("x", Outcome::Failed))
            .await
            .unwrap();
    }
    let total = store.count().await.unwrap();
    let successes = store
        .list(ReceiptFilter {
            outcome: Some(Outcome::Complete),
            ..Default::default()
        })
        .await
        .unwrap()
        .len();
    let failures = store
        .list(ReceiptFilter {
            outcome: Some(Outcome::Failed),
            ..Default::default()
        })
        .await
        .unwrap()
        .len();
    assert_eq!(total, 10);
    assert_eq!(successes, 7);
    assert_eq!(failures, 3);
}

#[tokio::test]
async fn file_count_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn file_count_reflects_inserts() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    for _ in 0..4 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 4);
}

// ════════════════════════════════════════════════════════════════════
//  13. Concurrent access
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn concurrent_writers_memory() {
    let store = Arc::new(InMemoryReceiptStore::new());
    let mut handles = Vec::new();
    for _ in 0..25 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.store(&make_receipt("concurrent", Outcome::Complete))
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 25);
}

#[tokio::test]
async fn concurrent_readers_memory() {
    let store = Arc::new(InMemoryReceiptStore::new());
    for _ in 0..10 {
        store
            .store(&make_receipt("bg", Outcome::Complete))
            .await
            .unwrap();
    }
    let mut handles = Vec::new();
    for _ in 0..20 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let results = s.list(ReceiptFilter::default()).await.unwrap();
            assert_eq!(results.len(), 10);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn concurrent_read_write_memory() {
    let store = Arc::new(InMemoryReceiptStore::new());
    for _ in 0..5 {
        store
            .store(&make_receipt("init", Outcome::Complete))
            .await
            .unwrap();
    }
    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.store(&make_receipt("new", Outcome::Complete))
                .await
                .unwrap();
        }));
    }
    for _ in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let _ = s.list(ReceiptFilter::default()).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 15);
}

#[tokio::test]
async fn concurrent_writers_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(file_store(&dir));
    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.store(&make_receipt("concurrent", Outcome::Complete))
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 10);
}

#[tokio::test]
async fn concurrent_delete_memory() {
    let store = Arc::new(InMemoryReceiptStore::new());
    let mut ids = Vec::new();
    for _ in 0..10 {
        let r = make_receipt("x", Outcome::Complete);
        ids.push(r.meta.run_id.to_string());
        store.store(&r).await.unwrap();
    }
    let mut handles = Vec::new();
    for id in ids {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.delete(&id).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 0);
}

// ════════════════════════════════════════════════════════════════════
//  14. Empty store edge cases
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn empty_memory_list_returns_empty() {
    let store = InMemoryReceiptStore::new();
    assert!(
        store
            .list(ReceiptFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn empty_memory_list_with_filter_returns_empty() {
    let store = InMemoryReceiptStore::new();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        backend: Some("x".into()),
        time_range: Some((Utc::now(), Utc::now())),
        limit: Some(10),
        offset: Some(0),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn empty_file_list_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert!(
        store
            .list(ReceiptFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn empty_file_get_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert!(store.get("anything").await.unwrap().is_none());
}

#[tokio::test]
async fn empty_file_delete_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert!(!store.delete("anything").await.unwrap());
}

#[tokio::test]
async fn empty_memory_count_is_zero() {
    assert_eq!(InMemoryReceiptStore::new().count().await.unwrap(), 0);
}

// ════════════════════════════════════════════════════════════════════
//  Chain validation extras
// ════════════════════════════════════════════════════════════════════

#[test]
fn chain_empty_is_valid() {
    let result = validate_chain(&[]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn chain_single_hashed_valid() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let result = validate_chain(&[r]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 1);
}

#[test]
fn chain_multiple_hashed_valid() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let r1 = make_hashed_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_hashed_receipt_at("b", Outcome::Failed, t2);
    let result = validate_chain(&[r1, r2]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 2);
}

#[test]
fn chain_tampered_hash_detected() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed;
    let result = validate_chain(&[r]);
    assert!(!result.valid);
    assert!(result.errors[0].message.contains("hash mismatch"));
}

#[test]
fn chain_duplicate_ids_detected() {
    let id = Uuid::new_v4();
    let r1 = make_receipt_with_id("a", Outcome::Complete, id);
    let r2 = make_receipt_with_id("b", Outcome::Failed, id);
    let result = validate_chain(&[r1, r2]);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("duplicate"))
    );
}

#[test]
fn chain_out_of_order_detected() {
    let t_later = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = make_receipt_at("a", Outcome::Complete, t_later);
    let r2 = make_receipt_at("b", Outcome::Complete, t_earlier);
    let result = validate_chain(&[r1, r2]);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.message.contains("earlier")));
}

#[test]
fn chain_no_hash_skips_hash_check() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    let result = validate_chain(&[r]);
    assert!(result.valid);
}

#[test]
fn chain_same_timestamp_is_valid() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let r1 = make_receipt_at("a", Outcome::Complete, ts);
    let r2 = make_receipt_at("b", Outcome::Complete, ts);
    let result = validate_chain(&[r1, r2]);
    assert!(result.valid);
}

// ════════════════════════════════════════════════════════════════════
//  ReceiptIndex tests
// ════════════════════════════════════════════════════════════════════

#[test]
fn index_new_is_empty() {
    let idx = ReceiptIndex::new();
    assert!(idx.is_empty());
    assert_eq!(idx.len(), 0);
}

#[test]
fn index_insert_increments_len() {
    let mut idx = ReceiptIndex::new();
    idx.insert(&make_receipt("a", Outcome::Complete));
    assert_eq!(idx.len(), 1);
    assert!(!idx.is_empty());
    idx.insert(&make_receipt("b", Outcome::Failed));
    assert_eq!(idx.len(), 2);
}

#[test]
fn index_by_backend_returns_correct_ids() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("alpha", Outcome::Complete);
    let r2 = make_receipt("beta", Outcome::Complete);
    let r3 = make_receipt("alpha", Outcome::Failed);
    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);

    let alpha_ids = idx.by_backend("alpha");
    assert_eq!(alpha_ids.len(), 2);
    assert!(alpha_ids.contains(&r1.meta.run_id.to_string()));
    assert!(alpha_ids.contains(&r3.meta.run_id.to_string()));

    let beta_ids = idx.by_backend("beta");
    assert_eq!(beta_ids.len(), 1);
    assert!(beta_ids.contains(&r2.meta.run_id.to_string()));
}

#[test]
fn index_by_outcome_returns_correct_ids() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let r3 = make_receipt("c", Outcome::Complete);
    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);

    let complete = idx.by_outcome(&Outcome::Complete);
    assert_eq!(complete.len(), 2);
    let failed = idx.by_outcome(&Outcome::Failed);
    assert_eq!(failed.len(), 1);
}

#[test]
fn index_by_time_range_returns_correct_ids() {
    let mut idx = ReceiptIndex::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_receipt_at("b", Outcome::Complete, t2);
    let r3 = make_receipt_at("c", Outcome::Complete, t3);
    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);

    let range = idx.by_time_range(
        Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap(),
    );
    assert_eq!(range.len(), 1);
    assert!(range.contains(&r2.meta.run_id.to_string()));
}

#[test]
fn index_remove_cleans_all_maps() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.insert(&r);
    idx.remove(&r);
    assert!(idx.is_empty());
    assert!(idx.by_backend("mock").is_empty());
    assert!(idx.by_outcome(&Outcome::Complete).is_empty());
}

#[test]
fn index_remove_one_of_many() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("mock", Outcome::Complete);
    let r2 = make_receipt("mock", Outcome::Failed);
    idx.insert(&r1);
    idx.insert(&r2);
    idx.remove(&r1);
    assert_eq!(idx.len(), 1);
    assert_eq!(idx.by_backend("mock").len(), 1);
}

#[test]
fn index_nonexistent_backend_returns_empty() {
    let idx = ReceiptIndex::new();
    assert!(idx.by_backend("ghost").is_empty());
}

#[test]
fn index_nonexistent_outcome_returns_empty() {
    let idx = ReceiptIndex::new();
    assert!(idx.by_outcome(&Outcome::Partial).is_empty());
}

#[test]
fn index_time_range_no_match_returns_empty() {
    let mut idx = ReceiptIndex::new();
    let ts = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    idx.insert(&make_receipt_at("a", Outcome::Complete, ts));

    let range = idx.by_time_range(
        Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2030, 12, 1, 0, 0, 0).unwrap(),
    );
    assert!(range.is_empty());
}

// ════════════════════════════════════════════════════════════════════
//  ReceiptFilter unit tests
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn filter_default_matches_everything() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("any", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("any", Outcome::Failed))
        .await
        .unwrap();
    store
        .store(&make_receipt("any", Outcome::Partial))
        .await
        .unwrap();
    let results = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn filter_rejects_wrong_outcome() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("x", Outcome::Failed))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_rejects_wrong_backend() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("other", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        backend: Some("wanted".into()),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_rejects_outside_time_range() {
    let ts = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("x", Outcome::Complete, ts))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_paginate_empty_store() {
    let store = InMemoryReceiptStore::new();
    let filter = ReceiptFilter {
        limit: Some(10),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_paginate_limit_via_store() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(2),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 2);
}

#[tokio::test]
async fn filter_paginate_offset_via_store() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        offset: Some(3),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 2);
}

#[tokio::test]
async fn filter_paginate_both_via_store() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(2),
        offset: Some(1),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 2);
}

// ════════════════════════════════════════════════════════════════════
//  StoreError display
// ════════════════════════════════════════════════════════════════════

#[test]
fn error_display_duplicate() {
    let e = StoreError::DuplicateId("abc".into());
    assert_eq!(e.to_string(), "duplicate receipt id: abc");
}

#[test]
fn error_display_invalid() {
    let e = StoreError::InvalidId("bad".into());
    assert_eq!(e.to_string(), "invalid receipt id: bad");
}

#[test]
fn error_display_other() {
    let e = StoreError::Other("boom".into());
    assert_eq!(e.to_string(), "store error: boom");
}

#[test]
fn error_is_std_error() {
    let e: Box<dyn std::error::Error> = Box::new(StoreError::Other("test".into()));
    assert!(e.source().is_none());
}

// ════════════════════════════════════════════════════════════════════
//  File store JSONL format integrity
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn file_store_produces_valid_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    store
        .store(&make_receipt("c", Outcome::Partial))
        .await
        .unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3);
    for line in &lines {
        serde_json::from_str::<Receipt>(line).unwrap();
    }
}

#[tokio::test]
async fn file_store_delete_rewrites_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let id1 = r1.meta.run_id.to_string();
    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.delete(&id1).await.unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1);
    let remaining: Receipt = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(remaining.backend.id, "b");
}

// ════════════════════════════════════════════════════════════════════
//  Trait object usage (dyn ReceiptStore)
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn trait_object_memory() {
    let store: Box<dyn ReceiptStore> = Box::new(InMemoryReceiptStore::new());
    let r = make_receipt("dyn-test", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.get(&id).await.unwrap().is_some());
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn trait_object_file() {
    let dir = tempfile::tempdir().unwrap();
    let store: Box<dyn ReceiptStore> = Box::new(FileReceiptStore::new(dir.path().join("r.jsonl")));
    let r = make_receipt("dyn-test", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.get(&id).await.unwrap().is_some());
    assert_eq!(store.count().await.unwrap(), 1);
}

// ════════════════════════════════════════════════════════════════════
//  ChainValidation struct + Display
// ════════════════════════════════════════════════════════════════════

#[test]
fn chain_validation_error_display() {
    let e = abp_receipt_store::ChainValidationError {
        index: 5,
        message: "problem here".into(),
    };
    assert_eq!(e.to_string(), "[5]: problem here");
}

#[test]
fn chain_validation_valid_field() {
    let result = validate_chain(&[]);
    assert!(result.valid);
    let bad_chain = {
        let id = Uuid::new_v4();
        vec![
            make_receipt_with_id("a", Outcome::Complete, id),
            make_receipt_with_id("b", Outcome::Complete, id),
        ]
    };
    let result = validate_chain(&bad_chain);
    assert!(!result.valid);
}
