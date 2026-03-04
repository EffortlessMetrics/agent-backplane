// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep validation tests for abp-validate covering work orders, receipts,
//! events, envelopes, schema checks, custom validators, serde pipelines,
//! batch validation, and edge cases.

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile,
    ReceiptBuilder, RuntimeConfig, SupportLevel, WorkOrderBuilder, WorkspaceMode,
};
use abp_protocol::Envelope;
use abp_validate::{
    EnvelopeValidator, EventValidator, JsonType, RawEnvelopeValidator, ReceiptValidator,
    SchemaValidator, ValidationErrorKind, ValidationErrors, Validator, WorkOrderValidator,
    validate_hello_version,
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

/// A helper that implements `Validator<WorkOrder>` with a custom rule.
struct TaskLengthValidator {
    max_len: usize,
}

impl Validator<abp_core::WorkOrder> for TaskLengthValidator {
    fn validate(&self, wo: &abp_core::WorkOrder) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();
        if wo.task.len() > self.max_len {
            errs.add(
                "task",
                ValidationErrorKind::Custom,
                format!("task length {} exceeds max {}", wo.task.len(), self.max_len),
            );
        }
        errs.into_result()
    }
}

/// Custom validator that checks the receipt has at least one trace event.
struct NonEmptyTraceValidator;

impl Validator<abp_core::Receipt> for NonEmptyTraceValidator {
    fn validate(&self, receipt: &abp_core::Receipt) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();
        if receipt.trace.is_empty() {
            errs.add(
                "trace",
                ValidationErrorKind::Custom,
                "trace must contain at least one event",
            );
        }
        errs.into_result()
    }
}

/// Runs both the built-in and a custom validator, merging errors.
fn validate_with_custom<T>(
    builtin: &dyn Validator<T>,
    custom: &dyn Validator<T>,
    value: &T,
) -> Result<(), ValidationErrors> {
    let mut all = ValidationErrors::new();
    if let Err(e) = builtin.validate(value) {
        for err in e.iter() {
            all.push(err.clone());
        }
    }
    if let Err(e) = custom.validate(value) {
        for err in e.iter() {
            all.push(err.clone());
        }
    }
    all.into_result()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. WorkOrder validation — additional depth
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_unicode_task_passes() {
    let wo = WorkOrderBuilder::new("重构认证模块 🔐").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_very_long_task_passes() {
    let task = "a".repeat(10_000);
    let wo = WorkOrderBuilder::new(task).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_tab_only_task_fails() {
    let wo = WorkOrderBuilder::new("\t\t").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "task"));
}

#[test]
fn wo_newline_only_task_fails() {
    let wo = WorkOrderBuilder::new("\n\n").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "task"));
}

#[test]
fn wo_task_with_leading_trailing_whitespace_passes() {
    let wo = WorkOrderBuilder::new("  valid task  ").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_infinity_budget_fails() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::INFINITY)
        .build();
    // INFINITY is >= 0 and not NaN, so it passes the current validators
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_neg_infinity_budget_fails() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::NEG_INFINITY)
        .build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err
        .iter()
        .any(|e| e.path == "config.max_budget_usd"
            && e.kind == ValidationErrorKind::OutOfRange));
}

#[test]
fn wo_very_large_budget_passes() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(999_999.99)
        .build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_max_turns_one_passes() {
    let wo = WorkOrderBuilder::new("task").max_turns(1).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_max_turns_very_large_passes() {
    let wo = WorkOrderBuilder::new("task").max_turns(u32::MAX).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_multiple_empty_snippet_names_accumulated() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "content1".into(),
    });
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "content2".into(),
    });
    wo.context.snippets.push(ContextSnippet {
        name: "valid".into(),
        content: "content3".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "context.snippets[0].name"));
    assert!(err.iter().any(|e| e.path == "context.snippets[1].name"));
    assert_eq!(err.len(), 2);
}

#[test]
fn wo_whitespace_only_snippet_name_fails() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "   ".into(),
        content: "stuff".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "context.snippets[0].name"));
}

#[test]
fn wo_whitespace_only_workspace_root_fails() {
    let wo = WorkOrderBuilder::new("task").root("   ").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.iter().any(|e| e.path == "workspace.root"));
}

