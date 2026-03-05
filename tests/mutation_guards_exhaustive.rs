#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::needless_update)]
#![allow(clippy::field_reassign_with_default)]
#![allow(unknown_lints)]
//! Exhaustive mutation-guard tests.
//!
//! Each test is designed so that a single mutation (flipping a boolean,
//! swapping a variant, removing a field, or changing a constant) in the
//! core logic will cause at least one deterministic failure.
//!
//! ## Categories
//! 1. Hash integrity guards (receipt hash changes when any field changes)
//! 2. Serialization fidelity guards (enum/struct serde output)
//! 3. Policy enforcement guards (deny actually blocks)
//! 4. Error code guards (category, retryability, display)

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use abp_core::{
    receipt_hash, AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability,
    CapabilityManifest, ExecutionMode, Outcome, Receipt, ReceiptBuilder, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport, CONTRACT_VERSION,
};
use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_policy::{Decision, PolicyEngine};
use serde_json::{json, Value};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn baseline_receipt() -> Receipt {
    ReceiptBuilder::new("mutation-test-backend").build()
}

fn baseline_hash() -> String {
    receipt_hash(&baseline_receipt()).unwrap()
}

fn hash_of(r: &Receipt) -> String {
    receipt_hash(r).unwrap()
}

fn policy_engine(profile: abp_core::PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(&profile).expect("compile policy")
}

/// All 36 error codes for exhaustive iteration.
const ALL_CODES: &[ErrorCode] = &[
    ErrorCode::ProtocolInvalidEnvelope,
    ErrorCode::ProtocolHandshakeFailed,
    ErrorCode::ProtocolMissingRefId,
    ErrorCode::ProtocolUnexpectedMessage,
    ErrorCode::ProtocolVersionMismatch,
    ErrorCode::MappingUnsupportedCapability,
    ErrorCode::MappingDialectMismatch,
    ErrorCode::MappingLossyConversion,
    ErrorCode::MappingUnmappableTool,
    ErrorCode::BackendNotFound,
    ErrorCode::BackendUnavailable,
    ErrorCode::BackendTimeout,
    ErrorCode::BackendRateLimited,
    ErrorCode::BackendAuthFailed,
    ErrorCode::BackendModelNotFound,
    ErrorCode::BackendCrashed,
    ErrorCode::ExecutionToolFailed,
    ErrorCode::ExecutionWorkspaceError,
    ErrorCode::ExecutionPermissionDenied,
    ErrorCode::ContractVersionMismatch,
    ErrorCode::ContractSchemaViolation,
    ErrorCode::ContractInvalidReceipt,
    ErrorCode::CapabilityUnsupported,
    ErrorCode::CapabilityEmulationFailed,
    ErrorCode::PolicyDenied,
    ErrorCode::PolicyInvalid,
    ErrorCode::WorkspaceInitFailed,
    ErrorCode::WorkspaceStagingFailed,
    ErrorCode::IrLoweringFailed,
    ErrorCode::IrInvalid,
    ErrorCode::ReceiptHashMismatch,
    ErrorCode::ReceiptChainBroken,
    ErrorCode::DialectUnknown,
    ErrorCode::DialectMappingFailed,
    ErrorCode::ConfigInvalid,
    ErrorCode::Internal,
];

