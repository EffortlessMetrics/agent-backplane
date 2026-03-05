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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Integration tests for receipt verification and batch auditing.

use abp_receipt::verify::{verify_receipt, AuditIssue, ReceiptAuditor};
use abp_receipt::{Outcome, Receipt, ReceiptBuilder, CONTRACT_VERSION};
use chrono::{TimeZone, Utc};
use std::time::Duration;
use uuid::Uuid;

// ── Helper ─────────────────────────────────────────────────────────

fn valid_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

fn valid_receipt_at(backend: &str, ts: chrono::DateTime<Utc>, dur: Duration) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(ts)
        .duration(dur)
        .with_hash()
        .unwrap()
}

// ── verify_receipt: hash checks ────────────────────────────────────

#[test]
fn verify_valid_receipt_passes_all() {
    let r = valid_receipt("mock");
    let res = verify_receipt(&r);
    assert!(res.is_verified());
    assert!(res.issues.is_empty());
    assert!(res.hash_valid);
    assert!(res.contract_valid);
    assert!(res.timestamps_valid);
    assert!(res.outcome_consistent);
}

#[test]
fn verify_receipt_without_hash_is_valid() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let res = verify_receipt(&r);
    assert!(res.hash_valid);
    assert!(res.is_verified());
}

#[test]
fn verify_tampered_hash_detected() {
    let mut r = valid_receipt("mock");
    r.receipt_sha256 =
        Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into());
    let res = verify_receipt(&r);
    assert!(!res.hash_valid);
    assert!(!res.is_verified());
    assert!(res.issues.iter().any(|i| i.contains("hash")));
}

#[test]
fn verify_tampered_outcome_breaks_hash() {
    let mut r = valid_receipt("mock");
    r.outcome = Outcome::Failed;
    let res = verify_receipt(&r);
    assert!(!res.hash_valid);
}

// ── verify_receipt: contract version ───────────────────────────────

#[test]
fn verify_wrong_contract_version() {
    let mut r = valid_receipt("mock");
    r.meta.contract_version = "abp/v99.0".into();
    // Hash will also be wrong since we changed the contract_version
    let res = verify_receipt(&r);
    assert!(!res.contract_valid);
    assert!(res.issues.iter().any(|i| i.contains("contract version")));
}

#[test]
fn verify_correct_contract_version() {
    let r = valid_receipt("mock");
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    let res = verify_receipt(&r);
    assert!(res.contract_valid);
}

// ── verify_receipt: timestamps ─────────────────────────────────────

#[test]
fn verify_finished_before_started() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let mut r = ReceiptBuilder::new("mock")
        .started_at(t1)
        .finished_at(t2)
        .build();
    // Manually fix to create an invalid state (builder clamps duration).
    r.meta.started_at = t1;
    r.meta.finished_at = t2;
    let res = verify_receipt(&r);
    assert!(!res.timestamps_valid);
    assert!(res.issues.iter().any(|i| i.contains("finished_at")));
}

#[test]
fn verify_inconsistent_duration() {
    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.duration_ms = 99999;
    let res = verify_receipt(&r);
    assert!(!res.timestamps_valid);
    assert!(res.issues.iter().any(|i| i.contains("duration_ms")));
}

#[test]
fn verify_consistent_timestamps_pass() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .started_at(t1)
        .duration(Duration::from_secs(5))
        .build();
    let res = verify_receipt(&r);
    assert!(res.timestamps_valid);
}

// ── verify_receipt: outcome consistency ────────────────────────────

#[test]
fn verify_failed_with_no_error_event_flagged() {
    use abp_core::{AgentEvent, AgentEventKind};

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .build();
    let res = verify_receipt(&r);
    assert!(!res.outcome_consistent);
    assert!(res
        .issues
        .iter()
        .any(|i| i.contains("Failed") && i.contains("no error")));
}

#[test]
fn verify_complete_with_error_event_flagged() {
    use abp_core::{AgentEvent, AgentEventKind};

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        })
        .build();
    let res = verify_receipt(&r);
    assert!(!res.outcome_consistent);
    assert!(res
        .issues
        .iter()
        .any(|i| i.contains("Complete") && i.contains("error")));
}

#[test]
fn verify_failed_with_error_event_consistent() {
    let r = ReceiptBuilder::new("mock").error("something broke").build();
    let res = verify_receipt(&r);
    assert!(res.outcome_consistent);
}

#[test]
fn verify_failed_with_empty_trace_consistent() {
    // Failed outcome with empty trace is allowed (backend may not emit events).
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let res = verify_receipt(&r);
    assert!(res.outcome_consistent);
}

#[test]
fn verify_partial_outcome_consistent() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    let res = verify_receipt(&r);
    assert!(res.outcome_consistent);
}

// ── verify_receipt: display ────────────────────────────────────────

#[test]
fn verification_result_display_verified() {
    let r = valid_receipt("mock");
    let res = verify_receipt(&r);
    let display = res.to_string();
    assert!(display.contains("verified"));
}

#[test]
fn verification_result_display_failed() {
    let mut r = valid_receipt("mock");
    r.meta.contract_version = "wrong".into();
    let res = verify_receipt(&r);
    let display = res.to_string();
    assert!(display.contains("failed"));
    assert!(display.contains("issues"));
}

