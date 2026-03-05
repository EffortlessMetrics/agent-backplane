#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive test suite for `abp-receipt-store`.
//!
//! 100+ tests covering CRUD, filtering, pagination, persistence,
//! chain validation, indexing, serialization, error handling, and edge cases.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use uuid::Uuid;

use abp_core::{Outcome, Receipt};
use abp_receipt_store::{
    ChainValidation, ChainValidationError, FileReceiptStore, InMemoryReceiptStore, ReceiptFilter,
    ReceiptIndex, ReceiptStore, StoreError, validate_chain,
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

// ====================================================================
// InMemoryReceiptStore — CRUD
// ====================================================================

#[tokio::test]
async fn mem_store_single_receipt() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("alpha", Outcome::Complete);
    store.store(&r).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn mem_get_returns_stored_receipt() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("alpha", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.meta.run_id, r.meta.run_id);
    assert_eq!(got.backend.id, "alpha");
    assert_eq!(got.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mem_get_nonexistent_returns_none() {
    let store = InMemoryReceiptStore::new();
    let result = store.get("no-such-id").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn mem_get_with_random_uuid_returns_none() {
    let store = InMemoryReceiptStore::new();
    let result = store.get(&Uuid::new_v4().to_string()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn mem_store_duplicate_id_returns_error() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("alpha", Outcome::Complete);
    store.store(&r).await.unwrap();
    let err = store.store(&r).await.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("duplicate"));
}

#[tokio::test]
async fn mem_duplicate_does_not_increase_count() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("alpha", Outcome::Complete);
    store.store(&r).await.unwrap();
    let _ = store.store(&r).await;
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn mem_delete_existing_returns_true() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("alpha", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.delete(&id).await.unwrap());
}

#[tokio::test]
async fn mem_delete_removes_receipt() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("alpha", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    store.delete(&id).await.unwrap();
    assert!(store.get(&id).await.unwrap().is_none());
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn mem_delete_nonexistent_returns_false() {
    let store = InMemoryReceiptStore::new();
    assert!(!store.delete("no-such-id").await.unwrap());
}

#[tokio::test]
async fn mem_delete_twice_second_returns_false() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("alpha", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.delete(&id).await.unwrap());
    assert!(!store.delete(&id).await.unwrap());
}

#[tokio::test]
async fn mem_count_empty_store() {
    let store = InMemoryReceiptStore::new();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn mem_count_after_multiple_inserts() {
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
async fn mem_count_after_insert_and_delete() {
    let store = InMemoryReceiptStore::new();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let id1 = r1.meta.run_id.to_string();
    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();
    store.delete(&id1).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn mem_reinsert_after_delete() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("alpha", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    store.delete(&id).await.unwrap();
    // Should succeed since original was deleted
    store.store(&r).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn mem_store_many_different_backends() {
    let store = InMemoryReceiptStore::new();
    let backends = ["mock", "sidecar:node", "sidecar:python", "openai", "gemini"];
    for b in &backends {
        store
            .store(&make_receipt(b, Outcome::Complete))
            .await
            .unwrap();
    }
    assert_eq!(store.count().await.unwrap(), backends.len());
}

#[tokio::test]
async fn mem_store_preserves_all_fields() {
    let store = InMemoryReceiptStore::new();
    let mut r = make_receipt("mock", Outcome::Partial);
    r.meta.duration_ms = 12345;
    r.meta.work_order_id = Uuid::new_v4();
    r.backend.backend_version = Some("1.2.3".to_string());
    r.backend.adapter_version = Some("0.0.1".to_string());
    r.usage.input_tokens = Some(100);
    r.usage.output_tokens = Some(200);
    r.verification.harness_ok = true;
    r.verification.git_diff = Some("diff content".to_string());
    let id = r.meta.run_id.to_string();

    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();

    assert_eq!(got.meta.duration_ms, 12345);
    assert_eq!(got.meta.work_order_id, r.meta.work_order_id);
    assert_eq!(got.backend.backend_version, Some("1.2.3".to_string()));
    assert_eq!(got.backend.adapter_version, Some("0.0.1".to_string()));
    assert_eq!(got.usage.input_tokens, Some(100));
    assert_eq!(got.usage.output_tokens, Some(200));
    assert!(got.verification.harness_ok);
    assert_eq!(got.verification.git_diff.as_deref(), Some("diff content"));
    assert_eq!(got.outcome, Outcome::Partial);
}

#[tokio::test]
async fn mem_default_constructor() {
    let store = InMemoryReceiptStore::default();
    assert_eq!(store.count().await.unwrap(), 0);
}

// ====================================================================
// InMemoryReceiptStore — list & filter
// ====================================================================

#[tokio::test]
async fn mem_list_all_empty() {
    let store = InMemoryReceiptStore::new();
    let results = store.list(ReceiptFilter::default()).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn mem_list_all_returns_everything() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..4 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let results = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(results.len(), 4);
}

#[tokio::test]
async fn mem_filter_outcome_complete() {
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
async fn mem_filter_outcome_partial() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Partial))
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
async fn mem_filter_outcome_no_match() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn mem_filter_backend_exact_match() {
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
        backend: Some("beta".to_string()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "beta");
}

#[tokio::test]
async fn mem_filter_backend_no_match() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        backend: Some("nonexistent".to_string()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn mem_filter_backend_multiple_matches() {
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
        .store(&make_receipt("beta", Outcome::Complete))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        backend: Some("alpha".to_string()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.backend.id == "alpha"));
}

#[tokio::test]
async fn mem_filter_time_range_inclusive() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("a", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("b", Outcome::Complete, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("c", Outcome::Complete, t3))
        .await
        .unwrap();

    // Range exactly matching t2 boundaries
    let filter = ReceiptFilter {
        time_range: Some((t2, t2)),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "b");
}

#[tokio::test]
async fn mem_filter_time_range_all_included() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();

    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("a", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("b", Outcome::Complete, t2))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        time_range: Some((t1, t2)),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn mem_filter_time_range_none_included() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("a", Outcome::Complete, t1))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn mem_filter_combined_backend_outcome() {
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
        backend: Some("alpha".to_string()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn mem_filter_combined_all_criteria() {
    let t1 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap();

    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("alpha", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("alpha", Outcome::Failed, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("beta", Outcome::Failed, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("alpha", Outcome::Failed, t3))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        backend: Some("alpha".to_string()),
        outcome: Some(Outcome::Failed),
        time_range: Some((t1, t2)),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
    assert_eq!(results[0].meta.started_at, t2);
}

// ====================================================================
// Pagination
// ====================================================================

#[tokio::test]
async fn mem_pagination_limit_only() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..8 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(3),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 3);
}

#[tokio::test]
async fn mem_pagination_offset_only() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..8 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        offset: Some(5),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 3);
}

#[tokio::test]
async fn mem_pagination_limit_and_offset() {
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
    assert_eq!(store.list(filter).await.unwrap().len(), 3);
}

#[tokio::test]
async fn mem_pagination_offset_at_end() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        offset: Some(5),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn mem_pagination_offset_beyond_end() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        offset: Some(999),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn mem_pagination_limit_zero() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(0),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn mem_pagination_limit_exceeds_total() {
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
async fn mem_pagination_with_filter() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("alpha", Outcome::Complete))
            .await
            .unwrap();
    }
    for _ in 0..5 {
        store
            .store(&make_receipt("beta", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        backend: Some("alpha".to_string()),
        limit: Some(2),
        offset: Some(1),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.backend.id == "alpha"));
}

// ====================================================================
// FileReceiptStore — CRUD
// ====================================================================

#[tokio::test]
async fn file_store_and_retrieve() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("fs-backend", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();

    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "fs-backend");
    assert_eq!(got.outcome, Outcome::Complete);
}

#[tokio::test]
async fn file_get_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert!(store.get("missing-id").await.unwrap().is_none());
}

#[tokio::test]
async fn file_duplicate_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("mock", Outcome::Complete);
    store.store(&r).await.unwrap();
    assert!(store.store(&r).await.is_err());
}

