// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use uuid::Uuid;

use abp_core::{Outcome, Receipt};

use crate::chain::{ChainValidationError, validate_chain};
use crate::diff::diff_receipts;
use crate::export::{
    export_csv, export_json, export_jsonl, export_summary, import_json, import_jsonl,
};
use crate::filter::ReceiptFilter;
use crate::index::ReceiptIndex;
use crate::retention::ReceiptRetention;
use crate::stats::ReceiptStats;
use crate::{FileReceiptStore, InMemoryReceiptStore, ReceiptStore};

// ── Helpers ────────────────────────────────────────────────────────

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    abp_core::Receipt {
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

fn make_receipt_for_work_order(backend: &str, outcome: Outcome, woid: Uuid) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.meta.work_order_id = woid;
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

// ── InMemoryReceiptStore CRUD ──────────────────────────────────────

#[tokio::test]
async fn memory_store_and_get() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
}

#[tokio::test]
async fn memory_get_missing() {
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
async fn memory_store_duplicate() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    store.store(&r).await.unwrap();
    assert!(store.store(&r).await.is_err());
}

#[tokio::test]
async fn memory_delete_existing() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.delete(&id).await.unwrap());
    assert!(store.get(&id).await.unwrap().is_none());
}

#[tokio::test]
async fn memory_delete_missing() {
    let store = InMemoryReceiptStore::new();
    assert!(!store.delete(&Uuid::new_v4().to_string()).await.unwrap());
}

#[tokio::test]
async fn memory_count_empty() {
    let store = InMemoryReceiptStore::new();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn memory_count_after_inserts() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    assert_eq!(store.count().await.unwrap(), 2);
}

#[tokio::test]
async fn memory_count_after_delete() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    store.delete(&id).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn memory_list_all() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(all.len(), 2);
}

// ── InMemoryReceiptStore filtering ─────────────────────────────────

#[tokio::test]
async fn memory_filter_by_outcome() {
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
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn memory_filter_by_backend() {
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
async fn memory_filter_by_time_range() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("early", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("mid", Outcome::Complete, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("late", Outcome::Complete, t3))
        .await
        .unwrap();

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
async fn memory_filter_combined() {
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

// ── Pagination ─────────────────────────────────────────────────────

#[tokio::test]
async fn memory_pagination_limit() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(3),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn memory_pagination_offset() {
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
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn memory_pagination_limit_and_offset() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..10 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(3),
        offset: Some(2),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 3);
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
    let results = store.list(filter).await.unwrap();
    assert!(results.is_empty());
}

// ── FileReceiptStore CRUD ──────────────────────────────────────────

#[tokio::test]
async fn file_store_and_get() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();

    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
}

#[tokio::test]
async fn file_get_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);
    assert!(
        store
            .get(&Uuid::new_v4().to_string())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn file_store_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let r = make_receipt("mock", Outcome::Complete);
    store.store(&r).await.unwrap();
    assert!(store.store(&r).await.is_err());
}

#[tokio::test]
async fn file_delete_existing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.delete(&id).await.unwrap());
    assert!(store.get(&id).await.unwrap().is_none());
}

#[tokio::test]
async fn file_delete_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);
    assert!(!store.delete(&Uuid::new_v4().to_string()).await.unwrap());
}

#[tokio::test]
async fn file_count_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn file_count_after_inserts() {
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
    assert_eq!(store.count().await.unwrap(), 2);
}

#[tokio::test]
async fn file_list_all() {
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
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn file_filter_by_outcome() {
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
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn file_filter_by_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);
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
async fn file_filter_by_time_range() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    store
        .store(&make_receipt_at("early", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("mid", Outcome::Complete, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("late", Outcome::Complete, t3))
        .await
        .unwrap();

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
async fn file_pagination_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(3),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn file_pagination_offset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);
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
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 2);
}

// ── FileReceiptStore persistence ───────────────────────────────────

#[tokio::test]
async fn file_persists_across_instances() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");

    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();

    {
        let store = FileReceiptStore::new(&path);
        store.store(&r).await.unwrap();
    }

    // New instance should see the same data.
    let store2 = FileReceiptStore::new(&path);
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
}

// ── Chain validation ───────────────────────────────────────────────

#[test]
fn chain_empty_is_valid() {
    let result = validate_chain(&[]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn chain_single_valid() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let result = validate_chain(&[r]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 1);
}

#[test]
fn chain_multiple_valid() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let r1 = make_hashed_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_hashed_receipt_at("b", Outcome::Partial, t2);
    let r3 = make_hashed_receipt_at("c", Outcome::Complete, t3);

    let result = validate_chain(&[r1, r2, r3]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 3);
}

#[test]
fn chain_broken_hash() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper after hashing
    let result = validate_chain(&[r]);
    assert!(!result.valid);
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("hash mismatch"));
}

#[test]
fn chain_duplicate_ids() {
    let id = Uuid::new_v4();
    let r1 = make_receipt_with_id("a", Outcome::Complete, id);
    let r2 = make_receipt_with_id("b", Outcome::Complete, id);
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
fn chain_broken_ordering() {
    let t_later = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t_later);
    let r2 = make_receipt_at("b", Outcome::Complete, t_earlier);

    let result = validate_chain(&[r1, r2]);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.message.contains("earlier")));
}

#[test]
fn chain_multiple_errors() {
    let id = Uuid::new_v4();
    let t_later = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t_later);
    let mut r2 = make_receipt_at("b", Outcome::Complete, t_earlier);
    r2.meta.run_id = id;
    let mut r3 = make_receipt_at("c", Outcome::Complete, t_earlier);
    r3.meta.run_id = id; // duplicate

    let result = validate_chain(&[r1, r2, r3]);
    assert!(!result.valid);
    assert!(result.errors.len() >= 2);
}

