#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for canonical, audit_trail, export, and compliance modules.

use abp_receipt::canonical::{
    CanonicalReceipt, canonical_hash, canonicalize, diff_canonical, verify,
};
use abp_receipt::audit_trail::{AuditAction, AuditEntry, AuditTrail};
use abp_receipt::compliance::{ComplianceCheck, Severity};
use abp_receipt::export;
use abp_receipt::{Outcome, Receipt, ReceiptBuilder};
use std::time::Duration;
use uuid::Uuid;

// -- Helper --

fn sample_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(100, 200)
        .build()
}

fn hashed_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(100, 200)
        .with_hash()
        .unwrap()
}

// == canonical.rs tests ==

#[test]
fn canonical_receipt_from_receipt() {
    let r = sample_receipt();
    let c = CanonicalReceipt::from_receipt(&r);
    assert_eq!(c.backend_id, "mock");
    assert_eq!(c.outcome, "Complete");
    assert_eq!(c.input_tokens, Some(100));
    assert_eq!(c.output_tokens, Some(200));
}

#[test]
fn canonical_receipt_strips_hash_field() {
    let mut r = sample_receipt();
    r.receipt_sha256 = Some("deadbeef".into());
    let c = CanonicalReceipt::from_receipt(&r);
    let json = serde_json::to_string(&c).unwrap();
    assert!(!json.contains("receipt_sha256"));
}

#[test]
fn canonicalize_deterministic() {
    let r = sample_receipt();
    let b1 = canonicalize(&r).unwrap();
    let b2 = canonicalize(&r).unwrap();
    assert_eq!(b1, b2);
}

#[test]
fn canonicalize_independent_of_hash() {
    let r1 = sample_receipt();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("anything".into());
    assert_eq!(canonicalize(&r1).unwrap(), canonicalize(&r2).unwrap());
}

#[test]
fn canonical_hash_length() {
    let r = sample_receipt();
    let h = canonical_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn canonical_hash_deterministic() {
    let r = sample_receipt();
    let h1 = canonical_hash(&r).unwrap();
    let h2 = canonical_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn canonical_hash_changes_with_outcome() {
    let r1 = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
    let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).error("boom").build();
    assert_ne!(canonical_hash(&r1).unwrap(), canonical_hash(&r2).unwrap());
}

#[test]
fn verify_with_correct_hash() {
    let r = sample_receipt();
    let hash = canonical_hash(&r).unwrap();
    assert!(verify(&r, &hash));
}

#[test]
fn verify_with_wrong_hash() {
    let r = sample_receipt();
    assert!(!verify(&r, "wrong_hash"));
}

#[test]
fn diff_canonical_identical() {
    let r = sample_receipt();
    let diffs = diff_canonical(&r, &r);
    assert!(diffs.is_empty());
}

#[test]
fn diff_canonical_detects_backend_change() {
    let a = ReceiptBuilder::new("alpha").outcome(Outcome::Complete).build();
    let b = ReceiptBuilder::new("beta").outcome(Outcome::Complete).build();
    let diffs = diff_canonical(&a, &b);
    assert!(diffs.iter().any(|d| d.field == "backend_id"));
}

#[test]
fn diff_canonical_detects_outcome_change() {
    let a = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
    let b = ReceiptBuilder::new("mock").outcome(Outcome::Failed).error("oops").build();
    let diffs = diff_canonical(&a, &b);
    assert!(diffs.iter().any(|d| d.field == "outcome"));
}

#[test]
fn diff_canonical_detects_token_change() {
    let a = ReceiptBuilder::new("mock").usage_tokens(10, 20).build();
    let b = ReceiptBuilder::new("mock").usage_tokens(10, 30).build();
    let diffs = diff_canonical(&a, &b);
    assert!(diffs.iter().any(|d| d.field == "output_tokens"));
}

// == audit_trail.rs tests ==

#[test]
fn audit_trail_new_is_empty() {
    let trail = AuditTrail::new();
    assert!(trail.is_empty());
    assert_eq!(trail.len(), 0);
}

#[test]
fn audit_trail_record_and_len() {
    let mut trail = AuditTrail::new();
    let id = Uuid::new_v4();
    trail.record(id, "system", AuditAction::Created);
    trail.record(id, "system", AuditAction::Hashed);
    assert_eq!(trail.len(), 2);
}

#[test]
fn audit_trail_entries_for_run() {
    let mut trail = AuditTrail::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    trail.record(id1, "sys", AuditAction::Created);
    trail.record(id2, "sys", AuditAction::Created);
    trail.record(id1, "sys", AuditAction::Hashed);
    assert_eq!(trail.entries_for_run(id1).len(), 2);
    assert_eq!(trail.entries_for_run(id2).len(), 1);
}

#[test]
fn audit_trail_entries_by_actor() {
    let mut trail = AuditTrail::new();
    let id = Uuid::new_v4();
    trail.record(id, "alice", AuditAction::Created);
    trail.record(id, "bob", AuditAction::Hashed);
    trail.record(id, "alice", AuditAction::Archived);
    assert_eq!(trail.entries_by_actor("alice").len(), 2);
    assert_eq!(trail.entries_by_actor("bob").len(), 1);
}

#[test]
fn audit_trail_record_with_details() {
    let mut trail = AuditTrail::new();
    let id = Uuid::new_v4();
    trail.record_with_details(id, "sys", AuditAction::Verified { success: true }, "all checks passed");
    let entry = &trail.entries()[0];
    assert_eq!(entry.details.as_deref(), Some("all checks passed"));
}

#[test]
fn audit_entry_new_sets_timestamp() {
    let id = Uuid::new_v4();
    let entry = AuditEntry::new(id, "test", AuditAction::Created);
    assert_eq!(entry.run_id, id);
    assert_eq!(entry.actor, "test");
    assert_eq!(entry.action, AuditAction::Created);
}

#[test]
fn audit_entry_with_details() {
    let id = Uuid::new_v4();
    let entry = AuditEntry::new(id, "test", AuditAction::Created)
        .with_details("initial creation");
    assert_eq!(entry.details.as_deref(), Some("initial creation"));
}

#[test]
fn audit_action_serde_roundtrip() {
    let actions = vec![
        AuditAction::Created,
        AuditAction::Hashed,
        AuditAction::Verified { success: true },
        AuditAction::Verified { success: false },
        AuditAction::Archived,
        AuditAction::Exported { format: "json".into() },
        AuditAction::Custom { description: "test".into() },
    ];
    for action in &actions {
        let json = serde_json::to_string(action).unwrap();
        let back: AuditAction = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, action);
    }
}

