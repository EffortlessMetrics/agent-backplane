#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deep comprehensive tests for `abp-receipt-store` crate — persistence,
//! querying, chain validation, indexing, filter semantics, concurrent access,
//! capacity handling, and serde roundtrips.

use std::sync::Arc;

use abp_core::{CONTRACT_VERSION, Outcome, Receipt, receipt_hash};
use abp_receipt_store::{
    ChainValidation, ChainValidationError, FileReceiptStore, InMemoryReceiptStore, ReceiptFilter,
    ReceiptIndex, ReceiptStore, StoreError, validate_chain,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap()
}

fn ts(offset_secs: i64) -> DateTime<Utc> {
    base_time() + Duration::seconds(offset_secs)
}

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    Receipt {
        meta: abp_core::RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
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

fn make_receipt_at(backend: &str, outcome: Outcome, at: DateTime<Utc>) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.meta.started_at = at;
    r.meta.finished_at = at;
    r
}

fn make_receipt_with_id(backend: &str, outcome: Outcome, id: Uuid) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.meta.run_id = id;
    r
}

fn make_hashed(backend: &str, outcome: Outcome) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.receipt_sha256 = Some(receipt_hash(&r).unwrap());
    r
}

fn make_hashed_at(backend: &str, outcome: Outcome, at: DateTime<Utc>) -> Receipt {
    let mut r = make_receipt_at(backend, outcome, at);
    r.receipt_sha256 = Some(receipt_hash(&r).unwrap());
    r
}

// ===========================================================================
// 1. InMemoryReceiptStore — CRUD
// ===========================================================================

#[tokio::test]
async fn mem_store_then_get() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
    assert_eq!(got.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mem_get_nonexistent_returns_none() {
    let s = InMemoryReceiptStore::new();
    assert!(s.get(&Uuid::new_v4().to_string()).await.unwrap().is_none());
}

#[tokio::test]
async fn mem_store_duplicate_fails() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    s.store(&r).await.unwrap();
    let err = s.store(&r).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("duplicate"));
}

#[tokio::test]
async fn mem_delete_existing_returns_true() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    assert!(s.delete(&id).await.unwrap());
}

#[tokio::test]
async fn mem_delete_missing_returns_false() {
    let s = InMemoryReceiptStore::new();
    assert!(!s.delete(&Uuid::new_v4().to_string()).await.unwrap());
}

#[tokio::test]
async fn mem_get_after_delete_returns_none() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    s.delete(&id).await.unwrap();
    assert!(s.get(&id).await.unwrap().is_none());
}

#[tokio::test]
async fn mem_count_empty() {
    let s = InMemoryReceiptStore::new();
    assert_eq!(s.count().await.unwrap(), 0);
}

#[tokio::test]
async fn mem_count_after_inserts() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("b", Outcome::Failed)).await.unwrap();
    assert_eq!(s.count().await.unwrap(), 2);
}

#[tokio::test]
async fn mem_count_after_delete() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("x", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    s.delete(&id).await.unwrap();
    assert_eq!(s.count().await.unwrap(), 0);
}

#[tokio::test]
async fn mem_list_all() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("b", Outcome::Failed)).await.unwrap();
    let all = s.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn mem_list_empty_store() {
    let s = InMemoryReceiptStore::new();
    let all = s.list(ReceiptFilter::default()).await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn mem_store_preserves_all_fields() {
    let s = InMemoryReceiptStore::new();
    let mut r = make_receipt("my-backend", Outcome::Partial);
    r.meta.duration_ms = 42;
    r.meta.work_order_id = Uuid::new_v4();
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.meta.duration_ms, 42);
    assert_eq!(got.meta.work_order_id, r.meta.work_order_id);
    assert_eq!(got.outcome, Outcome::Partial);
}

#[tokio::test]
async fn mem_delete_then_reinsert() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    s.delete(&id).await.unwrap();
    // Should be able to re-insert same ID after delete
    s.store(&r).await.unwrap();
    assert!(s.get(&id).await.unwrap().is_some());
}

#[tokio::test]
async fn mem_double_delete_second_returns_false() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    assert!(s.delete(&id).await.unwrap());
    assert!(!s.delete(&id).await.unwrap());
}

// ===========================================================================
// 2. InMemoryReceiptStore — Filtering
// ===========================================================================

#[tokio::test]
async fn mem_filter_by_outcome() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("b", Outcome::Failed)).await.unwrap();
    s.store(&make_receipt("c", Outcome::Partial)).await.unwrap();
    let f = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn mem_filter_by_backend() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("beta", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        backend: Some("alpha".into()),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].backend.id, "alpha");
}

#[tokio::test]
async fn mem_filter_by_time_range() {
    let t1 = ts(0);
    let t2 = ts(3600);
    let t3 = ts(7200);

    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt_at("early", Outcome::Complete, t1))
        .await
        .unwrap();
    s.store(&make_receipt_at("mid", Outcome::Complete, t2))
        .await
        .unwrap();
    s.store(&make_receipt_at("late", Outcome::Complete, t3))
        .await
        .unwrap();

    let f = ReceiptFilter {
        time_range: Some((ts(1800), ts(5400))),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].backend.id, "mid");
}