// ═══════════════════════════════════════════════════════════════════════════════
// 1. HASH INTEGRITY GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn hash_changes_when_backend_id_mutated() {
    let mut r = baseline_receipt();
    r.backend.id = "different-backend".into();
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_outcome_mutated() {
    let mut r = baseline_receipt();
    r.outcome = Outcome::Failed;
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_outcome_partial() {
    let mut r = baseline_receipt();
    r.outcome = Outcome::Partial;
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_duration_mutated() {
    let mut r = baseline_receipt();
    r.meta.duration_ms = 99999;
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_contract_version_mutated() {
    let mut r = baseline_receipt();
    r.meta.contract_version = "abp/v999".into();
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_trace_added() {
    let r = ReceiptBuilder::new("mutation-test-backend")
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .build();
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_usage_tokens_mutated() {
    let mut r = baseline_receipt();
    r.usage.input_tokens = Some(42);
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_usage_output_tokens_mutated() {
    let mut r = baseline_receipt();
    r.usage.output_tokens = Some(100);
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_extensions_added_to_usage_raw() {
    let mut r = baseline_receipt();
    r.usage_raw = json!({"custom_field": true});
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_changes_when_mode_mutated() {
    let mut r = baseline_receipt();
    r.mode = ExecutionMode::Passthrough;
    assert_ne!(hash_of(&r), baseline_hash());
}

#[test]
fn hash_excludes_receipt_sha256_field() {
    let r1 = baseline_receipt();
    let mut r2 = Receipt { ..r1.clone() };
    r2.receipt_sha256 = Some("decafbad".into());
    assert_eq!(hash_of(&r1), hash_of(&r2));
}

#[test]
fn with_hash_populates_receipt_sha256() {
    let r = baseline_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let h = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(h.len(), 64, "SHA-256 hex must be 64 chars");
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. SERIALIZATION FIDELITY GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn outcome_variants_serialize_distinctly() {
    let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    let jsons: Vec<String> = outcomes
        .iter()
        .map(|o| serde_json::to_string(o).unwrap())
        .collect();
    let unique: HashSet<&str> = jsons.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        unique.len(),
        3,
        "all Outcome variants must serialize uniquely"
    );
}

#[test]
fn outcome_uses_snake_case_rename() {
    assert_eq!(
        serde_json::to_string(&Outcome::Complete).unwrap(),
        r#""complete""#
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Partial).unwrap(),
        r#""partial""#
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Failed).unwrap(),
        r#""failed""#
    );
}

#[test]
fn execution_mode_variants_serialize_distinctly() {
    let modes = [ExecutionMode::Passthrough, ExecutionMode::Mapped];
    let jsons: Vec<String> = modes
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();
    let unique: HashSet<&str> = jsons.iter().map(|s| s.as_str()).collect();
    assert_eq!(unique.len(), 2);
}

#[test]
fn execution_mode_uses_snake_case_rename() {
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Passthrough).unwrap(),
        r#""passthrough""#
    );
    assert_eq!(
        serde_json::to_string(&ExecutionMode::Mapped).unwrap(),
        r#""mapped""#
    );
}

#[test]
fn support_level_variants_serialize_distinctly() {
    let levels = [
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "test".into(),
        },
    ];
    let jsons: Vec<String> = levels
        .iter()
        .map(|l| serde_json::to_string(l).unwrap())
        .collect();
    let unique: HashSet<&str> = jsons.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        unique.len(),
        4,
        "all SupportLevel variants must serialize uniquely"
    );
}

#[test]
fn receipt_json_contains_all_required_fields() {
    let r = baseline_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    let obj = v.as_object().unwrap();
    let required = [
        "meta",
        "backend",
        "capabilities",
        "mode",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
        "receipt_sha256",
    ];
    for field in &required {
        assert!(obj.contains_key(*field), "missing required field: {field}");
    }
}

#[test]
fn receipt_meta_json_contains_all_required_fields() {
    let r = baseline_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    let meta = v["meta"].as_object().unwrap();
    let required = [
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ];
    for field in &required {
        assert!(meta.contains_key(*field), "missing meta field: {field}");
    }
}

#[test]
fn contract_version_matches_constant() {
    let r = baseline_receipt();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn agent_event_kind_variants_produce_distinct_json() {
    let events: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: Some("id".into()),
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: Some("id".into()),
            output: "o".into(),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f".into(),
            summary: String::new(),
        },
        AgentEventKind::CommandExecuted {
            command: "cmd".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
    ];
    let jsons: Vec<String> = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    let unique: HashSet<&str> = jsons.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        unique.len(),
        events.len(),
        "every AgentEventKind variant must produce distinct JSON"
    );
}

#[test]
fn receipt_roundtrip_preserves_all_fields() {
    let r = ReceiptBuilder::new("roundtrip-backend")
        .outcome(Outcome::Partial)
        .mode(ExecutionMode::Passthrough)
        .usage(UsageNormalized {
            input_tokens: Some(10),
            output_tokens: Some(20),
            ..UsageNormalized::default()
        })
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.mode, r2.mode);
    assert_eq!(r.usage.input_tokens, r2.usage.input_tokens);
    assert_eq!(r.usage.output_tokens, r2.usage.output_tokens);
    assert_eq!(r.meta.contract_version, r2.meta.contract_version);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. POLICY ENFORCEMENT GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn tool_deny_actually_blocks() {
    let p = abp_core::PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let e = policy_engine(p);
    let d = e.can_use_tool("Bash");
    assert_eq!(d.allowed, false, "denied tool must not be allowed");
    assert!(d.reason.is_some(), "denial must include a reason");
}

#[test]
fn tool_deny_wildcard_blocks_all_matching() {
    let p = abp_core::PolicyProfile {
        disallowed_tools: vec!["Bash*".into()],
        ..Default::default()
    };
    let e = policy_engine(p);
    assert_eq!(e.can_use_tool("BashExec").allowed, false);
    assert_eq!(e.can_use_tool("Bash").allowed, false);
}

#[test]
fn tool_allowlist_blocks_unlisted() {
    let p = abp_core::PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..Default::default()
    };
    let e = policy_engine(p);
    assert_eq!(e.can_use_tool("Read").allowed, true);
    assert_eq!(e.can_use_tool("Grep").allowed, true);
    assert_eq!(e.can_use_tool("Bash").allowed, false);
}

