// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_core::validate::{ValidationError, validate_receipt};
use abp_core::*;
use chrono::Utc;
use uuid::Uuid;

/// Helper: build a minimal valid receipt.
fn valid_receipt() -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
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
}

#[test]
fn valid_receipt_passes() {
    assert!(validate_receipt(&valid_receipt()).is_ok());
}

#[test]
fn empty_backend_id_fails() {
    let mut r = valid_receipt();
    r.backend.id = String::new();
    let errs = validate_receipt(&r).unwrap_err();
    assert!(errs.contains(&ValidationError::EmptyBackendId));
}

#[test]
fn wrong_hash_fails() {
    let mut r = valid_receipt();
    r.receipt_sha256 = Some("badhash".into());
    let errs = validate_receipt(&r).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ValidationError::InvalidHash { .. }))
    );
}

#[test]
fn correct_hash_passes() {
    let r = valid_receipt().with_hash().unwrap();
    assert!(validate_receipt(&r).is_ok());
}

#[test]
fn mismatched_contract_version_fails() {
    let mut r = valid_receipt();
    r.meta.contract_version = "abp/v999".into();
    let errs = validate_receipt(&r).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ValidationError::InvalidOutcome { .. }))
    );
}

#[test]
fn started_after_finished_fails() {
    let mut r = valid_receipt();
    r.meta.started_at = r.meta.finished_at + chrono::Duration::seconds(1);
    let errs = validate_receipt(&r).unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        ValidationError::InvalidOutcome { reason } if reason.contains("started_at")
    )));
}

#[test]
fn multiple_errors_collected() {
    let mut r = valid_receipt();
    r.backend.id = String::new();
    r.meta.contract_version = "wrong".into();
    r.receipt_sha256 = Some("badhash".into());
    let errs = validate_receipt(&r).unwrap_err();
    // Should have at least: EmptyBackendId + contract mismatch + hash mismatch
    assert!(
        errs.len() >= 3,
        "expected >=3 errors, got {}: {errs:?}",
        errs.len()
    );
}