#[tokio::test]
async fn mem_filter_combined_backend_and_outcome() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("alpha", Outcome::Failed))
        .await
        .unwrap();
    s.store(&make_receipt("beta", Outcome::Failed))
        .await
        .unwrap();
    let f = ReceiptFilter {
        backend: Some("alpha".into()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].backend.id, "alpha");
    assert_eq!(res[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn mem_filter_no_match_returns_empty() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert!(res.is_empty());
}

#[tokio::test]
async fn mem_filter_time_range_inclusive_boundary() {
    let t = ts(100);
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt_at("x", Outcome::Complete, t))
        .await
        .unwrap();

    // Range exactly matches the timestamp
    let f = ReceiptFilter {
        time_range: Some((t, t)),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
}

#[tokio::test]
async fn mem_filter_time_range_excludes_before() {
    let t = ts(0);
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt_at("x", Outcome::Complete, t))
        .await
        .unwrap();

    let f = ReceiptFilter {
        time_range: Some((ts(100), ts(200))),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert!(res.is_empty());
}

#[tokio::test]
async fn mem_filter_time_range_excludes_after() {
    let t = ts(500);
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt_at("x", Outcome::Complete, t))
        .await
        .unwrap();

    let f = ReceiptFilter {
        time_range: Some((ts(0), ts(100))),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert!(res.is_empty());
}

#[tokio::test]
async fn mem_filter_all_three_criteria() {
    let t = ts(100);
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt_at("match", Outcome::Complete, t))
        .await
        .unwrap();
    s.store(&make_receipt_at("match", Outcome::Failed, t))
        .await
        .unwrap();
    s.store(&make_receipt_at("other", Outcome::Complete, t))
        .await
        .unwrap();
    s.store(&make_receipt_at("match", Outcome::Complete, ts(999)))
        .await
        .unwrap();

    let f = ReceiptFilter {
        backend: Some("match".into()),
        outcome: Some(Outcome::Complete),
        time_range: Some((ts(0), ts(200))),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].backend.id, "match");
}

// ===========================================================================
// 3. Pagination
// ===========================================================================

#[tokio::test]
async fn mem_paginate_limit() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..5 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        limit: Some(3),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 3);
}

#[tokio::test]
async fn mem_paginate_offset() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..5 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        offset: Some(3),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 2);
}

#[tokio::test]
async fn mem_paginate_limit_and_offset() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..10 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        limit: Some(3),
        offset: Some(2),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 3);
}

#[tokio::test]
async fn mem_paginate_offset_beyond_end() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        offset: Some(100),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert!(res.is_empty());
}

#[tokio::test]
async fn mem_paginate_limit_zero() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        limit: Some(0),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert!(res.is_empty());
}

#[tokio::test]
async fn mem_paginate_limit_exceeds_count() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..3 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        limit: Some(100),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 3);
}

// ===========================================================================
// 4. FileReceiptStore — CRUD
// ===========================================================================

#[tokio::test]
async fn file_store_and_get() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.jsonl");
    let s = FileReceiptStore::new(&path);
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
}

#[tokio::test]
async fn file_get_missing() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    assert!(s.get(&Uuid::new_v4().to_string()).await.unwrap().is_none());
}

#[tokio::test]
async fn file_store_duplicate_fails() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt("mock", Outcome::Complete);
    s.store(&r).await.unwrap();
    let err = s.store(&r).await.unwrap_err();
    assert!(err.to_string().contains("duplicate"));
}

#[tokio::test]
async fn file_delete_existing() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    assert!(s.delete(&id).await.unwrap());
    assert!(s.get(&id).await.unwrap().is_none());
}

#[tokio::test]
async fn file_delete_missing() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    assert!(!s.delete(&Uuid::new_v4().to_string()).await.unwrap());
}

#[tokio::test]
async fn file_count_empty() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    assert_eq!(s.count().await.unwrap(), 0);
}

#[tokio::test]
async fn file_count_after_inserts() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    s.store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("b", Outcome::Failed)).await.unwrap();
    assert_eq!(s.count().await.unwrap(), 2);
}

#[tokio::test]
async fn file_list_all() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    s.store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("b", Outcome::Failed)).await.unwrap();
    assert_eq!(s.list(ReceiptFilter::default()).await.unwrap().len(), 2);
}

#[tokio::test]
async fn file_delete_then_reinsert() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    s.delete(&id).await.unwrap();
    s.store(&r).await.unwrap();
    assert!(s.get(&id).await.unwrap().is_some());
}

// ===========================================================================
// 5. FileReceiptStore — Filtering
// ===========================================================================

#[tokio::test]
async fn file_filter_by_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    s.store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("b", Outcome::Failed)).await.unwrap();
    let f = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn file_filter_by_backend() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    s.store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("beta", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        backend: Some("alpha".into()),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].backend.id, "alpha");
}

#[tokio::test]
async fn file_filter_by_time_range() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let t1 = ts(0);
    let t2 = ts(3600);
    let t3 = ts(7200);
    s.store(&make_receipt_at("early", Outcome::Complete, t1))
        .await
        .unwrap();
    s.store(&make_receipt_at("mid", Outcome::Complete, t2))
        .await
        .unwrap();
    s.store(&make_receipt_at("late", Outcome::Complete, t3))
        .await
        .unwrap();
    let f = ReceiptFilter {
        time_range: Some((ts(1800), ts(5400))),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].backend.id, "mid");
}

#[tokio::test]
async fn file_filter_combined() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    s.store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("alpha", Outcome::Failed))
        .await
        .unwrap();
    s.store(&make_receipt("beta", Outcome::Failed))
        .await
        .unwrap();
    let f = ReceiptFilter {
        backend: Some("alpha".into()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert_eq!(res.len(), 1);
}

#[tokio::test]
async fn file_paginate_limit() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    for _ in 0..5 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        limit: Some(3),
        ..Default::default()
    };
    assert_eq!(s.list(f).await.unwrap().len(), 3);
}

