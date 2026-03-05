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
//! Integration tests for abp-validate.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ContextPacket, ContextSnippet,
    ExecutionMode, Outcome, ReceiptBuilder, WorkOrderBuilder,
};
use abp_protocol::Envelope;
use abp_validate::{
    validate_hello_version, EnvelopeValidator, EventValidator, JsonType, RawEnvelopeValidator,
    ReceiptValidator, SchemaValidator, ValidationErrorKind, Validator, WorkOrderValidator,
};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════════════════
// WorkOrderValidator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn valid_work_order_passes() {
    let wo = WorkOrderBuilder::new("Refactor auth module").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn empty_task_fails() {
    let wo = WorkOrderBuilder::new("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "task"));
    assert!(err.iter().any(|e| e.kind == ValidationErrorKind::Required));
}

#[test]
fn whitespace_only_task_fails() {
    let wo = WorkOrderBuilder::new("   ").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "task"));
}

#[test]
fn empty_workspace_root_fails() {
    let wo = WorkOrderBuilder::new("task").root("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "workspace.root"));
}

#[test]
fn negative_budget_fails() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(-1.0).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "config.max_budget_usd"));
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::OutOfRange));
}

#[test]
fn zero_max_turns_fails() {
    let wo = WorkOrderBuilder::new("task").max_turns(0).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "config.max_turns"));
}

#[test]
fn valid_max_turns_passes() {
    let wo = WorkOrderBuilder::new("task").max_turns(10).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn empty_snippet_name_fails() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "some content".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "context.snippets[0].name"));
}

#[test]
fn conflicting_policy_tools_fail() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("bash".into());
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "policy"));
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn multiple_work_order_errors_accumulated() {
    let mut wo = WorkOrderBuilder::new("")
        .root("")
        .max_budget_usd(-5.0)
        .build();
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "x".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.len() >= 3);
}

#[test]
fn nan_budget_fails() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::NAN)
        .build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "config.max_budget_usd"));
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn valid_budget_zero_passes() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(0.0).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// ReceiptValidator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn valid_receipt_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_with_correct_hash_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_with_incorrect_hash_fails() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.receipt_sha256 = Some("a".repeat(64));
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "receipt_sha256"));
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn receipt_short_hash_fails() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.receipt_sha256 = Some("tooshort".into());
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "receipt_sha256"));
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn receipt_empty_backend_id_fails() {
    let receipt = ReceiptBuilder::new("").build();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "backend.id"));
}

#[test]
fn receipt_invalid_contract_version_fails() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "invalid".into();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "meta.contract_version"));
}

#[test]
fn receipt_empty_contract_version_fails() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "".into();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "meta.contract_version"));
}

#[test]
fn receipt_finished_before_started_fails() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let receipt = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(earlier)
        .build();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "meta.finished_at"));
}

#[test]
fn receipt_failed_with_harness_ok_fails() {
    let mut receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    receipt.verification.harness_ok = true;
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "verification.harness_ok"));
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidReference));
}

#[test]
fn receipt_complete_with_harness_ok_passes() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    receipt.verification.harness_ok = true;
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_no_hash_passes() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert!(receipt.receipt_sha256.is_none());
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// EventValidator
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

#[test]
fn empty_event_sequence_passes() {
    let events: Vec<AgentEvent> = vec![];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn valid_event_sequence_passes() {
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
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(2),
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn non_monotonic_timestamps_fail() {
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
    assert!(err.iter().any(|e| e.path == "events[1].ts"));
}

#[test]
fn first_event_not_run_started_fails() {
    let events = vec![make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    })];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err.iter().any(|e| e.path == "events[0].kind"));
}

#[test]
fn last_event_not_run_completed_fails() {
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
fn tool_result_without_call_fails() {
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
fn tool_call_then_result_passes() {
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
fn multiple_event_errors_accumulated() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let events = vec![
        make_event_at(AgentEventKind::AssistantMessage { text: "hi".into() }, now),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            earlier,
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    // first event not RunStarted, last event not RunCompleted, non-monotonic, orphan tool_result
    assert!(err.len() >= 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// EnvelopeValidator (typed)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn valid_hello_envelope_passes() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn hello_empty_backend_id_fails() {
    let env = Envelope::Hello {
        contract_version: abp_core::CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "backend.id"));
}

#[test]
fn hello_invalid_contract_version_fails() {
    let env = Envelope::Hello {
        contract_version: "bad".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "contract_version"));
}

#[test]
fn run_envelope_empty_id_fails() {
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "".into(),
        work_order: wo,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "id"));
}

