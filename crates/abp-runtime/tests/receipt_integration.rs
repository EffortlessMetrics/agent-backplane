// SPDX-License-Identifier: MIT OR Apache-2.0

//! Integration tests: abp-receipt wired into abp-runtime.

use abp_core::{Outcome, WorkOrderBuilder};
use abp_receipt::{ReceiptBuilder, ReceiptChain, diff_receipts, verify_hash};
use abp_runtime::Runtime;
use chrono::{TimeZone, Utc};
use tokio_stream::StreamExt;

// ── Helper ─────────────────────────────────────────────────────────

fn simple_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task).root(".").build()
}

// ── 1. Receipt built via builder matches expected schema ───────────

#[test]
fn receipt_builder_matches_schema() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .backend_version("0.1")
        .work_order_id(uuid::Uuid::nil())
        .build();

    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));
    assert!(receipt.receipt_sha256.is_none(), "unhashed builder");

    // Serialize and check it round-trips through JSON.
    let json = serde_json::to_value(&receipt).unwrap();
    assert_eq!(json["outcome"], "complete");
    assert_eq!(json["backend"]["id"], "mock");
    assert!(json["meta"]["run_id"].is_string());
}

#[test]
fn receipt_builder_with_hash_matches_schema() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt));

    let json = serde_json::to_value(&receipt).unwrap();
    assert!(json["receipt_sha256"].is_string());
    assert_eq!(json["receipt_sha256"].as_str().unwrap().len(), 64);
}

// ── 2. Receipt chain with 2-3 linked receipts ─────────────────────

#[test]
fn chain_two_linked_receipts() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 5, 0).unwrap();

    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();

    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .started_at(ts2)
        .finished_at(ts2)
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();

    assert_eq!(chain.len(), 2);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_three_linked_receipts() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 1, 1, 1, 0, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2025, 1, 1, 2, 0, 0).unwrap();

    let r1 = ReceiptBuilder::new("backend-a")
        .outcome(Outcome::Complete)
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();

    let r2 = ReceiptBuilder::new("backend-b")
        .outcome(Outcome::Complete)
        .started_at(ts2)
        .finished_at(ts2)
        .with_hash()
        .unwrap();

    let r3 = ReceiptBuilder::new("backend-c")
        .outcome(Outcome::Failed)
        .started_at(ts3)
        .finished_at(ts3)
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    chain.push(r3).unwrap();

    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());

    // Latest is the last pushed.
    assert_eq!(chain.latest().unwrap().backend.id, "backend-c");
}

// ── 3. Receipt diff between two runs ───────────────────────────────

#[test]
fn diff_between_two_runs() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();

    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .run_id(uuid::Uuid::nil())
        .build();

    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    r2.backend.id = "other-backend".into();

    let diff = diff_receipts(&r1, &r2);
    assert!(!diff.is_empty());
    assert!(diff.changes.iter().any(|d| d.field == "outcome"));
    assert!(diff.changes.iter().any(|d| d.field == "backend.id"));
}

#[test]
fn diff_identical_runs_is_empty() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(uuid::Uuid::nil())
        .build();

    let diff = diff_receipts(&r, &r.clone());
    assert!(diff.is_empty());
    assert_eq!(diff.len(), 0);
}

// ── 4. Receipt verification after chain construction ───────────────

#[test]
fn verification_after_chain_construction() {
    let ts1 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 3, 1, 1, 0, 0).unwrap();

    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();

    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .started_at(ts2)
        .finished_at(ts2)
        .with_hash()
        .unwrap();

    // Both individual receipts verify.
    assert!(verify_hash(&r1));
    assert!(verify_hash(&r2));

    let mut chain = ReceiptChain::new();
    chain.push(r1.clone()).unwrap();
    chain.push(r2.clone()).unwrap();

    // Chain verifies as a whole.
    assert!(chain.verify().is_ok());

    // Each receipt in the chain still verifies individually.
    for receipt in chain.iter() {
        assert!(verify_hash(receipt));
    }
}

#[test]
fn chain_rejects_tampered_receipt() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    // Tamper after hashing.
    r.outcome = Outcome::Failed;

    let mut chain = ReceiptChain::new();
    assert!(
        chain.push(r).is_err(),
        "tampered receipt should be rejected"
    );
}

// ── 5. Runtime chain accumulation across runs ──────────────────────

#[tokio::test]
async fn runtime_accumulates_receipt_chain() {
    let rt = Runtime::with_default_backends();
    let wo1 = simple_work_order("task one");
    let wo2 = simple_work_order("task two");

    // First run.
    let handle1 = rt.run_streaming("mock", wo1).await.unwrap();
    // Drain events.
    let _events1: Vec<_> = tokio_stream::StreamExt::collect(handle1.events).await;
    let receipt1 = handle1.receipt.await.unwrap().unwrap();
    assert!(verify_hash(&receipt1));

    // Second run.
    let handle2 = rt.run_streaming("mock", wo2).await.unwrap();
    let _events2: Vec<_> = tokio_stream::StreamExt::collect(handle2.events).await;
    let receipt2 = handle2.receipt.await.unwrap().unwrap();
    assert!(verify_hash(&receipt2));

    // Chain should have 2 entries.
    let chain = rt.receipt_chain();
    let chain_guard = chain.lock().await;
    assert_eq!(chain_guard.len(), 2);
    assert!(chain_guard.verify().is_ok());
}

#[tokio::test]
async fn runtime_receipt_uses_builder_hash() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("hash check");

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    // Receipt must have a valid hash computed via abp-receipt.
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);

    // Recompute via abp_receipt::compute_hash to confirm consistency.
    let recomputed = abp_receipt::compute_hash(&receipt).unwrap();
    assert_eq!(hash, &recomputed);
}