#[test]
fn chain_no_hash_still_valid() {
    // Receipts without receipt_sha256 skip the hash check.
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    let result = validate_chain(&[r]);
    assert!(result.valid);
}

#[test]
fn chain_validation_error_display() {
    let e = ChainValidationError {
        index: 2,
        message: "test error".to_string(),
    };
    assert_eq!(e.to_string(), "[2]: test error");
}

// ── Serialization roundtrip ────────────────────────────────────────

#[tokio::test]
async fn serialization_roundtrip_memory() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();

    // Verify receipt survives store→get cycle.
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(r.meta.run_id, got.meta.run_id);
    assert_eq!(r.backend.id, got.backend.id);
    assert_eq!(r.outcome, got.outcome);
}

#[tokio::test]
async fn serialization_roundtrip_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();

    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(r.meta.run_id, got.meta.run_id);
    assert_eq!(r.backend.id, got.backend.id);
    assert_eq!(r.outcome, got.outcome);
    assert_eq!(r.receipt_sha256, got.receipt_sha256);
}

#[tokio::test]
async fn serialization_json_lines_format() {
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

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2);
    // Each line should be valid JSON.
    for line in &lines {
        serde_json::from_str::<Receipt>(line).unwrap();
    }
}

// ── ReceiptIndex ───────────────────────────────────────────────────

#[test]
fn index_empty() {
    let idx = ReceiptIndex::new();
    assert!(idx.is_empty());
    assert_eq!(idx.len(), 0);
}

#[test]
fn index_insert_and_by_backend() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    idx.insert(&r);

    let ids = idx.by_backend("mock");
    assert!(ids.contains(&id));
    assert_eq!(idx.len(), 1);
}

#[test]
fn index_insert_and_by_outcome() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    idx.insert(&r1);
    idx.insert(&r2);

    let complete_ids = idx.by_outcome(&Outcome::Complete);
    assert_eq!(complete_ids.len(), 1);
    assert!(complete_ids.contains(&r1.meta.run_id.to_string()));

    let failed_ids = idx.by_outcome(&Outcome::Failed);
    assert_eq!(failed_ids.len(), 1);
    assert!(failed_ids.contains(&r2.meta.run_id.to_string()));
}

#[test]
fn index_by_time_range() {
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

    let range_ids = idx.by_time_range(
        Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap(),
    );
    assert_eq!(range_ids.len(), 1);
    assert!(range_ids.contains(&r2.meta.run_id.to_string()));
}

#[test]
fn index_remove() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.insert(&r);
    assert_eq!(idx.len(), 1);

    idx.remove(&r);
    assert_eq!(idx.len(), 0);
    assert!(idx.is_empty());
    assert!(idx.by_backend("mock").is_empty());
    assert!(idx.by_outcome(&Outcome::Complete).is_empty());
}

#[test]
fn index_multiple_same_backend() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("mock", Outcome::Complete);
    let r2 = make_receipt("mock", Outcome::Failed);
    idx.insert(&r1);
    idx.insert(&r2);

    let ids = idx.by_backend("mock");
    assert_eq!(ids.len(), 2);
    assert_eq!(idx.len(), 2);
}

#[test]
fn index_nonexistent_backend() {
    let idx = ReceiptIndex::new();
    assert!(idx.by_backend("nonexistent").is_empty());
}

#[test]
fn index_nonexistent_outcome() {
    let idx = ReceiptIndex::new();
    assert!(idx.by_outcome(&Outcome::Failed).is_empty());
}

#[test]
fn index_time_range_empty_result() {
    let mut idx = ReceiptIndex::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = make_receipt_at("a", Outcome::Complete, t1);
    idx.insert(&r);

    let range_ids = idx.by_time_range(
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap(),
    );
    assert!(range_ids.is_empty());
}

// ── Empty store edge cases ─────────────────────────────────────────

#[tokio::test]
async fn empty_store_list() {
    let store = InMemoryReceiptStore::new();
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn empty_store_list_with_filter() {
    let store = InMemoryReceiptStore::new();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        backend: Some("x".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn empty_file_store_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert!(all.is_empty());
}

// ── Concurrent access ──────────────────────────────────────────────

#[tokio::test]
async fn concurrent_writers_memory() {
    let store = Arc::new(InMemoryReceiptStore::new());
    let mut handles = Vec::new();

    for _ in 0..20 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let r = make_receipt("concurrent", Outcome::Complete);
            s.store(&r).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 20);
}

#[tokio::test]
async fn concurrent_writers_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = Arc::new(FileReceiptStore::new(&path));
    let mut handles = Vec::new();

    for _ in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let r = make_receipt("concurrent", Outcome::Complete);
            s.store(&r).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 10);
}

#[tokio::test]
async fn concurrent_read_write_memory() {
    let store = Arc::new(InMemoryReceiptStore::new());

    // Insert some initial data.
    for _ in 0..5 {
        store
            .store(&make_receipt("bg", Outcome::Complete))
            .await
            .unwrap();
    }

    let mut handles = Vec::new();

    // Spawn writers.
    for _ in 0..5 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let r = make_receipt("new", Outcome::Complete);
            s.store(&r).await.unwrap();
        }));
    }

    // Spawn readers.
    for _ in 0..5 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let _ = s.list(ReceiptFilter::default()).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(store.count().await.unwrap(), 10);
}

