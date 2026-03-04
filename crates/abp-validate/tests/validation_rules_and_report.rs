// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep validation tests for ConfigValidator, ValidationReport, and ValidationRule.
#![allow(clippy::all)]

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    ReceiptBuilder, WorkOrderBuilder,
};
use abp_protocol::Envelope;
use abp_validate::config::ConfigValidator;
use abp_validate::report::{Severity, ValidationReport};
use abp_validate::rule::{
    ClosureRule, ExpectedType, NonEmptyCollectionRule, NonEmptyStringRule, NumberRangeRule,
    OneOfRule, RequiredFieldRule, StringLengthRule, TypeCheckRule, ValidationRule,
};
use abp_validate::{
    EnvelopeValidator, EventValidator, ReceiptValidator, Validator, WorkOrderValidator,
};
use chrono::Utc;
use serde_json::json;

// ── helpers ────────────────────────────────────────────────────────────

fn make_event_at(kind: AgentEventKind, ts: chrono::DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn ts(secs: i64) -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp(1_700_000_000 + secs, 0).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════
// ValidationReport
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn report_new_is_empty_and_valid() {
    let report = ValidationReport::new();
    assert!(report.is_empty());
    assert!(report.is_valid());
    assert_eq!(report.len(), 0);
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 0);
    assert_eq!(report.info_count(), 0);
}

#[test]
fn report_add_error_makes_invalid() {
    let mut report = ValidationReport::new();
    report.error("field", "bad value");
    assert!(!report.is_valid());
    assert!(report.has_errors());
    assert_eq!(report.error_count(), 1);
}

#[test]
fn report_add_warning_stays_valid() {
    let mut report = ValidationReport::new();
    report.warn("field", "might be wrong");
    assert!(report.is_valid());
    assert!(report.has_warnings());
    assert_eq!(report.warning_count(), 1);
}

#[test]
fn report_add_info_stays_valid() {
    let mut report = ValidationReport::new();
    report.info("field", "just FYI");
    assert!(report.is_valid());
    assert_eq!(report.info_count(), 1);
}

#[test]
fn report_mixed_severities() {
    let mut report = ValidationReport::new();
    report.info("a", "info");
    report.warn("b", "warning");
    report.error("c", "error");
    assert_eq!(report.len(), 3);
    assert!(!report.is_valid());
    assert_eq!(report.error_count(), 1);
    assert_eq!(report.warning_count(), 1);
    assert_eq!(report.info_count(), 1);
}

#[test]
fn report_filter_by_severity() {
    let mut report = ValidationReport::new();
    report.info("a", "msg");
    report.warn("b", "msg");
    report.error("c", "msg");
    report.error("d", "msg");
    assert_eq!(report.filter_by_severity(Severity::Error).len(), 2);
    assert_eq!(report.filter_by_severity(Severity::Warning).len(), 1);
    assert_eq!(report.filter_by_severity(Severity::Info).len(), 1);
}

#[test]
fn report_filter_at_or_above() {
    let mut report = ValidationReport::new();
    report.info("a", "msg");
    report.warn("b", "msg");
    report.error("c", "msg");
    assert_eq!(report.filter_at_or_above(Severity::Info).len(), 3);
    assert_eq!(report.filter_at_or_above(Severity::Warning).len(), 2);
    assert_eq!(report.filter_at_or_above(Severity::Error).len(), 1);
}

#[test]
fn report_filter_by_path() {
    let mut report = ValidationReport::new();
    report.error("config.port", "bad");
    report.error("config.log_level", "bad");
    report.error("backends.mock", "bad");
    assert_eq!(report.filter_by_path("config").len(), 2);
    assert_eq!(report.filter_by_path("backends").len(), 1);
}

#[test]
fn report_merge() {
    let mut a = ValidationReport::new();
    a.error("a", "err");
    let mut b = ValidationReport::new();
    b.warn("b", "warn");
    a.merge(b);
    assert_eq!(a.len(), 2);
}

#[test]
fn report_into_issues() {
    let mut report = ValidationReport::new();
    report.error("x", "y");
    let issues = report.into_issues();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "x");
}

#[test]
fn report_format_empty() {
    let report = ValidationReport::new();
    assert!(report.format().contains("no issues"));
}

#[test]
fn report_format_with_issues() {
    let mut report = ValidationReport::new();
    report.error("port", "out of range");
    report.warn("default_backend", "not configured");
    let formatted = report.format();
    assert!(formatted.contains("2 issue(s)"));
    assert!(formatted.contains("FAIL"));
}

