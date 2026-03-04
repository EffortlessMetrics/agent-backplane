#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
//! Deep mutation-resilience tests.
//!
//! Each test is designed so that a single-site mutation in the SUT would
//! cause the test to fail. Categories:
//!
//!  1. Receipt hash mutations
//!  2. WorkOrder field sensitivity
//!  3. ErrorCode completeness
//!  4. Capability support level ordering
//!  5. Policy allow/deny decision flips
//!  6. Protocol envelope tag mutations
//!  7. Serde field-name stability
//!  8. Default value correctness
//!  9. Version string mutations
//! 10. IR normalization stability

use std::path::Path;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Receipt hash mutations — changing any field changes the hash
// ═══════════════════════════════════════════════════════════════════════════

fn base_receipt() -> abp_core::Receipt {
    abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build()
}

fn hash(r: &abp_core::Receipt) -> String {
    abp_core::receipt_hash(r).unwrap()
}

#[test]
fn hash_mutate_backend_id() {
    let h1 = hash(&base_receipt());
    let mut r2 = base_receipt();
    r2.backend.id = "other".into();
    assert_ne!(h1, hash(&r2));
}

#[test]
fn hash_mutate_outcome_complete_vs_failed() {
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let r2 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Failed)
        .build();
    assert_ne!(hash(&r1), hash(&r2));
}

#[test]
fn hash_mutate_outcome_complete_vs_partial() {
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let r2 = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Partial)
        .build();
    assert_ne!(hash(&r1), hash(&r2));
}

#[test]
fn hash_mutate_mode() {
    let h1 = hash(&base_receipt());
    let mut r2 = base_receipt();
    r2.mode = abp_core::ExecutionMode::Passthrough;
    assert_ne!(h1, hash(&r2));
}

#[test]
fn hash_mutate_contract_version() {
    let h1 = hash(&base_receipt());
    let mut r2 = base_receipt();
    r2.meta.contract_version = "abp/v999".into();
    assert_ne!(h1, hash(&r2));
}

#[test]
fn hash_mutate_duration_ms() {
    let h1 = hash(&base_receipt());
    let mut r2 = base_receipt();
    r2.meta.duration_ms = 999_999;
    assert_ne!(h1, hash(&r2));
}

#[test]
fn hash_mutate_verification_harness_ok() {
    let h1 = hash(&base_receipt());
    let mut r2 = base_receipt();
    r2.verification.harness_ok = true;
    assert_ne!(h1, hash(&r2));
}

#[test]
fn hash_mutate_usage_raw() {
    let h1 = hash(&base_receipt());
    let mut r2 = base_receipt();
    r2.usage_raw = serde_json::json!({"tokens": 100});
    assert_ne!(h1, hash(&r2));
}

#[test]
fn hash_nulls_stored_hash_before_computing() {
    let mut r = base_receipt();
    let h1 = hash(&r);
    r.receipt_sha256 = Some("aaaa".into());
    let h2 = hash(&r);
    assert_eq!(h1, h2, "stored hash must be nulled before hashing");
}

