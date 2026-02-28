// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `abp-runtime`.

use abp_core::{
    BackendIdentity, CapabilityManifest, ExecutionMode, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport, CONTRACT_VERSION,
};
use abp_runtime::store::ReceiptStore;
use abp_runtime::{BackendRegistry, RuntimeError};
use chrono::{TimeZone, Utc};
use proptest::prelude::*;
use uuid::Uuid;

// ── Arbitrary strategies ────────────────────────────────────────────

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<u128>().prop_map(Uuid::from_u128)
}

fn arb_datetime() -> impl Strategy<Value = chrono::DateTime<Utc>> {
    (0i64..2_000_000_000).prop_map(|secs| Utc.timestamp_opt(secs, 0).unwrap())
}

/// Strategy for non-empty backend names (identifiers).
fn backend_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_:]{0,15}".prop_map(|s| s)
}

fn arb_receipt() -> impl Strategy<Value = Receipt> {
    (arb_uuid(), arb_uuid(), arb_datetime(), arb_datetime()).prop_map(
        |(run_id, wo_id, started, finished)| Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: wo_id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: 42,
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
        },
    )
}

// ── 1. RuntimeError display is always non-empty ─────────────────────

proptest! {
    #[test]
    fn runtime_error_display_non_empty(name in ".*") {
        let err = RuntimeError::UnknownBackend { name };
        let msg = err.to_string();
        prop_assert!(!msg.is_empty(), "display must be non-empty");
    }
}

proptest! {
    #[test]
    fn runtime_error_capability_display_non_empty(reason in ".*") {
        let err = RuntimeError::CapabilityCheckFailed(reason);
        let msg = err.to_string();
        prop_assert!(!msg.is_empty(), "display must be non-empty");
    }
}

// ── 2. BackendRegistry: register then contains == true ──────────────

proptest! {
    #[test]
    fn registry_register_then_contains(name in backend_name()) {
        let mut reg = BackendRegistry::default();
        prop_assert!(!reg.contains(&name));

        reg.register(&name, abp_integrations::MockBackend);
        prop_assert!(reg.contains(&name), "registered backend must be found");
        prop_assert!(reg.get(&name).is_some());
    }
}

proptest! {
    #[test]
    fn registry_list_includes_registered(name in backend_name()) {
        let mut reg = BackendRegistry::default();
        reg.register(&name, abp_integrations::MockBackend);
        let names = reg.list();
        prop_assert!(names.contains(&name.as_str()), "list must contain registered name");
    }
}

proptest! {
    #[test]
    fn registry_remove_then_not_contains(name in backend_name()) {
        let mut reg = BackendRegistry::default();
        reg.register(&name, abp_integrations::MockBackend);
        prop_assert!(reg.contains(&name));

        reg.remove(&name);
        prop_assert!(!reg.contains(&name), "removed backend must not be found");
    }
}

// ── 3. ReceiptStore save/load round-trip ────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn receipt_store_round_trip(receipt in arb_receipt()) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(tmp.path());

        // Attach a canonical hash before saving.
        let receipt = receipt.with_hash().unwrap();

        let saved_path = store.save(&receipt).unwrap();
        prop_assert!(saved_path.exists(), "receipt file must exist");

        let loaded = store.load(receipt.meta.run_id).unwrap();
        prop_assert_eq!(
            receipt.receipt_sha256.as_deref(),
            loaded.receipt_sha256.as_deref(),
            "hash must survive round-trip"
        );
        prop_assert_eq!(receipt.meta.run_id, loaded.meta.run_id);
        prop_assert_eq!(receipt.meta.work_order_id, loaded.meta.work_order_id);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn receipt_store_verify_after_save(receipt in arb_receipt()) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(tmp.path());

        let receipt = receipt.with_hash().unwrap();
        store.save(&receipt).unwrap();

        let ok = store.verify(receipt.meta.run_id).unwrap();
        prop_assert!(ok, "freshly saved receipt must verify");
    }
}
