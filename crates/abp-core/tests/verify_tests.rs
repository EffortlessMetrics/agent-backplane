// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for the `verify` module.

use abp_core::verify::{ChainVerifier, ReceiptVerifier};
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, CONTRACT_VERSION,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

fn valid_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

fn valid_receipt_at(started: chrono::DateTime<chrono::Utc>) -> Receipt {
    let finished = started + Duration::seconds(1);
    ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .started_at(started)
        .finished_at(finished)
        .with_hash()
        .unwrap()
}

// ---------------------------------------------------------------------------
// ReceiptVerifier – individual receipt checks
// ---------------------------------------------------------------------------

#[test]
fn valid_receipt_passes_all_checks() {
    let r = valid_receipt();
    let report = ReceiptVerifier::new().verify(&r);
    assert!(report.passed, "checks failed: {:#?}", report.checks);
    assert_eq!(report.receipt_id, r.meta.run_id.to_string());
}

#[test]
fn hash_mismatch_detected() {
    let mut r = valid_receipt();
    r.receipt_sha256 = Some("bad_hash".into());
    let report = ReceiptVerifier::new().verify(&r);
    assert!(!report.passed);
    let check = report.checks.iter().find(|c| c.name == "hash_integrity").unwrap();
    assert!(!check.passed);
    assert!(check.detail.contains("bad_hash"));
}

#[test]
fn missing_hash_passes() {
    let r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .build();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "hash_integrity").unwrap();
    assert!(check.passed);
}

#[test]
fn empty_contract_version_fails() {
    let mut r = valid_receipt();
    r.meta.contract_version = String::new();
    // Recompute hash after mutation so hash check passes.
    r = r.with_hash().unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "contract_version").unwrap();
    assert!(!check.passed);
}

#[test]
fn invalid_contract_version_format_fails() {
    let mut r = valid_receipt();
    r.meta.contract_version = "bad".into();
    r = r.with_hash().unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "contract_version").unwrap();
    assert!(!check.passed);
    assert!(check.detail.contains("invalid format"));
}

#[test]
fn valid_contract_version_passes() {
    let r = valid_receipt();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "contract_version").unwrap();
    assert!(check.passed);
    assert!(check.detail.contains(CONTRACT_VERSION));
}

#[test]
fn nil_work_order_id_fails() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.meta.work_order_id = Uuid::nil();
    r = r.with_hash().unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "work_order_id").unwrap();
    assert!(!check.passed);
}

#[test]
fn valid_work_order_id_passes() {
    let r = valid_receipt();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "work_order_id").unwrap();
    assert!(check.passed);
}

#[test]
fn nil_run_id_fails() {
    let mut r = valid_receipt();
    r.meta.run_id = Uuid::nil();
    r = r.with_hash().unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "run_id").unwrap();
    assert!(!check.passed);
}

#[test]
fn valid_run_id_passes() {
    let r = valid_receipt();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "run_id").unwrap();
    assert!(check.passed);
}

#[test]
fn outcome_always_recognized() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let r = ReceiptBuilder::new("mock")
            .work_order_id(Uuid::new_v4())
            .outcome(outcome)
            .with_hash()
            .unwrap();
        let report = ReceiptVerifier::new().verify(&r);
        let check = report.checks.iter().find(|c| c.name == "outcome").unwrap();
        assert!(check.passed);
    }
}

#[test]
fn empty_backend_fails() {
    let mut r = ReceiptBuilder::new("")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .build();
    r = r.with_hash().unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "backend").unwrap();
    assert!(!check.passed);
}

#[test]
fn present_backend_passes() {
    let r = valid_receipt();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "backend").unwrap();
    assert!(check.passed);
}

#[test]
fn timestamps_started_after_finished_fails() {
    let now = Utc::now();
    let mut r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .started_at(now + Duration::seconds(10))
        .finished_at(now)
        .outcome(Outcome::Complete)
        .build();
    r = r.with_hash().unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "timestamps").unwrap();
    assert!(!check.passed);
}

#[test]
fn timestamps_equal_passes() {
    let now = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .started_at(now)
        .finished_at(now)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "timestamps").unwrap();
    assert!(check.passed);
}

#[test]
fn trace_order_sequential_passes() {
    let t1 = Utc::now();
    let t2 = t1 + Duration::seconds(1);
    let r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: t1,
            kind: AgentEventKind::RunStarted { message: "start".into() },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: t2,
            kind: AgentEventKind::RunCompleted { message: "done".into() },
            ext: None,
        })
        .with_hash()
        .unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "trace_order").unwrap();
    assert!(check.passed);
}

#[test]
fn trace_order_out_of_order_fails() {
    let t1 = Utc::now();
    let t2 = t1 - Duration::seconds(5);
    let mut r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: t1,
            kind: AgentEventKind::RunStarted { message: "start".into() },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: t2,
            kind: AgentEventKind::RunCompleted { message: "done".into() },
            ext: None,
        })
        .build();
    r = r.with_hash().unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "trace_order").unwrap();
    assert!(!check.passed);
}

#[test]
fn trace_empty_passes_order_check() {
    let r = valid_receipt();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "trace_order").unwrap();
    assert!(check.passed);
}

