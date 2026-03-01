// SPDX-License-Identifier: MIT OR Apache-2.0

//! Property-based tests for receipt validation.

use abp_core::validate::{ValidationError, validate_receipt};
use abp_core::*;
use proptest::prelude::*;

// ── Strategies ──────────────────────────────────────────────────────

fn arb_non_empty_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,32}"
}

fn arb_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
    ]
}

fn arb_execution_mode() -> impl Strategy<Value = ExecutionMode> {
    prop_oneof![
        Just(ExecutionMode::Passthrough),
        Just(ExecutionMode::Mapped),
    ]
}

// ── Property tests ──────────────────────────────────────────────────

proptest! {
    /// A receipt built with `ReceiptBuilder` and `.with_hash()` always passes validation.
    #[test]
    fn builder_with_hash_always_valid(
        backend_id in arb_non_empty_string(),
        outcome in arb_outcome(),
        mode in arb_execution_mode(),
    ) {
        let receipt = ReceiptBuilder::new(backend_id)
            .outcome(outcome)
            .mode(mode)
            .with_hash()
            .expect("hashing should succeed");

        prop_assert!(validate_receipt(&receipt).is_ok());
    }

    /// A receipt with `CONTRACT_VERSION` always passes the version check.
    #[test]
    fn contract_version_always_passes(
        backend_id in arb_non_empty_string(),
    ) {
        let receipt = ReceiptBuilder::new(backend_id).build();
        prop_assert_eq!(&receipt.meta.contract_version, CONTRACT_VERSION);
        // Must not contain a contract-version-related error.
        let result = validate_receipt(&receipt);
        if let Err(errs) = &result {
            for e in errs {
                if let ValidationError::InvalidOutcome { reason } = e {
                    prop_assert!(!reason.contains("contract_version"));
                }
            }
        }
    }

    /// Tampering with any single field after hashing always causes `InvalidHash`.
    #[test]
    fn tamper_after_hash_causes_invalid_hash(
        field_index in 0u8..5,
    ) {
        let mut receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .with_hash()
            .expect("hashing should succeed");

        // Tamper with exactly one field based on `field_index`.
        match field_index {
            0 => receipt.backend.id = "tampered".into(),
            1 => receipt.outcome = Outcome::Failed,
            2 => receipt.meta.duration_ms += 1,
            3 => receipt.mode = ExecutionMode::Passthrough,
            4 => receipt.usage_raw = serde_json::json!({"tampered": true}),
            _ => unreachable!(),
        }

        let errs = validate_receipt(&receipt).unwrap_err();
        prop_assert!(
            errs.iter().any(|e| matches!(e, ValidationError::InvalidHash { .. })),
            "expected InvalidHash after tampering field_index={field_index}, got: {errs:?}"
        );
    }

    /// A non-empty `backend_id` always passes the `EmptyBackendId` check.
    #[test]
    fn non_empty_backend_id_passes(
        backend_id in arb_non_empty_string(),
    ) {
        let receipt = ReceiptBuilder::new(backend_id).build();
        let result = validate_receipt(&receipt);
        // Must not contain EmptyBackendId.
        if let Err(errs) = &result {
            for e in errs {
                prop_assert_ne!(e, &ValidationError::EmptyBackendId);
            }
        }
    }
}