#[tokio::test]
async fn file_paginate_offset() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    for _ in 0..5 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        offset: Some(3),
        ..Default::default()
    };
    assert_eq!(s.list(f).await.unwrap().len(), 2);
}

// ===========================================================================
// 6. FileReceiptStore — Persistence
// ===========================================================================

#[tokio::test]
async fn file_persists_across_instances() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.jsonl");
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    {
        let s = FileReceiptStore::new(&path);
        s.store(&r).await.unwrap();
    }
    let s2 = FileReceiptStore::new(&path);
    let got = s2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
}

#[tokio::test]
async fn file_stores_valid_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.jsonl");
    let s = FileReceiptStore::new(&path);
    s.store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    s.store(&make_receipt("b", Outcome::Failed)).await.unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in &lines {
        serde_json::from_str::<Receipt>(line).unwrap();
    }
}

#[tokio::test]
async fn file_persists_hashed_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.jsonl");
    let s = FileReceiptStore::new(&path);
    let r = make_hashed("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();

    let s2 = FileReceiptStore::new(&path);
    let got = s2.get(&id).await.unwrap().unwrap();
    assert_eq!(got.receipt_sha256, r.receipt_sha256);
}

#[tokio::test]
async fn file_count_after_delete() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt("x", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    s.delete(&id).await.unwrap();
    assert_eq!(s.count().await.unwrap(), 0);
}

#[tokio::test]
async fn file_empty_store_list_with_filter() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let f = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        backend: Some("z".into()),
        ..Default::default()
    };
    assert!(s.list(f).await.unwrap().is_empty());
}

// ===========================================================================
// 7. Chain Validation
// ===========================================================================

#[test]
fn chain_empty_is_valid() {
    let cv = validate_chain(&[]);
    assert!(cv.valid);
    assert_eq!(cv.receipt_count, 0);
    assert!(cv.errors.is_empty());
}

#[test]
fn chain_single_hashed_valid() {
    let r = make_hashed("mock", Outcome::Complete);
    let cv = validate_chain(&[r]);
    assert!(cv.valid);
    assert_eq!(cv.receipt_count, 1);
}

#[test]
fn chain_single_unhashed_valid() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    let cv = validate_chain(&[r]);
    assert!(cv.valid);
}

#[test]
fn chain_chronological_valid() {
    let r1 = make_hashed_at("a", Outcome::Complete, ts(0));
    let r2 = make_hashed_at("b", Outcome::Complete, ts(100));
    let r3 = make_hashed_at("c", Outcome::Complete, ts(200));
    let cv = validate_chain(&[r1, r2, r3]);
    assert!(cv.valid);
    assert_eq!(cv.receipt_count, 3);
}

#[test]
fn chain_same_timestamp_valid() {
    let t = ts(0);
    let r1 = make_hashed_at("a", Outcome::Complete, t);
    let r2 = make_hashed_at("b", Outcome::Complete, t);
    let cv = validate_chain(&[r1, r2]);
    assert!(cv.valid);
}

#[test]
fn chain_detects_broken_hash() {
    let mut r = make_hashed("mock", Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper after hashing
    let cv = validate_chain(&[r]);
    assert!(!cv.valid);
    assert_eq!(cv.errors.len(), 1);
    assert!(cv.errors[0].message.contains("hash mismatch"));
}

#[test]
fn chain_detects_duplicate_ids() {
    let id = Uuid::new_v4();
    let r1 = make_receipt_with_id("a", Outcome::Complete, id);
    let r2 = make_receipt_with_id("b", Outcome::Complete, id);
    let cv = validate_chain(&[r1, r2]);
    assert!(!cv.valid);
    assert!(cv.errors.iter().any(|e| e.message.contains("duplicate")));
}

#[test]
fn chain_detects_broken_ordering() {
    let r1 = make_receipt_at("a", Outcome::Complete, ts(200));
    let r2 = make_receipt_at("b", Outcome::Complete, ts(100));
    let cv = validate_chain(&[r1, r2]);
    assert!(!cv.valid);
    assert!(cv.errors.iter().any(|e| e.message.contains("earlier")));
}

#[test]
fn chain_multiple_errors_reported() {
    let id = Uuid::new_v4();
    let r1 = make_receipt_at("a", Outcome::Complete, ts(200));
    let mut r2 = make_receipt_at("b", Outcome::Complete, ts(100));
    r2.meta.run_id = id;
    let mut r3 = make_receipt_at("c", Outcome::Complete, ts(50));
    r3.meta.run_id = id; // duplicate + ordering
    let cv = validate_chain(&[r1, r2, r3]);
    assert!(!cv.valid);
    assert!(cv.errors.len() >= 2);
}

#[test]
fn chain_validation_error_display() {
    let e = ChainValidationError {
        index: 5,
        message: "bad hash".to_string(),
    };
    assert_eq!(e.to_string(), "[5]: bad hash");
}

#[test]
fn chain_validation_error_display_zero_index() {
    let e = ChainValidationError {
        index: 0,
        message: "first error".to_string(),
    };
    assert_eq!(e.to_string(), "[0]: first error");
}

#[test]
fn chain_valid_all_outcomes() {
    let r1 = make_hashed_at("a", Outcome::Complete, ts(0));
    let r2 = make_hashed_at("b", Outcome::Failed, ts(100));
    let r3 = make_hashed_at("c", Outcome::Partial, ts(200));
    let cv = validate_chain(&[r1, r2, r3]);
    assert!(cv.valid);
    assert_eq!(cv.receipt_count, 3);
}

#[test]
fn chain_tampered_hash_string() {
    let mut r = make_hashed("mock", Outcome::Complete);
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".to_string());
    let cv = validate_chain(&[r]);
    assert!(!cv.valid);
    assert!(cv.errors[0].message.contains("hash mismatch"));
}

#[test]
fn chain_large_valid_chain() {
    let mut receipts = Vec::new();
    for i in 0..50 {
        receipts.push(make_hashed_at("backend", Outcome::Complete, ts(i * 10)));
    }
    let cv = validate_chain(&receipts);
    assert!(cv.valid);
    assert_eq!(cv.receipt_count, 50);
}

// ===========================================================================
// 8. ReceiptIndex — Insert, Query, Remove
// ===========================================================================

#[test]
fn index_new_is_empty() {
    let idx = ReceiptIndex::new();
    assert!(idx.is_empty());
    assert_eq!(idx.len(), 0);
}

#[test]
fn index_insert_single() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.insert(&r);
    assert_eq!(idx.len(), 1);
    assert!(!idx.is_empty());
}

