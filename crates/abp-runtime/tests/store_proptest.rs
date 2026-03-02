// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for [`ReceiptStore`].

use abp_core::{
    BackendIdentity, CapabilityManifest, ExecutionMode, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport, CONTRACT_VERSION,
};
use abp_runtime::store::ReceiptStore;
use chrono::{TimeZone, Utc};
use proptest::prelude::*;
use uuid::Uuid;

// ── Strategies ──────────────────────────────────────────────────────

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<u128>().prop_map(Uuid::from_u128)
}

fn arb_datetime() -> impl Strategy<Value = chrono::DateTime<Utc>> {
    (0i64..2_000_000_000).prop_map(|secs| Utc.timestamp_opt(secs, 0).unwrap())
}

fn arb_receipt() -> impl Strategy<Value = Receipt> {
    (
        arb_uuid(),
        arb_uuid(),
        arb_datetime(),
        arb_datetime(),
        any::<u64>(),
    )
        .prop_map(|(run_id, wo_id, started, finished, dur)| {
            Receipt {
                meta: RunMetadata {
                    run_id,
                    work_order_id: wo_id,
                    contract_version: CONTRACT_VERSION.to_string(),
                    started_at: started,
                    finished_at: finished,
                    duration_ms: dur,
                },
                backend: BackendIdentity {
                    id: "mock".into(),
                    backend_version: None,
                    adapter_version: None,
                },
                capabilities: CapabilityManifest::new(),
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
        })
}

// ── 1. Any receipt saved and loaded back is equal ───────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn save_load_roundtrip_equality(receipt in arb_receipt()) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(tmp.path());

        store.save(&receipt).unwrap();
        let loaded = store.load(receipt.meta.run_id).unwrap();

        // Full JSON equality — covers every field.
        let orig_json = serde_json::to_value(&receipt).unwrap();
        let loaded_json = serde_json::to_value(&loaded).unwrap();
        prop_assert_eq!(orig_json, loaded_json, "round-tripped receipt must be identical");
    }
}

// ── 2. Multiple receipts saved can all be listed ────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn multiple_saved_all_listed(count in 1usize..=10) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(tmp.path());
        let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        let mut ids = Vec::new();
        for _ in 0..count {
            let id = Uuid::new_v4();
            ids.push(id);
            let r = Receipt {
                meta: RunMetadata {
                    run_id: id,
                    work_order_id: Uuid::nil(),
                    contract_version: CONTRACT_VERSION.to_string(),
                    started_at: ts,
                    finished_at: ts,
                    duration_ms: 0,
                },
                backend: BackendIdentity { id: "mock".into(), backend_version: None, adapter_version: None },
                capabilities: CapabilityManifest::new(),
                mode: ExecutionMode::default(),
                usage_raw: serde_json::json!({}),
                usage: UsageNormalized::default(),
                trace: vec![],
                artifacts: vec![],
                verification: VerificationReport::default(),
                outcome: Outcome::Complete,
                receipt_sha256: None,
            }.with_hash().unwrap();
            store.save(&r).unwrap();
        }

        let listed = store.list().unwrap();
        prop_assert_eq!(listed.len(), count, "listed count must match saved count");
        for id in &ids {
            prop_assert!(listed.contains(id), "listed must contain {}", id);
        }
    }
}

// ── 3. Verify always passes for freshly saved valid receipt ─────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn verify_passes_for_fresh_receipt(receipt in arb_receipt()) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(tmp.path());

        store.save(&receipt).unwrap();
        prop_assert!(store.verify(receipt.meta.run_id).unwrap(), "freshly saved receipt must verify");
    }
}

// ── 4. Store directory can be reused across ReceiptStore instances ──

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn store_reuse_across_instances(receipt in arb_receipt()) {
        let tmp = tempfile::tempdir().unwrap();

        // Save with one instance.
        let store1 = ReceiptStore::new(tmp.path());
        store1.save(&receipt).unwrap();

        // Load with a fresh instance pointing at the same directory.
        let store2 = ReceiptStore::new(tmp.path());
        let loaded = store2.load(receipt.meta.run_id).unwrap();
        prop_assert_eq!(receipt.meta.run_id, loaded.meta.run_id);
        prop_assert_eq!(receipt.receipt_sha256, loaded.receipt_sha256);

        // List and verify with the second instance.
        let ids = store2.list().unwrap();
        prop_assert!(ids.contains(&receipt.meta.run_id));
        prop_assert!(store2.verify(receipt.meta.run_id).unwrap());
    }
}