#[test]
fn report_format_pass() {
    let mut report = ValidationReport::new();
    report.warn("x", "minor");
    let formatted = report.format();
    assert!(formatted.contains("PASS"));
}

#[test]
fn report_display_trait() {
    let mut report = ValidationReport::new();
    report.info("field", "ok");
    let s = format!("{}", report);
    assert!(s.contains("info"));
}

#[test]
fn report_add_with_rule() {
    let mut report = ValidationReport::new();
    report.add_with_rule("field", Severity::Error, "bad", "my_rule");
    let issues = report.into_issues();
    assert_eq!(issues[0].rule.as_deref(), Some("my_rule"));
}

#[test]
fn severity_ordering() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
}

#[test]
fn severity_display() {
    assert_eq!(format!("{}", Severity::Info), "info");
    assert_eq!(format!("{}", Severity::Warning), "warning");
    assert_eq!(format!("{}", Severity::Error), "error");
}

// ═══════════════════════════════════════════════════════════════════════
// ValidationRule trait + built-in rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn required_field_rule_missing() {
    let rule = RequiredFieldRule::new("name");
    let mut report = ValidationReport::new();
    rule.check(&json!({}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn required_field_rule_null() {
    let rule = RequiredFieldRule::new("name");
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": null}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn required_field_rule_present() {
    let rule = RequiredFieldRule::new("name");
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": "Alice"}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn type_check_rule_string_ok() {
    let rule = TypeCheckRule::new("name", ExpectedType::String);
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": "hello"}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn type_check_rule_string_fail() {
    let rule = TypeCheckRule::new("name", ExpectedType::String);
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": 42}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn type_check_rule_number() {
    let rule = TypeCheckRule::new("count", ExpectedType::Number);
    let mut report = ValidationReport::new();
    rule.check(&json!({"count": 10}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn type_check_rule_bool_fail() {
    let rule = TypeCheckRule::new("flag", ExpectedType::Bool);
    let mut report = ValidationReport::new();
    rule.check(&json!({"flag": "yes"}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn type_check_rule_object() {
    let rule = TypeCheckRule::new("meta", ExpectedType::Object);
    let mut report = ValidationReport::new();
    rule.check(&json!({"meta": {}}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn type_check_rule_array() {
    let rule = TypeCheckRule::new("items", ExpectedType::Array);
    let mut report = ValidationReport::new();
    rule.check(&json!({"items": []}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn type_check_rule_null_skipped() {
    let rule = TypeCheckRule::new("name", ExpectedType::String);
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": null}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn string_length_rule_too_short() {
    let rule = StringLengthRule::new("name", Some(3), None);
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": "ab"}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn string_length_rule_too_long() {
    let rule = StringLengthRule::new("name", None, Some(5));
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": "toolong"}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn string_length_rule_ok() {
    let rule = StringLengthRule::new("name", Some(1), Some(10));
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": "hello"}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn number_range_rule_below() {
    let rule = NumberRangeRule::new("score", Some(0.0), None);
    let mut report = ValidationReport::new();
    rule.check(&json!({"score": -1}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn number_range_rule_above() {
    let rule = NumberRangeRule::new("score", None, Some(100.0));
    let mut report = ValidationReport::new();
    rule.check(&json!({"score": 101}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn number_range_rule_ok() {
    let rule = NumberRangeRule::new("score", Some(0.0), Some(100.0));
    let mut report = ValidationReport::new();
    rule.check(&json!({"score": 50}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn non_empty_string_rule_empty() {
    let rule = NonEmptyStringRule::new("name");
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": "  "}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn non_empty_string_rule_ok() {
    let rule = NonEmptyStringRule::new("name");
    let mut report = ValidationReport::new();
    rule.check(&json!({"name": "Alice"}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn one_of_rule_invalid() {
    let rule = OneOfRule::new("level", vec!["info".into(), "debug".into()]);
    let mut report = ValidationReport::new();
    rule.check(&json!({"level": "banana"}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn one_of_rule_valid() {
    let rule = OneOfRule::new("level", vec!["info".into(), "debug".into()]);
    let mut report = ValidationReport::new();
    rule.check(&json!({"level": "info"}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn non_empty_collection_rule_empty_object() {
    let rule = NonEmptyCollectionRule::new("backends");
    let mut report = ValidationReport::new();
    rule.check(&json!({"backends": {}}), &mut report);
    assert_eq!(report.warning_count(), 1);
}

#[test]
fn non_empty_collection_rule_empty_array() {
    let rule = NonEmptyCollectionRule::new("items");
    let mut report = ValidationReport::new();
    rule.check(&json!({"items": []}), &mut report);
    assert_eq!(report.warning_count(), 1);
}

#[test]
fn non_empty_collection_rule_populated() {
    let rule = NonEmptyCollectionRule::new("items");
    let mut report = ValidationReport::new();
    rule.check(&json!({"items": [1, 2]}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn closure_rule() {
    let rule = ClosureRule::new("custom_check", |val, report| {
        if let Some(obj) = val.as_object() {
            if obj.contains_key("forbidden") {
                report.error("forbidden", "field 'forbidden' must not exist");
            }
        }
    });
    assert_eq!(rule.name(), "custom_check");
    let mut report = ValidationReport::new();
    rule.check(&json!({"forbidden": true}), &mut report);
    assert_eq!(report.error_count(), 1);
}

#[test]
fn closure_rule_passes() {
    let rule = ClosureRule::new("ok", |_, _| {});
    let mut report = ValidationReport::new();
    rule.check(&json!({}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn rule_name_accessors() {
    assert_eq!(RequiredFieldRule::new("x").name(), "required_field");
    assert_eq!(
        TypeCheckRule::new("x", ExpectedType::String).name(),
        "type_check"
    );
    assert_eq!(
        StringLengthRule::new("x", None, None).name(),
        "string_length"
    );
    assert_eq!(NumberRangeRule::new("x", None, None).name(), "number_range");
    assert_eq!(NonEmptyStringRule::new("x").name(), "non_empty_string");
    assert_eq!(OneOfRule::new("x", vec![]).name(), "one_of");
    assert_eq!(
        NonEmptyCollectionRule::new("x").name(),
        "non_empty_collection"
    );
}

#[test]
fn rules_on_non_object_are_no_ops() {
    let rules: Vec<Box<dyn ValidationRule>> = vec![
        Box::new(RequiredFieldRule::new("x")),
        Box::new(TypeCheckRule::new("x", ExpectedType::String)),
        Box::new(StringLengthRule::new("x", Some(1), None)),
        Box::new(NumberRangeRule::new("x", Some(0.0), None)),
        Box::new(NonEmptyStringRule::new("x")),
        Box::new(OneOfRule::new("x", vec!["a".into()])),
        Box::new(NonEmptyCollectionRule::new("x")),
    ];
    for rule in &rules {
        let mut report = ValidationReport::new();
        rule.check(&json!("not an object"), &mut report);
        assert!(
            report.is_empty(),
            "rule {} should be no-op on non-object",
            rule.name()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ConfigValidator
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_validator_empty_config() {
    let report = ConfigValidator::new().validate(&json!({}));
    assert!(report.is_valid());
}

#[test]
fn config_validator_valid_minimal() {
    let cfg = json!({
        "log_level": "info",
        "backends": {}
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.is_valid());
}

#[test]
fn config_validator_invalid_log_level() {
    let cfg = json!({ "log_level": "banana" });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_log_level_type_error() {
    let cfg = json!({ "log_level": 42 });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_port_out_of_range() {
    let cfg = json!({ "port": 0 });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_port_too_high() {
    let cfg = json!({ "port": 70000 });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_port_valid() {
    let cfg = json!({ "port": 8080 });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.is_valid());
}

#[test]
fn config_validator_backends_not_object() {
    let cfg = json!({ "backends": "not_an_object" });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_sidecar_missing_command() {
    let cfg = json!({
        "backends": {
            "sc": { "type": "sidecar" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_sidecar_empty_command() {
    let cfg = json!({
        "backends": {
            "sc": { "type": "sidecar", "command": "  " }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_sidecar_valid() {
    let cfg = json!({
        "backends": {
            "sc": { "type": "sidecar", "command": "node host.js" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.is_valid());
}

#[test]
fn config_validator_sidecar_timeout_zero() {
    let cfg = json!({
        "backends": {
            "sc": { "type": "sidecar", "command": "node", "timeout_secs": 0 }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_sidecar_timeout_too_high() {
    let cfg = json!({
        "backends": {
            "sc": { "type": "sidecar", "command": "node", "timeout_secs": 100000 }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_sidecar_large_timeout_warns() {
    let cfg = json!({
        "backends": {
            "sc": { "type": "sidecar", "command": "node", "timeout_secs": 7200 }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.is_valid());
    assert!(report.has_warnings());
}

#[test]
fn config_validator_unknown_backend_type() {
    let cfg = json!({
        "backends": {
            "x": { "type": "unknown" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_backend_missing_type() {
    let cfg = json!({
        "backends": {
            "x": { "command": "echo" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_mock_backend() {
    let cfg = json!({
        "backends": {
            "m": { "type": "mock" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.is_valid());
}

#[test]
fn config_validator_default_backend_not_found() {
    let cfg = json!({
        "default_backend": "missing",
        "backends": {
            "m": { "type": "mock" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_warnings());
}

#[test]
fn config_validator_default_backend_found() {
    let cfg = json!({
        "default_backend": "m",
        "backends": {
            "m": { "type": "mock" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.is_valid());
}

#[test]
fn config_validator_non_object_input() {
    let report = ConfigValidator::new().validate(&json!("not an object"));
    assert!(report.has_errors());
}

#[test]
fn config_validator_with_custom_rule() {
    let cv = ConfigValidator::empty().with_rule(RequiredFieldRule::new("custom_field"));
    let report = cv.validate(&json!({}));
    assert!(report.has_errors());
}

#[test]
fn config_validator_rule_count() {
    let cv = ConfigValidator::new();
    assert!(cv.rule_count() > 0);
    let cv2 = ConfigValidator::empty();
    assert_eq!(cv2.rule_count(), 0);
}

#[test]
fn config_validator_policy_profiles_type() {
    let cfg = json!({ "policy_profiles": "not_array" });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

#[test]
fn config_validator_all_log_levels() {
    for level in &["trace", "debug", "info", "warn", "error"] {
        let cfg = json!({ "log_level": level });
        let report = ConfigValidator::new().validate(&cfg);
        assert!(report.is_valid(), "log_level '{}' should be valid", level);
    }
}

#[test]
fn config_validator_backend_entry_not_object() {
    let cfg = json!({
        "backends": {
            "bad": "just a string"
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
}

// ═══════════════════════════════════════════════════════════════════════
// WorkOrderValidator deepened
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_validator_valid_order() {
    let wo = WorkOrderBuilder::new("do something").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn work_order_validator_whitespace_task() {
    let mut wo = WorkOrderBuilder::new("ok").build();
    wo.task = "   ".to_string();
    assert!(WorkOrderValidator.validate(&wo).is_err());
}

#[test]
fn work_order_validator_negative_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(-5.0).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("max_budget")));
}

#[test]
fn work_order_validator_zero_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(0).build();
    assert!(WorkOrderValidator.validate(&wo).is_err());
}

#[test]
fn work_order_validator_tool_in_both_lists() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("bash".into());
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.message.contains("bash")));
}

// ═══════════════════════════════════════════════════════════════════════
// ReceiptValidator deepened
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_validator_valid() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_validator_valid_with_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_validator_empty_backend_id() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.backend.id = "".to_string();
    assert!(ReceiptValidator.validate(&receipt).is_err());
}

#[test]
fn receipt_validator_bad_contract_version() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "wrong".to_string();
    assert!(ReceiptValidator.validate(&receipt).is_err());
}

#[test]
fn receipt_validator_finished_before_started() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.finished_at = receipt.meta.started_at - chrono::Duration::seconds(10);
    assert!(ReceiptValidator.validate(&receipt).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// EventValidator deepened
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_validator_empty_trace() {
    assert!(EventValidator.validate(&vec![]).is_ok());
}

#[test]
fn event_validator_valid_bookends() {
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ts(0),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ts(1),
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn event_validator_missing_run_started() {
    let events = vec![make_event_at(
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ts(0),
    )];
    assert!(EventValidator.validate(&events).is_err());
}

#[test]
fn event_validator_non_monotonic_timestamps() {
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ts(5),
        ),
        make_event_at(AgentEventKind::AssistantDelta { text: "x".into() }, ts(2)),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ts(10),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("ts")));
}

#[test]
fn event_validator_tool_result_without_call() {
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ts(0),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            ts(1),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ts(2),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err.iter().any(|e| e.message.contains("bash")));
}

// ═══════════════════════════════════════════════════════════════════════
// EnvelopeValidator deepened
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_validator_valid_hello() {
    let env = Envelope::Hello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_validator_empty_contract_version() {
    let env = Envelope::Hello {
        contract_version: "".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    assert!(EnvelopeValidator.validate(&env).is_err());
}

#[test]
fn envelope_validator_run_empty_id() {
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "".into(),
        work_order: wo,
    };
    assert!(EnvelopeValidator.validate(&env).is_err());
}

#[test]
fn envelope_validator_fatal_empty_error() {
    let env = Envelope::Fatal {
        ref_id: Some("r".into()),
        error: "".into(),
        error_code: None,
    };
    assert!(EnvelopeValidator.validate(&env).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// Integration: ConfigValidator + ValidationReport
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_validator_report_format_includes_all() {
    let cfg = json!({
        "log_level": "banana",
        "port": 0,
        "backends": {
            "sc": { "type": "sidecar" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    let formatted = report.format();
    assert!(formatted.contains("FAIL"));
    assert!(report.error_count() >= 2);
}

#[test]
fn config_validator_full_valid_config() {
    let cfg = json!({
        "log_level": "debug",
        "default_backend": "mock",
        "workspace_dir": "/tmp/ws",
        "receipts_dir": "/tmp/receipts",
        "bind_address": "127.0.0.1",
        "port": 9090,
        "policy_profiles": ["default.toml"],
        "backends": {
            "mock": { "type": "mock" },
            "node": { "type": "sidecar", "command": "node host.js", "timeout_secs": 60 }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(
        report.is_valid(),
        "full config should be valid: {}",
        report.format()
    );
}

#[test]
fn multiple_rules_accumulate_errors() {
    let rules: Vec<Box<dyn ValidationRule>> = vec![
        Box::new(RequiredFieldRule::new("a")),
        Box::new(RequiredFieldRule::new("b")),
        Box::new(RequiredFieldRule::new("c")),
    ];
    let mut report = ValidationReport::new();
    for rule in &rules {
        rule.check(&json!({}), &mut report);
    }
    assert_eq!(report.error_count(), 3);
}

#[test]
fn type_check_expected_type_display() {
    assert_eq!(format!("{}", ExpectedType::String), "string");
    assert_eq!(format!("{}", ExpectedType::Number), "number");
    assert_eq!(format!("{}", ExpectedType::Bool), "boolean");
    assert_eq!(format!("{}", ExpectedType::Object), "object");
    assert_eq!(format!("{}", ExpectedType::Array), "array");
}

#[test]
fn config_validator_multiple_backends() {
    let cfg = json!({
        "backends": {
            "a": { "type": "mock" },
            "b": { "type": "sidecar", "command": "python host.py" },
            "c": { "type": "sidecar", "command": "" }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.has_errors());
    assert_eq!(
        report.filter_by_path("backends.c").len(),
        1,
        "only backend 'c' should have errors"
    );
}

#[test]
fn report_iter_yields_all() {
    let mut report = ValidationReport::new();
    report.info("a", "1");
    report.warn("b", "2");
    report.error("c", "3");
    let paths: Vec<&str> = report.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, vec!["a", "b", "c"]);
}

#[test]
fn config_validator_default_is_new() {
    let cv = ConfigValidator::default();
    assert!(cv.rule_count() > 0);
}

#[test]
fn report_has_no_false_warnings() {
    let report = ValidationReport::new();
    assert!(!report.has_warnings());
    assert!(!report.has_errors());
}

#[test]
fn string_length_boundary_exact_min() {
    let rule = StringLengthRule::new("x", Some(3), None);
    let mut report = ValidationReport::new();
    rule.check(&json!({"x": "abc"}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn string_length_boundary_exact_max() {
    let rule = StringLengthRule::new("x", None, Some(3));
    let mut report = ValidationReport::new();
    rule.check(&json!({"x": "abc"}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn number_range_boundary_exact_min() {
    let rule = NumberRangeRule::new("x", Some(0.0), None);
    let mut report = ValidationReport::new();
    rule.check(&json!({"x": 0}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn number_range_boundary_exact_max() {
    let rule = NumberRangeRule::new("x", None, Some(100.0));
    let mut report = ValidationReport::new();
    rule.check(&json!({"x": 100}), &mut report);
    assert!(report.is_empty());
}

#[test]
fn config_validator_sidecar_timeout_boundary_valid() {
    let cfg = json!({
        "backends": {
            "sc": { "type": "sidecar", "command": "node", "timeout_secs": 1 }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.is_valid());
}

#[test]
fn config_validator_sidecar_timeout_boundary_max() {
    let cfg = json!({
        "backends": {
            "sc": { "type": "sidecar", "command": "node", "timeout_secs": 86400 }
        }
    });
    let report = ConfigValidator::new().validate(&cfg);
    assert!(report.is_valid());
    assert!(report.has_warnings());
}