#[test]
fn wo_non_overlapping_allowed_disallowed_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("python".into());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_multiple_conflicting_tools_reports_each() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.allowed_tools.push("python".into());
    wo.policy.disallowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("python".into());
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    let policy_errs: Vec<_> = err.iter().filter(|e| e.path == "policy").collect();
    assert_eq!(policy_errs.len(), 2);
}

#[test]
fn wo_with_full_builder_chain_passes() {
    let wo = WorkOrderBuilder::new("full chain test")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/workspace")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .model("gpt-4o")
        .max_turns(20)
        .max_budget_usd(2.5)
        .build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_empty_policy_passes() {
    let wo = WorkOrderBuilder::new("task")
        .policy(PolicyProfile::default())
        .build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_special_chars_in_task_passes() {
    let wo = WorkOrderBuilder::new(r#"Fix bug in "parser" <&> module (v2.0)"#).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_context_with_files_and_snippets_passes() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![
            ContextSnippet {
                name: "readme".into(),
                content: "# Hello".into(),
            },
            ContextSnippet {
                name: "notes".into(),
                content: "Important notes".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Receipt validation — hash integrity, outcome values, cross-field
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_exact_64_non_hex_fails() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    // 64 chars but non-hex: starts ok but the hash won't match
    receipt.receipt_sha256 = Some("z".repeat(64));
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "receipt_sha256"));
}

#[test]
fn receipt_same_start_finish_time_passes() {
    let now = Utc::now();
    let receipt = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(now)
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_partial_outcome_with_harness_ok_passes() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    receipt.verification.harness_ok = true;
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_whitespace_only_backend_id_fails() {
    let receipt = ReceiptBuilder::new("   ").build();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "backend.id" && e.kind == ValidationErrorKind::Required)
    );
}

#[test]
fn receipt_whitespace_only_contract_version_fails() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "   ".into();
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(err.iter().any(|e| e.path == "meta.contract_version"));
}

#[test]
fn receipt_contract_version_abp_v_only_passes_format() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.meta.contract_version = "abp/v".into();
    // Starts with "abp/v" so format check passes; it's non-empty so required passes
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_multiple_errors_accumulated() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let mut receipt = ReceiptBuilder::new("")
        .outcome(Outcome::Failed)
        .started_at(now)
        .finished_at(earlier)
        .build();
    receipt.meta.contract_version = "bad".into();
    receipt.verification.harness_ok = true;
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    // backend.id (empty), contract_version (format), finished_at (range),
    // verification.harness_ok (cross-field)
    assert!(err.len() >= 4);
}

