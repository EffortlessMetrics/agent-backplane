#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep comprehensive tests for abp-validate.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ContextSnippet, ExecutionMode,
    Outcome, ReceiptBuilder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::Envelope;
use abp_validate::{
    validate_hello_version, EnvelopeValidator, EventValidator, JsonType, RawEnvelopeValidator,
    ReceiptValidator, SchemaValidator, ValidationErrorKind, ValidationErrors, Validator,
    WorkOrderValidator,
};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_at(kind: AgentEventKind, ts: chrono::DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn valid_event_sequence() -> Vec<AgentEvent> {
    let now = Utc::now();
    vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(1),
        ),
    ]
}

fn backend_id(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: None,
        adapter_version: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. WorkOrder Validation – Required Fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_valid_minimal_passes() {
    let wo = WorkOrderBuilder::new("fix bug").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_empty_task_required_error() {
    let wo = WorkOrderBuilder::new("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "task" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn wo_whitespace_task_required_error() {
    let wo = WorkOrderBuilder::new("   \t  ").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "task"));
}

#[test]
fn wo_newline_only_task_required_error() {
    let wo = WorkOrderBuilder::new("\n\n").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "task"));
}

#[test]
fn wo_empty_workspace_root_required_error() {
    let wo = WorkOrderBuilder::new("task").root("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "workspace.root" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn wo_whitespace_workspace_root_required_error() {
    let wo = WorkOrderBuilder::new("task").root("  ").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "workspace.root"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. WorkOrder Validation – Budget Range
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_negative_budget_out_of_range() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(-0.01).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "config.max_budget_usd" && e.kind == ValidationErrorKind::OutOfRange));
}

#[test]
fn wo_large_negative_budget_fails() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(-1000.0)
        .build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "config.max_budget_usd"));
}

#[test]
fn wo_nan_budget_invalid_format() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::NAN)
        .build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "config.max_budget_usd"
                && e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn wo_zero_budget_passes() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(0.0).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_positive_budget_passes() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(100.0).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_tiny_positive_budget_passes() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(0.001).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_infinity_budget_passes() {
    // infinity is not negative and not NaN so should pass
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::INFINITY)
        .build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_neg_infinity_budget_fails() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::NEG_INFINITY)
        .build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "config.max_budget_usd"));
}

#[test]
fn wo_no_budget_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.max_budget_usd.is_none());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. WorkOrder Validation – Max Turns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_zero_max_turns_out_of_range() {
    let wo = WorkOrderBuilder::new("task").max_turns(0).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "config.max_turns" && e.kind == ValidationErrorKind::OutOfRange));
}

#[test]
fn wo_one_max_turns_passes() {
    let wo = WorkOrderBuilder::new("task").max_turns(1).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_large_max_turns_passes() {
    let wo = WorkOrderBuilder::new("task").max_turns(10000).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_no_max_turns_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.max_turns.is_none());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. WorkOrder Validation – Context Snippets
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_empty_snippet_name_required_error() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "some content".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "context.snippets[0].name"));
}

#[test]
fn wo_whitespace_snippet_name_required_error() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "  ".into(),
        content: "content".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("snippets[0].name")));
}

#[test]
fn wo_valid_snippet_name_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "readme".into(),
        content: "# Hello".into(),
    });
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_multiple_empty_snippet_names_report_each() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "a".into(),
    });
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "b".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "context.snippets[0].name"));
    assert!(err.iter().any(|e| e.path == "context.snippets[1].name"));
}

#[test]
fn wo_mixed_valid_and_empty_snippet_names() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "good".into(),
        content: "a".into(),
    });
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "b".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_eq!(err.len(), 1);
    assert!(err.iter().any(|e| e.path == "context.snippets[1].name"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. WorkOrder Validation – Policy Conflicts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_conflicting_tool_policy_invalid_reference() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("bash".into());
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "policy" && e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn wo_multiple_conflicting_tools_report_each() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.allowed_tools.push("python".into());
    wo.policy.disallowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("python".into());
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.len() >= 2);
}

#[test]
fn wo_non_overlapping_policy_tools_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("python".into());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_empty_policy_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. WorkOrder Validation – Error Accumulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_multiple_errors_accumulated() {
    let mut wo = WorkOrderBuilder::new("")
        .root("")
        .max_budget_usd(-5.0)
        .build();
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "x".into(),
    });
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("bash".into());
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    // task + workspace.root + budget + snippet + policy
    assert!(err.len() >= 5);
}