#[test]
fn index_by_backend_hit() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    idx.insert(&r);
    let ids = idx.by_backend("mock");
    assert!(ids.contains(&id));
}

#[test]
fn index_by_backend_miss() {
    let idx = ReceiptIndex::new();
    assert!(idx.by_backend("nonexistent").is_empty());
}

#[test]
fn index_by_outcome_hit() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("a", Outcome::Failed);
    let id = r.meta.run_id.to_string();
    idx.insert(&r);
    let ids = idx.by_outcome(&Outcome::Failed);
    assert!(ids.contains(&id));
}

#[test]
fn index_by_outcome_miss() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("a", Outcome::Complete);
    idx.insert(&r);
    assert!(idx.by_outcome(&Outcome::Failed).is_empty());
}

#[test]
fn index_by_time_range_hit() {
    let mut idx = ReceiptIndex::new();
    let t = ts(100);
    let r = make_receipt_at("a", Outcome::Complete, t);
    let id = r.meta.run_id.to_string();
    idx.insert(&r);
    let ids = idx.by_time_range(ts(0), ts(200));
    assert!(ids.contains(&id));
}

#[test]
fn index_by_time_range_miss() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt_at("a", Outcome::Complete, ts(0));
    idx.insert(&r);
    let ids = idx.by_time_range(ts(100), ts(200));
    assert!(ids.is_empty());
}

#[test]
fn index_by_time_range_boundary_inclusive() {
    let mut idx = ReceiptIndex::new();
    let t = ts(100);
    let r = make_receipt_at("a", Outcome::Complete, t);
    let id = r.meta.run_id.to_string();
    idx.insert(&r);
    let ids = idx.by_time_range(t, t);
    assert!(ids.contains(&id));
}

#[test]
fn index_remove_single() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.insert(&r);
    idx.remove(&r);
    assert_eq!(idx.len(), 0);
    assert!(idx.is_empty());
    assert!(idx.by_backend("mock").is_empty());
    assert!(idx.by_outcome(&Outcome::Complete).is_empty());
}

#[test]
fn index_remove_nonexistent_is_noop() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.remove(&r); // should not panic
    assert!(idx.is_empty());
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
}

#[test]
fn index_multiple_backends() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("alpha", Outcome::Complete);
    let r2 = make_receipt("beta", Outcome::Complete);
    idx.insert(&r1);
    idx.insert(&r2);
    assert_eq!(idx.by_backend("alpha").len(), 1);
    assert_eq!(idx.by_backend("beta").len(), 1);
    assert_eq!(idx.len(), 2);
}

#[test]
fn index_remove_one_of_many() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("mock", Outcome::Complete);
    let r2 = make_receipt("mock", Outcome::Failed);
    let id2 = r2.meta.run_id.to_string();
    idx.insert(&r1);
    idx.insert(&r2);
    idx.remove(&r1);
    assert_eq!(idx.len(), 1);
    assert!(idx.by_backend("mock").contains(&id2));
}

#[test]
fn index_time_range_multiple_at_same_time() {
    let mut idx = ReceiptIndex::new();
    let t = ts(100);
    let r1 = make_receipt_at("a", Outcome::Complete, t);
    let r2 = make_receipt_at("b", Outcome::Complete, t);
    idx.insert(&r1);
    idx.insert(&r2);
    let ids = idx.by_time_range(t, t);
    assert_eq!(ids.len(), 2);
}

#[test]
fn index_time_range_spans_multiple_timestamps() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt_at("a", Outcome::Complete, ts(10));
    let r2 = make_receipt_at("b", Outcome::Complete, ts(20));
    let r3 = make_receipt_at("c", Outcome::Complete, ts(30));
    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);
    let ids = idx.by_time_range(ts(10), ts(30));
    assert_eq!(ids.len(), 3);
}

#[test]
fn index_len_after_mixed_ops() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let r3 = make_receipt("c", Outcome::Partial);
    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);
    assert_eq!(idx.len(), 3);
    idx.remove(&r2);
    assert_eq!(idx.len(), 2);
}