#[test]
fn audit_trail_serde_roundtrip() {
    let mut trail = AuditTrail::new();
    let id = Uuid::new_v4();
    trail.record(id, "sys", AuditAction::Created);
    trail.record(id, "sys", AuditAction::Hashed);

    let json = trail.to_json().unwrap();
    let restored = AuditTrail::from_json(&json).unwrap();
    assert_eq!(restored.len(), 2);
}

#[test]
fn audit_trail_custom_action() {
    let mut trail = AuditTrail::new();
    let id = Uuid::new_v4();
    trail.record(id, "plugin", AuditAction::Custom { description: "re-signed".into() });
    let e = &trail.entries()[0];
    assert!(matches!(&e.action, AuditAction::Custom { description } if description == "re-signed"));
}

// == export.rs tests ==

#[test]
fn export_to_json_valid() {
    let receipts = vec![sample_receipt(), sample_receipt()];
    let json = export::to_json(&receipts).unwrap();
    let parsed: Vec<Receipt> = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.len(), 2);
}

#[test]
fn export_to_json_empty() {
    let json = export::to_json(&[]).unwrap();
    assert_eq!(json.trim(), "[]");
}

#[test]
fn export_to_jsonl_line_count() {
    let receipts = vec![sample_receipt(), sample_receipt(), sample_receipt()];
    let jsonl = export::to_jsonl(&receipts).unwrap();
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), 3);
}

#[test]
fn export_to_jsonl_each_line_is_valid_json() {
    let receipts = vec![sample_receipt()];
    let jsonl = export::to_jsonl(&receipts).unwrap();
    for line in jsonl.lines() {
        let _: Receipt = serde_json::from_str(line).unwrap();
    }
}

#[test]
fn export_to_jsonl_empty() {
    let jsonl = export::to_jsonl(&[]).unwrap();
    assert!(jsonl.is_empty());
}

#[test]
fn export_to_csv_header() {
    let csv = export::to_csv(&[sample_receipt()]);
    let first_line = csv.lines().next().unwrap();
    assert!(first_line.starts_with("run_id,"));
    assert!(first_line.contains("outcome"));
    assert!(first_line.contains("hash"));
}

#[test]
fn export_to_csv_row_count() {
    let receipts = vec![sample_receipt(), sample_receipt()];
    let csv = export::to_csv(&receipts);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3);
}