#[test]
fn path_deny_read_blocks() {
    let p = abp_core::PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..Default::default()
    };
    let e = policy_engine(p);
    let d = e.can_read_path(Path::new("config.secret"));
    assert_eq!(d.allowed, false);
    assert!(d.reason.is_some());
}

#[test]
fn path_deny_write_blocks() {
    let p = abp_core::PolicyProfile {
        deny_write: vec!["*.lock".into()],
        ..Default::default()
    };
    let e = policy_engine(p);
    let d = e.can_write_path(Path::new("Cargo.lock"));
    assert_eq!(d.allowed, false);
    assert!(d.reason.is_some());
}

#[test]
fn removing_deny_rule_changes_behavior() {
    let p_with = abp_core::PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let p_without = abp_core::PolicyProfile::default();
    let e_with = policy_engine(p_with);
    let e_without = policy_engine(p_without);
    assert_eq!(e_with.can_use_tool("Bash").allowed, false);
    assert_eq!(e_without.can_use_tool("Bash").allowed, true);
}

#[test]
fn empty_policy_allows_everything() {
    let e = policy_engine(abp_core::PolicyProfile::default());
    assert_eq!(e.can_use_tool("AnyTool").allowed, true);
    assert_eq!(e.can_read_path(Path::new("any/file.rs")).allowed, true);
    assert_eq!(e.can_write_path(Path::new("any/file.rs")).allowed, true);
}

#[test]
fn deny_beats_allow_for_tools() {
    let p = abp_core::PolicyProfile {
        allowed_tools: vec!["Bash".into()],
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let e = policy_engine(p);
    assert_eq!(
        e.can_use_tool("Bash").allowed,
        false,
        "deny must take precedence over allow"
    );
}

#[test]
fn deny_read_does_not_affect_write() {
    let p = abp_core::PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..Default::default()
    };
    let e = policy_engine(p);
    assert_eq!(e.can_read_path(Path::new("a.secret")).allowed, false);
    assert_eq!(
        e.can_write_path(Path::new("a.secret")).allowed,
        true,
        "deny_read must not block writes"
    );
}