#[test]
fn index_all_outcome_variants() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let r3 = make_receipt("c", Outcome::Partial);
    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);
    assert_eq!(idx.by_outcome(&Outcome::Complete).len(), 1);
    assert_eq!(idx.by_outcome(&Outcome::Failed).len(), 1);
    assert_eq!(idx.by_outcome(&Outcome::Partial).len(), 1);
}

#[test]
fn index_insert_same_receipt_twice_idempotent_backend() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.insert(&r);
    idx.insert(&r);
    // HashSet deduplicates, so by_backend should have 1 entry
    assert_eq!(idx.by_backend("mock").len(), 1);
}

#[test]
fn index_default_is_empty() {
    let idx = ReceiptIndex::default();
    assert!(idx.is_empty());
    assert_eq!(idx.len(), 0);
}

// ===========================================================================
// 9. StoreError — Display and Source
// ===========================================================================

#[test]
fn store_error_duplicate_id_display() {
    let e = StoreError::DuplicateId("abc-123".into());
    assert_eq!(e.to_string(), "duplicate receipt id: abc-123");
}

#[test]
fn store_error_invalid_id_display() {
    let e = StoreError::InvalidId("bad-id".into());
    assert_eq!(e.to_string(), "invalid receipt id: bad-id");
}

#[test]
fn store_error_other_display() {
    let e = StoreError::Other("something broke".into());
    assert_eq!(e.to_string(), "store error: something broke");
}

#[test]
fn store_error_io_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let e = StoreError::Io(io_err);
    assert!(e.to_string().contains("I/O error"));
}

#[test]
fn store_error_json_display() {
    let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let e = StoreError::Json(json_err);
    assert!(e.to_string().contains("JSON error"));
}

#[test]
fn store_error_io_has_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
    let e = StoreError::Io(io_err);
    assert!(std::error::Error::source(&e).is_some());
}

#[test]
fn store_error_json_has_source() {
    let json_err = serde_json::from_str::<serde_json::Value>("nope").unwrap_err();
    let e = StoreError::Json(json_err);
    assert!(std::error::Error::source(&e).is_some());
}

#[test]
fn store_error_duplicate_no_source() {
    let e = StoreError::DuplicateId("x".into());
    assert!(std::error::Error::source(&e).is_none());
}

#[test]
fn store_error_invalid_no_source() {
    let e = StoreError::InvalidId("x".into());
    assert!(std::error::Error::source(&e).is_none());
}

#[test]
fn store_error_other_no_source() {
    let e = StoreError::Other("x".into());
    assert!(std::error::Error::source(&e).is_none());
}

#[test]
fn store_error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access");
    let e: StoreError = io_err.into();
    assert!(matches!(e, StoreError::Io(_)));
}

#[test]
fn store_error_from_json() {
    let json_err = serde_json::from_str::<Receipt>("{}").unwrap_err();
    let e: StoreError = json_err.into();
    assert!(matches!(e, StoreError::Json(_)));
}

#[test]
fn store_error_debug_format() {
    let e = StoreError::DuplicateId("test".into());
    let debug = format!("{:?}", e);
    assert!(debug.contains("DuplicateId"));
}

// ===========================================================================
// 10. ReceiptFilter — Unit Tests
// ===========================================================================

// Test filter matching indirectly through store.list()
#[tokio::test]
async fn filter_default_matches_everything_via_list() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter::default();
    assert_eq!(s.list(f).await.unwrap().len(), 1);
}

#[tokio::test]
async fn filter_outcome_mismatch_returns_empty() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    assert!(s.list(f).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_outcome_match_returns_result() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("x", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    assert_eq!(s.list(f).await.unwrap().len(), 1);
}

#[tokio::test]
async fn filter_backend_mismatch_returns_empty() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("other", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        backend: Some("wanted".into()),
        ..Default::default()
    };
    assert!(s.list(f).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_backend_match_returns_result() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt("wanted", Outcome::Complete))
        .await
        .unwrap();
    let f = ReceiptFilter {
        backend: Some("wanted".into()),
        ..Default::default()
    };
    assert_eq!(s.list(f).await.unwrap().len(), 1);
}

#[tokio::test]
async fn filter_time_range_mismatch_returns_empty() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt_at("x", Outcome::Complete, ts(0)))
        .await
        .unwrap();
    let f = ReceiptFilter {
        time_range: Some((ts(100), ts(200))),
        ..Default::default()
    };
    assert!(s.list(f).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_time_range_match_returns_result() {
    let s = InMemoryReceiptStore::new();
    s.store(&make_receipt_at("x", Outcome::Complete, ts(100)))
        .await
        .unwrap();
    let f = ReceiptFilter {
        time_range: Some((ts(0), ts(200))),
        ..Default::default()
    };
    assert_eq!(s.list(f).await.unwrap().len(), 1);
}

#[tokio::test]
async fn filter_paginate_empty_store() {
    let s = InMemoryReceiptStore::new();
    let f = ReceiptFilter {
        limit: Some(10),
        ..Default::default()
    };
    assert!(s.list(f).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_paginate_limit_only_via_list() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..5 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        limit: Some(2),
        ..Default::default()
    };
    assert_eq!(s.list(f).await.unwrap().len(), 2);
}

#[tokio::test]
async fn filter_paginate_offset_only_via_list() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..5 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        offset: Some(3),
        ..Default::default()
    };
    assert_eq!(s.list(f).await.unwrap().len(), 2);
}

#[tokio::test]
async fn filter_paginate_both_via_list() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..10 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        limit: Some(2),
        offset: Some(1),
        ..Default::default()
    };
    assert_eq!(s.list(f).await.unwrap().len(), 2);
}