#[tokio::test]
async fn file_delete_existing() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.delete(&id).await.unwrap());
    assert!(store.get(&id).await.unwrap().is_none());
}

#[tokio::test]
async fn file_delete_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert!(!store.delete("nope").await.unwrap());
}

#[tokio::test]
async fn file_count_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn file_count_after_inserts() {
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
    assert_eq!(store.count().await.unwrap(), 2);
}

#[tokio::test]
async fn file_count_after_delete() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("a", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    store.delete(&id).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn file_list_all() {
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
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(all.len(), 2);
}

// ====================================================================
// FileReceiptStore — filtering
// ====================================================================

#[tokio::test]
async fn file_filter_by_outcome() {
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
        backend: Some("alpha".to_string()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
}

#[tokio::test]
async fn file_filter_by_time_range() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    store
        .store(&make_receipt_at("a", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("b", Outcome::Complete, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("c", Outcome::Complete, t3))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2025, 4, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 8, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "b");
}

#[tokio::test]
async fn file_filter_combined_backend_and_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
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
        backend: Some("alpha".to_string()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn file_pagination_limit() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
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
async fn file_pagination_offset() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
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
async fn file_pagination_limit_and_offset() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    for _ in 0..10 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(3),
        offset: Some(4),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 3);
}

// ====================================================================
// FileReceiptStore — persistence
// ====================================================================

#[tokio::test]
async fn file_persists_across_instances() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let r = make_receipt("persist", Outcome::Complete);
    let id = r.meta.run_id.to_string();

    {
        let store = FileReceiptStore::new(&path);
        store.store(&r).await.unwrap();
    }
    // New store instance reads same file
    let store2 = FileReceiptStore::new(&path);
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "persist");
}