#[test]
fn wo_all_valid_fields_combined() {
    let mut wo = WorkOrderBuilder::new("refactor code")
        .root("/home/user/project")
        .max_budget_usd(50.0)
        .max_turns(10)
        .model("gpt-4")
        .build();
    wo.context.snippets.push(ContextSnippet {
        name: "readme".into(),
        content: "# Project".into(),
    });
    wo.context.files.push("src/main.rs".into());
    wo.policy.allowed_tools.push("bash".into());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Receipt Validation – Required Fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_valid_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_empty_backend_id_required() {
    let receipt = ReceiptBuilder::new("").build();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "backend.id" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn receipt_whitespace_backend_id_required() {
    let receipt = ReceiptBuilder::new("  ").build();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "backend.id"));
}

#[test]
fn receipt_empty_contract_version_required() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "".into();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "meta.contract_version" && e.kind == ValidationErrorKind::Required));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Receipt Validation – Contract Version Format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_invalid_contract_version_format() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "v0.1".into();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "meta.contract_version"
                && e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn receipt_random_contract_version_format_error() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "random_string".into();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "meta.contract_version"));
}

#[test]
fn receipt_valid_contract_version_passes() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert!(receipt.meta.contract_version.starts_with("abp/v"));
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Receipt Validation – Hash Integrity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_correct_hash_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_incorrect_hash_invalid_reference() {
    let mut receipt = ReceiptBuilder::new("mock").with_hash().unwrap();
    receipt.receipt_sha256 = Some("a".repeat(64));
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "receipt_sha256" && e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn receipt_short_hash_invalid_format() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.receipt_sha256 = Some("abc123".into());
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "receipt_sha256" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn receipt_long_hash_invalid_format() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.receipt_sha256 = Some("a".repeat(65));
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "receipt_sha256" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn receipt_empty_hash_invalid_format() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.receipt_sha256 = Some("".into());
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "receipt_sha256"));
}

#[test]
fn receipt_no_hash_passes() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert!(receipt.receipt_sha256.is_none());
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_hash_63_chars_invalid_format() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.receipt_sha256 = Some("a".repeat(63));
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "receipt_sha256"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Receipt Validation – Timestamps
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_finished_before_started_out_of_range() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let receipt = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(earlier)
        .build();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "meta.finished_at" && e.kind == ValidationErrorKind::OutOfRange));
}

#[test]
fn receipt_same_start_finish_passes() {
    let now = Utc::now();
    let receipt = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(now)
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_finished_after_started_passes() {
    let now = Utc::now();
    let later = now + chrono::Duration::seconds(30);
    let receipt = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(later)
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Receipt Validation – Cross-field: outcome + harness_ok
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_failed_harness_ok_invalid_reference() {
    let mut receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    receipt.verification.harness_ok = true;
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "verification.harness_ok"
            && e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn receipt_failed_harness_not_ok_passes() {
    let mut receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    receipt.verification.harness_ok = false;
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_complete_harness_ok_passes() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    receipt.verification.harness_ok = true;
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_partial_harness_ok_passes() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    receipt.verification.harness_ok = true;
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_partial_harness_not_ok_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Receipt Validation – Error Accumulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_multiple_errors_accumulated() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let mut receipt = ReceiptBuilder::new("")
        .started_at(now)
        .finished_at(earlier)
        .outcome(Outcome::Failed)
        .build();
    receipt.meta.contract_version = "bad".into();
    receipt.verification.harness_ok = true;
    receipt.receipt_sha256 = Some("short".into());
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    // backend.id + contract_version (required still passes since non-empty) + contract_version format
    // + finished_at + harness_ok + receipt_sha256
    assert!(err.len() >= 5);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Event Validation – Empty / Minimal
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn events_empty_passes() {
    let events: Vec<AgentEvent> = vec![];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_single_run_started_passes() {
    let events = vec![make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    })];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_valid_full_sequence_passes() {
    assert!(EventValidator.validate(&valid_event_sequence()).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Event Validation – Timestamp Monotonicity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn events_non_monotonic_ts_fails() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::AssistantMessage { text: "hi".into() },
            earlier,
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "events[1].ts" && e.kind == ValidationErrorKind::OutOfRange));
}

#[test]
fn events_equal_timestamps_passes() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now,
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_multiple_decreasing_timestamps_report_each() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::AssistantMessage { text: "a".into() },
            now - chrono::Duration::seconds(1),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now - chrono::Duration::seconds(2),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err.iter().any(|e| e.path == "events[1].ts"));
    assert!(err.iter().any(|e| e.path == "events[2].ts"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Event Validation – Bookend Events
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn events_first_not_run_started_fails() {
    let events = vec![make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    })];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "events[0].kind" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn events_last_not_run_completed_fails() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::AssistantMessage { text: "hi".into() },
            now + chrono::Duration::milliseconds(1),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("kind")));
}

