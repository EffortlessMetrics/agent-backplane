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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Comprehensive validation tests for abp-validate covering work orders,
//! receipts, envelopes, events, schema checks, policy, capabilities,
//! error messages, batch validation, custom validators, and serde pipelines.

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextSnippet,
    ExecutionMode, MinSupport, Outcome, ReceiptBuilder, SupportLevel, WorkOrderBuilder,
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

fn ev(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn ev_at(kind: AgentEventKind, ts: chrono::DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: None,
        adapter_version: None,
    }
}

fn assert_has_path(errs: &ValidationErrors, path: &str) {
    assert!(
        errs.iter().any(|e| e.path == path),
        "expected error at path '{}', got: {:?}",
        path,
        errs.iter().map(|e| &e.path).collect::<Vec<_>>()
    );
}

fn assert_has_kind(errs: &ValidationErrors, kind: &ValidationErrorKind) {
    assert!(
        errs.iter().any(|e| &e.kind == kind),
        "expected error kind '{kind}', got: {:?}",
        errs.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. WorkOrder validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_valid_minimal_passes() {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_empty_task_required_error() {
    let wo = WorkOrderBuilder::new("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "task");
    assert_has_kind(&err, &ValidationErrorKind::Required);
}

#[test]
fn wo_whitespace_only_task_fails() {
    let wo = WorkOrderBuilder::new("   \n\t  ").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "task");
}

#[test]
fn wo_uuid_present_in_built_order() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(!wo.id.is_nil());
}

#[test]
fn wo_task_with_special_chars_passes() {
    let wo = WorkOrderBuilder::new("Fix <html> & \"quotes\" in output").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_task_with_emoji_passes() {
    let wo = WorkOrderBuilder::new("Deploy 🚀 to production").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_task_very_long_passes() {
    let wo = WorkOrderBuilder::new("x".repeat(50_000)).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_empty_workspace_root_required() {
    let wo = WorkOrderBuilder::new("task").root("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "workspace.root");
    assert_has_kind(&err, &ValidationErrorKind::Required);
}

#[test]
fn wo_whitespace_workspace_root_fails() {
    let wo = WorkOrderBuilder::new("task").root("  \t").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "workspace.root");
}

#[test]
fn wo_negative_budget_out_of_range() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(-0.01).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "config.max_budget_usd");
    assert_has_kind(&err, &ValidationErrorKind::OutOfRange);
}

#[test]
fn wo_nan_budget_invalid_format() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::NAN)
        .build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "config.max_budget_usd");
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn wo_inf_budget_out_of_range() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::NEG_INFINITY)
        .build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "config.max_budget_usd");
}

#[test]
fn wo_zero_budget_passes() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(0.0).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_zero_max_turns_out_of_range() {
    let wo = WorkOrderBuilder::new("task").max_turns(0).build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "config.max_turns");
    assert_has_kind(&err, &ValidationErrorKind::OutOfRange);
}

#[test]
fn wo_max_turns_one_passes() {
    let wo = WorkOrderBuilder::new("task").max_turns(1).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_snippet_empty_name_required() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "data".into(),
    });
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "context.snippets[0].name");
}

#[test]
fn wo_multiple_empty_snippet_names_all_reported() {
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
    assert!(err.len() >= 2);
    assert_has_path(&err, "context.snippets[0].name");
    assert_has_path(&err, "context.snippets[1].name");
}

#[test]
fn wo_conflicting_policy_tools_invalid_reference() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("bash".into());
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert_has_path(&err, "policy");
    assert_has_kind(&err, &ValidationErrorKind::InvalidReference);
}

#[test]
fn wo_non_overlapping_tools_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools.push("read".into());
    wo.policy.disallowed_tools.push("bash".into());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn wo_accumulates_multiple_errors() {
    let mut wo = WorkOrderBuilder::new("")
        .root("")
        .max_budget_usd(-1.0)
        .max_turns(0)
        .build();
    wo.context.snippets.push(ContextSnippet {
        name: "".into(),
        content: "x".into(),
    });
    wo.policy.allowed_tools.push("bash".into());
    wo.policy.disallowed_tools.push("bash".into());
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    assert!(err.len() >= 5, "expected >=5 errors, got {}", err.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Receipt validation — hash integrity and outcome consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_valid_passes() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn receipt_correct_hash_passes() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn receipt_tampered_hash_invalid_reference() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("0".repeat(64));
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "receipt_sha256");
    assert_has_kind(&err, &ValidationErrorKind::InvalidReference);
}