#[tokio::test]
async fn file_delete_persists_across_instances() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let r = make_receipt("del", Outcome::Complete);
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
async fn file_multiple_inserts_persist() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");

    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let id1 = r1.meta.run_id.to_string();
    let id2 = r2.meta.run_id.to_string();

    {
        let store = FileReceiptStore::new(&path);
        store.store(&r1).await.unwrap();
        store.store(&r2).await.unwrap();
    }
    let store2 = FileReceiptStore::new(&path);
    assert!(store2.get(&id1).await.unwrap().is_some());
    assert!(store2.get(&id2).await.unwrap().is_some());
    assert_eq!(store2.count().await.unwrap(), 2);
}

// ====================================================================
// Serialization
// ====================================================================

#[tokio::test]
async fn file_produces_valid_jsonl() {
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
async fn file_hashed_receipt_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    let original_hash = r.receipt_sha256.clone();

    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.receipt_sha256, original_hash);
}

#[tokio::test]
async fn mem_serialization_preserves_contract_version() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn file_serialization_preserves_usage_raw() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    let mut r = make_receipt("mock", Outcome::Complete);
    r.usage_raw = serde_json::json!({"prompt_tokens": 10, "completion_tokens": 20});
    let id = r.meta.run_id.to_string();

    store.store(&r).await.unwrap();

    // Reopen
    let store2 = FileReceiptStore::new(&path);
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.usage_raw["prompt_tokens"], 10);
    assert_eq!(got.usage_raw["completion_tokens"], 20);
}

// ====================================================================
// Chain validation
// ====================================================================

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
fn chain_single_unhashed_valid() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    let result = validate_chain(&[r]);
    assert!(result.valid);
}

#[test]
fn chain_chronological_order_valid() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let r1 = make_hashed_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_hashed_receipt_at("b", Outcome::Partial, t2);
    let r3 = make_hashed_receipt_at("c", Outcome::Failed, t3);

    let result = validate_chain(&[r1, r2, r3]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 3);
}

#[test]
fn chain_same_timestamp_valid() {
    let t = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let r1 = make_hashed_receipt_at("a", Outcome::Complete, t);
    let r2 = make_hashed_receipt_at("b", Outcome::Complete, t);
    let result = validate_chain(&[r1, r2]);
    assert!(result.valid);
}

#[test]
fn chain_detects_broken_hash() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper
    let result = validate_chain(&[r]);
    assert!(!result.valid);
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("hash mismatch"));
}

#[test]
fn chain_detects_duplicate_ids() {
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
fn chain_detects_out_of_order() {
    let t_later = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
    let t_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = make_receipt_at("a", Outcome::Complete, t_later);
    let r2 = make_receipt_at("b", Outcome::Complete, t_earlier);
    let result = validate_chain(&[r1, r2]);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.message.contains("earlier")));
}