#[test]
fn export_to_csv_empty() {
    let csv = export::to_csv(&[]);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 1);
}

#[test]
fn export_to_summary_table_header() {
    let table = export::to_summary_table(&[sample_receipt()]);
    assert!(table.contains("RUN_ID"));
    assert!(table.contains("OUTCOME"));
    assert!(table.contains("BACKEND"));
}

#[test]
fn export_to_summary_table_footer() {
    let receipts = vec![
        ReceiptBuilder::new("a").outcome(Outcome::Complete).build(),
        ReceiptBuilder::new("b").outcome(Outcome::Failed).error("err").build(),
    ];
    let table = export::to_summary_table(&receipts);
    assert!(table.contains("2 receipts, 1 complete"));
}

#[test]
fn export_to_summary_table_empty() {
    let table = export::to_summary_table(&[]);
    assert!(table.contains("0 receipts, 0 complete"));
}

#[test]
fn export_to_csv_includes_hash() {
    let r = hashed_receipt();
    let csv = export::to_csv(&[r]);
    let data_line = csv.lines().nth(1).unwrap();
    let fields: Vec<&str> = data_line.split(',').collect();
    assert_eq!(fields.last().unwrap().len(), 64);
}

// == compliance.rs tests ==

#[test]
fn compliance_valid_receipt() {
    let r = hashed_receipt();
    let report = ComplianceCheck::new().check(&r);
    assert!(report.is_compliant());
}

#[test]
fn compliance_missing_hash_is_warning() {
    let r = sample_receipt();
    let report = ComplianceCheck::new().check(&r);
    assert!(report.is_compliant());
    assert!(report.warnings().iter().any(|f| f.field == "receipt_sha256"));
}

#[test]
fn compliance_tampered_hash_is_error() {
    let mut r = hashed_receipt();
    r.receipt_sha256 = Some("tampered".into());
    let report = ComplianceCheck::new().check(&r);
    assert!(!report.is_compliant());
    assert!(report.errors().iter().any(|f| f.field == "receipt_sha256"));
}

#[test]
fn compliance_wrong_contract_version() {
    let mut r = hashed_receipt();
    r.meta.contract_version = "wrong/v99".into();
    r.receipt_sha256 = Some(abp_receipt::compute_hash(&r).unwrap());
    let report = ComplianceCheck::new().check(&r);
    assert!(!report.is_compliant());
    assert!(report.errors().iter().any(|f| f.field == "meta.contract_version"));
}

#[test]
fn compliance_empty_backend_id() {
    let r = ReceiptBuilder::new("").outcome(Outcome::Complete).with_hash().unwrap();
    let report = ComplianceCheck::new().check(&r);
    assert!(!report.is_compliant());
    assert!(report.errors().iter().any(|f| f.field == "backend.id"));
}

#[test]
fn compliance_timestamps_inverted() {
    let now = chrono::Utc::now();
    let earlier = now - chrono::Duration::seconds(10);
    let mut r = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(earlier)
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some(abp_receipt::compute_hash(&r).unwrap());
    let report = ComplianceCheck::new().check(&r);
    assert!(!report.is_compliant());
}

#[test]
fn compliance_excessive_duration_warning() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .duration(Duration::from_secs(7200))
        .with_hash()
        .unwrap();
    let report = ComplianceCheck::new()
        .max_duration_ms(3_600_000)
        .check(&r);
    assert!(report.is_compliant());
    assert!(report.warnings().iter().any(|f| f.field == "meta.duration_ms"));
}

#[test]
fn compliance_check_batch() {
    let receipts = vec![hashed_receipt(), hashed_receipt()];
    let reports = ComplianceCheck::new().check_batch(&receipts);
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.is_compliant()));
}

#[test]
fn compliance_report_len_and_is_empty() {
    let r = hashed_receipt();
    let report = ComplianceCheck::new().check(&r);
    assert_eq!(report.len(), report.findings.len());
}

#[test]
fn compliance_custom_max_duration() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .duration(Duration::from_millis(500))
        .with_hash()
        .unwrap();
    let report = ComplianceCheck::new()
        .max_duration_ms(100)
        .check(&r);
    assert!(report.warnings().iter().any(|f| f.message.contains("exceeds threshold")));
}

#[test]
fn compliance_severity_levels() {
    assert_ne!(Severity::Info, Severity::Warning);
    assert_ne!(Severity::Warning, Severity::Error);
    assert_ne!(Severity::Info, Severity::Error);
}