// ── StoreError display ─────────────────────────────────────────────

#[test]
fn error_display_duplicate_id() {
    let e = crate::StoreError::DuplicateId("abc".into());
    assert_eq!(e.to_string(), "duplicate receipt id: abc");
}

#[test]
fn error_display_invalid_id() {
    let e = crate::StoreError::InvalidId("bad".into());
    assert_eq!(e.to_string(), "invalid receipt id: bad");
}

#[test]
fn error_display_other() {
    let e = crate::StoreError::Other("oops".into());
    assert_eq!(e.to_string(), "store error: oops");
}

// ── ReceiptFilter unit tests ───────────────────────────────────────

#[test]
fn filter_matches_all_default() {
    let f = ReceiptFilter::default();
    let r = make_receipt("x", Outcome::Complete);
    assert!(f.matches(&r));
}

#[test]
fn filter_rejects_wrong_outcome() {
    let f = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let r = make_receipt("x", Outcome::Complete);
    assert!(!f.matches(&r));
}

#[test]
fn filter_rejects_wrong_backend() {
    let f = ReceiptFilter {
        backend: Some("wanted".into()),
        ..Default::default()
    };
    let r = make_receipt("other", Outcome::Complete);
    assert!(!f.matches(&r));
}

#[test]
fn filter_paginate_empty() {
    let f = ReceiptFilter {
        limit: Some(10),
        offset: Some(0),
        ..Default::default()
    };
    let result: Vec<i32> = f.paginate(vec![]);
    assert!(result.is_empty());
}

#[test]
fn filter_paginate_limit_only() {
    let f = ReceiptFilter {
        limit: Some(2),
        ..Default::default()
    };
    let result = f.paginate(vec![1, 2, 3, 4, 5]);
    assert_eq!(result, vec![1, 2]);
}

#[test]
fn filter_paginate_offset_only() {
    let f = ReceiptFilter {
        offset: Some(3),
        ..Default::default()
    };
    let result = f.paginate(vec![1, 2, 3, 4, 5]);
    assert_eq!(result, vec![4, 5]);
}

// ── Work order ID filter ──────────────────────────────────────────

#[test]
fn filter_matches_work_order_id() {
    let woid = Uuid::new_v4();
    let r = make_receipt_for_work_order("mock", Outcome::Complete, woid);
    let f = ReceiptFilter {
        work_order_id: Some(woid.to_string()),
        ..Default::default()
    };
    assert!(f.matches(&r));
}

#[test]
fn filter_rejects_wrong_work_order_id() {
    let r = make_receipt_for_work_order("mock", Outcome::Complete, Uuid::new_v4());
    let f = ReceiptFilter {
        work_order_id: Some(Uuid::new_v4().to_string()),
        ..Default::default()
    };
    assert!(!f.matches(&r));
}

#[tokio::test]
async fn memory_filter_by_work_order_id() {
    let woid = Uuid::new_v4();
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_for_work_order("a", Outcome::Complete, woid))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Complete))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        work_order_id: Some(woid.to_string()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].meta.work_order_id, woid);
}

// ── get_by_work_order_id on InMemoryReceiptStore ──────────────────

#[tokio::test]
async fn memory_get_by_work_order_id() {
    let woid = Uuid::new_v4();
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_for_work_order("a", Outcome::Complete, woid))
        .await
        .unwrap();
    store
        .store(&make_receipt_for_work_order("b", Outcome::Failed, woid))
        .await
        .unwrap();
    store
        .store(&make_receipt("c", Outcome::Complete))
        .await
        .unwrap();

    let results = store.get_by_work_order_id(&woid.to_string()).await.unwrap();
    assert_eq!(results.len(), 2);
    for r in &results {
        assert_eq!(r.meta.work_order_id, woid);
    }
}

#[tokio::test]
async fn memory_get_by_work_order_id_empty() {
    let store = InMemoryReceiptStore::new();
    let results = store
        .get_by_work_order_id(&Uuid::new_v4().to_string())
        .await
        .unwrap();
    assert!(results.is_empty());
}

// ── InMemoryReceiptStore index integration ────────────────────────

#[tokio::test]
async fn memory_index_reflects_inserts_and_deletes() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();

    store.store(&r).await.unwrap();
    let idx = store.index().await;
    assert_eq!(idx.len(), 1);
    assert!(idx.by_backend("mock").contains(&id));

    store.delete(&id).await.unwrap();
    let idx = store.index().await;
    assert!(idx.is_empty());
}

// ── ReceiptIndex work_order_id ────────────────────────────────────

#[test]
fn index_by_work_order_id() {
    let woid = Uuid::new_v4();
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt_for_work_order("a", Outcome::Complete, woid);
    let r2 = make_receipt_for_work_order("b", Outcome::Failed, woid);
    let r3 = make_receipt("c", Outcome::Complete);

    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);

    let ids = idx.by_work_order_id(&woid.to_string());
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&r1.meta.run_id.to_string()));
    assert!(ids.contains(&r2.meta.run_id.to_string()));
}

#[test]
fn index_by_work_order_id_nonexistent() {
    let idx = ReceiptIndex::new();
    assert!(idx.by_work_order_id(&Uuid::new_v4().to_string()).is_empty());
}

#[test]
fn index_remove_cleans_work_order() {
    let woid = Uuid::new_v4();
    let mut idx = ReceiptIndex::new();
    let r = make_receipt_for_work_order("mock", Outcome::Complete, woid);
    idx.insert(&r);
    assert_eq!(idx.by_work_order_id(&woid.to_string()).len(), 1);

    idx.remove(&r);
    assert!(idx.by_work_order_id(&woid.to_string()).is_empty());
}