#[test]
fn run_envelope_empty_task_fails() {
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "work_order.task"));
}

#[test]
fn valid_event_envelope_passes() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn event_envelope_empty_ref_id_fails() {
    let env = Envelope::Event {
        ref_id: "".into(),
        event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "ref_id"));
}

#[test]
fn final_envelope_empty_ref_id_fails() {
    let receipt = ReceiptBuilder::new("mock").build();
    let env = Envelope::Final {
        ref_id: "".into(),
        receipt,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "ref_id"));
}

#[test]
fn fatal_envelope_empty_error_fails() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "".into(),
        error_code: None,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert!(err.iter().any(|e| e.path == "error"));
}

#[test]
fn valid_fatal_envelope_passes() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something broke".into(),
        error_code: None,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// RawEnvelopeValidator (JSON value)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn raw_envelope_missing_tag_fails() {
    let val = serde_json::json!({"ref_id": "run-1"});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "t"));
    assert!(err.iter().any(|e| e.kind == ValidationErrorKind::Required));
}

#[test]
fn raw_envelope_unknown_tag_fails() {
    let val = serde_json::json!({"t": "bogus"});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "t"));
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn raw_envelope_tag_not_string_fails() {
    let val = serde_json::json!({"t": 42});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "t"));
}

#[test]
fn raw_envelope_not_object_fails() {
    let val = serde_json::json!("just a string");
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn raw_hello_missing_contract_version_fails() {
    let val = serde_json::json!({"t": "hello", "backend": {"id": "x"}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "contract_version"));
}

#[test]
fn raw_run_missing_id_fails() {
    let val = serde_json::json!({"t": "run", "work_order": {}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "id"));
}

#[test]
fn raw_event_missing_ref_id_fails() {
    let val = serde_json::json!({"t": "event", "event": {}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "ref_id"));
}

#[test]
fn raw_final_missing_ref_id_fails() {
    let val = serde_json::json!({"t": "final", "receipt": {}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "ref_id"));
}

#[test]
fn raw_valid_fatal_passes() {
    let val = serde_json::json!({"t": "fatal", "error": "oops"});
    assert!(RawEnvelopeValidator.validate(&val).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// validate_hello_version
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_version_compatible_passes() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert!(validate_hello_version(&env).is_ok());
}

#[test]
fn hello_version_incompatible_fails() {
    let env = Envelope::Hello {
        contract_version: "abp/v9.0".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = validate_hello_version(&env).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidReference));
}

// ═══════════════════════════════════════════════════════════════════════════
// SchemaValidator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn schema_valid_work_order_json_passes() {
    let wo = WorkOrderBuilder::new("do stuff").build();
    let val = serde_json::to_value(&wo).unwrap();
    assert!(SchemaValidator::work_order().validate(&val).is_ok());
}

#[test]
fn schema_missing_required_field_fails() {
    let val = serde_json::json!({"id": "x"});
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "task"));
    assert!(err.iter().any(|e| e.kind == ValidationErrorKind::Required));
}

#[test]
fn schema_null_required_field_fails() {
    let val = serde_json::json!({
        "id": "x",
        "task": null,
        "lane": "patch_first",
        "workspace": {},
        "context": {},
        "policy": {},
        "config": {},
    });
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "task" && e.kind == ValidationErrorKind::Required));
}

#[test]
fn schema_wrong_type_fails() {
    let val = serde_json::json!({
        "id": "x",
        "task": 42,
        "lane": "patch_first",
        "workspace": {},
        "context": {},
        "policy": {},
        "config": {},
    });
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "task" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn schema_valid_receipt_json_passes() {
    let receipt = ReceiptBuilder::new("mock").build();
    let val = serde_json::to_value(&receipt).unwrap();
    assert!(SchemaValidator::receipt().validate(&val).is_ok());
}

#[test]
fn schema_receipt_missing_meta_fails() {
    let val = serde_json::json!({
        "backend": {"id": "mock"},
        "outcome": "complete",
        "trace": [],
        "artifacts": [],
    });
    let err = SchemaValidator::receipt().validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "meta"));
}

