// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for [`ReceiptStore`] — chain verification, concurrency, and edge cases.

use abp_core::{
    BackendIdentity, CONTRACT_VERSION, ExecutionMode, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport,
};
use abp_runtime::store::ReceiptStore;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

/// Build a hashed receipt at a given minute offset from a fixed baseline.
fn receipt_at(run_id: Uuid, minutes: i64) -> Receipt {
    let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let started = base + chrono::Duration::minutes(minutes);
    let finished = started + chrono::Duration::minutes(1);
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: started,
            finished_at: finished,
            duration_ms: 60_000,
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

fn _sample_receipt(run_id: Uuid) -> Receipt {
    receipt_at(run_id, 0)
}

// ── 1. Chain verification with 100 receipts ─────────────────────────

#[test]
fn chain_verification_100_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    for i in 0..100 {
        let id = Uuid::new_v4();
        store.save(&receipt_at(id, i * 5)).unwrap();
    }

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 100);
    assert_eq!(chain.gaps.len(), 99);
}

// ── 2. Tampered receipt detected in chain ───────────────────────────

#[test]
fn chain_detects_single_tampered_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    // 5 good receipts
    for i in 0..5 {
        store.save(&receipt_at(Uuid::new_v4(), i * 10)).unwrap();
    }

    // 1 tampered receipt
    let bad_id = Uuid::new_v4();
    let mut bad = receipt_at(bad_id, 60);
    bad.receipt_sha256 = Some("bad_hash".into());
    let path = dir.path().join(format!("{bad_id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&bad).unwrap()).unwrap();

    let chain = store.verify_chain().unwrap();
    assert!(!chain.is_valid);
    assert_eq!(chain.valid_count, 5);
    assert_eq!(chain.invalid_hashes, vec![bad_id]);
}

// ── 3. Multiple tampered receipts detected ──────────────────────────

#[test]
fn chain_detects_multiple_tampered_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    store.save(&receipt_at(Uuid::new_v4(), 0)).unwrap();

    let mut bad_ids = Vec::new();
    for i in 1..=3 {
        let id = Uuid::new_v4();
        bad_ids.push(id);
        let mut bad = receipt_at(id, i * 10);
        bad.receipt_sha256 = Some("tampered".into());
        let path = dir.path().join(format!("{id}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&bad).unwrap()).unwrap();
    }

    let chain = store.verify_chain().unwrap();
    assert!(!chain.is_valid);
    assert_eq!(chain.valid_count, 1);
    assert_eq!(chain.invalid_hashes.len(), 3);
    for id in &bad_ids {
        assert!(chain.invalid_hashes.contains(id));
    }
}

// ── 4. Concurrent store access (multi-threaded save/load) ───────────

#[test]
fn concurrent_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(ReceiptStore::new(dir.path()));

    let mut handles = Vec::new();
    let ids: Vec<Uuid> = (0..20).map(|_| Uuid::new_v4()).collect();

    // Concurrent saves
    for (i, &id) in ids.iter().enumerate() {
        let store = std::sync::Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            store.save(&receipt_at(id, i as i64)).unwrap();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // Concurrent loads
    let mut load_handles = Vec::new();
    for &id in &ids {
        let store = std::sync::Arc::clone(&store);
        load_handles.push(std::thread::spawn(move || {
            let r = store.load(id).unwrap();
            assert_eq!(r.meta.run_id, id);
        }));
    }
    for h in load_handles {
        h.join().unwrap();
    }

    assert_eq!(store.list().unwrap().len(), 20);
}

// ── 5. Empty store operations ───────────────────────────────────────

#[test]
fn empty_store_list_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn empty_store_verify_chain_is_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 0);
    assert!(chain.invalid_hashes.is_empty());
    assert!(chain.gaps.is_empty());
}

// ── 6. Duplicate receipt IDs (overwrite) ────────────────────────────

#[test]
fn duplicate_receipt_id_overwrites_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let id = Uuid::new_v4();

    let r1 = receipt_at(id, 0);
    store.save(&r1).unwrap();

    let r2 = receipt_at(id, 10);
    store.save(&r2).unwrap();

    // Only one entry
    assert_eq!(store.list().unwrap().len(), 1);

    // Loaded receipt matches the second save
    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.meta.started_at, r2.meta.started_at);

    // Hash is still valid (second receipt was hashed correctly)
    assert!(store.verify(id).unwrap());
}

// ── 7. Large number of receipts ─────────────────────────────────────

#[test]
fn store_200_receipts_and_list_all() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let count = 200;
    let mut expected = Vec::with_capacity(count);
    for i in 0..count {
        let id = Uuid::new_v4();
        expected.push(id);
        store.save(&receipt_at(id, i as i64)).unwrap();
    }

    let listed = store.list().unwrap();
    assert_eq!(listed.len(), count);
    for id in &expected {
        assert!(listed.contains(id));
    }
}

// ── 8. Receipt retrieval by run_id ──────────────────────────────────

#[test]
fn load_specific_receipt_among_many() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let target_id = Uuid::new_v4();
    for i in 0..10 {
        let id = if i == 5 { target_id } else { Uuid::new_v4() };
        store.save(&receipt_at(id, i * 5)).unwrap();
    }

    let loaded = store.load(target_id).unwrap();
    assert_eq!(loaded.meta.run_id, target_id);
    assert!(store.verify(target_id).unwrap());
}

// ── 9. Chain gap detection ──────────────────────────────────────────

#[test]
fn chain_gaps_reflect_time_between_runs() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    // Receipt 0: starts at minute 0, finishes at minute 1
    // Receipt 1: starts at minute 10, finishes at minute 11
    // Receipt 2: starts at minute 20, finishes at minute 21
    for i in 0..3 {
        store.save(&receipt_at(Uuid::new_v4(), i * 10)).unwrap();
    }

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.gaps.len(), 2);

    // Each gap is (finished_at of prev, started_at of next).
    // Gap 0: minute 1 -> minute 10
    // Gap 1: minute 11 -> minute 20
    for (finished, started) in &chain.gaps {
        assert!(started > finished, "started_at must be after finished_at of previous");
    }
}

// ── 10. Verify individual receipt returns false for tampered ────────

#[test]
fn verify_returns_false_for_tampered_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let id = Uuid::new_v4();
    let mut r = receipt_at(id, 0);
    r.receipt_sha256 = Some("0".repeat(64));
    let path = dir.path().join(format!("{id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&r).unwrap()).unwrap();

    assert!(!store.verify(id).unwrap());
}

// ── 11. Verify nonexistent receipt returns error ────────────────────

#[test]
fn verify_nonexistent_receipt_errors() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    assert!(store.verify(Uuid::new_v4()).is_err());
}

// ── 12. Save and reload preserves all fields ────────────────────────

#[test]
fn save_reload_preserves_fields() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let id = Uuid::new_v4();
    let original = receipt_at(id, 42);

    store.save(&original).unwrap();
    let loaded = store.load(id).unwrap();

    assert_eq!(loaded.meta.run_id, original.meta.run_id);
    assert_eq!(loaded.meta.contract_version, original.meta.contract_version);
    assert_eq!(loaded.meta.duration_ms, original.meta.duration_ms);
    assert_eq!(loaded.backend.id, original.backend.id);
    assert_eq!(loaded.receipt_sha256, original.receipt_sha256);
    assert_eq!(loaded.meta.started_at, original.meta.started_at);
    assert_eq!(loaded.meta.finished_at, original.meta.finished_at);
}