// ── ReceiptStats ──────────────────────────────────────────────────

#[test]
fn stats_empty() {
    let stats = ReceiptStats::from_receipts(&[]);
    assert_eq!(stats.total, 0);
    assert!(stats.avg_duration_ms.is_none());
    assert!(stats.success_rate.is_none());
}

#[test]
fn stats_single_receipt() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 100;
    r.usage.input_tokens = Some(500);
    r.usage.output_tokens = Some(200);

    let stats = ReceiptStats::from_receipts(&[r]);
    assert_eq!(stats.total, 1);
    assert_eq!(stats.total_input_tokens, 500);
    assert_eq!(stats.total_output_tokens, 200);
    assert!((stats.avg_duration_ms.unwrap() - 100.0).abs() < f64::EPSILON);
    assert!((stats.success_rate.unwrap() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn stats_multiple_receipts() {
    let mut r1 = make_receipt("a", Outcome::Complete);
    r1.meta.duration_ms = 100;
    r1.usage.input_tokens = Some(500);

    let mut r2 = make_receipt("a", Outcome::Failed);
    r2.meta.duration_ms = 300;
    r2.usage.input_tokens = Some(300);

    let mut r3 = make_receipt("b", Outcome::Complete);
    r3.meta.duration_ms = 200;
    r3.usage.output_tokens = Some(100);

    let stats = ReceiptStats::from_receipts(&[r1, r2, r3]);
    assert_eq!(stats.total, 3);
    assert_eq!(*stats.by_backend.get("a").unwrap(), 2);
    assert_eq!(*stats.by_backend.get("b").unwrap(), 1);
    assert_eq!(stats.min_duration_ms, Some(100));
    assert_eq!(stats.max_duration_ms, Some(300));
    assert!((stats.avg_duration_ms.unwrap() - 200.0).abs() < f64::EPSILON);
    assert_eq!(stats.total_input_tokens, 800);
    assert_eq!(stats.total_output_tokens, 100);
    // 2 Complete out of 3
    assert!((stats.success_rate.unwrap() - 2.0 / 3.0).abs() < 0.001);
}

#[test]
fn stats_by_outcome_counts() {
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let r3 = make_receipt("c", Outcome::Partial);
    let r4 = make_receipt("d", Outcome::Failed);

    let stats = ReceiptStats::from_receipts(&[r1, r2, r3, r4]);
    assert_eq!(*stats.by_outcome.get("Complete").unwrap(), 1);
    assert_eq!(*stats.by_outcome.get("Failed").unwrap(), 2);
    assert_eq!(*stats.by_outcome.get("Partial").unwrap(), 1);
}

// ── Receipt diffing ───────────────────────────────────────────────

#[test]
fn diff_identical_receipts() {
    let r = make_receipt("mock", Outcome::Complete);
    let diff = diff_receipts(&r, &r);
    assert!(diff.is_empty());
}

#[test]
fn diff_different_outcome() {
    let r1 = make_receipt("mock", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;

    let diff = diff_receipts(&r1, &r2);
    assert!(!diff.is_empty());
    assert!(diff.differences.iter().any(|d| d.field == "outcome"));
}

#[test]
fn diff_different_backend() {
    let r1 = make_receipt("alpha", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.backend.id = "beta".to_string();

    let diff = diff_receipts(&r1, &r2);
    assert!(diff.differences.iter().any(|d| d.field == "backend.id"));
}

#[test]
fn diff_different_usage() {
    let mut r1 = make_receipt("mock", Outcome::Complete);
    let mut r2 = r1.clone();
    r1.usage.input_tokens = Some(100);
    r2.usage.input_tokens = Some(200);

    let diff = diff_receipts(&r1, &r2);
    assert!(
        diff.differences
            .iter()
            .any(|d| d.field == "usage.input_tokens")
    );
}

#[test]
fn diff_different_duration() {
    let mut r1 = make_receipt("mock", Outcome::Complete);
    let mut r2 = r1.clone();
    r1.meta.duration_ms = 100;
    r2.meta.duration_ms = 999;

    let diff = diff_receipts(&r1, &r2);
    assert!(
        diff.differences
            .iter()
            .any(|d| d.field == "meta.duration_ms")
    );
}

#[test]
fn diff_ids() {
    let r1 = make_receipt("mock", Outcome::Complete);
    let r2 = make_receipt("mock", Outcome::Complete);

    let diff = diff_receipts(&r1, &r2);
    assert_eq!(diff.left_id, r1.meta.run_id.to_string());
    assert_eq!(diff.right_id, r2.meta.run_id.to_string());
}

// ── Export / Import ───────────────────────────────────────────────

#[test]
fn export_import_json_roundtrip() {
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let receipts = vec![r1.clone(), r2.clone()];

    let json = export_json(&receipts).unwrap();
    let imported = import_json(&json).unwrap();

    assert_eq!(imported.len(), 2);
    assert_eq!(imported[0].meta.run_id, r1.meta.run_id);
    assert_eq!(imported[1].meta.run_id, r2.meta.run_id);
}

#[test]
fn export_import_jsonl_roundtrip() {
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let receipts = vec![r1.clone(), r2.clone()];

    let jsonl = export_jsonl(&receipts).unwrap();
    let imported = import_jsonl(&jsonl).unwrap();

    assert_eq!(imported.len(), 2);
    assert_eq!(imported[0].meta.run_id, r1.meta.run_id);
    assert_eq!(imported[1].meta.run_id, r2.meta.run_id);
}

#[test]
fn export_json_empty() {
    let json = export_json(&[]).unwrap();
    let imported = import_json(&json).unwrap();
    assert!(imported.is_empty());
}

#[test]
fn export_jsonl_empty() {
    let jsonl = export_jsonl(&[]).unwrap();
    let imported = import_jsonl(&jsonl).unwrap();
    assert!(imported.is_empty());
}

#[test]
fn import_jsonl_skips_blank_lines() {
    let r = make_receipt("mock", Outcome::Complete);
    let line = serde_json::to_string(&r).unwrap();
    let data = format!("\n{line}\n\n{line}\n\n");
    let imported = import_jsonl(&data).unwrap();
    assert_eq!(imported.len(), 2);
}

#[test]
fn import_json_bad_data() {
    let result = import_json("not json");
    assert!(result.is_err());
}

#[test]
fn import_jsonl_bad_line() {
    let result = import_jsonl("not json\n");
    assert!(result.is_err());
}

// ── Chain validation with parent hashes ───────────────────────────

#[test]
fn chain_with_parents_valid_linked() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let r1 = make_hashed_receipt_at("a", Outcome::Complete, t1);
    let parent_hash = r1.receipt_sha256.clone().unwrap();
    let r2 = make_hashed_receipt_at("b", Outcome::Complete, t2);

    let result = crate::chain::validate_chain_with_parents(&[r1, r2], &[None, Some(parent_hash)]);
    assert!(result.valid, "errors: {:?}", result.errors);
}

#[test]
fn chain_with_parents_broken_link() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let r1 = make_hashed_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_hashed_receipt_at("b", Outcome::Complete, t2);

    let result = crate::chain::validate_chain_with_parents(
        &[r1, r2],
        &[None, Some("wrong_hash".to_string())],
    );
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("parent hash mismatch"))
    );
}