#[test]
fn schema_agent_event_json_passes() {
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let val = serde_json::to_value(&event).unwrap();
    assert!(SchemaValidator::agent_event().validate(&val).is_ok());
}

#[test]
fn schema_agent_event_missing_type_fails() {
    let val = serde_json::json!({"ts": "2024-01-01T00:00:00Z"});
    let err = SchemaValidator::agent_event().validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "type"));
}

#[test]
fn schema_not_object_fails() {
    let val = serde_json::json!([1, 2, 3]);
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn schema_custom_fields() {
    let validator = SchemaValidator::new(vec![
        ("name".into(), JsonType::String),
        ("count".into(), JsonType::Number),
        ("active".into(), JsonType::Bool),
    ]);
    let val = serde_json::json!({"name": "test", "count": 42, "active": true});
    assert!(validator.validate(&val).is_ok());
}

#[test]
fn schema_custom_wrong_type_for_number() {
    let validator = SchemaValidator::new(vec![("count".into(), JsonType::Number)]);
    let val = serde_json::json!({"count": "not-a-number"});
    let err = validator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "count"));
}

#[test]
fn schema_custom_wrong_type_for_bool() {
    let validator = SchemaValidator::new(vec![("active".into(), JsonType::Bool)]);
    let val = serde_json::json!({"active": "yes"});
    let err = validator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "active"));
}

#[test]
fn schema_custom_any_type_passes() {
    let validator = SchemaValidator::new(vec![("data".into(), JsonType::Any)]);
    let val = serde_json::json!({"data": [1, 2, 3]});
    assert!(validator.validate(&val).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_errors_display() {
    let mut errs = abp_validate::ValidationErrors::new();
    errs.add("field", ValidationErrorKind::Required, "missing");
    let msg = format!("{errs}");
    assert!(msg.contains("1 error"));
    assert!(msg.contains("field"));
}

#[test]
fn validation_errors_default_is_empty() {
    let errs = abp_validate::ValidationErrors::default();
    assert!(errs.is_empty());
    assert_eq!(errs.len(), 0);
}

#[test]
fn validation_errors_into_inner() {
    let mut errs = abp_validate::ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "missing a");
    errs.add("b", ValidationErrorKind::Custom, "bad b");
    let inner = errs.into_inner();
    assert_eq!(inner.len(), 2);
    assert_eq!(inner[0].path, "a");
    assert_eq!(inner[1].path, "b");
}

#[test]
fn validation_error_kind_display() {
    assert_eq!(format!("{}", ValidationErrorKind::Required), "required");
    assert_eq!(
        format!("{}", ValidationErrorKind::InvalidFormat),
        "invalid_format"
    );
    assert_eq!(
        format!("{}", ValidationErrorKind::OutOfRange),
        "out_of_range"
    );
    assert_eq!(
        format!("{}", ValidationErrorKind::InvalidReference),
        "invalid_reference"
    );
    assert_eq!(format!("{}", ValidationErrorKind::Custom), "custom");
}

#[test]
fn validation_error_display() {
    let err = abp_validate::ValidationError {
        path: "foo.bar".into(),
        kind: ValidationErrorKind::Required,
        message: "missing".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("foo.bar"));
    assert!(msg.contains("missing"));
}

#[test]
fn work_order_with_config_passes() {
    let wo = WorkOrderBuilder::new("hello")
        .model("gpt-4")
        .max_turns(5)
        .max_budget_usd(10.0)
        .build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn work_order_context_with_files_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn schema_nested_object_field_check() {
    let validator = SchemaValidator::new(vec![("meta".into(), JsonType::Object)]);
    let val = serde_json::json!({"meta": "not-an-object"});
    let err = validator.validate(&val).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "meta" && e.kind == ValidationErrorKind::InvalidFormat));
}

#[test]
fn schema_array_field_check() {
    let validator = SchemaValidator::new(vec![("items".into(), JsonType::Array)]);
    let val = serde_json::json!({"items": "not-array"});
    let err = validator.validate(&val).unwrap_err();
    assert!(err.iter().any(|e| e.path == "items"));
}

#[test]
fn schema_empty_object_missing_all_fields() {
    let val = serde_json::json!({});
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    // Should report all missing required fields
    assert!(err.len() >= 5);
}
