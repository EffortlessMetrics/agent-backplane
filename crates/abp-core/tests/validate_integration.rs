#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]

//! Integration tests for receipt validation depth and error accumulation.

use std::collections::HashSet;

use abp_core::validate::{ValidationError, validate_receipt};
use abp_core::*;
use chrono::{TimeZone, Utc};

/// Helper: build a minimal valid receipt (no hash).
fn valid_receipt() -> Receipt {
    ReceiptBuilder::new("mock").build()
}

// ── Tests ───────────────────────────────────────────────────────────

/// A receipt constructed via the builder (matching what `MockBackend` would
/// produce) always passes validation.
#[test]
fn mock_style_receipt_passes_validation() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .expect("hashing should succeed");

    validate_receipt(&receipt).expect("mock receipt should be valid");
}

/// A receipt without a hash still passes every other validation check.
#[test]
fn receipt_without_hash_passes_other_checks() {
    let receipt = valid_receipt();
    assert!(receipt.receipt_sha256.is_none());
    validate_receipt(&receipt).expect("unhashed receipt should pass");
}

/// Validation accumulates all errors rather than short-circuiting.
#[test]
fn validation_accumulates_multiple_errors() {
    let start = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let before_start = Utc.with_ymd_and_hms(2025, 6, 1, 11, 0, 0).unwrap();

    let mut receipt = valid_receipt();
    // Inject three independent failures:
    receipt.backend.id = String::new(); // EmptyBackendId
    receipt.meta.contract_version = "wrong/v999".into(); // version mismatch
    receipt.meta.started_at = start; // started_at > finished_at
    receipt.meta.finished_at = before_start;
    receipt.receipt_sha256 = Some("badhash".into()); // hash mismatch

    let errs = validate_receipt(&receipt).unwrap_err();

    let has_empty_backend = errs.contains(&ValidationError::EmptyBackendId);
    let has_version = errs
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidOutcome { reason } if reason.contains("contract_version")));
    let has_time = errs
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidOutcome { reason } if reason.contains("started_at")));
    let has_hash = errs
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidHash { .. }));

    assert!(has_empty_backend, "missing EmptyBackendId: {errs:?}");
    assert!(has_version, "missing contract_version error: {errs:?}");
    assert!(has_time, "missing started_at error: {errs:?}");
    assert!(has_hash, "missing InvalidHash error: {errs:?}");
    assert!(
        errs.len() >= 4,
        "expected ≥4 errors, got {}: {errs:?}",
        errs.len()
    );
}

/// Every `ValidationError` variant produces a unique and informative display string.
#[test]
fn validation_error_display_messages_are_unique() {
    let errors = [
        ValidationError::EmptyBackendId,
        ValidationError::MissingField { field: "task" },
        ValidationError::InvalidHash {
            expected: "abc123".into(),
            actual: "def456".into(),
        },
        ValidationError::InvalidOutcome {
            reason: "test failure".into(),
        },
    ];

    let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();

    // All messages must be unique.
    let unique: HashSet<&String> = messages.iter().collect();
    assert_eq!(
        unique.len(),
        messages.len(),
        "display messages are not all unique: {messages:?}"
    );

    // Every message must contain meaningful content (not just empty or boilerplate).
    for msg in &messages {
        assert!(
            msg.len() > 10,
            "message too short to be informative: {msg:?}"
        );
    }
}

/// A receipt with `finished_at` before `started_at` fails validation.
#[test]
fn future_started_at_before_finished_at_fails() {
    let later = Utc.with_ymd_and_hms(2099, 12, 31, 23, 59, 59).unwrap();
    let earlier = Utc.with_ymd_and_hms(2099, 12, 31, 22, 0, 0).unwrap();

    let receipt = ReceiptBuilder::new("mock")
        .started_at(later)
        .finished_at(earlier)
        .build();

    let errs = validate_receipt(&receipt).unwrap_err();
    assert!(
        errs.iter().any(
            |e| matches!(e, ValidationError::InvalidOutcome { reason } if reason.contains("started_at"))
        ),
        "expected started_at error, got: {errs:?}"
    );
}