#[test]
fn chain_with_parents_genesis_should_not_have_parent() {
    let r1 = make_hashed_receipt("a", Outcome::Complete);
    let result = crate::chain::validate_chain_with_parents(&[r1], &[Some("some_hash".to_string())]);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("genesis receipt"))
    );
}

#[test]
fn chain_with_parents_missing_prev_hash() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    // r1 has no hash.
    let r1 = make_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_receipt_at("b", Outcome::Complete, t2);

    let result = crate::chain::validate_chain_with_parents(
        &[r1, r2],
        &[None, Some("expected_parent".to_string())],
    );
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("previous receipt has no hash"))
    );
}

// ── Concurrent access with index ──────────────────────────────────

#[tokio::test]
async fn concurrent_writers_preserve_index() {
    let store = Arc::new(InMemoryReceiptStore::new());
    let mut handles = Vec::new();

    for _ in 0..20 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let r = make_receipt("concurrent", Outcome::Complete);
            s.store(&r).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let idx = store.index().await;
    assert_eq!(idx.len(), 20);
    assert_eq!(idx.by_backend("concurrent").len(), 20);
}

// ── ReceiptFilter text_search ─────────────────────────────────────

#[test]
fn filter_text_search_matches_backend() {
    let r = make_receipt("openai-gpt4", Outcome::Complete);
    let f = ReceiptFilter {
        text_search: Some("openai".into()),
        ..Default::default()
    };
    assert!(f.matches(&r));
}

#[test]
fn filter_text_search_case_insensitive() {
    let r = make_receipt("OpenAI-GPT4", Outcome::Complete);
    let f = ReceiptFilter {
        text_search: Some("openai".into()),
        ..Default::default()
    };
    assert!(f.matches(&r));
}

#[test]
fn filter_text_search_no_match() {
    let r = make_receipt("anthropic", Outcome::Complete);
    let f = ReceiptFilter {
        text_search: Some("openai".into()),
        ..Default::default()
    };
    assert!(!f.matches(&r));
}

#[test]
fn filter_text_search_matches_outcome() {
    let r = make_receipt("mock", Outcome::Failed);
    let f = ReceiptFilter {
        text_search: Some("fail".into()),
        ..Default::default()
    };
    assert!(f.matches(&r));
}

#[test]
fn filter_text_search_matches_contract_version() {
    let r = make_receipt("mock", Outcome::Complete);
    let f = ReceiptFilter {
        text_search: Some("abp/v0".to_string()),
        ..Default::default()
    };
    assert!(f.matches(&r));
}

#[test]
fn filter_text_search_combined_with_outcome() {
    let r1 = make_receipt("openai", Outcome::Complete);
    let r2 = make_receipt("openai", Outcome::Failed);
    let f = ReceiptFilter {
        text_search: Some("openai".into()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    assert!(!f.matches(&r1));
    assert!(f.matches(&r2));
}

#[tokio::test]
async fn memory_filter_text_search() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("openai-gpt4", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("anthropic-claude", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("local-llama", Outcome::Complete))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        text_search: Some("claude".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "anthropic-claude");
}

#[tokio::test]
async fn file_filter_text_search() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);
    store
        .store(&make_receipt("openai-gpt4", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("anthropic-claude", Outcome::Failed))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        text_search: Some("gpt4".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "openai-gpt4");
}

// ── ReceiptFilter min/max duration ────────────────────────────────

#[test]
fn filter_min_duration() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 500;
    let f = ReceiptFilter {
        min_duration_ms: Some(300),
        ..Default::default()
    };
    assert!(f.matches(&r));

    let f2 = ReceiptFilter {
        min_duration_ms: Some(600),
        ..Default::default()
    };
    assert!(!f2.matches(&r));
}