#[test]
fn chain_detects_multiple_errors() {
    let id = Uuid::new_v4();
    let t_later = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
    let t_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t_later);
    let r2 = make_receipt_with_id("b", Outcome::Complete, id);
    let mut r3 = make_receipt_with_id("c", Outcome::Complete, id);
    r3.meta.started_at = t_earlier;

    let result = validate_chain(&[r1, r2, r3]);
    assert!(!result.valid);
    assert!(result.errors.len() >= 2);
}

#[test]
fn chain_large_valid_chain() {
    let base = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let receipts: Vec<Receipt> = (0..50)
        .map(|i| {
            let ts = base + chrono::Duration::hours(i);
            make_hashed_receipt_at("mock", Outcome::Complete, ts)
        })
        .collect();
    let result = validate_chain(&receipts);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 50);
}

#[test]
fn chain_validation_error_display_format() {
    let e = ChainValidationError {
        index: 5,
        message: "something went wrong".to_string(),
    };
    assert_eq!(e.to_string(), "[5]: something went wrong");
}

#[test]
fn chain_validation_struct_fields() {
    let v = ChainValidation {
        valid: true,
        receipt_count: 3,
        errors: vec![],
    };
    assert!(v.valid);
    assert_eq!(v.receipt_count, 3);
    assert!(v.errors.is_empty());
}

// ====================================================================
// ReceiptIndex
// ====================================================================

#[test]
fn index_new_is_empty() {
    let idx = ReceiptIndex::new();
    assert!(idx.is_empty());
    assert_eq!(idx.len(), 0);
}

#[test]
fn index_default_is_empty() {
    let idx = ReceiptIndex::default();
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
fn index_by_backend_nonexistent() {
    let idx = ReceiptIndex::new();
    assert!(idx.by_backend("ghost").is_empty());
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
    assert!(failed.contains(&r2.meta.run_id.to_string()));
}

#[test]
fn index_by_outcome_nonexistent() {
    let idx = ReceiptIndex::new();
    assert!(idx.by_outcome(&Outcome::Partial).is_empty());
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

    let ids = idx.by_time_range(
        Utc.with_ymd_and_hms(2025, 4, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2025, 8, 1, 0, 0, 0).unwrap(),
    );
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&r2.meta.run_id.to_string()));
}

#[test]
fn index_by_time_range_all_included() {
    let mut idx = ReceiptIndex::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_receipt_at("b", Outcome::Complete, t2);

    idx.insert(&r1);
    idx.insert(&r2);

    let ids = idx.by_time_range(t1, t2);
    assert_eq!(ids.len(), 2);
}

#[test]
fn index_by_time_range_none_included() {
    let mut idx = ReceiptIndex::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = make_receipt_at("a", Outcome::Complete, t1);
    idx.insert(&r);

    let ids = idx.by_time_range(
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap(),
    );
    assert!(ids.is_empty());
}

#[test]
fn index_by_time_range_boundary_inclusive() {
    let mut idx = ReceiptIndex::new();
    let t = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
    let r = make_receipt_at("a", Outcome::Complete, t);
    let id = r.meta.run_id.to_string();
    idx.insert(&r);

    // Range endpoints exactly equal to the timestamp
    let ids = idx.by_time_range(t, t);
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&id));
}

#[test]
fn index_remove_cleans_all_maps() {
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
fn index_remove_one_of_many_same_backend() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("mock", Outcome::Complete);
    let r2 = make_receipt("mock", Outcome::Failed);
    let id2 = r2.meta.run_id.to_string();

    idx.insert(&r1);
    idx.insert(&r2);
    idx.remove(&r1);

    assert_eq!(idx.len(), 1);
    let ids = idx.by_backend("mock");
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&id2));
}

#[test]
fn index_remove_nonexistent_is_noop() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    // Removing something that was never inserted should not panic
    idx.remove(&r);
    assert!(idx.is_empty());
}

#[test]
fn index_multiple_receipts_same_timestamp() {
    let mut idx = ReceiptIndex::new();
    let t = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let r1 = make_receipt_at("a", Outcome::Complete, t);
    let r2 = make_receipt_at("b", Outcome::Failed, t);

    idx.insert(&r1);
    idx.insert(&r2);

    let ids = idx.by_time_range(t, t);
    assert_eq!(ids.len(), 2);
}