#[tokio::test]
async fn filter_paginate_offset_past_end_via_list() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..3 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        offset: Some(100),
        ..Default::default()
    };
    assert!(s.list(f).await.unwrap().is_empty());
}

#[tokio::test]
async fn filter_paginate_no_limit_no_offset_via_list() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..3 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter::default();
    assert_eq!(s.list(f).await.unwrap().len(), 3);
}

#[test]
fn filter_default_fields_are_none() {
    let f = ReceiptFilter::default();
    assert!(f.outcome.is_none());
    assert!(f.backend.is_none());
    assert!(f.time_range.is_none());
    assert!(f.limit.is_none());
    assert!(f.offset.is_none());
}

#[test]
fn filter_debug_format() {
    let f = ReceiptFilter {
        backend: Some("test".into()),
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    let debug = format!("{:?}", f);
    assert!(debug.contains("test"));
    assert!(debug.contains("Complete"));
}

// ===========================================================================
// 11. Serde Roundtrips via File Store
// ===========================================================================

#[tokio::test]
async fn serde_roundtrip_preserves_run_id() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id;
    s.store(&r).await.unwrap();
    let got = s.get(&id.to_string()).await.unwrap().unwrap();
    assert_eq!(got.meta.run_id, id);
}

#[tokio::test]
async fn serde_roundtrip_preserves_backend_id() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt("my-special-backend", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "my-special-backend");
}

#[tokio::test]
async fn serde_roundtrip_preserves_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    for outcome in [Outcome::Complete, Outcome::Failed, Outcome::Partial] {
        let r = make_receipt("test", outcome.clone());
        let id = r.meta.run_id.to_string();
        s.store(&r).await.unwrap();
        let got = s.get(&id).await.unwrap().unwrap();
        assert_eq!(got.outcome, outcome);
    }
}

#[tokio::test]
async fn serde_roundtrip_preserves_hash() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_hashed("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    let hash = r.receipt_sha256.clone();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.receipt_sha256, hash);
}

#[tokio::test]
async fn serde_roundtrip_preserves_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let t = ts(42);
    let r = make_receipt_at("mock", Outcome::Complete, t);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.meta.started_at, t);
    assert_eq!(got.meta.finished_at, t);
}

#[tokio::test]
async fn serde_roundtrip_preserves_duration() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 12345;
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.meta.duration_ms, 12345);
}

#[tokio::test]
async fn serde_roundtrip_preserves_contract_version() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.meta.contract_version, CONTRACT_VERSION);
}

// ===========================================================================
// 12. Concurrent Access
// ===========================================================================