#[test]
fn hash_is_64_hex_chars() {
    let h = hash(&base_receipt());
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn with_hash_populates_field() {
    let r = base_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_crate_compute_hash_matches_core() {
    let r = base_receipt();
    let h_core = hash(&r);
    let h_crate = abp_receipt::compute_hash(&r).unwrap();
    assert_eq!(h_core, h_crate);
}

#[test]
fn receipt_crate_verify_hash_detects_tamper() {
    let mut r = base_receipt();
    r.receipt_sha256 = Some(abp_receipt::compute_hash(&r).unwrap());
    assert!(abp_receipt::verify_hash(&r));
    r.outcome = abp_core::Outcome::Failed;
    assert!(!abp_receipt::verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. WorkOrder field sensitivity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_task_round_trips() {
    let wo = abp_core::WorkOrderBuilder::new("do stuff").build();
    assert_eq!(wo.task, "do stuff");
}

#[test]
fn work_order_lane_defaults_to_patch_first() {
    let wo = abp_core::WorkOrderBuilder::new("x").build();
    assert!(matches!(wo.lane, abp_core::ExecutionLane::PatchFirst));
}

#[test]
fn work_order_workspace_mode_defaults_to_staged() {
    let wo = abp_core::WorkOrderBuilder::new("x").build();
    assert!(matches!(wo.workspace.mode, abp_core::WorkspaceMode::Staged));
}

#[test]
fn work_order_root_defaults_to_dot() {
    let wo = abp_core::WorkOrderBuilder::new("x").build();
    assert_eq!(wo.workspace.root, ".");
}

#[test]
fn work_order_model_setter_works() {
    let wo = abp_core::WorkOrderBuilder::new("x").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn work_order_max_turns_setter_works() {
    let wo = abp_core::WorkOrderBuilder::new("x").max_turns(5).build();
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn work_order_max_budget_setter_works() {
    let wo = abp_core::WorkOrderBuilder::new("x")
        .max_budget_usd(1.5)
        .build();
    assert_eq!(wo.config.max_budget_usd, Some(1.5));
}

#[test]
fn work_order_serialization_includes_task() {
    let wo = abp_core::WorkOrderBuilder::new("hello").build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"task\":\"hello\""));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. ErrorCode completeness — every code has distinct as_str() and message()
// ═══════════════════════════════════════════════════════════════════════════

fn all_error_codes() -> Vec<abp_error::ErrorCode> {
    vec![
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
        abp_error::ErrorCode::ProtocolHandshakeFailed,
        abp_error::ErrorCode::ProtocolMissingRefId,
        abp_error::ErrorCode::ProtocolUnexpectedMessage,
        abp_error::ErrorCode::ProtocolVersionMismatch,
        abp_error::ErrorCode::MappingUnsupportedCapability,
        abp_error::ErrorCode::MappingDialectMismatch,
        abp_error::ErrorCode::MappingLossyConversion,
        abp_error::ErrorCode::MappingUnmappableTool,
        abp_error::ErrorCode::BackendNotFound,
        abp_error::ErrorCode::BackendUnavailable,
        abp_error::ErrorCode::BackendTimeout,
        abp_error::ErrorCode::BackendRateLimited,
        abp_error::ErrorCode::BackendAuthFailed,
        abp_error::ErrorCode::BackendModelNotFound,
        abp_error::ErrorCode::BackendCrashed,
        abp_error::ErrorCode::ExecutionToolFailed,
        abp_error::ErrorCode::ExecutionWorkspaceError,
        abp_error::ErrorCode::ExecutionPermissionDenied,
        abp_error::ErrorCode::ContractVersionMismatch,
        abp_error::ErrorCode::ContractSchemaViolation,
        abp_error::ErrorCode::ContractInvalidReceipt,
        abp_error::ErrorCode::CapabilityUnsupported,
        abp_error::ErrorCode::CapabilityEmulationFailed,
        abp_error::ErrorCode::PolicyDenied,
        abp_error::ErrorCode::PolicyInvalid,
        abp_error::ErrorCode::WorkspaceInitFailed,
        abp_error::ErrorCode::WorkspaceStagingFailed,
        abp_error::ErrorCode::IrLoweringFailed,
        abp_error::ErrorCode::IrInvalid,
        abp_error::ErrorCode::ReceiptHashMismatch,
        abp_error::ErrorCode::ReceiptChainBroken,
        abp_error::ErrorCode::DialectUnknown,
        abp_error::ErrorCode::DialectMappingFailed,
        abp_error::ErrorCode::ConfigInvalid,
        abp_error::ErrorCode::Internal,
    ]
}

#[test]
fn error_code_as_str_all_unique() {
    let codes = all_error_codes();
    let strs: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
    let unique: std::collections::HashSet<&str> = strs.iter().copied().collect();
    assert_eq!(strs.len(), unique.len(), "as_str() must be unique per code");
}

#[test]
fn error_code_message_all_unique() {
    let codes = all_error_codes();
    let msgs: Vec<&str> = codes.iter().map(|c| c.message()).collect();
    let unique: std::collections::HashSet<&str> = msgs.iter().copied().collect();
    assert_eq!(
        msgs.len(),
        unique.len(),
        "message() must be unique per code"
    );
}

#[test]
fn error_code_as_str_is_non_empty() {
    for code in all_error_codes() {
        assert!(!code.as_str().is_empty(), "{code:?} has empty as_str()");
    }
}

#[test]
fn error_code_message_is_non_empty() {
    for code in all_error_codes() {
        assert!(!code.message().is_empty(), "{code:?} has empty message()");
    }
}

#[test]
fn error_code_category_is_consistent() {
    use abp_error::ErrorCategory;
    assert_eq!(
        abp_error::ErrorCode::BackendTimeout.category(),
        ErrorCategory::Backend
    );
    assert_eq!(
        abp_error::ErrorCode::PolicyDenied.category(),
        ErrorCategory::Policy
    );
    assert_eq!(
        abp_error::ErrorCode::IrInvalid.category(),
        ErrorCategory::Ir
    );
    assert_eq!(
        abp_error::ErrorCode::Internal.category(),
        ErrorCategory::Internal
    );
}

#[test]
fn error_code_retryable_set_is_specific() {
    assert!(abp_error::ErrorCode::BackendUnavailable.is_retryable());
    assert!(abp_error::ErrorCode::BackendTimeout.is_retryable());
    assert!(abp_error::ErrorCode::BackendRateLimited.is_retryable());
    assert!(abp_error::ErrorCode::BackendCrashed.is_retryable());
    // Non-retryable codes
    assert!(!abp_error::ErrorCode::PolicyDenied.is_retryable());
    assert!(!abp_error::ErrorCode::Internal.is_retryable());
    assert!(!abp_error::ErrorCode::BackendNotFound.is_retryable());
    assert!(!abp_error::ErrorCode::BackendAuthFailed.is_retryable());
}

#[test]
fn error_code_display_matches_message() {
    for code in all_error_codes() {
        assert_eq!(
            format!("{code}"),
            code.message(),
            "Display must delegate to message()"
        );
    }
}

#[test]
fn error_code_serde_roundtrip() {
    for code in all_error_codes() {
        let json = serde_json::to_string(&code).unwrap();
        let back: abp_error::ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, back, "serde roundtrip for {code:?}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Capability support level ordering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn support_level_native_satisfies_native() {
    assert!(abp_core::SupportLevel::Native.satisfies(&abp_core::MinSupport::Native));
}

#[test]
fn support_level_native_satisfies_emulated() {
    assert!(abp_core::SupportLevel::Native.satisfies(&abp_core::MinSupport::Emulated));
}

#[test]
fn support_level_emulated_does_not_satisfy_native() {
    assert!(!abp_core::SupportLevel::Emulated.satisfies(&abp_core::MinSupport::Native));
}

#[test]
fn support_level_emulated_satisfies_emulated() {
    assert!(abp_core::SupportLevel::Emulated.satisfies(&abp_core::MinSupport::Emulated));
}

#[test]
fn support_level_unsupported_satisfies_nothing() {
    assert!(!abp_core::SupportLevel::Unsupported.satisfies(&abp_core::MinSupport::Native));
    assert!(!abp_core::SupportLevel::Unsupported.satisfies(&abp_core::MinSupport::Emulated));
}

#[test]
fn support_level_restricted_satisfies_emulated() {
    let restricted = abp_core::SupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(restricted.satisfies(&abp_core::MinSupport::Emulated));
}

#[test]
fn support_level_restricted_does_not_satisfy_native() {
    let restricted = abp_core::SupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(!restricted.satisfies(&abp_core::MinSupport::Native));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Policy allow/deny decisions flip on mutation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_deny_tool_exact_match() {
    let policy = abp_core::PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_deny_tool_one_char_change_allows() {
    let policy = abp_core::PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    // "bash" (lowercase) should not match "Bash" unless glob is case-insensitive
    // The exact behavior depends on glob, but "Xash" definitely differs.
    assert!(engine.can_use_tool("Xash").allowed);
}

#[test]
fn policy_allowlist_blocks_unlisted() {
    let policy = abp_core::PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
}

#[test]
fn policy_deny_write_glob_match() {
    let policy = abp_core::PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_deny_read_glob_match() {
    let policy = abp_core::PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("keys.secret")).allowed);
    assert!(engine.can_read_path(Path::new("keys.txt")).allowed);
}

#[test]
fn policy_deny_write_one_char_diff_pattern() {
    let policy1 = abp_core::PolicyProfile {
        deny_write: vec!["*.log".into()],
        ..Default::default()
    };
    let policy2 = abp_core::PolicyProfile {
        deny_write: vec!["*.loc".into()],
        ..Default::default()
    };
    let e1 = abp_policy::PolicyEngine::new(&policy1).unwrap();
    let e2 = abp_policy::PolicyEngine::new(&policy2).unwrap();
    // "app.log" is denied by *.log but allowed by *.loc
    assert!(!e1.can_write_path(Path::new("app.log")).allowed);
    assert!(e2.can_write_path(Path::new("app.log")).allowed);
}

#[test]
fn policy_empty_allows_everything() {
    let policy = abp_core::PolicyProfile::default();
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("anything").allowed);
    assert!(engine.can_read_path(Path::new("any/path")).allowed);
    assert!(engine.can_write_path(Path::new("any/path")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Protocol envelope tag mutations — wrong tag → parse error
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_hello_uses_tag_t() {
    let hello = abp_protocol::Envelope::hello(
        abp_core::BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        abp_core::CapabilityManifest::new(),
    );
    let json = abp_protocol::JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("\"t\":\"hello\""), "tag field must be 't'");
}

#[test]
fn envelope_wrong_tag_key_fails_parse() {
    // Use "type" instead of "t" — must fail
    let bad = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    assert!(abp_protocol::JsonlCodec::decode(bad).is_err());
}

#[test]
fn envelope_wrong_tag_value_fails_parse() {
    let bad = r#"{"t":"helo","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    assert!(abp_protocol::JsonlCodec::decode(bad).is_err());
}

#[test]
fn envelope_fatal_roundtrip() {
    let fatal = abp_protocol::Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = abp_protocol::JsonlCodec::encode(&fatal).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        abp_protocol::Envelope::Fatal { error, .. } => assert_eq!(error, "boom"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_run_roundtrip() {
    let wo = abp_core::WorkOrderBuilder::new("test task").build();
    let run = abp_protocol::Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let json = abp_protocol::JsonlCodec::encode(&run).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        abp_protocol::Envelope::Run { id, work_order } => {
            assert_eq!(id, "r1");
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn envelope_fatal_with_error_code() {
    let env = abp_protocol::Envelope::fatal_with_code(
        Some("ref".into()),
        "timeout",
        abp_error::ErrorCode::BackendTimeout,
    );
    let json = abp_protocol::JsonlCodec::encode(&env).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(abp_error::ErrorCode::BackendTimeout)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Serde field-name stability — rename breaks deserialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn outcome_serde_snake_case_complete() {
    let json = serde_json::to_string(&abp_core::Outcome::Complete).unwrap();
    assert_eq!(json, "\"complete\"");
    let back: abp_core::Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(back, abp_core::Outcome::Complete);
}

#[test]
fn outcome_serde_snake_case_partial() {
    let json = serde_json::to_string(&abp_core::Outcome::Partial).unwrap();
    assert_eq!(json, "\"partial\"");
}

#[test]
fn outcome_serde_snake_case_failed() {
    let json = serde_json::to_string(&abp_core::Outcome::Failed).unwrap();
    assert_eq!(json, "\"failed\"");
}

#[test]
fn execution_mode_serde_snake_case() {
    let json = serde_json::to_string(&abp_core::ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, "\"passthrough\"");
    let json2 = serde_json::to_string(&abp_core::ExecutionMode::Mapped).unwrap();
    assert_eq!(json2, "\"mapped\"");
}

#[test]
fn execution_lane_serde_snake_case() {
    let json = serde_json::to_string(&abp_core::ExecutionLane::PatchFirst).unwrap();
    assert_eq!(json, "\"patch_first\"");
    let json2 = serde_json::to_string(&abp_core::ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(json2, "\"workspace_first\"");
}

#[test]
fn workspace_mode_serde_snake_case() {
    let json = serde_json::to_string(&abp_core::WorkspaceMode::Staged).unwrap();
    assert_eq!(json, "\"staged\"");
    let json2 = serde_json::to_string(&abp_core::WorkspaceMode::PassThrough).unwrap();
    assert_eq!(json2, "\"pass_through\"");
}

#[test]
fn ir_role_serde_snake_case() {
    let json = serde_json::to_string(&abp_ir::IrRole::System).unwrap();
    assert_eq!(json, "\"system\"");
    let json2 = serde_json::to_string(&abp_ir::IrRole::Assistant).unwrap();
    assert_eq!(json2, "\"assistant\"");
}

#[test]
fn agent_event_kind_tag_is_type() {
    let kind = abp_core::AgentEventKind::AssistantMessage { text: "hi".into() };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "assistant_message");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Default value correctness — defaults used when field missing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(
        abp_core::ExecutionMode::default(),
        abp_core::ExecutionMode::Mapped
    );
}

#[test]
fn runtime_config_default_model_is_none() {
    let cfg = abp_core::RuntimeConfig::default();
    assert!(cfg.model.is_none());
}

#[test]
fn runtime_config_default_max_turns_is_none() {
    let cfg = abp_core::RuntimeConfig::default();
    assert!(cfg.max_turns.is_none());
}

#[test]
fn runtime_config_default_max_budget_is_none() {
    let cfg = abp_core::RuntimeConfig::default();
    assert!(cfg.max_budget_usd.is_none());
}

#[test]
fn policy_profile_default_is_empty() {
    let p = abp_core::PolicyProfile::default();
    assert!(p.allowed_tools.is_empty());
    assert!(p.disallowed_tools.is_empty());
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
}

#[test]
fn verification_report_default_harness_ok_false() {
    let v = abp_core::VerificationReport::default();
    assert!(!v.harness_ok);
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
}

#[test]
fn usage_normalized_default_all_none() {
    let u = abp_core::UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn context_packet_default_is_empty() {
    let ctx = abp_core::ContextPacket::default();
    assert!(ctx.files.is_empty());
    assert!(ctx.snippets.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Version string mutations — wrong version → rejection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_is_abp_v01() {
    assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_embedded_in_receipt() {
    let r = base_receipt();
    assert_eq!(r.meta.contract_version, "abp/v0.1");
}

#[test]
fn contract_version_embedded_in_hello() {
    let hello = abp_protocol::Envelope::hello(
        abp_core::BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        abp_core::CapabilityManifest::new(),
    );
    let json = serde_json::to_string(&hello).unwrap();
    assert!(json.contains("\"contract_version\":\"abp/v0.1\""));
}

#[test]
fn mutated_version_changes_hash() {
    let r1 = base_receipt();
    let mut r2 = base_receipt();
    r2.meta.contract_version = "abp/v0.2".into();
    assert_ne!(hash(&r1), hash(&r2));
}

#[test]
fn version_prefix_matters() {
    // Verify the exact prefix format
    assert!(abp_core::CONTRACT_VERSION.starts_with("abp/"));
}

#[test]
fn version_mutation_off_by_one() {
    // "abp/v0.1" vs "abp/v0.2" must be different strings
    assert_ne!(abp_core::CONTRACT_VERSION, "abp/v0.2");
    assert_ne!(abp_core::CONTRACT_VERSION, "abp/v1.0");
    assert_ne!(abp_core::CONTRACT_VERSION, "ABP/v0.1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. IR normalization stability — same input → same normalized output
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn normalize_is_idempotent() {
    let conv = abp_ir::IrConversation::new()
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::System, "  sys  "))
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::User, " hi "))
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::System, " extra "));
    let once = abp_ir::normalize::normalize(&conv);
    let twice = abp_ir::normalize::normalize(&once);
    assert_eq!(once, twice);
}

#[test]
fn normalize_dedup_system_merges() {
    let conv = abp_ir::IrConversation::new()
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::System, "a"))
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::User, "u"))
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::System, "b"));
    let out = abp_ir::normalize::dedup_system(&conv);
    let sys_count = out
        .messages
        .iter()
        .filter(|m| m.role == abp_ir::IrRole::System)
        .count();
    assert_eq!(sys_count, 1);
    assert_eq!(out.messages[0].text_content(), "a\nb");
}

#[test]
fn normalize_trim_text_strips_whitespace() {
    let conv =
        abp_ir::IrConversation::new().push(abp_ir::IrMessage::text(abp_ir::IrRole::User, "  hi  "));
    let out = abp_ir::normalize::trim_text(&conv);
    assert_eq!(out.messages[0].text_content(), "hi");
}

#[test]
fn normalize_strip_empty_removes_content_free_messages() {
    let empty_msg = abp_ir::IrMessage::new(abp_ir::IrRole::User, vec![]);
    let conv = abp_ir::IrConversation::new()
        .push(empty_msg)
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::User, "hi"));
    let out = abp_ir::normalize::strip_empty(&conv);
    assert_eq!(out.messages.len(), 1);
    assert_eq!(out.messages[0].text_content(), "hi");
}

#[test]
fn normalize_merge_adjacent_text_coalesces() {
    let msg = abp_ir::IrMessage::new(
        abp_ir::IrRole::User,
        vec![
            abp_ir::IrContentBlock::Text {
                text: "hello ".into(),
            },
            abp_ir::IrContentBlock::Text {
                text: "world".into(),
            },
        ],
    );
    let conv = abp_ir::IrConversation::from_messages(vec![msg]);
    let out = abp_ir::normalize::merge_adjacent_text(&conv);
    assert_eq!(out.messages[0].content.len(), 1);
    assert_eq!(out.messages[0].text_content(), "hello world");
}

#[test]
fn normalize_role_system_canonical() {
    assert_eq!(
        abp_ir::normalize::normalize_role("system"),
        Some(abp_ir::IrRole::System)
    );
    assert_eq!(
        abp_ir::normalize::normalize_role("developer"),
        Some(abp_ir::IrRole::System)
    );
}

#[test]
fn normalize_role_user_canonical() {
    assert_eq!(
        abp_ir::normalize::normalize_role("user"),
        Some(abp_ir::IrRole::User)
    );
    assert_eq!(
        abp_ir::normalize::normalize_role("human"),
        Some(abp_ir::IrRole::User)
    );
}

#[test]
fn normalize_role_assistant_canonical() {
    assert_eq!(
        abp_ir::normalize::normalize_role("assistant"),
        Some(abp_ir::IrRole::Assistant)
    );
    assert_eq!(
        abp_ir::normalize::normalize_role("model"),
        Some(abp_ir::IrRole::Assistant)
    );
    assert_eq!(
        abp_ir::normalize::normalize_role("bot"),
        Some(abp_ir::IrRole::Assistant)
    );
}

#[test]
fn normalize_role_tool_canonical() {
    assert_eq!(
        abp_ir::normalize::normalize_role("tool"),
        Some(abp_ir::IrRole::Tool)
    );
    assert_eq!(
        abp_ir::normalize::normalize_role("function"),
        Some(abp_ir::IrRole::Tool)
    );
}

#[test]
fn normalize_role_unknown_returns_none() {
    assert_eq!(abp_ir::normalize::normalize_role("narrator"), None);
    assert_eq!(abp_ir::normalize::normalize_role(""), None);
    assert_eq!(abp_ir::normalize::normalize_role("SYSTEM"), None);
}

#[test]
fn normalize_same_input_same_output() {
    let conv =
        abp_ir::IrConversation::new().push(abp_ir::IrMessage::text(abp_ir::IrRole::User, "hello"));
    let out1 = abp_ir::normalize::normalize(&conv);
    let out2 = abp_ir::normalize::normalize(&conv);
    assert_eq!(out1, out2, "same input must produce same output");
}

#[test]
fn normalize_extract_system_returns_none_when_absent() {
    let conv =
        abp_ir::IrConversation::new().push(abp_ir::IrMessage::text(abp_ir::IrRole::User, "hi"));
    let (sys, rest) = abp_ir::normalize::extract_system(&conv);
    assert!(sys.is_none());
    assert_eq!(rest.messages.len(), 1);
}

#[test]
fn normalize_extract_system_merges_text() {
    let conv = abp_ir::IrConversation::new()
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::System, "a"))
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::User, "u"))
        .push(abp_ir::IrMessage::text(abp_ir::IrRole::System, "b"));
    let (sys, _rest) = abp_ir::normalize::extract_system(&conv);
    assert_eq!(sys.unwrap(), "a\nb");
}