#[test]
fn index_clone() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.insert(&r);

    let cloned = idx.clone();
    assert_eq!(cloned.len(), 1);
    assert!(!cloned.by_backend("mock").is_empty());
}

// ====================================================================
// ReceiptFilter — exercised through store.list()
// ====================================================================

#[tokio::test]
async fn filter_default_matches_all_outcomes() {
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
    let results = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn filter_outcome_rejects_mismatch_via_store() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Failed))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Partial))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_backend_rejects_mismatch_via_store() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("other", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        backend: Some("wanted".to_string()),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_time_range_rejects_before_via_store() {
    let store = InMemoryReceiptStore::new();
    let t = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    store
        .store(&make_receipt_at("x", Outcome::Complete, t))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_time_range_rejects_after_via_store() {
    let store = InMemoryReceiptStore::new();
    let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    store
        .store(&make_receipt_at("x", Outcome::Complete, t))
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
async fn filter_paginate_empty_store_via_list() {
    let store = InMemoryReceiptStore::new();
    let filter = ReceiptFilter {
        limit: Some(10),
        offset: Some(0),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_paginate_no_limit_no_offset_via_store() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter::default();
    assert_eq!(store.list(filter).await.unwrap().len(), 5);
}

#[tokio::test]
async fn filter_paginate_limit_only_via_store() {
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
async fn filter_paginate_offset_only_via_store() {
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
    for _ in 0..10 {
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

#[tokio::test]
async fn filter_paginate_offset_past_end_via_store() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..3 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        offset: Some(100),
        ..Default::default()
    };
    assert!(store.list(filter).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_paginate_limit_larger_than_remaining_via_store() {
    let store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let filter = ReceiptFilter {
        limit: Some(10),
        offset: Some(3),
        ..Default::default()
    };
    assert_eq!(store.list(filter).await.unwrap().len(), 2);
}

#[test]
fn filter_clone() {
    let f = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        backend: Some("mock".to_string()),
        limit: Some(10),
        offset: Some(5),
        ..Default::default()
    };
    let cloned = f.clone();
    assert_eq!(cloned.outcome, Some(Outcome::Complete));
    assert_eq!(cloned.backend, Some("mock".to_string()));
    assert_eq!(cloned.limit, Some(10));
    assert_eq!(cloned.offset, Some(5));
}

// ====================================================================
// StoreError
// ====================================================================

#[test]
fn error_display_duplicate_id() {
    let e = StoreError::DuplicateId("abc-123".to_string());
    assert_eq!(e.to_string(), "duplicate receipt id: abc-123");
}

#[test]
fn error_display_invalid_id() {
    let e = StoreError::InvalidId("not-a-uuid".to_string());
    assert_eq!(e.to_string(), "invalid receipt id: not-a-uuid");
}

#[test]
fn error_display_other() {
    let e = StoreError::Other("something failed".to_string());
    assert_eq!(e.to_string(), "store error: something failed");
}

#[test]
fn error_display_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let e = StoreError::Io(io_err);
    let msg = e.to_string();
    assert!(msg.contains("I/O error"));
}

#[test]
fn error_display_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
    let e = StoreError::Json(json_err);
    let msg = e.to_string();
    assert!(msg.contains("JSON error"));
}

#[test]
fn error_source_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::Other, "test");
    let e = StoreError::Io(io_err);
    assert!(std::error::Error::source(&e).is_some());
}

#[test]
fn error_source_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
    let e = StoreError::Json(json_err);
    assert!(std::error::Error::source(&e).is_some());
}

#[test]
fn error_source_duplicate_is_none() {
    let e = StoreError::DuplicateId("x".to_string());
    assert!(std::error::Error::source(&e).is_none());
}

#[test]
fn error_source_invalid_is_none() {
    let e = StoreError::InvalidId("x".to_string());
    assert!(std::error::Error::source(&e).is_none());
}

#[test]
fn error_source_other_is_none() {
    let e = StoreError::Other("x".to_string());
    assert!(std::error::Error::source(&e).is_none());
}

#[test]
fn error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let store_err: StoreError = io_err.into();
    assert!(matches!(store_err, StoreError::Io(_)));
}

#[test]
fn error_from_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let store_err: StoreError = json_err.into();
    assert!(matches!(store_err, StoreError::Json(_)));
}