#[test]
fn deny_write_does_not_affect_read() {
    let p = abp_core::PolicyProfile {
        deny_write: vec!["*.lock".into()],
        ..Default::default()
    };
    let e = policy_engine(p);
    assert_eq!(e.can_write_path(Path::new("Cargo.lock")).allowed, false);
    assert_eq!(
        e.can_read_path(Path::new("Cargo.lock")).allowed,
        true,
        "deny_write must not block reads"
    );
}

#[test]
fn decision_deny_has_reason_decision_allow_has_none() {
    let allow = Decision::allow();
    let deny = Decision::deny("blocked");
    assert_eq!(allow.allowed, true);
    assert!(allow.reason.is_none());
    assert_eq!(deny.allowed, false);
    assert_eq!(deny.reason.as_deref(), Some("blocked"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. ERROR CODE GUARDS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_count_is_36() {
    assert_eq!(ALL_CODES.len(), 36, "must have exactly 36 error codes");
}

#[test]
fn every_code_maps_to_a_category() {
    for &code in ALL_CODES {
        let _ = code.category(); // should not panic
    }
}

#[test]
fn protocol_codes_map_to_protocol_category() {
    let protocol_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in &protocol_codes {
        assert_eq!(
            code.category(),
            ErrorCategory::Protocol,
            "{code:?} must map to Protocol"
        );
    }
}

#[test]
fn backend_codes_map_to_backend_category() {
    let backend_codes = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
    ];
    for code in &backend_codes {
        assert_eq!(
            code.category(),
            ErrorCategory::Backend,
            "{code:?} must map to Backend"
        );
    }
}

#[test]
fn only_four_codes_are_retryable() {
    let retryable: Vec<&ErrorCode> = ALL_CODES.iter().filter(|c| c.is_retryable()).collect();
    assert_eq!(retryable.len(), 4, "exactly 4 codes must be retryable");
    let expected_retryable = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ];
    for code in &expected_retryable {
        assert!(code.is_retryable(), "{code:?} must be retryable");
    }
}

#[test]
fn non_backend_transient_codes_are_not_retryable() {
    let non_retryable = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::PolicyDenied,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::Internal,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
    ];
    for code in &non_retryable {
        assert_eq!(code.is_retryable(), false, "{code:?} must NOT be retryable");
    }
}

#[test]
fn all_error_messages_are_non_empty() {
    for &code in ALL_CODES {
        let msg = code.message();
        assert!(!msg.is_empty(), "{code:?} has empty message");
    }
}

#[test]
fn all_error_messages_are_unique() {
    let mut seen = HashSet::new();
    for &code in ALL_CODES {
        let msg = code.message();
        assert!(seen.insert(msg), "duplicate message for {code:?}: {msg}");
    }
}

#[test]
fn all_as_str_values_are_unique() {
    let mut seen = HashSet::new();
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str for {code:?}: {s}");
    }
}

#[test]
fn error_display_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "test denial");
    let display = format!("{err}");
    assert!(
        display.contains("policy_denied"),
        "display must include the code string"
    );
    assert!(
        display.contains("test denial"),
        "display must include the message"
    );
}

#[test]
fn abp_error_retryability_delegates_to_code() {
    let retryable = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let not_retryable = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert_eq!(retryable.is_retryable(), true);
    assert_eq!(not_retryable.is_retryable(), false);
}

#[test]
fn abp_error_category_delegates_to_code() {
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "fail");
    assert_eq!(err.category(), ErrorCategory::Workspace);
}

#[test]
fn error_info_preserves_retryability() {
    let err = AbpError::new(ErrorCode::BackendRateLimited, "slow down");
    let info = err.to_info();
    assert_eq!(info.is_retryable, true);
    assert_eq!(info.code, ErrorCode::BackendRateLimited);
}
