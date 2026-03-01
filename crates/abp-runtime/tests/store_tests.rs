// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_core::{
    BackendIdentity, CONTRACT_VERSION, ExecutionMode, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport,
};
use abp_runtime::store::ReceiptStore;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

fn sample_receipt(run_id: Uuid) -> Receipt {
    let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 42,
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
fn store_save_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let run_id = Uuid::new_v4();
    let receipt = sample_receipt(run_id);

    let path = store.save(&receipt).unwrap();
    assert!(path.exists());

    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
    assert_eq!(loaded.receipt_sha256, receipt.receipt_sha256);
}

#[test]
fn store_list_returns_saved_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    store.save(&sample_receipt(id1)).unwrap();
    store.save(&sample_receipt(id2)).unwrap();

    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}

#[test]
fn store_verify_returns_true_for_valid_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let run_id = Uuid::new_v4();
    store.save(&sample_receipt(run_id)).unwrap();

    assert!(store.verify(run_id).unwrap());
}

#[test]
fn store_load_nonexistent_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let result = store.load(Uuid::new_v4());
    assert!(result.is_err());
}

#[test]
fn store_multiple_saves_and_list_all() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let mut expected_ids = Vec::new();
    for _ in 0..5 {
        let id = Uuid::new_v4();
        expected_ids.push(id);
        store.save(&sample_receipt(id)).unwrap();
    }

    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 5);
    for id in &expected_ids {
        assert!(ids.contains(id));
    }
}