#[test]
fn error_is_debug() {
    let e = StoreError::DuplicateId("test".to_string());
    let debug = format!("{e:?}");
    assert!(debug.contains("DuplicateId"));
}

// ====================================================================
// Concurrent access
// ====================================================================

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
            .store(&make_receipt("data", Outcome::Complete))
            .await
            .unwrap();
    }

    let mut handles = Vec::new();
    for _ in 0..20 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let list = s.list(ReceiptFilter::default()).await.unwrap();
            assert_eq!(list.len(), 10);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn concurrent_mixed_operations_memory() {
    let store = Arc::new(InMemoryReceiptStore::new());
    // Pre-populate
    for _ in 0..5 {
        store
            .store(&make_receipt("pre", Outcome::Complete))
            .await
            .unwrap();
    }

    let mut handles = Vec::new();

    // Writers
    for _ in 0..5 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.store(&make_receipt("new", Outcome::Failed))
                .await
                .unwrap();
        }));
    }

    // Readers
    for _ in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let _ = s.count().await.unwrap();
            let _ = s.list(ReceiptFilter::default()).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 10);
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

// ====================================================================
// Trait object usage
// ====================================================================

#[tokio::test]
async fn trait_object_memory() {
    let store: Box<dyn ReceiptStore> = Box::new(InMemoryReceiptStore::new());
    let r = make_receipt("trait", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.get(&id).await.unwrap().is_some());
    assert_eq!(store.count().await.unwrap(), 1);
    assert!(store.delete(&id).await.unwrap());
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn trait_object_file() {
    let dir = tempfile::tempdir().unwrap();
    let store: Box<dyn ReceiptStore> = Box::new(file_store(&dir));
    let r = make_receipt("trait", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.get(&id).await.unwrap().is_some());
    assert_eq!(store.count().await.unwrap(), 1);
    assert!(store.delete(&id).await.unwrap());
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn trait_object_arc() {
    let store: Arc<dyn ReceiptStore> = Arc::new(InMemoryReceiptStore::new());
    let r = make_receipt("arc", Outcome::Complete);
    store.store(&r).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

// ====================================================================
// Edge cases
// ====================================================================

#[tokio::test]
async fn mem_store_with_empty_backend_name() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "");
}

#[tokio::test]
async fn mem_store_with_unicode_backend() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("后端-テスト", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "后端-テスト");
}

#[tokio::test]
async fn mem_store_with_special_chars_backend() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("back/end:v1.0@test#1", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "back/end:v1.0@test#1");
}

#[tokio::test]
async fn mem_get_empty_string_id() {
    let store = InMemoryReceiptStore::new();
    assert!(store.get("").await.unwrap().is_none());
}

#[tokio::test]
async fn mem_delete_empty_string_id() {
    let store = InMemoryReceiptStore::new();
    assert!(!store.delete("").await.unwrap());
}

#[tokio::test]
async fn file_store_with_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    // Create an empty file
    tokio::fs::write(&path, "").await.unwrap();
    let store = FileReceiptStore::new(&path);
    assert_eq!(store.count().await.unwrap(), 0);
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn file_store_nonexistent_file_reads_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("does_not_exist.jsonl");
    let store = FileReceiptStore::new(&path);
    assert_eq!(store.count().await.unwrap(), 0);
    assert!(store.get("anything").await.unwrap().is_none());
}

#[tokio::test]
async fn file_store_with_blank_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");
    let store = FileReceiptStore::new(&path);

    // Store a receipt, then manually add blank lines
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let with_blanks = format!("\n{}\n\n", content.trim());
    tokio::fs::write(&path, with_blanks).await.unwrap();

    // Should still be able to read it
    let store2 = FileReceiptStore::new(&path);
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
    assert_eq!(store2.count().await.unwrap(), 1);
}

#[tokio::test]
async fn file_store_preserves_nil_uuid_work_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();

    let store2 = FileReceiptStore::new(dir.path().join("receipts.jsonl"));
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.meta.work_order_id, Uuid::nil());
}