#[test]
fn filter_max_duration() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 500;
    let f = ReceiptFilter {
        max_duration_ms: Some(1000),
        ..Default::default()
    };
    assert!(f.matches(&r));

    let f2 = ReceiptFilter {
        max_duration_ms: Some(400),
        ..Default::default()
    };
    assert!(!f2.matches(&r));
}

#[test]
fn filter_duration_range() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 500;
    let f = ReceiptFilter {
        min_duration_ms: Some(100),
        max_duration_ms: Some(1000),
        ..Default::default()
    };
    assert!(f.matches(&r));

    let f2 = ReceiptFilter {
        min_duration_ms: Some(600),
        max_duration_ms: Some(1000),
        ..Default::default()
    };
    assert!(!f2.matches(&r));
}

#[tokio::test]
async fn memory_filter_by_duration_range() {
    let store = InMemoryReceiptStore::new();
    let mut fast = make_receipt("a", Outcome::Complete);
    fast.meta.duration_ms = 100;
    let mut medium = make_receipt("b", Outcome::Complete);
    medium.meta.duration_ms = 500;
    let mut slow = make_receipt("c", Outcome::Complete);
    slow.meta.duration_ms = 2000;

    store.store(&fast).await.unwrap();
    store.store(&medium).await.unwrap();
    store.store(&slow).await.unwrap();

    let filter = ReceiptFilter {
        min_duration_ms: Some(200),
        max_duration_ms: Some(1000),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].meta.duration_ms, 500);
}

// ── CSV export ────────────────────────────────────────────────────

#[test]
fn export_csv_empty() {
    let csv = export_csv(&[]).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 1); // header only
    assert!(lines[0].starts_with("run_id,"));
}

#[test]
fn export_csv_single_receipt() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 42;
    r.usage.input_tokens = Some(100);
    r.usage.output_tokens = Some(50);
    let csv = export_csv(&[r.clone()]).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("mock"));
    assert!(lines[1].contains("Complete"));
    assert!(lines[1].contains("42"));
    assert!(lines[1].contains("100"));
    assert!(lines[1].contains("50"));
}

#[test]
fn export_csv_multiple_receipts() {
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let csv = export_csv(&[r1, r2]).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3);
}

#[test]
fn export_csv_missing_tokens() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.usage.input_tokens.is_none());
    let csv = export_csv(&[r]).unwrap();
    // Should not panic; missing tokens leave columns empty.
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn export_csv_header_columns() {
    let csv = export_csv(&[]).unwrap();
    let header = csv.lines().next().unwrap();
    let cols: Vec<&str> = header.split(',').collect();
    assert_eq!(cols.len(), 9);
    assert_eq!(cols[0], "run_id");
    assert_eq!(cols[2], "backend");
    assert_eq!(cols[3], "outcome");
    assert_eq!(cols[6], "duration_ms");
}

// ── Summary export ────────────────────────────────────────────────

#[test]
fn export_summary_empty() {
    let summary = export_summary(&[]);
    assert!(summary.contains("No receipts"));
}

#[test]
fn export_summary_single() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 100;
    r.usage.input_tokens = Some(500);
    r.usage.output_tokens = Some(200);
    let summary = export_summary(&[r]);
    assert!(summary.contains("1 total"));
    assert!(summary.contains("100.0%"));
    assert!(summary.contains("100.0 ms"));
    assert!(summary.contains("500 in"));
    assert!(summary.contains("200 out"));
}

#[test]
fn export_summary_multiple() {
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let r3 = make_receipt("a", Outcome::Complete);
    let summary = export_summary(&[r1, r2, r3]);
    assert!(summary.contains("3 total"));
    assert!(summary.contains("By outcome:"));
    assert!(summary.contains("By backend:"));
    assert!(summary.contains("Complete: 2"));
    assert!(summary.contains("Failed: 1"));
}

#[test]
fn export_summary_contains_separator() {
    let r = make_receipt("mock", Outcome::Complete);
    let summary = export_summary(&[r]);
    assert!(summary.contains("===="));
}

// ── Retention policy ──────────────────────────────────────────────

#[test]
fn retention_none_keeps_all() {
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let policy = ReceiptRetention::none();
    let (kept, result) = policy.apply(vec![r1.clone(), r2.clone()]);
    assert_eq!(kept.len(), 2);
    assert_eq!(result.kept, 2);
    assert_eq!(result.pruned, 0);
    assert!(result.pruned_ids.is_empty());
}

#[test]
fn retention_max_count_prunes_oldest() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_receipt_at("b", Outcome::Complete, t2);
    let r3 = make_receipt_at("c", Outcome::Complete, t3);

    let policy = ReceiptRetention::none().with_max_count(2);
    let (kept, result) = policy.apply(vec![r3.clone(), r1.clone(), r2.clone()]);
    assert_eq!(kept.len(), 2);
    assert_eq!(result.pruned, 1);
    // The oldest (r1) should be pruned.
    assert!(result.pruned_ids.contains(&r1.meta.run_id.to_string()));
    // r2 and r3 should be kept.
    assert!(kept.iter().any(|r| r.meta.run_id == r2.meta.run_id));
    assert!(kept.iter().any(|r| r.meta.run_id == r3.meta.run_id));
}

#[test]
fn retention_max_count_no_prune_when_under() {
    let r1 = make_receipt("a", Outcome::Complete);
    let policy = ReceiptRetention::none().with_max_count(5);
    let (kept, result) = policy.apply(vec![r1]);
    assert_eq!(kept.len(), 1);
    assert_eq!(result.pruned, 0);
}