// ── ReceiptAuditor: basic ──────────────────────────────────────────

#[test]
fn audit_empty_batch() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[]);
    assert!(report.is_clean());
    assert_eq!(report.total, 0);
    assert_eq!(report.valid, 0);
    assert_eq!(report.invalid, 0);
}

#[test]
fn audit_single_valid_receipt() {
    let auditor = ReceiptAuditor::new();
    let r = valid_receipt("mock");
    let report = auditor.audit_batch(&[r]);
    assert!(report.is_clean());
    assert_eq!(report.total, 1);
    assert_eq!(report.valid, 1);
    assert_eq!(report.invalid, 0);
}

#[test]
fn audit_multiple_valid_receipts() {
    let auditor = ReceiptAuditor::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap();

    let r1 = valid_receipt_at("mock", t1, Duration::from_secs(5));
    let r2 = valid_receipt_at("mock", t2, Duration::from_secs(5));

    let report = auditor.audit_batch(&[r1, r2]);
    assert!(report.is_clean());
    assert_eq!(report.total, 2);
    assert_eq!(report.valid, 2);
}

// ── ReceiptAuditor: duplicate hashes ───────────────────────────────

#[test]
fn audit_detects_duplicate_hashes() {
    let auditor = ReceiptAuditor::new();
    // Build two receipts that happen to get the same hash (same fixed inputs).
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let id = Uuid::nil();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(id)
        .work_order_id(id)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    // Clone gives same hash but also same run_id, which will also be flagged.
    let r2 = r1.clone();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(!report.duplicate_hashes.is_empty());
    assert!(!report.is_clean());
}

// ── ReceiptAuditor: duplicate run IDs ──────────────────────────────

#[test]
fn audit_detects_duplicate_run_ids() {
    let auditor = ReceiptAuditor::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a").run_id(id).with_hash().unwrap();
    let r2 = ReceiptBuilder::new("b").run_id(id).with_hash().unwrap();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(report
        .issues
        .iter()
        .any(|i| i.description.contains("duplicate run_id")));
}

// ── ReceiptAuditor: timeline consistency ───────────────────────────

#[test]
fn audit_detects_overlapping_timeline() {
    let auditor = ReceiptAuditor::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    // Second receipt starts before the first one finishes (overlap).
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 3).unwrap();

    let r1 = valid_receipt_at("same-backend", t1, Duration::from_secs(10));
    let r2 = valid_receipt_at("same-backend", t2, Duration::from_secs(5));

    let report = auditor.audit_batch(&[r1, r2]);
    assert!(report
        .issues
        .iter()
        .any(|i| i.description.contains("overlapping")));
}

#[test]
fn audit_allows_non_overlapping_same_backend() {
    let auditor = ReceiptAuditor::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap();

    let r1 = valid_receipt_at("same-backend", t1, Duration::from_secs(5));
    let r2 = valid_receipt_at("same-backend", t2, Duration::from_secs(5));

    let report = auditor.audit_batch(&[r1, r2]);
    // No timeline overlap issues
    assert!(!report
        .issues
        .iter()
        .any(|i| i.description.contains("overlapping")));
}

#[test]
fn audit_allows_overlapping_different_backends() {
    let auditor = ReceiptAuditor::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    let r1 = valid_receipt_at("backend-a", t1, Duration::from_secs(30));
    let r2 = valid_receipt_at("backend-b", t1, Duration::from_secs(30));

    let report = auditor.audit_batch(&[r1, r2]);
    assert!(!report
        .issues
        .iter()
        .any(|i| i.description.contains("overlapping")));
}

// ── ReceiptAuditor: invalid receipt in batch ───────────────────────

#[test]
fn audit_counts_invalid_receipts() {
    let auditor = ReceiptAuditor::new();
    let good = valid_receipt("mock");
    let mut bad = valid_receipt("mock");
    bad.meta.contract_version = "wrong/v0".into();

    let report = auditor.audit_batch(&[good, bad]);
    assert_eq!(report.valid, 1);
    assert_eq!(report.invalid, 1);
    assert!(!report.is_clean());
}

// ── AuditReport display ───────────────────────────────────────────

#[test]
fn audit_report_display() {
    let auditor = ReceiptAuditor::new();
    let r = valid_receipt("mock");
    let report = auditor.audit_batch(&[r]);
    let display = report.to_string();
    assert!(display.contains("total: 1"));
    assert!(display.contains("valid: 1"));
}

// ── AuditIssue display ────────────────────────────────────────────

#[test]
fn audit_issue_display_with_all_fields() {
    let issue = AuditIssue {
        receipt_index: Some(0),
        run_id: Some("abc-123".into()),
        description: "test issue".into(),
    };
    let s = issue.to_string();
    assert!(s.contains("#0"));
    assert!(s.contains("abc-123"));
    assert!(s.contains("test issue"));
}

#[test]
fn audit_issue_display_no_context() {
    let issue = AuditIssue {
        receipt_index: None,
        run_id: None,
        description: "standalone issue".into(),
    };
    assert_eq!(issue.to_string(), "standalone issue");
}
