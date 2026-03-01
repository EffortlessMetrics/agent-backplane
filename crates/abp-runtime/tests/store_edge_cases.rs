// SPDX-License-Identifier: MIT OR Apache-2.0
//! Edge-case tests for [`ReceiptStore`].

use abp_core::{
    BackendIdentity, CONTRACT_VERSION, ExecutionMode, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport,
};
use abp_runtime::store::ReceiptStore;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

/// Helper: build a hashed receipt with the given `run_id` and optional field
/// overrides for backend id.
fn receipt_with(run_id: Uuid, backend_id: &str) -> Receipt {
    let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: backend_id.into(),
            backend_version: Some("v1 «special»".into()),
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

/// Shortcut with default backend id.
fn sample_receipt(run_id: Uuid) -> Receipt {
    receipt_with(run_id, "mock")
}

/// Build a receipt whose `started_at` is offset by `minutes` from a baseline.
fn receipt_at(run_id: Uuid, minutes: i64) -> Receipt {
    let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let started = base + chrono::Duration::minutes(minutes);
    let finished = started + chrono::Duration::minutes(5);
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: started,
            finished_at: finished,
            duration_ms: 300_000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
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

// ── 1. Save receipt with special characters in fields ───────────────

#[test]
fn save_receipt_with_special_characters() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let run_id = Uuid::new_v4();

    let receipt = receipt_with(run_id, "bäck-end/ñ «ö» 🚀\ttab\nnewline");
    store.save(&receipt).unwrap();

    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.backend.id, receipt.backend.id);
    assert_eq!(
        loaded.backend.backend_version,
        receipt.backend.backend_version
    );
    assert!(store.verify(run_id).unwrap());
}

// ── 2. Load from empty directory ────────────────────────────────────

#[test]
fn load_from_empty_directory_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let result = store.load(Uuid::new_v4());
    assert!(result.is_err(), "loading from an empty dir should error");
}

// ── 3. Load from non-existent directory ─────────────────────────────

#[test]
fn load_from_nonexistent_directory_returns_error() {
    let store = ReceiptStore::new("/tmp/abp_nonexistent_dir_12345");
    let result = store.load(Uuid::new_v4());
    assert!(result.is_err(), "loading from a missing dir should error");
}

#[test]
fn list_from_nonexistent_directory_returns_empty() {
    let store = ReceiptStore::new("/tmp/abp_nonexistent_dir_12345");
    let ids = store.list().unwrap();
    assert!(
        ids.is_empty(),
        "list on missing dir should return empty vec"
    );
}

// ── 4. Save two receipts with same UUID (overwrite) ─────────────────

#[test]
fn save_same_uuid_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let run_id = Uuid::new_v4();

    let r1 = receipt_with(run_id, "first");
    store.save(&r1).unwrap();

    let r2 = receipt_with(run_id, "second");
    store.save(&r2).unwrap();

    // The second write wins.
    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.backend.id, "second");

    // Only one file on disk.
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 1);
}

// ── 5. List returns correct count ───────────────────────────────────

#[test]
fn list_returns_correct_count() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    for _ in 0..7 {
        store.save(&sample_receipt(Uuid::new_v4())).unwrap();
    }

    assert_eq!(store.list().unwrap().len(), 7);
}

#[test]
fn list_ignores_non_json_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    store.save(&sample_receipt(Uuid::new_v4())).unwrap();

    // Drop a non-JSON file into the store directory.
    std::fs::write(dir.path().join("notes.txt"), "not a receipt").unwrap();
    // Drop a JSON file with a non-UUID name.
    std::fs::write(dir.path().join("not-a-uuid.json"), "{}").unwrap();

    assert_eq!(store.list().unwrap().len(), 1);
}

// ── 6. Verify chain with single receipt ─────────────────────────────

#[test]
fn verify_chain_single_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    store.save(&sample_receipt(Uuid::new_v4())).unwrap();

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 1);
    assert!(chain.invalid_hashes.is_empty());
    assert!(chain.gaps.is_empty(), "single receipt must have no gaps");
}

// ── 7. Verify chain with tampered receipt detects corruption ────────

#[test]
fn verify_chain_detects_tampered_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let good_id = Uuid::new_v4();
    store.save(&receipt_at(good_id, 0)).unwrap();

    // Manually write a receipt with an invalid hash.
    let bad_id = Uuid::new_v4();
    let mut bad = receipt_at(bad_id, 10);
    bad.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let path = dir.path().join(format!("{bad_id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&bad).unwrap()).unwrap();

    let chain = store.verify_chain().unwrap();
    assert!(!chain.is_valid);
    assert_eq!(chain.valid_count, 1);
    assert_eq!(chain.invalid_hashes.len(), 1);
    assert_eq!(chain.invalid_hashes[0], bad_id);
}

#[test]
fn verify_chain_detects_missing_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    // Write a receipt whose receipt_sha256 is null (no hash at all).
    let id = Uuid::new_v4();
    let mut r = receipt_at(id, 0);
    r.receipt_sha256 = None;
    let path = dir.path().join(format!("{id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&r).unwrap()).unwrap();

    let chain = store.verify_chain().unwrap();
    assert!(!chain.is_valid);
    assert_eq!(chain.invalid_hashes, vec![id]);
}

// ── 8. Large number of receipts (50+) ───────────────────────────────

#[test]
fn large_number_of_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let count = 60;
    let mut expected_ids = Vec::with_capacity(count);
    for i in 0..count {
        let id = Uuid::new_v4();
        expected_ids.push(id);
        store.save(&receipt_at(id, (i as i64) * 10)).unwrap();
    }

    // List returns all.
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), count);
    for id in &expected_ids {
        assert!(ids.contains(id), "missing receipt {id}");
    }

    // Every receipt verifies individually.
    for id in &expected_ids {
        assert!(store.verify(*id).unwrap(), "receipt {id} must verify");
    }

    // Chain is fully valid.
    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, count);
    assert_eq!(chain.gaps.len(), count - 1);
}