#[test]
fn retention_max_count_exact_boundary() {
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Complete);
    let policy = ReceiptRetention::none().with_max_count(2);
    let (kept, result) = policy.apply(vec![r1, r2]);
    assert_eq!(kept.len(), 2);
    assert_eq!(result.pruned, 0);
}

#[test]
fn retention_max_total_bytes_prunes_oldest() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_receipt_at("b", Outcome::Complete, t2);

    let one_receipt_size = serde_json::to_string(&r1).unwrap().len() as u64;
    // Allow space for roughly one receipt.
    let policy = ReceiptRetention::none().with_max_total_bytes(one_receipt_size + 10);
    let (kept, result) = policy.apply(vec![r1.clone(), r2.clone()]);
    assert_eq!(kept.len(), 1);
    assert_eq!(result.pruned, 1);
    // Should keep the newest (r2).
    assert_eq!(kept[0].meta.run_id, r2.meta.run_id);
}

#[test]
fn retention_max_total_bytes_keeps_all_under_budget() {
    let r1 = make_receipt("a", Outcome::Complete);
    let policy = ReceiptRetention::none().with_max_total_bytes(1_000_000);
    let (kept, result) = policy.apply(vec![r1]);
    assert_eq!(kept.len(), 1);
    assert_eq!(result.pruned, 0);
}

#[test]
fn retention_empty_input() {
    let policy = ReceiptRetention::none().with_max_count(5);
    let (kept, result) = policy.apply(vec![]);
    assert!(kept.is_empty());
    assert_eq!(result.kept, 0);
    assert_eq!(result.pruned, 0);
}

#[test]
fn retention_combined_count_and_size() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_receipt_at("b", Outcome::Complete, t2);
    let r3 = make_receipt_at("c", Outcome::Complete, t3);

    let policy = ReceiptRetention::none()
        .with_max_count(2)
        .with_max_total_bytes(1_000_000);
    let (kept, result) = policy.apply(vec![r1, r2, r3]);
    assert_eq!(kept.len(), 2);
    assert_eq!(result.pruned, 1);
}

#[tokio::test]
async fn retention_applied_to_store_contents() {
    let store = InMemoryReceiptStore::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_receipt_at("b", Outcome::Complete, t2);
    let r3 = make_receipt_at("c", Outcome::Complete, t3);

    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.store(&r3).await.unwrap();

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    let policy = ReceiptRetention::none().with_max_count(2);
    let (_kept, result) = policy.apply(all);

    // Apply by deleting pruned IDs.
    for id in &result.pruned_ids {
        store.delete(id).await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 2);
}

// ── FileReceiptStore indexed lookup ───────────────────────────────

#[tokio::test]
async fn file_store_reopen_and_filter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");

    {
        let store = FileReceiptStore::new(&path);
        store
            .store(&make_receipt("alpha", Outcome::Complete))
            .await
            .unwrap();
        store
            .store(&make_receipt("beta", Outcome::Failed))
            .await
            .unwrap();
    }

    // Reopen and filter.
    let store = FileReceiptStore::new(&path);
    let filter = ReceiptFilter {
        backend: Some("beta".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "beta");
}

#[tokio::test]
async fn file_store_filter_duration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let mut fast = make_receipt("a", Outcome::Complete);
    fast.meta.duration_ms = 50;
    let mut slow = make_receipt("b", Outcome::Complete);
    slow.meta.duration_ms = 5000;

    store.store(&fast).await.unwrap();
    store.store(&slow).await.unwrap();

    let filter = ReceiptFilter {
        min_duration_ms: Some(1000),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].meta.duration_ms, 5000);
}

// ── ReceiptIndex additional tests ─────────────────────────────────

#[test]
fn index_insert_remove_reinsert() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.insert(&r);
    assert_eq!(idx.len(), 1);
    idx.remove(&r);
    assert_eq!(idx.len(), 0);
    idx.insert(&r);
    assert_eq!(idx.len(), 1);
}

#[test]
fn index_multiple_outcomes() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let r3 = make_receipt("c", Outcome::Partial);
    let r4 = make_receipt("d", Outcome::Complete);
    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);
    idx.insert(&r4);

    assert_eq!(idx.by_outcome(&Outcome::Complete).len(), 2);
    assert_eq!(idx.by_outcome(&Outcome::Failed).len(), 1);
    assert_eq!(idx.by_outcome(&Outcome::Partial).len(), 1);
    assert_eq!(idx.len(), 4);
}

#[test]
fn index_time_range_boundary_inclusive() {
    let mut idx = ReceiptIndex::new();
    let t = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let r = make_receipt_at("a", Outcome::Complete, t);
    idx.insert(&r);

    // Exact match at boundary should be included.
    let ids = idx.by_time_range(t, t);
    assert_eq!(ids.len(), 1);
}

#[test]
fn index_work_order_multiple_receipts() {
    let mut idx = ReceiptIndex::new();
    let woid = Uuid::new_v4();
    let r1 = make_receipt_for_work_order("a", Outcome::Complete, woid);
    let r2 = make_receipt_for_work_order("b", Outcome::Failed, woid);
    let r3 = make_receipt_for_work_order("c", Outcome::Complete, woid);
    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);

    assert_eq!(idx.by_work_order_id(&woid.to_string()).len(), 3);
}

// ── ReceiptStats additional tests ─────────────────────────────────

#[test]
fn stats_zero_duration() {
    let r = make_receipt("mock", Outcome::Complete);
    // duration_ms defaults to 0
    let stats = ReceiptStats::from_receipts(&[r]);
    assert_eq!(stats.min_duration_ms, Some(0));
    assert_eq!(stats.max_duration_ms, Some(0));
}