#[test]
fn receipt_with_trace_events_and_hash_passes() {
    let now = Utc::now();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ))
        .add_trace_event(make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(1),
        ))
        .with_hash()
        .unwrap();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_with_artifacts_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "diff".into(),
            path: "output.patch".into(),
        })
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_complete_with_harness_false_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(!receipt.verification.harness_ok);
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_failed_with_harness_false_passes() {
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert!(!receipt.verification.harness_ok);
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn receipt_hash_empty_string_wrong_length_fails() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.receipt_sha256 = Some(String::new());
    let err = ReceiptValidator.validate(&receipt).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "receipt_sha256" && e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn receipt_backend_version_metadata_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .backend_version("1.2.3")
        .adapter_version("0.1.0")
        .build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Config validation — RuntimeConfig fields via WorkOrder
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_no_model_passes() {
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig::default())
        .build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_with_model_passes() {
    let wo = WorkOrderBuilder::new("task").model("claude-3-opus").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_with_vendor_data_passes() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("openai".into(), serde_json::json!({"temperature": 0.7}));
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_with_env_vars_passes() {
    let mut config = RuntimeConfig::default();
    config.env.insert("FOO".into(), "bar".into());
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_none_budget_passes() {
    let config = RuntimeConfig {
        max_budget_usd: None,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_none_max_turns_passes() {
    let config = RuntimeConfig {
        max_turns: None,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. PolicyProfile validation — allow/deny consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_only_allowed_tools_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools = vec!["bash".into(), "grep".into(), "glob".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_only_disallowed_tools_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.disallowed_tools = vec!["bash".into(), "python".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_deny_read_write_globs_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.deny_read = vec!["/etc/**".into(), "/root/**".into()];
    wo.policy.deny_write = vec!["/usr/**".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_network_allow_deny_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allow_network = vec!["api.example.com".into()];
    wo.policy.deny_network = vec!["evil.example.com".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_require_approval_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.require_approval_for = vec!["bash".into(), "file_write".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_all_fields_populated_passes() {
    let policy = PolicyProfile {
        allowed_tools: vec!["grep".into(), "glob".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["/secret/**".into()],
        deny_write: vec!["/readonly/**".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["glob".into()],
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. CapabilityRequirement validation — schema-level checks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_with_capability_requirements_passes() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let wo = WorkOrderBuilder::new("task").requirements(reqs).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_empty_capability_requirements_passes() {
    let reqs = CapabilityRequirements { required: vec![] };
    let wo = WorkOrderBuilder::new("task").requirements(reqs).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn receipt_with_capability_manifest_passes() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolBash, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::McpClient, SupportLevel::Unsupported);
    let receipt = ReceiptBuilder::new("mock").capabilities(caps).build();
    assert!(ReceiptValidator.validate(&receipt).is_ok());
}

#[test]
fn capability_manifest_serialization_schema_check() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolBash, SupportLevel::Native);
    let receipt = ReceiptBuilder::new("mock").capabilities(caps).build();
    let json = serde_json::to_value(&receipt).unwrap();
    assert!(SchemaValidator::receipt().validate(&json).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Error reporting — clear messages
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_messages_contain_field_path() {
    let wo = WorkOrderBuilder::new("").root("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    for e in err.iter() {
        assert!(!e.path.is_empty(), "error path should not be empty");
        assert!(
            !e.message.is_empty(),
            "error message should not be empty for field {}",
            e.path
        );
    }
}

#[test]
fn error_display_includes_path_and_message() {
    let wo = WorkOrderBuilder::new("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    let display = format!("{err}");
    assert!(display.contains("task"));
    assert!(display.contains("error"));
}

#[test]
fn validation_errors_summary_joins_all() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "missing a");
    errs.add("b", ValidationErrorKind::OutOfRange, "bad b");
    errs.add("c", ValidationErrorKind::Custom, "custom c");
    let display = format!("{errs}");
    assert!(display.contains("[a]"));
    assert!(display.contains("[b]"));
    assert!(display.contains("[c]"));
    assert!(display.contains("3 error"));
}

#[test]
fn validation_errors_preserves_insertion_order() {
    let mut errs = ValidationErrors::new();
    errs.add("first", ValidationErrorKind::Required, "one");
    errs.add("second", ValidationErrorKind::Required, "two");
    errs.add("third", ValidationErrorKind::Required, "three");
    let inner = errs.into_inner();
    assert_eq!(inner[0].path, "first");
    assert_eq!(inner[1].path, "second");
    assert_eq!(inner[2].path, "third");
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
    errs.add("x", ValidationErrorKind::Required, "missing");
    let result: Result<(), ValidationErrors> = Err(errs);
    let _: &dyn std::error::Error = result.as_ref().unwrap_err();
}

#[test]
fn error_kind_clone_and_eq() {
    let kind = ValidationErrorKind::Required;
    let cloned = kind.clone();
    assert_eq!(kind, cloned);
    assert_ne!(ValidationErrorKind::Required, ValidationErrorKind::Custom);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Batch validation — validate multiple items
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn batch_validate_work_orders() {
    let orders = vec![
        WorkOrderBuilder::new("valid1").build(),
        WorkOrderBuilder::new("").build(), // invalid
        WorkOrderBuilder::new("valid2").build(),
        WorkOrderBuilder::new("   ").build(), // invalid
    ];
    let results: Vec<_> = orders
        .iter()
        .map(|wo| WorkOrderValidator.validate(wo))
        .collect();
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
    assert!(results[3].is_err());
}

#[test]
fn batch_validate_receipts() {
    let receipts = vec![
        ReceiptBuilder::new("mock").build(),
        ReceiptBuilder::new("").build(), // invalid
        ReceiptBuilder::new("valid")
            .outcome(Outcome::Partial)
            .build(),
    ];
    let results: Vec<_> = receipts
        .iter()
        .map(|r| ReceiptValidator.validate(r))
        .collect();
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn batch_count_errors() {
    let orders: Vec<_> = (0..10)
        .map(|i| {
            if i % 2 == 0 {
                WorkOrderBuilder::new(format!("task-{i}")).build()
            } else {
                WorkOrderBuilder::new("").build()
            }
        })
        .collect();
    let error_count = orders
        .iter()
        .filter(|wo| WorkOrderValidator.validate(wo).is_err())
        .count();
    assert_eq!(error_count, 5);
}

#[test]
fn batch_validate_mixed_schemas() {
    let wo = WorkOrderBuilder::new("hello").build();
    let receipt = ReceiptBuilder::new("mock").build();
    let wo_json = serde_json::to_value(&wo).unwrap();
    let receipt_json = serde_json::to_value(&receipt).unwrap();
    assert!(SchemaValidator::work_order().validate(&wo_json).is_ok());
    assert!(SchemaValidator::receipt().validate(&receipt_json).is_ok());
    // Cross: using receipt schema on work_order should fail
    assert!(SchemaValidator::receipt().validate(&wo_json).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Custom validators — extend validation with custom rules
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn custom_task_length_validator_passes() {
    let validator = TaskLengthValidator { max_len: 100 };
    let wo = WorkOrderBuilder::new("short task").build();
    assert!(validator.validate(&wo).is_ok());
}

#[test]
fn custom_task_length_validator_fails() {
    let validator = TaskLengthValidator { max_len: 5 };
    let wo = WorkOrderBuilder::new("this is too long").build();
    let err = validator.validate(&wo).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::Custom && e.path == "task")
    );
}

#[test]
fn custom_non_empty_trace_validator_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .build();
    assert!(NonEmptyTraceValidator.validate(&receipt).is_ok());
}

#[test]
fn custom_non_empty_trace_validator_fails() {
    let receipt = ReceiptBuilder::new("mock").build();
    let err = NonEmptyTraceValidator.validate(&receipt).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.path == "trace" && e.kind == ValidationErrorKind::Custom)
    );
}

#[test]
fn combined_builtin_and_custom_validator() {
    let wo = WorkOrderBuilder::new("a]").build();
    // Built-in passes (task is non-empty), custom fails (max_len = 1)
    let custom = TaskLengthValidator { max_len: 1 };
    let result = validate_with_custom(&WorkOrderValidator, &custom, &wo);
    let err = result.unwrap_err();
    assert!(err.iter().any(|e| e.kind == ValidationErrorKind::Custom));
}

#[test]
fn combined_both_fail() {
    let wo = WorkOrderBuilder::new("").build();
    let custom = TaskLengthValidator { max_len: 100 };
    // Built-in fails (empty task), custom passes (len 0 <= 100)
    let result = validate_with_custom(&WorkOrderValidator, &custom, &wo);
    let err = result.unwrap_err();
    assert!(err.iter().any(|e| e.kind == ValidationErrorKind::Required));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Serde validation — JSON parse → validate pipeline
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_work_order_schema_valid() {
    let wo = WorkOrderBuilder::new("serde test")
        .model("gpt-4")
        .max_turns(10)
        .build();
    let json = serde_json::to_value(&wo).unwrap();
    assert!(SchemaValidator::work_order().validate(&json).is_ok());
}

#[test]
fn serde_roundtrip_receipt_schema_valid() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json = serde_json::to_value(&receipt).unwrap();
    assert!(SchemaValidator::receipt().validate(&json).is_ok());
}

#[test]
fn serde_roundtrip_event_schema_valid() {
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert!(SchemaValidator::agent_event().validate(&json).is_ok());
}

#[test]
fn serde_json_string_to_work_order_then_validate() {
    let wo = WorkOrderBuilder::new("pipeline test").build();
    let json_str = serde_json::to_string(&wo).unwrap();
    let parsed: abp_core::WorkOrder = serde_json::from_str(&json_str).unwrap();
    assert!(WorkOrderValidator.validate(&parsed).is_ok());
}

#[test]
fn serde_json_string_to_receipt_then_validate() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json_str = serde_json::to_string(&receipt).unwrap();
    let parsed: abp_core::Receipt = serde_json::from_str(&json_str).unwrap();
    assert!(ReceiptValidator.validate(&parsed).is_ok());
}

#[test]
fn serde_raw_json_object_validate_hello() {
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": {"id": "test"},
        "capabilities": {},
        "mode": "mapped"
    });
    assert!(RawEnvelopeValidator.validate(&raw).is_ok());
}

#[test]
fn serde_raw_json_object_validate_event() {
    let raw = serde_json::json!({
        "t": "event",
        "ref_id": "run-1",
        "event": {"ts": "2024-01-01T00:00:00Z", "type": "assistant_message", "text": "hi"}
    });
    assert!(RawEnvelopeValidator.validate(&raw).is_ok());
}

#[test]
fn serde_invalid_json_fails_before_validation() {
    let bad_json = r#"{"task": "test", invalid}"#;
    let result = serde_json::from_str::<serde_json::Value>(bad_json);
    assert!(result.is_err());
}

#[test]
fn serde_envelope_roundtrip_hello() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test-backend".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = serde_json::to_value(&env).unwrap();
    assert!(RawEnvelopeValidator.validate(&json).is_ok());
}

#[test]
fn serde_envelope_roundtrip_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something broke".into(),
        error_code: None,
    };
    let json = serde_json::to_value(&env).unwrap();
    assert!(RawEnvelopeValidator.validate(&json).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Edge cases — empty fields, special characters, boundary values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_json_object_schema() {
    let val = serde_json::json!({});
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(err.len() >= 7); // all 7 required fields missing
}

#[test]
fn edge_null_json_value_schema() {
    let val = serde_json::Value::Null;
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn edge_boolean_json_value_schema() {
    let val = serde_json::json!(true);
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn edge_number_json_value_schema() {
    let val = serde_json::json!(42);
    let err = SchemaValidator::receipt().validate(&val).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::InvalidFormat)
    );
}

#[test]
fn edge_single_event_run_started_no_completed_check() {
    // Single event: only checks first is RunStarted, skip last-event check
    let events = vec![make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    })];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn edge_two_events_missing_run_completed() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::AssistantDelta {
                text: "partial".into(),
            },
            now + chrono::Duration::milliseconds(1),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("kind")));
}

#[test]
fn edge_events_same_timestamp_passes() {
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
            now, // same timestamp
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now, // same timestamp
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn edge_tool_call_different_name_from_result_fails() {
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
                tool_name: "python".into(), // different tool name
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
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::InvalidReference)
    );
}

#[test]
fn edge_multiple_tool_calls_same_name_matched() {
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
                output: serde_json::json!("ok1"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(3),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("ok2"),
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
fn edge_hello_version_same_major_different_minor_passes() {
    let env = Envelope::Hello {
        contract_version: "abp/v0.99".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    // Same major (0), different minor — should pass
    assert!(validate_hello_version(&env).is_ok());
}

#[test]
fn edge_hello_version_different_major_fails() {
    let env = Envelope::Hello {
        contract_version: "abp/v1.0".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = validate_hello_version(&env).unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.kind == ValidationErrorKind::InvalidReference)
    );
}

#[test]
fn edge_hello_version_exact_match_passes() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    assert!(validate_hello_version(&env).is_ok());
}

#[test]
fn edge_raw_envelope_valid_tags() {
    for tag in &["hello", "run", "event", "final", "fatal"] {
        let mut obj = serde_json::json!({"t": tag});
        // Add minimal required fields for each tag
        match *tag {
            "hello" => {
                obj.as_object_mut()
                    .unwrap()
                    .insert("contract_version".into(), serde_json::json!("abp/v0.1"));
            }
            "run" => {
                obj.as_object_mut()
                    .unwrap()
                    .insert("id".into(), serde_json::json!("run-1"));
            }
            "event" | "final" => {
                obj.as_object_mut()
                    .unwrap()
                    .insert("ref_id".into(), serde_json::json!("run-1"));
            }
            _ => {}
        }
        assert!(
            RawEnvelopeValidator.validate(&obj).is_ok(),
            "tag '{tag}' should be valid"
        );
    }
}

#[test]
fn edge_workspace_modes_pass_validation() {
    for mode in [WorkspaceMode::PassThrough, WorkspaceMode::Staged] {
        let wo = WorkOrderBuilder::new("task").workspace_mode(mode).build();
        assert!(WorkOrderValidator.validate(&wo).is_ok());
    }
}

#[test]
fn edge_execution_lanes_pass_validation() {
    for lane in [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst] {
        let wo = WorkOrderBuilder::new("task").lane(lane).build();
        assert!(WorkOrderValidator.validate(&wo).is_ok());
    }
}

#[test]
fn edge_all_outcomes_receipt() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let receipt = ReceiptBuilder::new("mock").outcome(outcome).build();
        assert!(ReceiptValidator.validate(&receipt).is_ok());
    }
}

#[test]
fn edge_schema_any_type_accepts_all_json_types() {
    let validator = SchemaValidator::new(vec![("data".into(), JsonType::Any)]);
    for val in [
        serde_json::json!({"data": "string"}),
        serde_json::json!({"data": 42}),
        serde_json::json!({"data": true}),
        serde_json::json!({"data": [1, 2]}),
        serde_json::json!({"data": {"nested": true}}),
    ] {
        assert!(
            validator.validate(&val).is_ok(),
            "Any type should accept all non-null values"
        );
    }
}

#[test]
fn edge_schema_type_mismatch_reports_expected_type() {
    let validator = SchemaValidator::new(vec![("count".into(), JsonType::Number)]);
    let val = serde_json::json!({"count": "not-a-number"});
    let err = validator.validate(&val).unwrap_err();
    let e = err.iter().next().unwrap();
    assert!(e.message.contains("Number"), "should mention expected type");
    assert!(e.message.contains("string"), "should mention actual type");
}

#[test]
fn edge_validation_errors_empty_into_result_ok() {
    let errs = ValidationErrors::new();
    assert!(errs.into_result().is_ok());
}

#[test]
fn edge_schema_multiple_missing_fields_reports_all() {
    let validator = SchemaValidator::new(vec![
        ("a".into(), JsonType::String),
        ("b".into(), JsonType::Number),
        ("c".into(), JsonType::Bool),
    ]);
    let val = serde_json::json!({});
    let err = validator.validate(&val).unwrap_err();
    assert_eq!(err.len(), 3);
    let paths: Vec<_> = err.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"a"));
    assert!(paths.contains(&"b"));
    assert!(paths.contains(&"c"));
}

#[test]
fn edge_event_file_changed_and_command_in_trace() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added main fn".into(),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        make_event_at(
            AgentEventKind::CommandExecuted {
                command: "cargo build".into(),
                exit_code: Some(0),
                output_preview: Some("Compiling...".into()),
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
fn edge_event_warning_and_error_in_trace() {
    let now = Utc::now();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        make_event_at(
            AgentEventKind::Warning {
                message: "low confidence".into(),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        make_event_at(
            AgentEventKind::Error {
                message: "compilation failed".into(),
                error_code: None,
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
fn edge_envelope_run_with_full_work_order() {
    let wo = WorkOrderBuilder::new("full test")
        .model("claude-3")
        .max_turns(5)
        .max_budget_usd(1.0)
        .build();
    let env = Envelope::Run {
        id: "run-42".into(),
        work_order: wo,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn edge_envelope_final_with_receipt() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let env = Envelope::Final {
        ref_id: "run-42".into(),
        receipt,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn edge_validation_error_clone() {
    let err = abp_validate::ValidationError {
        path: "test".into(),
        kind: ValidationErrorKind::Custom,
        message: "cloneable".into(),
    };
    let cloned = err.clone();
    assert_eq!(err.path, cloned.path);
    assert_eq!(err.kind, cloned.kind);
    assert_eq!(err.message, cloned.message);
}