#[tokio::test]
async fn concurrent_memory_writers() {
    let s = Arc::new(InMemoryReceiptStore::new());
    let mut handles = Vec::new();
    for _ in 0..20 {
        let store = Arc::clone(&s);
        handles.push(tokio::spawn(async move {
            store
                .store(&make_receipt("c", Outcome::Complete))
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(s.count().await.unwrap(), 20);
}

#[tokio::test]
async fn concurrent_memory_readers_and_writers() {
    let s = Arc::new(InMemoryReceiptStore::new());
    for _ in 0..5 {
        s.store(&make_receipt("bg", Outcome::Complete))
            .await
            .unwrap();
    }
    let mut handles = Vec::new();
    for _ in 0..5 {
        let store = Arc::clone(&s);
        handles.push(tokio::spawn(async move {
            store
                .store(&make_receipt("new", Outcome::Complete))
                .await
                .unwrap();
        }));
    }
    for _ in 0..5 {
        let store = Arc::clone(&s);
        handles.push(tokio::spawn(async move {
            let _ = store.list(ReceiptFilter::default()).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(s.count().await.unwrap(), 10);
}

#[tokio::test]
async fn concurrent_file_writers() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(FileReceiptStore::new(dir.path().join("r.jsonl")));
    let mut handles = Vec::new();
    for _ in 0..10 {
        let store = Arc::clone(&s);
        handles.push(tokio::spawn(async move {
            store
                .store(&make_receipt("c", Outcome::Complete))
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(s.count().await.unwrap(), 10);
}

#[tokio::test]
async fn concurrent_memory_delete_and_insert() {
    let s = Arc::new(InMemoryReceiptStore::new());
    let receipts: Vec<Receipt> = (0..10)
        .map(|_| make_receipt("x", Outcome::Complete))
        .collect();
    for r in &receipts {
        s.store(r).await.unwrap();
    }
    let mut handles = Vec::new();
    // Delete first 5
    for r in receipts.iter().take(5) {
        let store = Arc::clone(&s);
        let id = r.meta.run_id.to_string();
        handles.push(tokio::spawn(async move {
            store.delete(&id).await.unwrap();
        }));
    }
    // Insert 5 new
    for _ in 0..5 {
        let store = Arc::clone(&s);
        handles.push(tokio::spawn(async move {
            store
                .store(&make_receipt("y", Outcome::Complete))
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(s.count().await.unwrap(), 10);
}

// ===========================================================================
// 13. Capacity / Stress Tests
// ===========================================================================

#[tokio::test]
async fn mem_store_100_receipts() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..100 {
        s.store(&make_receipt("load", Outcome::Complete))
            .await
            .unwrap();
    }
    assert_eq!(s.count().await.unwrap(), 100);
}

#[tokio::test]
async fn mem_list_100_receipts() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..100 {
        s.store(&make_receipt("load", Outcome::Complete))
            .await
            .unwrap();
    }
    let all = s.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(all.len(), 100);
}

#[tokio::test]
async fn file_store_50_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    for _ in 0..50 {
        s.store(&make_receipt("load", Outcome::Complete))
            .await
            .unwrap();
    }
    assert_eq!(s.count().await.unwrap(), 50);
}

#[tokio::test]
async fn mem_delete_all_receipts() {
    let s = InMemoryReceiptStore::new();
    let mut ids = Vec::new();
    for _ in 0..20 {
        let r = make_receipt("x", Outcome::Complete);
        ids.push(r.meta.run_id.to_string());
        s.store(&r).await.unwrap();
    }
    for id in &ids {
        s.delete(id).await.unwrap();
    }
    assert_eq!(s.count().await.unwrap(), 0);
}

#[tokio::test]
async fn mem_paginate_through_all() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..25 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let mut total = 0;
    let page_size = 10;
    let mut offset = 0;
    loop {
        let f = ReceiptFilter {
            limit: Some(page_size),
            offset: Some(offset),
            ..Default::default()
        };
        let page = s.list(f).await.unwrap();
        if page.is_empty() {
            break;
        }
        total += page.len();
        offset += page_size;
    }
    assert_eq!(total, 25);
}

// ===========================================================================
// 14. Index — Advanced Scenarios
// ===========================================================================

#[test]
fn index_rebuild_from_receipts() {
    let mut idx = ReceiptIndex::new();
    let receipts: Vec<Receipt> = (0..10)
        .map(|i| make_receipt_at(&format!("b{}", i % 3), Outcome::Complete, ts(i * 10)))
        .collect();
    for r in &receipts {
        idx.insert(r);
    }
    assert_eq!(idx.len(), 10);
    assert_eq!(idx.by_backend("b0").len(), 4);
    assert_eq!(idx.by_backend("b1").len(), 3);
    assert_eq!(idx.by_backend("b2").len(), 3);
}

#[test]
fn index_remove_cleans_up_backend_entry() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("only-one", Outcome::Complete);
    idx.insert(&r);
    idx.remove(&r);
    // Backend entry should be fully removed (not left as empty set)
    assert!(idx.by_backend("only-one").is_empty());
    assert!(idx.is_empty());
}

#[test]
fn index_remove_cleans_up_outcome_entry() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("x", Outcome::Partial);
    idx.insert(&r);
    idx.remove(&r);
    assert!(idx.by_outcome(&Outcome::Partial).is_empty());
}

#[test]
fn index_time_range_wide_captures_all() {
    let mut idx = ReceiptIndex::new();
    for i in 0..5 {
        idx.insert(&make_receipt_at("a", Outcome::Complete, ts(i * 100)));
    }
    let ids = idx.by_time_range(ts(0), ts(500));
    assert_eq!(ids.len(), 5);
}

#[test]
fn index_time_range_narrow_captures_one() {
    let mut idx = ReceiptIndex::new();
    for i in 0..5 {
        idx.insert(&make_receipt_at("a", Outcome::Complete, ts(i * 100)));
    }
    // Only ts(200) should match [200, 200]
    let ids = idx.by_time_range(ts(200), ts(200));
    assert_eq!(ids.len(), 1);
}

// ===========================================================================
// 15. ReceiptStore as trait object
// ===========================================================================

#[tokio::test]
async fn trait_object_memory() {
    let store: Box<dyn ReceiptStore> = Box::new(InMemoryReceiptStore::new());
    let r = make_receipt("dynamic", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "dynamic");
}

#[tokio::test]
async fn trait_object_file() {
    let dir = tempfile::tempdir().unwrap();
    let store: Box<dyn ReceiptStore> = Box::new(FileReceiptStore::new(dir.path().join("r.jsonl")));
    let r = make_receipt("dynamic", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "dynamic");
}

#[tokio::test]
async fn trait_object_count_and_delete() {
    let store: Box<dyn ReceiptStore> = Box::new(InMemoryReceiptStore::new());
    let r = make_receipt("x", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
    store.delete(&id).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 0);
}

// ===========================================================================
// 16. ChainValidation struct fields
// ===========================================================================

#[test]
fn chain_validation_struct_fields() {
    let cv = ChainValidation {
        valid: true,
        receipt_count: 42,
        errors: vec![],
    };
    assert!(cv.valid);
    assert_eq!(cv.receipt_count, 42);
    assert!(cv.errors.is_empty());
}

#[test]
fn chain_validation_debug() {
    let cv = validate_chain(&[]);
    let debug = format!("{:?}", cv);
    assert!(debug.contains("valid"));
    assert!(debug.contains("receipt_count"));
}

#[test]
fn chain_validation_error_clone() {
    let e = ChainValidationError {
        index: 3,
        message: "cloned".to_string(),
    };
    let e2 = e.clone();
    assert_eq!(e2.index, 3);
    assert_eq!(e2.message, "cloned");
}

#[test]
fn chain_validation_clone() {
    let cv = validate_chain(&[make_hashed("a", Outcome::Complete)]);
    let cv2 = cv.clone();
    assert_eq!(cv2.valid, cv.valid);
    assert_eq!(cv2.receipt_count, cv.receipt_count);
}

// ===========================================================================
// 17. Edge Cases
// ===========================================================================

#[tokio::test]
async fn mem_store_receipt_with_nil_uuid() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt_with_id("mock", Outcome::Complete, Uuid::nil());
    s.store(&r).await.unwrap();
    let got = s.get(&Uuid::nil().to_string()).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
}

#[tokio::test]
async fn file_store_receipt_with_nil_uuid() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt_with_id("mock", Outcome::Complete, Uuid::nil());
    s.store(&r).await.unwrap();
    let got = s.get(&Uuid::nil().to_string()).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
}

#[tokio::test]
async fn mem_store_empty_backend_id() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "");
}

#[tokio::test]
async fn mem_store_unicode_backend_id() {
    let s = InMemoryReceiptStore::new();
    let r = make_receipt("バックエンド", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "バックエンド");
}

#[tokio::test]
async fn mem_store_very_long_backend_id() {
    let s = InMemoryReceiptStore::new();
    let long_name = "x".repeat(1000);
    let r = make_receipt(&long_name, Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, long_name);
}

#[tokio::test]
async fn file_store_unicode_backend_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let s = FileReceiptStore::new(dir.path().join("r.jsonl"));
    let r = make_receipt("日本語バックエンド", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    s.store(&r).await.unwrap();
    let got = s.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "日本語バックエンド");
}

#[test]
fn chain_with_all_failed_receipts() {
    let r1 = make_hashed_at("a", Outcome::Failed, ts(0));
    let r2 = make_hashed_at("b", Outcome::Failed, ts(100));
    let cv = validate_chain(&[r1, r2]);
    assert!(cv.valid);
}

#[test]
fn chain_with_all_partial_receipts() {
    let r1 = make_hashed_at("a", Outcome::Partial, ts(0));
    let r2 = make_hashed_at("b", Outcome::Partial, ts(100));
    let cv = validate_chain(&[r1, r2]);
    assert!(cv.valid);
}

#[test]
fn index_empty_time_range() {
    let idx = ReceiptIndex::new();
    let ids = idx.by_time_range(ts(0), ts(100));
    assert!(ids.is_empty());
}

#[test]
fn receipt_filter_clone() {
    let f = ReceiptFilter {
        backend: Some("test".into()),
        outcome: Some(Outcome::Complete),
        work_order_id: None,
        limit: Some(10),
        offset: Some(5),
        time_range: Some((ts(0), ts(100))),
    };
    let f2 = f.clone();
    assert_eq!(f2.backend, f.backend);
    assert_eq!(f2.outcome, f.outcome);
    assert_eq!(f2.limit, f.limit);
    assert_eq!(f2.offset, f.offset);
    assert_eq!(f2.time_range, f.time_range);
}

#[test]
fn index_insert_and_remove_many() {
    let mut idx = ReceiptIndex::new();
    let receipts: Vec<Receipt> = (0..20)
        .map(|_| make_receipt("bulk", Outcome::Complete))
        .collect();
    for r in &receipts {
        idx.insert(r);
    }
    assert_eq!(idx.len(), 20);
    for r in &receipts {
        idx.remove(r);
    }
    assert_eq!(idx.len(), 0);
    assert!(idx.is_empty());
}

#[tokio::test]
async fn mem_list_filter_with_pagination_and_filter() {
    let s = InMemoryReceiptStore::new();
    for _ in 0..10 {
        s.store(&make_receipt("alpha", Outcome::Complete))
            .await
            .unwrap();
    }
    for _ in 0..5 {
        s.store(&make_receipt("beta", Outcome::Complete))
            .await
            .unwrap();
    }
    let f = ReceiptFilter {
        backend: Some("alpha".into()),
        limit: Some(3),
        offset: Some(2),
        ..Default::default()
    };
    let res = s.list(f).await.unwrap();
    assert!(res.len() <= 3);
    for r in &res {
        assert_eq!(r.backend.id, "alpha");
    }
}

#[tokio::test]
async fn file_store_concurrent_readers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.jsonl");
    let s = Arc::new(FileReceiptStore::new(&path));
    for _ in 0..5 {
        s.store(&make_receipt("x", Outcome::Complete))
            .await
            .unwrap();
    }
    let mut handles = Vec::new();
    for _ in 0..10 {
        let store = Arc::clone(&s);
        handles.push(tokio::spawn(async move {
            let count = store.count().await.unwrap();
            assert_eq!(count, 5);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[test]
fn index_clone() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    idx.insert(&r);
    let idx2 = idx.clone();
    assert_eq!(idx2.len(), 1);
    assert!(idx2.by_backend("mock").contains(&id));
}

#[tokio::test]
async fn mem_default_creates_empty_store() {
    let s = InMemoryReceiptStore::default();
    assert_eq!(s.count().await.unwrap(), 0);
}

#[test]
fn chain_duplicate_at_end() {
    let id = Uuid::new_v4();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt_with_id("b", Outcome::Complete, id);
    let r3 = make_receipt_with_id("c", Outcome::Complete, id);
    let cv = validate_chain(&[r1, r2, r3]);
    assert!(!cv.valid);
    assert!(cv.errors.iter().any(|e| e.message.contains("duplicate")));
}

#[test]
fn chain_ordering_three_reverse() {
    let r1 = make_receipt_at("a", Outcome::Complete, ts(300));
    let r2 = make_receipt_at("b", Outcome::Complete, ts(200));
    let r3 = make_receipt_at("c", Outcome::Complete, ts(100));
    let cv = validate_chain(&[r1, r2, r3]);
    assert!(!cv.valid);
    assert!(cv.errors.len() >= 2); // both r2 and r3 violate ordering
}