#[test]
fn trace_duplicate_tool_use_ids_fails() {
    let t = Utc::now();
    let mut r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: t,
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("id-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: t + Duration::seconds(1),
            kind: AgentEventKind::ToolCall {
                tool_name: "write".into(),
                tool_use_id: Some("id-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            ext: None,
        })
        .build();
    r = r.with_hash().unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "trace_no_duplicate_ids").unwrap();
    assert!(!check.passed);
    assert!(check.detail.contains("id-1"));
}

#[test]
fn trace_unique_tool_use_ids_passes() {
    let t = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: t,
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("id-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: t + Duration::seconds(1),
            kind: AgentEventKind::ToolCall {
                tool_name: "write".into(),
                tool_use_id: Some("id-2".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    let report = ReceiptVerifier::new().verify(&r);
    let check = report.checks.iter().find(|c| c.name == "trace_no_duplicate_ids").unwrap();
    assert!(check.passed);
}

// ---------------------------------------------------------------------------
// ChainVerifier – chain-level checks
// ---------------------------------------------------------------------------

#[test]
fn empty_chain_passes() {
    let report = ChainVerifier::verify_chain(&[]);
    assert!(report.all_valid);
    assert_eq!(report.receipt_count, 0);
}

#[test]
fn single_receipt_chain_passes() {
    let r = valid_receipt();
    let report = ChainVerifier::verify_chain(&[r]);
    assert!(report.all_valid);
    assert_eq!(report.receipt_count, 1);
}

#[test]
fn chain_ordered_by_timestamp_passes() {
    let t1 = Utc::now();
    let t2 = t1 + Duration::seconds(10);
    let r1 = valid_receipt_at(t1);
    let r2 = valid_receipt_at(t2);
    let report = ChainVerifier::verify_chain(&[r1, r2]);
    let check = report.chain_checks.iter().find(|c| c.name == "chain_order").unwrap();
    assert!(check.passed);
}

#[test]
fn chain_out_of_order_fails() {
    let t1 = Utc::now();
    let t2 = t1 + Duration::seconds(10);
    let r1 = valid_receipt_at(t1);
    let r2 = valid_receipt_at(t2);
    let report = ChainVerifier::verify_chain(&[r2, r1]);
    let check = report.chain_checks.iter().find(|c| c.name == "chain_order").unwrap();
    assert!(!check.passed);
    assert!(!report.all_valid);
}

#[test]
fn chain_duplicate_run_ids_fails() {
    let mut r1 = valid_receipt();
    let mut r2 = valid_receipt();
    let shared_id = Uuid::new_v4();
    r1.meta.run_id = shared_id;
    r1 = r1.with_hash().unwrap();
    r2.meta.run_id = shared_id;
    r2 = r2.with_hash().unwrap();
    let report = ChainVerifier::verify_chain(&[r1, r2]);
    let check = report.chain_checks.iter().find(|c| c.name == "no_duplicate_run_ids").unwrap();
    assert!(!check.passed);
}

#[test]
fn chain_unique_run_ids_passes() {
    let r1 = valid_receipt();
    let r2 = valid_receipt();
    let report = ChainVerifier::verify_chain(&[r1, r2]);
    let check = report.chain_checks.iter().find(|c| c.name == "no_duplicate_run_ids").unwrap();
    assert!(check.passed);
}

#[test]
fn chain_consistent_version_passes() {
    let r1 = valid_receipt();
    let r2 = valid_receipt();
    let report = ChainVerifier::verify_chain(&[r1, r2]);
    let check = report
        .chain_checks
        .iter()
        .find(|c| c.name == "consistent_contract_version")
        .unwrap();
    assert!(check.passed);
}

#[test]
fn chain_inconsistent_version_fails() {
    let r1 = valid_receipt();
    let mut r2 = valid_receipt();
    r2.meta.contract_version = "abp/v0.2".into();
    r2 = r2.with_hash().unwrap();
    let report = ChainVerifier::verify_chain(&[r1, r2]);
    let check = report
        .chain_checks
        .iter()
        .find(|c| c.name == "consistent_contract_version")
        .unwrap();
    assert!(!check.passed);
}

#[test]
fn chain_invalid_receipt_makes_all_valid_false() {
    let mut r1 = valid_receipt();
    r1.receipt_sha256 = Some("tampered".into());
    let r2 = valid_receipt();
    let report = ChainVerifier::verify_chain(&[r1, r2]);
    assert!(!report.all_valid);
    assert!(!report.individual_reports[0].passed);
    assert!(report.individual_reports[1].passed);
}

#[test]
fn report_check_count() {
    let r = valid_receipt();
    let report = ReceiptVerifier::new().verify(&r);
    // We define 9 checks.
    assert_eq!(report.checks.len(), 9);
}

#[test]
fn chain_report_has_individual_reports() {
    let r1 = valid_receipt();
    let r2 = valid_receipt();
    let report = ChainVerifier::verify_chain(&[r1, r2]);
    assert_eq!(report.individual_reports.len(), 2);
    assert_eq!(report.chain_checks.len(), 3);
}