#[test]
fn events_single_non_started_event_fails() {
    let events = vec![make_event(AgentEventKind::Warning {
        message: "warn".into(),
    })];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err.iter().any(|e| e.path == "events[0].kind"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Event Validation – Tool Call/Result Pairing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn events_tool_result_without_call_invalid_reference() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(1),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(2),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn events_tool_call_then_result_passes() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({"cmd": "ls"}),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(2),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(3),
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_multiple_tool_calls_and_results() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        make_event_at(
            AgentEventKind::ToolCall {
                tool_name: "python".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            now + chrono::Duration::milliseconds(2),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(3),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "python".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(4),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(5),
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_mismatched_tool_result_name_fails() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "python".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(2),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(3),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn events_duplicate_tool_calls_same_name() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        make_event_at(
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            now + chrono::Duration::milliseconds(2),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("1"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(3),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("2"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(4),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(5),
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Event Validation – Multiple Errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn events_multiple_errors_accumulated() {
    let now = Utc::now();
    let events = vec![
        make_event_at(AgentEventKind::AssistantMessage { text: "hi".into() }, now),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "x".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            now - chrono::Duration::hours(1),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    // first event not RunStarted + last event not RunCompleted + timestamp + orphan tool_result
    assert!(err.len() >= 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Envelope Validation – Hello
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_valid_hello_passes() {
    let env = Envelope::hello(backend_id("test"), CapabilityManifest::new());
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_hello_empty_backend_id_fails() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: backend_id(""),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "backend.id"));
}

#[test]
fn envelope_hello_empty_contract_version_fails() {
    let env = Envelope::Hello {
        contract_version: "".into(),
        backend: backend_id("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "contract_version" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn envelope_hello_invalid_contract_version_format() {
    let env = Envelope::Hello {
        contract_version: "v1.0".into(),
        backend: backend_id("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "contract_version" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn envelope_hello_whitespace_backend_fails() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: backend_id("  "),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "backend.id"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Envelope Validation – Run
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_run_valid_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_run_empty_id_fails() {
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "".into(),
        work_order: wo,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "id" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn envelope_run_empty_task_fails() {
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "work_order.task"));
}

#[test]
fn envelope_run_both_empty_fails_multiple() {
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: "".into(),
        work_order: wo,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Envelope Validation – Event / Final / Fatal
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_event_valid_passes() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_event_empty_ref_id_fails() {
    let env = Envelope::Event {
        ref_id: "".into(),
        event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "ref_id"));
}

#[test]
fn envelope_final_valid_passes() {
    let receipt = ReceiptBuilder::new("mock").build();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_final_empty_ref_id_fails() {
    let receipt = ReceiptBuilder::new("mock").build();
    let env = Envelope::Final {
        ref_id: "".into(),
        receipt,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "ref_id"));
}

#[test]
fn envelope_fatal_valid_passes() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something broke".into(),
        error_code: None,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_fatal_empty_error_fails() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "".into(),
        error_code: None,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "error" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn envelope_fatal_whitespace_error_fails() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "   ".into(),
        error_code: None,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "error"));
}

#[test]
fn envelope_fatal_no_ref_id_passes() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "oops".into(),
        error_code: None,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. RawEnvelopeValidator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn raw_envelope_not_object_fails() {
    let val = serde_json::json!("just a string");
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn raw_envelope_array_fails() {
    let val = serde_json::json!([1, 2, 3]);
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn raw_envelope_null_fails() {
    let val = serde_json::Value::Null;
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn raw_envelope_number_fails() {
    let val = serde_json::json!(42);
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn raw_envelope_missing_tag_required() {
    let val = serde_json::json!({"ref_id": "run-1"});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "t" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn raw_envelope_unknown_tag_invalid_format() {
    let val = serde_json::json!({"t": "bogus"});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "t" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn raw_envelope_tag_not_string_fails() {
    let val = serde_json::json!({"t": 42});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "t" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn raw_envelope_tag_bool_fails() {
    let val = serde_json::json!({"t": true});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "t"));
}

#[test]
fn raw_envelope_tag_null_fails() {
    let val = serde_json::json!({"t": null});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "t"));
}

#[test]
fn raw_hello_missing_contract_version_required() {
    let val = serde_json::json!({"t": "hello", "backend": {"id": "x"}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "contract_version" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn raw_hello_with_contract_version_passes() {
    let val =
        serde_json::json!({"t": "hello", "contract_version": "abp/v0.1", "backend": {"id": "x"}});
    assert!(RawEnvelopeValidator.validate(&val).is_ok());
}

#[test]
fn raw_run_missing_id_required() {
    let val = serde_json::json!({"t": "run", "work_order": {}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "id" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn raw_run_with_id_passes() {
    let val = serde_json::json!({"t": "run", "id": "run-1", "work_order": {}});
    assert!(RawEnvelopeValidator.validate(&val).is_ok());
}

#[test]
fn raw_event_missing_ref_id_required() {
    let val = serde_json::json!({"t": "event", "event": {}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "ref_id" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn raw_event_with_ref_id_passes() {
    let val = serde_json::json!({"t": "event", "ref_id": "run-1", "event": {}});
    assert!(RawEnvelopeValidator.validate(&val).is_ok());
}

#[test]
fn raw_final_missing_ref_id_required() {
    let val = serde_json::json!({"t": "final", "receipt": {}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "ref_id"));
}

#[test]
fn raw_final_with_ref_id_passes() {
    let val = serde_json::json!({"t": "final", "ref_id": "run-1", "receipt": {}});
    assert!(RawEnvelopeValidator.validate(&val).is_ok());
}

#[test]
fn raw_fatal_valid_passes() {
    let val = serde_json::json!({"t": "fatal", "error": "oops"});
    assert!(RawEnvelopeValidator.validate(&val).is_ok());
}

#[test]
fn raw_valid_all_tags() {
    for tag in &["hello", "run", "event", "final", "fatal"] {
        let mut val = serde_json::json!({"t": tag});
        // Add required fields for each tag
        match *tag {
            "hello" => {
                val.as_object_mut()
                    .unwrap()
                    .insert("contract_version".into(), serde_json::json!("abp/v0.1"));
            }
            "run" => {
                val.as_object_mut()
                    .unwrap()
                    .insert("id".into(), serde_json::json!("r1"));
            }
            "event" | "final" => {
                val.as_object_mut()
                    .unwrap()
                    .insert("ref_id".into(), serde_json::json!("r1"));
            }
            _ => {}
        }
        assert!(
            RawEnvelopeValidator.validate(&val).is_ok(),
            "tag '{tag}' should pass"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. validate_hello_version
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_version_exact_match_passes() {
    let env = Envelope::hello(backend_id("test"), CapabilityManifest::new());
    assert!(validate_hello_version(&env).is_ok());
}

#[test]
fn hello_version_incompatible_major_fails() {
    let env = Envelope::Hello {
        contract_version: "abp/v9.0".into(),
        backend: backend_id("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = validate_hello_version(&env).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn hello_version_same_major_different_minor_passes() {
    // Current version is "abp/v0.1", so "abp/v0.2" has same major 0
    let env = Envelope::Hello {
        contract_version: "abp/v0.2".into(),
        backend: backend_id("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    assert!(validate_hello_version(&env).is_ok());
}

#[test]
fn hello_version_non_envelope_passes() {
    // validate_hello_version on a non-hello envelope just passes
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    assert!(validate_hello_version(&env).is_ok());
}

#[test]
fn hello_version_non_abp_prefix_fails() {
    let env = Envelope::Hello {
        contract_version: "xyz/v0.1".into(),
        backend: backend_id("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    // "xyz/v0.1" doesn't strip "abp/v", so theirs is None while ours is Some("0")
    let result = validate_hello_version(&env);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. SchemaValidator – Work Order
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn schema_wo_valid_json_passes() {
    let wo = WorkOrderBuilder::new("do stuff").build();
    let val = serde_json::to_value(&wo).unwrap();
    assert!(SchemaValidator::work_order().validate(&val).is_ok());
}

#[test]
fn schema_wo_empty_object_fails_all_fields() {
    let val = serde_json::json!({});
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    // id, task, lane, workspace, context, policy, config
    assert!(err.len() >= 7);
}

#[test]
fn schema_wo_null_field_fails() {
    let val = serde_json::json!({
        "id": "x", "task": null, "lane": "patch_first",
        "workspace": {}, "context": {}, "policy": {}, "config": {},
    });
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "task" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn schema_wo_wrong_type_number_for_string() {
    let val = serde_json::json!({
        "id": "x", "task": 42, "lane": "patch_first",
        "workspace": {}, "context": {}, "policy": {}, "config": {},
    });
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "task" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn schema_wo_wrong_type_string_for_object() {
    let val = serde_json::json!({
        "id": "x", "task": "t", "lane": "patch_first",
        "workspace": "not-an-object", "context": {}, "policy": {}, "config": {},
    });
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "workspace" && e.kind == ValidationErrorKind::InvalidFormat));
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. SchemaValidator – Receipt
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn schema_receipt_valid_json_passes() {
    let receipt = ReceiptBuilder::new("mock").build();
    let val = serde_json::to_value(&receipt).unwrap();
    assert!(SchemaValidator::receipt().validate(&val).is_ok());
}

#[test]
fn schema_receipt_missing_meta_fails() {
    let val = serde_json::json!({
        "backend": {"id": "mock"}, "outcome": "complete", "trace": [], "artifacts": [],
    });
    let err = SchemaValidator::receipt().validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "meta" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn schema_receipt_missing_backend_fails() {
    let val = serde_json::json!({
        "meta": {}, "outcome": "complete", "trace": [], "artifacts": [],
    });
    let err = SchemaValidator::receipt().validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "backend"));
}

#[test]
fn schema_receipt_wrong_trace_type_fails() {
    let val = serde_json::json!({
        "meta": {}, "backend": {}, "outcome": "complete", "trace": "not-array", "artifacts": [],
    });
    let err = SchemaValidator::receipt().validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "trace" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn schema_receipt_empty_object_fails_all() {
    let val = serde_json::json!({});
    let err = SchemaValidator::receipt().validate(&val).unwrap_err();
    // meta, backend, outcome, trace, artifacts
    assert!(err.len() >= 5);
}

// ═══════════════════════════════════════════════════════════════════════════
// 25. SchemaValidator – Agent Event
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn schema_event_valid_json_passes() {
    let event = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let val = serde_json::to_value(&event).unwrap();
    assert!(SchemaValidator::agent_event().validate(&val).is_ok());
}

#[test]
fn schema_event_missing_type_fails() {
    let val = serde_json::json!({"ts": "2024-01-01T00:00:00Z"});
    let err = SchemaValidator::agent_event().validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "type"));
}

#[test]
fn schema_event_missing_ts_fails() {
    let val = serde_json::json!({"type": "run_started"});
    let err = SchemaValidator::agent_event().validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "ts"));
}

#[test]
fn schema_event_empty_object_fails() {
    let val = serde_json::json!({});
    let err = SchemaValidator::agent_event().validate(&val).unwrap_err();
    assert!(err.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 26. SchemaValidator – Custom Fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn schema_custom_string_type_passes() {
    let v = SchemaValidator::new(vec![("name".into(), JsonType::String)]);
    let val = serde_json::json!({"name": "test"});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_number_type_passes() {
    let v = SchemaValidator::new(vec![("count".into(), JsonType::Number)]);
    let val = serde_json::json!({"count": 42});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_bool_type_passes() {
    let v = SchemaValidator::new(vec![("active".into(), JsonType::Bool)]);
    let val = serde_json::json!({"active": true});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_object_type_passes() {
    let v = SchemaValidator::new(vec![("data".into(), JsonType::Object)]);
    let val = serde_json::json!({"data": {"nested": true}});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_array_type_passes() {
    let v = SchemaValidator::new(vec![("items".into(), JsonType::Array)]);
    let val = serde_json::json!({"items": [1, 2, 3]});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_any_type_passes_string() {
    let v = SchemaValidator::new(vec![("data".into(), JsonType::Any)]);
    let val = serde_json::json!({"data": "hello"});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_any_type_passes_array() {
    let v = SchemaValidator::new(vec![("data".into(), JsonType::Any)]);
    let val = serde_json::json!({"data": [1, 2]});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_any_type_passes_number() {
    let v = SchemaValidator::new(vec![("data".into(), JsonType::Any)]);
    let val = serde_json::json!({"data": 99});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_wrong_type_string_for_number() {
    let v = SchemaValidator::new(vec![("count".into(), JsonType::Number)]);
    let val = serde_json::json!({"count": "not-a-number"});
    let err = v.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "count" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn schema_custom_wrong_type_number_for_bool() {
    let v = SchemaValidator::new(vec![("active".into(), JsonType::Bool)]);
    let val = serde_json::json!({"active": 1});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "active"));
}

#[test]
fn schema_custom_wrong_type_string_for_object() {
    let v = SchemaValidator::new(vec![("data".into(), JsonType::Object)]);
    let val = serde_json::json!({"data": "not-object"});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "data"));
}

#[test]
fn schema_custom_wrong_type_string_for_array() {
    let v = SchemaValidator::new(vec![("items".into(), JsonType::Array)]);
    let val = serde_json::json!({"items": "not-array"});
    let err = v.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "items"));
}

#[test]
fn schema_custom_missing_field() {
    let v = SchemaValidator::new(vec![("required_field".into(), JsonType::String)]);
    let val = serde_json::json!({});
    let err = v.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "required_field" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn schema_custom_null_field_required() {
    let v = SchemaValidator::new(vec![("name".into(), JsonType::String)]);
    let val = serde_json::json!({"name": null});
    let err = v.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "name" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn schema_custom_multiple_fields() {
    let v = SchemaValidator::new(vec![
        ("name".into(), JsonType::String),
        ("age".into(), JsonType::Number),
        ("active".into(), JsonType::Bool),
    ]);
    let val = serde_json::json!({"name": "test", "age": 30, "active": true});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_custom_multiple_missing_fields() {
    let v = SchemaValidator::new(vec![
        ("a".into(), JsonType::String),
        ("b".into(), JsonType::Number),
        ("c".into(), JsonType::Bool),
    ]);
    let val = serde_json::json!({});
    let err = v.validate(&val).unwrap_err();
    assert_eq!(err.len(), 3);
}

#[test]
fn schema_not_object_fails() {
    let v = SchemaValidator::new(vec![("x".into(), JsonType::String)]);
    let val = serde_json::json!([1, 2, 3]);
    let err = v.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn schema_empty_required_fields_passes_any_object() {
    let v = SchemaValidator::new(vec![]);
    let val = serde_json::json!({"anything": "goes"});
    assert!(v.validate(&val).is_ok());
}

#[test]
fn schema_extra_fields_ignored() {
    let v = SchemaValidator::new(vec![("name".into(), JsonType::String)]);
    let val = serde_json::json!({"name": "test", "extra": 42, "another": true});
    assert!(v.validate(&val).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 27. ValidationErrors – Construction and Inspection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn errors_new_is_empty() {
    let errs = ValidationErrors::new();
    assert!(errs.is_empty());
    assert_eq!(errs.len(), 0);
}

#[test]
fn errors_default_is_empty() {
    let errs = ValidationErrors::default();
    assert!(errs.is_empty());
}

#[test]
fn errors_push_increments_len() {
    let mut errs = ValidationErrors::new();
    errs.push(abp_validate::ValidationError {
        path: "field".into(),
        kind: ValidationErrorKind::Required,
        message: "missing".into(),
    });
    assert_eq!(errs.len(), 1);
    assert!(!errs.is_empty());
}

#[test]
fn errors_add_increments_len() {
    let mut errs = ValidationErrors::new();
    errs.add("field", ValidationErrorKind::Required, "missing");
    assert_eq!(errs.len(), 1);
}

#[test]
fn errors_multiple_adds() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "missing a");
    errs.add("b", ValidationErrorKind::Custom, "bad b");
    errs.add("c", ValidationErrorKind::OutOfRange, "too big c");
    assert_eq!(errs.len(), 3);
}

#[test]
fn errors_into_inner_preserves_order() {
    let mut errs = ValidationErrors::new();
    errs.add("first", ValidationErrorKind::Required, "1");
    errs.add("second", ValidationErrorKind::Custom, "2");
    errs.add("third", ValidationErrorKind::OutOfRange, "3");
    let inner = errs.into_inner();
    assert_eq!(inner.len(), 3);
    assert_eq!(inner[0].path, "first");
    assert_eq!(inner[1].path, "second");
    assert_eq!(inner[2].path, "third");
}

#[test]
fn errors_iter_yields_all() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "m1");
    errs.add("b", ValidationErrorKind::Custom, "m2");
    let count = errs.iter().count();
    assert_eq!(count, 2);
}

#[test]
fn errors_into_result_ok_when_empty() {
    let errs = ValidationErrors::new();
    assert!(errs.into_result().is_ok());
}

#[test]
fn errors_into_result_err_when_non_empty() {
    let mut errs = ValidationErrors::new();
    errs.add("field", ValidationErrorKind::Required, "missing");
    assert!(errs.into_result().is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 28. ValidationErrors – Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn errors_display_contains_count() {
    let mut errs = ValidationErrors::new();
    errs.add("field", ValidationErrorKind::Required, "missing");
    let msg = format!("{errs}");
    assert!(msg.contains("1 error"));
}

#[test]
fn errors_display_contains_path() {
    let mut errs = ValidationErrors::new();
    errs.add("my.path", ValidationErrorKind::Required, "missing");
    let msg = format!("{errs}");
    assert!(msg.contains("my.path"));
}

#[test]
fn errors_display_multiple_contains_count() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "m1");
    errs.add("b", ValidationErrorKind::Required, "m2");
    let msg = format!("{errs}");
    assert!(msg.contains("2 error"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 29. ValidationErrorKind – Display (snake_case)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_kind_required_display() {
    assert_eq!(format!("{}", ValidationErrorKind::Required), "required");
}

#[test]
fn error_kind_invalid_format_display() {
    assert_eq!(
        format!("{}", ValidationErrorKind::InvalidFormat),
        "invalid_format"
    );
}

#[test]
fn error_kind_out_of_range_display() {
    assert_eq!(
        format!("{}", ValidationErrorKind::OutOfRange),
        "out_of_range"
    );
}

#[test]
fn error_kind_invalid_reference_display() {
    assert_eq!(
        format!("{}", ValidationErrorKind::InvalidReference),
        "invalid_reference"
    );
}

#[test]
fn error_kind_custom_display() {
    assert_eq!(format!("{}", ValidationErrorKind::Custom), "custom");
}

// ═══════════════════════════════════════════════════════════════════════════
// 30. ValidationError – Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_error_display_format() {
    let err = abp_validate::ValidationError {
        path: "foo.bar".into(),
        kind: ValidationErrorKind::Required,
        message: "field is required".into(),
    };
    let msg = format!("{err}");
    assert_eq!(msg, "foo.bar: field is required");
}

#[test]
fn validation_error_is_std_error() {
    let err = abp_validate::ValidationError {
        path: "x".into(),
        kind: ValidationErrorKind::Custom,
        message: "test".into(),
    };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn validation_errors_is_std_error() {
    let mut errs = ValidationErrors::new();
    errs.add("x", ValidationErrorKind::Required, "m");
    let _: &dyn std::error::Error = &errs;
}

// ═══════════════════════════════════════════════════════════════════════════
// 31. ValidationErrorKind – Equality
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_kind_equality() {
    assert_eq!(ValidationErrorKind::Required, ValidationErrorKind::Required);
    assert_eq!(ValidationErrorKind::Custom, ValidationErrorKind::Custom);
    assert_ne!(ValidationErrorKind::Required, ValidationErrorKind::Custom);
    assert_ne!(
        ValidationErrorKind::OutOfRange,
        ValidationErrorKind::InvalidFormat
    );
}

#[test]
fn error_kind_clone() {
    let kind = ValidationErrorKind::InvalidReference;
    let cloned = kind.clone();
    assert_eq!(kind, cloned);
}

// ═══════════════════════════════════════════════════════════════════════════
// 32. Custom Validators via Validator Trait
// ═══════════════════════════════════════════════════════════════════════════

struct NonEmptyStringValidator;

impl Validator<String> for NonEmptyStringValidator {
    fn validate(&self, value: &String) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();
        if value.trim().is_empty() {
            errs.add(
                "value",
                ValidationErrorKind::Required,
                "string must not be empty",
            );
        }
        errs.into_result()
    }
}

#[test]
fn custom_validator_passes_valid() {
    assert!(NonEmptyStringValidator
        .validate(&"hello".to_string())
        .is_ok());
}

#[test]
fn custom_validator_fails_empty() {
    let err = NonEmptyStringValidator
        .validate(&"".to_string())
        .unwrap_err();
    assert!(err.iter().any(|e| e.kind == ValidationErrorKind::Required));
}

#[test]
fn custom_validator_fails_whitespace() {
    let err = NonEmptyStringValidator
        .validate(&"   ".to_string())
        .unwrap_err();
    assert_eq!(err.len(), 1);
}

struct PositiveNumberValidator;

impl Validator<f64> for PositiveNumberValidator {
    fn validate(&self, value: &f64) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();
        if *value <= 0.0 {
            errs.add("value", ValidationErrorKind::OutOfRange, "must be positive");
        }
        if value.is_nan() {
            errs.add(
                "value",
                ValidationErrorKind::InvalidFormat,
                "must not be NaN",
            );
        }
        errs.into_result()
    }
}

#[test]
fn custom_positive_validator_passes() {
    assert!(PositiveNumberValidator.validate(&1.0).is_ok());
}

#[test]
fn custom_positive_validator_fails_zero() {
    assert!(PositiveNumberValidator.validate(&0.0).is_err());
}

#[test]
fn custom_positive_validator_fails_negative() {
    let err = PositiveNumberValidator.validate(&-5.0).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::OutOfRange));
}

struct RangeValidator {
    min: i64,
    max: i64,
}

impl Validator<i64> for RangeValidator {
    fn validate(&self, value: &i64) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();
        if *value < self.min {
            errs.add(
                "value",
                ValidationErrorKind::OutOfRange,
                format!("must be >= {}", self.min),
            );
        }
        if *value > self.max {
            errs.add(
                "value",
                ValidationErrorKind::OutOfRange,
                format!("must be <= {}", self.max),
            );
        }
        errs.into_result()
    }
}

#[test]
fn range_validator_in_range_passes() {
    let v = RangeValidator { min: 0, max: 100 };
    assert!(v.validate(&50).is_ok());
}

#[test]
fn range_validator_at_min_passes() {
    let v = RangeValidator { min: 0, max: 100 };
    assert!(v.validate(&0).is_ok());
}

#[test]
fn range_validator_at_max_passes() {
    let v = RangeValidator { min: 0, max: 100 };
    assert!(v.validate(&100).is_ok());
}

#[test]
fn range_validator_below_min_fails() {
    let v = RangeValidator { min: 0, max: 100 };
    assert!(v.validate(&-1).is_err());
}

#[test]
fn range_validator_above_max_fails() {
    let v = RangeValidator { min: 0, max: 100 };
    assert!(v.validate(&101).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 33. Field-Level Validation Rules (message content)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_error_message_contains_task() {
    let wo = WorkOrderBuilder::new("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    let task_err = err.iter().find(|e| e.path == "task").unwrap();
    assert!(task_err.message.contains("task"));
}

#[test]
fn wo_error_message_contains_workspace() {
    let wo = WorkOrderBuilder::new("task").root("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    let ws_err = err.iter().find(|e| e.path == "workspace.root").unwrap();
    assert!(ws_err.message.contains("workspace"));
}

#[test]
fn wo_error_message_budget_contains_negative() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(-1.0).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    let budget_err = err
        .iter()
        .find(|e| e.path == "config.max_budget_usd")
        .unwrap();
    assert!(budget_err.message.contains("negative"));
}

#[test]
fn wo_error_message_turns_contains_zero() {
    let wo = WorkOrderBuilder::new("task").max_turns(0).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    let turns_err = err.iter().find(|e| e.path == "config.max_turns").unwrap();
    assert!(turns_err.message.contains("zero"));
}

#[test]
fn receipt_error_message_hash_length() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.receipt_sha256 = Some("short".into());
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    let hash_err = err.iter().find(|e| e.path == "receipt_sha256").unwrap();
    assert!(hash_err.message.contains("64"));
}

#[test]
fn receipt_error_message_contract_version_prefix() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "bad".into();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    let cv_err = err
        .iter()
        .find(|e| e.path == "meta.contract_version" && e.kind == ValidationErrorKind::InvalidFormat)
        .unwrap();
    assert!(cv_err.message.contains("abp/v"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 34. WorkOrder Validator – Debug Trait
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_validator_debug() {
    let v = WorkOrderValidator;
    let dbg = format!("{v:?}");
    assert!(dbg.contains("WorkOrderValidator"));
}

#[test]
fn receipt_validator_debug() {
    let v = ReceiptValidator;
    let dbg = format!("{v:?}");
    assert!(dbg.contains("ReceiptValidator"));
}

#[test]
fn event_validator_debug() {
    let v = EventValidator;
    let dbg = format!("{v:?}");
    assert!(dbg.contains("EventValidator"));
}

#[test]
fn envelope_validator_debug() {
    let v = EnvelopeValidator;
    let dbg = format!("{v:?}");
    assert!(dbg.contains("EnvelopeValidator"));
}

#[test]
fn raw_envelope_validator_debug() {
    let v = RawEnvelopeValidator;
    let dbg = format!("{v:?}");
    assert!(dbg.contains("RawEnvelopeValidator"));
}

#[test]
fn schema_validator_debug() {
    let v = SchemaValidator::work_order();
    let dbg = format!("{v:?}");
    assert!(dbg.contains("SchemaValidator"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 35. Default Impls
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_validator_default() {
    let _v = WorkOrderValidator::default();
}

#[test]
fn receipt_validator_default() {
    let _v = ReceiptValidator::default();
}

#[test]
fn event_validator_default() {
    let _v = EventValidator::default();
}

#[test]
fn envelope_validator_default() {
    let _v = EnvelopeValidator::default();
}

#[test]
fn raw_envelope_validator_default() {
    let _v = RawEnvelopeValidator::default();
}

// ═══════════════════════════════════════════════════════════════════════════
// 36. Validation Error Clone
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_error_clone() {
    let err = abp_validate::ValidationError {
        path: "a.b".into(),
        kind: ValidationErrorKind::Required,
        message: "msg".into(),
    };
    let cloned = err.clone();
    assert_eq!(cloned.path, "a.b");
    assert_eq!(cloned.kind, ValidationErrorKind::Required);
    assert_eq!(cloned.message, "msg");
}

#[test]
fn validation_errors_clone() {
    let mut errs = ValidationErrors::new();
    errs.add("x", ValidationErrorKind::Required, "m");
    let cloned = errs.clone();
    assert_eq!(cloned.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 37. JsonType Equality and Clone
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn json_type_equality() {
    assert_eq!(JsonType::String, JsonType::String);
    assert_eq!(JsonType::Number, JsonType::Number);
    assert_eq!(JsonType::Bool, JsonType::Bool);
    assert_eq!(JsonType::Object, JsonType::Object);
    assert_eq!(JsonType::Array, JsonType::Array);
    assert_eq!(JsonType::Any, JsonType::Any);
    assert_ne!(JsonType::String, JsonType::Number);
}

#[test]
fn json_type_clone() {
    let t = JsonType::Object;
    let cloned = t.clone();
    assert_eq!(t, cloned);
}

#[test]
fn json_type_debug() {
    let dbg = format!("{:?}", JsonType::String);
    assert!(dbg.contains("String"));
}