#[test]
fn receipt_short_hash_invalid_format() {
    let mut r = ReceiptBuilder::new("mock").build();
    r.receipt_sha256 = Some("abc123".into());
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "receipt_sha256");
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn receipt_no_hash_passes() {
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.receipt_sha256.is_none());
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn receipt_empty_backend_id_required() {
    let r = ReceiptBuilder::new("").build();
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "backend.id");
    assert_has_kind(&err, &ValidationErrorKind::Required);
}

#[test]
fn receipt_whitespace_backend_id_fails() {
    let r = ReceiptBuilder::new("  \t").build();
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "backend.id");
}

#[test]
fn receipt_empty_contract_version_fails() {
    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.contract_version = "".into();
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "meta.contract_version");
}

#[test]
fn receipt_bad_contract_version_format() {
    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.contract_version = "xyz/1.0".into();
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "meta.contract_version");
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn receipt_finished_before_started_out_of_range() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let r = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(earlier)
        .build();
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "meta.finished_at");
    assert_has_kind(&err, &ValidationErrorKind::OutOfRange);
}

#[test]
fn receipt_same_start_finish_passes() {
    let now = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(now)
        .build();
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn receipt_failed_with_harness_ok_cross_field_error() {
    let mut r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    r.verification.harness_ok = true;
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "verification.harness_ok");
    assert_has_kind(&err, &ValidationErrorKind::InvalidReference);
}

#[test]
fn receipt_complete_with_harness_ok_passes() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.verification.harness_ok = true;
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn receipt_partial_with_harness_ok_passes() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    r.verification.harness_ok = true;
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn receipt_all_outcomes_valid_without_harness() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let r = ReceiptBuilder::new("mock").outcome(outcome).build();
        assert!(ReceiptValidator.validate(&r).is_ok());
    }
}