#[test]
fn stats_all_failed() {
    let r1 = make_receipt("a", Outcome::Failed);
    let r2 = make_receipt("b", Outcome::Failed);
    let stats = ReceiptStats::from_receipts(&[r1, r2]);
    assert!((stats.success_rate.unwrap()).abs() < f64::EPSILON);
    assert_eq!(*stats.by_outcome.get("Failed").unwrap(), 2);
}

#[test]
fn stats_mixed_outcomes_no_complete() {
    let r1 = make_receipt("a", Outcome::Failed);
    let r2 = make_receipt("b", Outcome::Partial);
    let stats = ReceiptStats::from_receipts(&[r1, r2]);
    assert!((stats.success_rate.unwrap()).abs() < f64::EPSILON);
}

#[test]
fn stats_token_aggregation() {
    let mut r1 = make_receipt("a", Outcome::Complete);
    r1.usage.input_tokens = Some(100);
    r1.usage.output_tokens = Some(50);
    let mut r2 = make_receipt("b", Outcome::Complete);
    r2.usage.input_tokens = Some(200);
    r2.usage.output_tokens = None;
    let mut r3 = make_receipt("c", Outcome::Complete);
    r3.usage.input_tokens = None;
    r3.usage.output_tokens = Some(30);

    let stats = ReceiptStats::from_receipts(&[r1, r2, r3]);
    assert_eq!(stats.total_input_tokens, 300);
    assert_eq!(stats.total_output_tokens, 80);
}

// ── Receipt diff additional tests ─────────────────────────────────

#[test]
fn diff_different_mode() {
    let r1 = make_receipt("mock", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.mode = abp_core::ExecutionMode::Passthrough;

    let diff = diff_receipts(&r1, &r2);
    assert!(diff.differences.iter().any(|d| d.field == "mode"));
}

#[test]
fn diff_different_verification() {
    let r1 = make_receipt("mock", Outcome::Complete);
    let mut r2 = r1.clone();
    r2.verification.harness_ok = true;

    let diff = diff_receipts(&r1, &r2);
    assert!(
        diff.differences
            .iter()
            .any(|d| d.field == "verification.harness_ok")
    );
}

#[test]
fn diff_count() {
    let r1 = make_receipt("mock", Outcome::Complete);
    // Diff against itself should be empty.
    let diff = diff_receipts(&r1, &r1);
    assert_eq!(diff.differences.len(), 0);
}

// ── Export/Import roundtrip with new formats ──────────────────────

#[test]
fn export_csv_roundtrip_field_count() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.usage.input_tokens = Some(42);
    r.usage.output_tokens = Some(7);
    let csv = export_csv(&[r]).unwrap();
    let data_line = csv.lines().nth(1).unwrap();
    let fields: Vec<&str> = data_line.split(',').collect();
    assert_eq!(fields.len(), 9);
}

#[test]
fn export_summary_with_tokens() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.usage.input_tokens = Some(1000);
    r.usage.output_tokens = Some(500);
    let summary = export_summary(&[r]);
    assert!(summary.contains("1000 in"));
    assert!(summary.contains("500 out"));
}

// ── Chain validation edge cases ───────────────────────────────────

#[test]
fn chain_same_timestamp_valid() {
    let t = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let r1 = make_receipt_at("a", Outcome::Complete, t);
    let r2 = make_receipt_at("b", Outcome::Complete, t);
    let result = validate_chain(&[r1, r2]);
    // Same timestamp is fine (non-decreasing).
    assert!(result.valid);
}

#[test]
fn chain_single_no_hash() {
    let r = make_receipt("mock", Outcome::Complete);
    let result = validate_chain(&[r]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 1);
}

// ── InMemoryReceiptStore work_order index accuracy ────────────────

#[tokio::test]
async fn memory_work_order_index_after_delete() {
    let woid = Uuid::new_v4();
    let store = InMemoryReceiptStore::new();
    let r = make_receipt_for_work_order("mock", Outcome::Complete, woid);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();

    let results = store.get_by_work_order_id(&woid.to_string()).await.unwrap();
    assert_eq!(results.len(), 1);

    store.delete(&id).await.unwrap();
    let results = store.get_by_work_order_id(&woid.to_string()).await.unwrap();
    assert!(results.is_empty());
}

// ── Retention builder pattern ─────────────────────────────────────

#[test]
fn retention_builder_chaining() {
    let policy = ReceiptRetention::none()
        .with_max_count(100)
        .with_max_total_bytes(1_000_000);
    assert_eq!(policy.max_count, Some(100));
    assert_eq!(policy.max_total_bytes, Some(1_000_000));
    assert!(policy.max_age.is_none());
}

#[test]
fn retention_default_is_no_limits() {
    let policy = ReceiptRetention::default();
    assert!(policy.max_count.is_none());
    assert!(policy.max_age.is_none());
    assert!(policy.max_total_bytes.is_none());
}

// ── File store edge cases ─────────────────────────────────────────

#[tokio::test]
async fn file_store_count_after_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);

    store.delete(&id).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn file_store_multiple_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Complete);
    let r3 = make_receipt("c", Outcome::Complete);
    let id1 = r1.meta.run_id.to_string();
    let id3 = r3.meta.run_id.to_string();

    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.store(&r3).await.unwrap();

    store.delete(&id1).await.unwrap();
    store.delete(&id3).await.unwrap();

    assert_eq!(store.count().await.unwrap(), 1);
    let remaining = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(remaining[0].backend.id, "b");
}