#[tokio::test]
async fn file_delete_then_reinsert() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();

    store.store(&r).await.unwrap();
    store.delete(&id).await.unwrap();
    // Reinserting should work
    store.store(&r).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn mem_list_filter_with_all_none_is_default() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let filter = ReceiptFilter {
        outcome: None,
        backend: None,
        time_range: None,
        work_order_id: None,
        limit: None,
        offset: None,
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn mem_purge_all_receipts() {
    let store = InMemoryReceiptStore::new();
    let mut ids = Vec::new();
    for _ in 0..10 {
        let r = make_receipt("x", Outcome::Complete);
        ids.push(r.meta.run_id.to_string());
        store.store(&r).await.unwrap();
    }
    for id in &ids {
        store.delete(id).await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 0);
    assert!(
        store
            .list(ReceiptFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn file_store_preserves_execution_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");

    let mut r = make_receipt("mock", Outcome::Complete);
    r.mode = abp_core::ExecutionMode::Passthrough;
    let id = r.meta.run_id.to_string();

    let store = FileReceiptStore::new(&path);
    store.store(&r).await.unwrap();

    let store2 = FileReceiptStore::new(&path);
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.mode, abp_core::ExecutionMode::Passthrough);
}

#[tokio::test]
async fn file_store_preserves_verification_report() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.jsonl");

    let mut r = make_receipt("mock", Outcome::Complete);
    r.verification.git_diff = Some("diff --git a/f b/f\n+line".to_string());
    r.verification.git_status = Some("M f".to_string());
    r.verification.harness_ok = true;
    let id = r.meta.run_id.to_string();

    let store = FileReceiptStore::new(&path);
    store.store(&r).await.unwrap();

    let store2 = FileReceiptStore::new(&path);
    let got = store2.get(&id).await.unwrap().unwrap();
    assert_eq!(
        got.verification.git_diff.as_deref(),
        Some("diff --git a/f b/f\n+line")
    );
    assert_eq!(got.verification.git_status.as_deref(), Some("M f"));
    assert!(got.verification.harness_ok);
}

// ====================================================================
// Chain validation with store roundtrip
// ====================================================================

#[tokio::test]
async fn chain_validates_after_store_roundtrip() {
    let store = InMemoryReceiptStore::new();
    let base = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    let mut receipts = Vec::new();
    for i in 0..5 {
        let ts = base + chrono::Duration::hours(i);
        let r = make_hashed_receipt_at("mock", Outcome::Complete, ts);
        store.store(&r).await.unwrap();
        receipts.push(r);
    }

    // Retrieve all and validate chain
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    // Sort by started_at for chain validation
    let mut sorted = all;
    sorted.sort_by_key(|r| r.meta.started_at);

    let validation = validate_chain(&sorted);
    assert!(validation.valid);
    assert_eq!(validation.receipt_count, 5);
}

#[tokio::test]
async fn file_chain_validates_after_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(&dir);
    let base = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    for i in 0..3 {
        let ts = base + chrono::Duration::hours(i);
        let r = make_hashed_receipt_at("mock", Outcome::Complete, ts);
        store.store(&r).await.unwrap();
    }

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    let validation = validate_chain(&all);
    assert!(validation.valid);
    assert_eq!(validation.receipt_count, 3);
}

// ====================================================================
// Index with store integration
// ====================================================================

#[test]
fn index_bulk_insert_and_query() {
    let mut idx = ReceiptIndex::new();
    let base = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    for i in 0..20 {
        let ts = base + chrono::Duration::days(i);
        let backend = if i % 2 == 0 { "even" } else { "odd" };
        let outcome = if i % 3 == 0 {
            Outcome::Complete
        } else {
            Outcome::Failed
        };
        idx.insert(&make_receipt_at(backend, outcome, ts));
    }

    assert_eq!(idx.len(), 20);
    assert_eq!(idx.by_backend("even").len(), 10);
    assert_eq!(idx.by_backend("odd").len(), 10);
}

#[test]
fn index_remove_all_returns_empty() {
    let mut idx = ReceiptIndex::new();
    let receipts: Vec<Receipt> = (0..5)
        .map(|_| make_receipt("mock", Outcome::Complete))
        .collect();

    for r in &receipts {
        idx.insert(r);
    }
    assert_eq!(idx.len(), 5);

    for r in &receipts {
        idx.remove(r);
    }
    assert_eq!(idx.len(), 0);
    assert!(idx.is_empty());
}