#[test]
fn receipt_with_artifacts_passes() {
    let r = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "changes.diff".into(),
        })
        .build();
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn receipt_multiple_errors_accumulated() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let mut r = ReceiptBuilder::new("")
        .started_at(now)
        .finished_at(earlier)
        .outcome(Outcome::Failed)
        .build();
    r.meta.contract_version = "bad".into();
    r.verification.harness_ok = true;
    r.receipt_sha256 = Some("short".into());
    let err = ReceiptValidator.validate(&r).unwrap_err();
    assert!(err.len() >= 4, "expected >=4 errors, got {}", err.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Config validation — budget, turns, model
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_none_budget_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.max_budget_usd.is_none());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_none_max_turns_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.max_turns.is_none());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_with_model_passes() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4o").build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_large_budget_passes() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(999_999.99)
        .build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_large_max_turns_passes() {
    let wo = WorkOrderBuilder::new("task").max_turns(1_000_000).build();
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn config_positive_infinity_budget_passes() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(f64::INFINITY)
        .build();
    // +Infinity is >= 0 and not NaN, so it passes basic validation
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Policy validation — tool conflicts, patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_empty_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_only_allowed_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools = vec!["read".into(), "write".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_only_disallowed_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.disallowed_tools = vec!["bash".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_multiple_conflicts_each_reported() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allowed_tools = vec!["bash".into(), "read".into()];
    wo.policy.disallowed_tools = vec!["bash".into(), "read".into()];
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    let policy_errors: Vec<_> = err.iter().filter(|e| e.path == "policy").collect();
    assert!(
        policy_errors.len() >= 2,
        "expected >=2 policy errors, got {}",
        policy_errors.len()
    );
}

#[test]
fn policy_deny_read_write_globs_pass_validation() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.deny_read = vec!["**/.env".into(), "**/secrets/**".into()];
    wo.policy.deny_write = vec!["**/node_modules/**".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_network_allow_deny_pass_validation() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.allow_network = vec!["*.example.com".into()];
    wo.policy.deny_network = vec!["*.evil.com".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn policy_require_approval_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.policy.require_approval_for = vec!["bash".into(), "write".into()];
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Capability validation — manifest completeness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_empty_manifest_passes() {
    let manifest = CapabilityManifest::new();
    let r = ReceiptBuilder::new("mock").capabilities(manifest).build();
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn capability_manifest_with_entries_passes() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
    manifest.insert(Capability::ToolBash, SupportLevel::Unsupported);
    let r = ReceiptBuilder::new("mock").capabilities(manifest).build();
    assert!(ReceiptValidator.validate(&r).is_ok());
}

#[test]
fn capability_requirements_in_work_order_passes() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    assert!(WorkOrderValidator.validate(&wo).is_ok());
}

#[test]
fn capability_manifest_schema_check() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolWrite, SupportLevel::Native);
    let val = serde_json::to_value(&manifest).unwrap();
    assert!(val.is_object());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Envelope validation — protocol compliance
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_valid_hello_passes() {
    let env = Envelope::hello(backend("test"), CapabilityManifest::new());
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_hello_empty_contract_version_fails() {
    let env = Envelope::Hello {
        contract_version: "".into(),
        backend: backend("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert_has_path(&err, "contract_version");
}

#[test]
fn envelope_hello_bad_version_prefix_fails() {
    let env = Envelope::Hello {
        contract_version: "v1.0".into(),
        backend: backend("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert_has_path(&err, "contract_version");
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn envelope_hello_empty_backend_id_fails() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: backend(""),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert_has_path(&err, "backend.id");
}

#[test]
fn envelope_run_empty_id_fails() {
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "".into(),
        work_order: wo,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert_has_path(&err, "id");
}

#[test]
fn envelope_run_empty_task_fails() {
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert_has_path(&err, "work_order.task");
}

#[test]
fn envelope_run_valid_passes() {
    let wo = WorkOrderBuilder::new("do stuff").build();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_event_empty_ref_id_fails() {
    let env = Envelope::Event {
        ref_id: "".into(),
        event: ev(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert_has_path(&err, "ref_id");
}

#[test]
fn envelope_event_valid_passes() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: ev(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn envelope_final_empty_ref_id_fails() {
    let r = ReceiptBuilder::new("mock").build();
    let env = Envelope::Final {
        ref_id: "".into(),
        receipt: r,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert_has_path(&err, "ref_id");
}

#[test]
fn envelope_fatal_empty_error_fails() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "".into(),
        error_code: None,
    };
    let err = EnvelopeValidator.validate(&env).unwrap_err();
    assert_has_path(&err, "error");
}

#[test]
fn envelope_fatal_valid_passes() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "process crashed".into(),
        error_code: None,
    };
    assert!(EnvelopeValidator.validate(&env).is_ok());
}

#[test]
fn hello_version_exact_match_passes() {
    let env = Envelope::hello(backend("test"), CapabilityManifest::new());
    assert!(validate_hello_version(&env).is_ok());
}

#[test]
fn hello_version_incompatible_major_fails() {
    let env = Envelope::Hello {
        contract_version: "abp/v99.0".into(),
        backend: backend("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let err = validate_hello_version(&env).unwrap_err();
    assert_has_kind(&err, &ValidationErrorKind::InvalidReference);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6b. Raw envelope validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn raw_envelope_missing_t_tag_fails() {
    let val = serde_json::json!({"ref_id": "run-1"});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert_has_path(&err, "t");
    assert_has_kind(&err, &ValidationErrorKind::Required);
}

#[test]
fn raw_envelope_unknown_tag_fails() {
    let val = serde_json::json!({"t": "unknown_type"});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert_has_path(&err, "t");
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn raw_envelope_numeric_tag_fails() {
    let val = serde_json::json!({"t": 42});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert_has_path(&err, "t");
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn raw_envelope_not_object_fails() {
    let val = serde_json::json!([1, 2, 3]);
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn raw_hello_missing_contract_version_fails() {
    let val = serde_json::json!({"t": "hello", "backend": {"id": "x"}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert_has_path(&err, "contract_version");
}

#[test]
fn raw_run_missing_id_fails() {
    let val = serde_json::json!({"t": "run", "work_order": {}});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert_has_path(&err, "id");
}

#[test]
fn raw_event_missing_ref_id_fails() {
    let val = serde_json::json!({"t": "event"});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert_has_path(&err, "ref_id");
}

#[test]
fn raw_final_missing_ref_id_fails() {
    let val = serde_json::json!({"t": "final"});
    let err = RawEnvelopeValidator.validate(&val).unwrap_err();
    assert_has_path(&err, "ref_id");
}

#[test]
fn raw_fatal_valid_passes() {
    let val = serde_json::json!({"t": "fatal", "error": "oops"});
    assert!(RawEnvelopeValidator.validate(&val).is_ok());
}

#[test]
fn raw_all_valid_tags_accepted() {
    for tag in &["hello", "run", "event", "final", "fatal"] {
        let mut val = serde_json::json!({"t": tag});
        // Add minimum required fields per tag
        match *tag {
            "hello" => {
                val["contract_version"] = serde_json::json!("abp/v0.1");
            }
            "run" => {
                val["id"] = serde_json::json!("r1");
            }
            "event" | "final" => {
                val["ref_id"] = serde_json::json!("r1");
            }
            _ => {}
        }
        assert!(
            RawEnvelopeValidator.validate(&val).is_ok(),
            "tag '{tag}' should be valid"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6c. Event sequence validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn events_empty_sequence_passes() {
    let events: Vec<AgentEvent> = vec![];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_valid_full_sequence_passes() {
    let now = Utc::now();
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        ev_at(
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "main.rs"}),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        ev_at(
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: None,
                output: serde_json::json!("fn main() {}"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(2),
        ),
        ev_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(3),
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_non_monotonic_timestamps_fail() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::hours(1);
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        ev_at(
            AgentEventKind::AssistantMessage { text: "hi".into() },
            earlier,
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert_has_path(&err, "events[1].ts");
    assert_has_kind(&err, &ValidationErrorKind::OutOfRange);
}

#[test]
fn events_same_timestamp_passes() {
    let now = Utc::now();
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        ev_at(AgentEventKind::AssistantMessage { text: "hi".into() }, now),
        ev_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now,
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_first_not_run_started_fails() {
    let events = vec![ev(AgentEventKind::AssistantMessage { text: "hi".into() })];
    let err = EventValidator.validate(&events).unwrap_err();
    assert_has_path(&err, "events[0].kind");
}

#[test]
fn events_last_not_run_completed_fails() {
    let now = Utc::now();
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        ev_at(
            AgentEventKind::AssistantMessage { text: "hi".into() },
            now + chrono::Duration::milliseconds(1),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert!(err.iter().any(|e| e.path.contains("kind")));
}

#[test]
fn events_orphan_tool_result_fails() {
    let now = Utc::now();
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        ev_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(1),
        ),
        ev_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(2),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert_has_kind(&err, &ValidationErrorKind::InvalidReference);
}

#[test]
fn events_tool_result_wrong_name_fails() {
    let now = Utc::now();
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        ev_at(
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        ev_at(
            AgentEventKind::ToolResult {
                tool_name: "write".into(),
                tool_use_id: None,
                output: serde_json::json!("ok"),
                is_error: false,
            },
            now + chrono::Duration::milliseconds(2),
        ),
        ev_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(3),
        ),
    ];
    let err = EventValidator.validate(&events).unwrap_err();
    assert_has_kind(&err, &ValidationErrorKind::InvalidReference);
}

#[test]
fn events_file_changed_and_command_in_trace_passes() {
    let now = Utc::now();
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        ev_at(
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added fn".into(),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        ev_at(
            AgentEventKind::CommandExecuted {
                command: "cargo build".into(),
                exit_code: Some(0),
                output_preview: None,
            },
            now + chrono::Duration::milliseconds(2),
        ),
        ev_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(3),
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

#[test]
fn events_warning_and_error_in_trace_passes() {
    let now = Utc::now();
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            now,
        ),
        ev_at(
            AgentEventKind::Warning {
                message: "low disk".into(),
            },
            now + chrono::Duration::milliseconds(1),
        ),
        ev_at(
            AgentEventKind::Error {
                message: "compilation failed".into(),
                error_code: None,
            },
            now + chrono::Duration::milliseconds(2),
        ),
        ev_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            now + chrono::Duration::milliseconds(3),
        ),
    ];
    assert!(EventValidator.validate(&events).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Error messages — clear, actionable
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_messages_contain_field_path() {
    let wo = WorkOrderBuilder::new("").root("").build();
    let err = WorkOrderValidator.validate(&wo).unwrap_err();
    for e in err.iter() {
        assert!(!e.path.is_empty(), "error path should not be empty");
        assert!(!e.message.is_empty(), "error message should not be empty");
    }
}

#[test]
fn error_display_includes_path_and_message() {
    let e = abp_validate::ValidationError {
        path: "config.max_budget_usd".into(),
        kind: ValidationErrorKind::OutOfRange,
        message: "must not be negative".into(),
    };
    let display = format!("{e}");
    assert!(display.contains("config.max_budget_usd"));
    assert!(display.contains("must not be negative"));
}

#[test]
fn error_kind_display_matches_snake_case() {
    assert_eq!(ValidationErrorKind::Required.to_string(), "required");
    assert_eq!(
        ValidationErrorKind::InvalidFormat.to_string(),
        "invalid_format"
    );
    assert_eq!(ValidationErrorKind::OutOfRange.to_string(), "out_of_range");
    assert_eq!(
        ValidationErrorKind::InvalidReference.to_string(),
        "invalid_reference"
    );
    assert_eq!(ValidationErrorKind::Custom.to_string(), "custom");
}

#[test]
fn errors_summary_joins_all() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "missing a");
    errs.add("b", ValidationErrorKind::Custom, "bad b");
    let msg = format!("{errs}");
    assert!(msg.contains("2 error"));
    assert!(msg.contains("[a]"));
    assert!(msg.contains("[b]"));
}

#[test]
fn errors_preserves_insertion_order() {
    let mut errs = ValidationErrors::new();
    errs.add("first", ValidationErrorKind::Required, "f");
    errs.add("second", ValidationErrorKind::Custom, "s");
    errs.add("third", ValidationErrorKind::OutOfRange, "t");
    let inner = errs.into_inner();
    assert_eq!(inner[0].path, "first");
    assert_eq!(inner[1].path, "second");
    assert_eq!(inner[2].path, "third");
}

#[test]
fn validation_error_implements_std_error() {
    let e = abp_validate::ValidationError {
        path: "x".into(),
        kind: ValidationErrorKind::Required,
        message: "missing".into(),
    };
    let _: &dyn std::error::Error = &e;
}

#[test]
fn validation_errors_implements_std_error() {
    let mut errs = ValidationErrors::new();
    errs.add("x", ValidationErrorKind::Required, "missing");
    let as_err: &dyn std::error::Error = &errs;
    assert!(!as_err.to_string().is_empty());
}

#[test]
fn errors_default_is_empty() {
    let errs = ValidationErrors::default();
    assert!(errs.is_empty());
    assert_eq!(errs.len(), 0);
    assert!(errs.into_result().is_ok());
}

#[test]
fn error_kind_clone_and_eq() {
    let a = ValidationErrorKind::Required;
    let b = a.clone();
    assert_eq!(a, b);
    assert_ne!(ValidationErrorKind::Required, ValidationErrorKind::Custom);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Batch validation — validate multiple items
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn batch_validate_work_orders() {
    let orders = vec![
        WorkOrderBuilder::new("good task").build(),
        WorkOrderBuilder::new("").build(),
        WorkOrderBuilder::new("another good task").build(),
    ];
    let results: Vec<_> = orders
        .iter()
        .map(|wo| WorkOrderValidator.validate(wo))
        .collect();
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn batch_validate_receipts() {
    let receipts = vec![
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build(),
        ReceiptBuilder::new("").build(),
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap(),
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
fn batch_count_total_errors() {
    let orders = vec![
        WorkOrderBuilder::new("").root("").build(),
        WorkOrderBuilder::new("ok").build(),
        WorkOrderBuilder::new("").build(),
    ];
    let total_errors: usize = orders
        .iter()
        .filter_map(|wo| WorkOrderValidator.validate(wo).err())
        .map(|e| e.len())
        .sum();
    assert!(total_errors >= 3); // 2 from first + 1 from third
}

#[test]
fn batch_validate_envelopes() {
    let envelopes = vec![
        Envelope::hello(backend("test"), CapabilityManifest::new()),
        Envelope::Fatal {
            ref_id: None,
            error: "".into(),
            error_code: None,
        },
        Envelope::Event {
            ref_id: "run-1".into(),
            event: ev(AgentEventKind::AssistantMessage { text: "hi".into() }),
        },
    ];
    let results: Vec<_> = envelopes
        .iter()
        .map(|e| EnvelopeValidator.validate(e))
        .collect();
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn batch_validate_schemas_mixed() {
    let schemas = vec![
        (SchemaValidator::work_order(), serde_json::json!({})),
        (
            SchemaValidator::receipt(),
            serde_json::to_value(ReceiptBuilder::new("m").build()).unwrap(),
        ),
        (
            SchemaValidator::agent_event(),
            serde_json::json!({"ts": "now"}),
        ),
    ];
    let results: Vec<_> = schemas.iter().map(|(v, val)| v.validate(val)).collect();
    assert!(results[0].is_err()); // empty object missing fields
    assert!(results[1].is_ok()); // valid receipt
    assert!(results[2].is_err()); // missing "type"
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Custom validators — composable validation rules
// ═══════════════════════════════════════════════════════════════════════════

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

fn compose_validators<T>(
    validators: &[&dyn Validator<T>],
    value: &T,
) -> Result<(), ValidationErrors> {
    let mut all = ValidationErrors::new();
    for v in validators {
        if let Err(e) = v.validate(value) {
            for err in e.iter() {
                all.push(err.clone());
            }
        }
    }
    all.into_result()
}

#[test]
fn custom_task_length_passes() {
    let v = TaskLengthValidator { max_len: 100 };
    let wo = WorkOrderBuilder::new("short task").build();
    assert!(v.validate(&wo).is_ok());
}

#[test]
fn custom_task_length_fails() {
    let v = TaskLengthValidator { max_len: 5 };
    let wo = WorkOrderBuilder::new("this is too long").build();
    let err = v.validate(&wo).unwrap_err();
    assert_has_path(&err, "task");
    assert_has_kind(&err, &ValidationErrorKind::Custom);
}

#[test]
fn custom_non_empty_trace_passes() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(ev(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .build();
    assert!(NonEmptyTraceValidator.validate(&r).is_ok());
}

#[test]
fn custom_non_empty_trace_fails() {
    let r = ReceiptBuilder::new("mock").build();
    let err = NonEmptyTraceValidator.validate(&r).unwrap_err();
    assert_has_path(&err, "trace");
}

#[test]
fn compose_builtin_and_custom_both_pass() {
    let wo = WorkOrderBuilder::new("ok").build();
    let builtin = WorkOrderValidator;
    let custom = TaskLengthValidator { max_len: 100 };
    let validators: Vec<&dyn Validator<abp_core::WorkOrder>> = vec![&builtin, &custom];
    assert!(compose_validators(&validators, &wo).is_ok());
}

#[test]
fn compose_builtin_passes_custom_fails() {
    let wo = WorkOrderBuilder::new("long task name").build();
    let builtin = WorkOrderValidator;
    let custom = TaskLengthValidator { max_len: 5 };
    let validators: Vec<&dyn Validator<abp_core::WorkOrder>> = vec![&builtin, &custom];
    let err = compose_validators(&validators, &wo).unwrap_err();
    assert_has_kind(&err, &ValidationErrorKind::Custom);
}

#[test]
fn compose_both_fail_accumulates() {
    let wo = WorkOrderBuilder::new("").build();
    let builtin = WorkOrderValidator;
    let custom = TaskLengthValidator { max_len: 1000 };
    // builtin will fail (empty task), custom passes (0 < 1000)
    let validators: Vec<&dyn Validator<abp_core::WorkOrder>> = vec![&builtin, &custom];
    let err = compose_validators(&validators, &wo).unwrap_err();
    assert_has_kind(&err, &ValidationErrorKind::Required);
}

#[test]
fn compose_receipt_validators() {
    let r = ReceiptBuilder::new("mock").build();
    let builtin = ReceiptValidator;
    let custom = NonEmptyTraceValidator;
    let validators: Vec<&dyn Validator<abp_core::Receipt>> = vec![&builtin, &custom];
    let err = compose_validators(&validators, &r).unwrap_err();
    assert_has_path(&err, "trace");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Serde validation — invalid JSON shapes caught
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_work_order_schema_valid() {
    let wo = WorkOrderBuilder::new("refactor").model("gpt-4").build();
    let val = serde_json::to_value(&wo).unwrap();
    assert!(SchemaValidator::work_order().validate(&val).is_ok());
}

#[test]
fn serde_roundtrip_receipt_schema_valid() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let val = serde_json::to_value(&r).unwrap();
    assert!(SchemaValidator::receipt().validate(&val).is_ok());
}

#[test]
fn serde_roundtrip_event_schema_valid() {
    let event = ev(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let val = serde_json::to_value(&event).unwrap();
    assert!(SchemaValidator::agent_event().validate(&val).is_ok());
}

#[test]
fn serde_invalid_json_string_fails_parse() {
    let bad_json = "{ this is not valid json }";
    assert!(serde_json::from_str::<serde_json::Value>(bad_json).is_err());
}

#[test]
fn serde_json_task_as_number_schema_fails() {
    let val = serde_json::json!({
        "id": "some-uuid",
        "task": 42,
        "lane": "patch_first",
        "workspace": {},
        "context": {},
        "policy": {},
        "config": {},
    });
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert_has_path(&err, "task");
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn serde_json_null_task_schema_fails() {
    let val = serde_json::json!({
        "id": "some-uuid",
        "task": null,
        "lane": "patch_first",
        "workspace": {},
        "context": {},
        "policy": {},
        "config": {},
    });
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert_has_path(&err, "task");
    assert_has_kind(&err, &ValidationErrorKind::Required);
}

#[test]
fn serde_json_missing_all_fields_reports_all() {
    let val = serde_json::json!({});
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert!(
        err.len() >= 5,
        "expected >=5 missing fields, got {}",
        err.len()
    );
}

#[test]
fn serde_receipt_missing_meta_schema_fails() {
    let val = serde_json::json!({
        "backend": {"id": "mock"},
        "outcome": "complete",
        "trace": [],
        "artifacts": [],
    });
    let err = SchemaValidator::receipt().validate(&val).unwrap_err();
    assert_has_path(&err, "meta");
}

#[test]
fn serde_event_missing_type_schema_fails() {
    let val = serde_json::json!({"ts": "2024-01-01T00:00:00Z"});
    let err = SchemaValidator::agent_event().validate(&val).unwrap_err();
    assert_has_path(&err, "type");
}

#[test]
fn serde_schema_not_object_fails() {
    let val = serde_json::json!("just a string");
    let err = SchemaValidator::work_order().validate(&val).unwrap_err();
    assert_has_kind(&err, &ValidationErrorKind::InvalidFormat);
}

#[test]
fn serde_schema_custom_type_checks() {
    let v = SchemaValidator::new(vec![
        ("name".into(), JsonType::String),
        ("count".into(), JsonType::Number),
        ("active".into(), JsonType::Bool),
        ("items".into(), JsonType::Array),
        ("meta".into(), JsonType::Object),
        ("data".into(), JsonType::Any),
    ]);
    let val = serde_json::json!({
        "name": "test",
        "count": 42,
        "active": true,
        "items": [1, 2],
        "meta": {"k": "v"},
        "data": null,
    });
    // "data" is null but Any requires non-null — checks null handling
    // Actually JsonType::Any accepts anything that's not null? Let's check the schema validator
    // From code: null is caught before type check, so null for "data" (Any) fails with Required
    let err = v.validate(&val).unwrap_err();
    assert_has_path(&err, "data");
}

#[test]
fn serde_schema_all_types_correct_passes() {
    let v = SchemaValidator::new(vec![
        ("s".into(), JsonType::String),
        ("n".into(), JsonType::Number),
        ("b".into(), JsonType::Bool),
        ("a".into(), JsonType::Array),
        ("o".into(), JsonType::Object),
        ("any".into(), JsonType::Any),
    ]);
    let val = serde_json::json!({
        "s": "hello",
        "n": 2.72,
        "b": false,
        "a": [],
        "o": {},
        "any": "anything",
    });
    assert!(v.validate(&val).is_ok());
}

#[test]
fn serde_envelope_roundtrip_hello() {
    let env = Envelope::hello(backend("test"), CapabilityManifest::new());
    let json = serde_json::to_string(&env).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(RawEnvelopeValidator.validate(&parsed).is_ok());
}

#[test]
fn serde_envelope_roundtrip_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "oops".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(RawEnvelopeValidator.validate(&parsed).is_ok());
}

#[test]
fn serde_work_order_json_string_then_validate() {
    let wo = WorkOrderBuilder::new("hello").build();
    let json_str = serde_json::to_string(&wo).unwrap();
    let parsed: abp_core::WorkOrder = serde_json::from_str(&json_str).unwrap();
    assert!(WorkOrderValidator.validate(&parsed).is_ok());
}

#[test]
fn serde_receipt_json_string_then_validate() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json_str = serde_json::to_string(&r).unwrap();
    let parsed: abp_core::Receipt = serde_json::from_str(&json_str).unwrap();
    assert!(ReceiptValidator.validate(&parsed).is_ok());
}
