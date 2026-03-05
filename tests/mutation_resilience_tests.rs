#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Mutation-resilience tests for critical ABP code paths.
//!
//! These tests are designed to catch common mutations (boundary changes,
//! boolean inversions, comparison operator swaps, constant replacements,
//! and return value mutations) that cargo-mutants might introduce.

use abp_capability::{
    CapabilityRegistry, CompatibilityReport, EmulationStrategy, NegotiationResult, SupportLevel,
    check_capability, generate_report, negotiate, negotiate_capabilities,
};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContractError, ExecutionMode, MinSupport,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, SupportLevel as CoreSupportLevel,
    UsageNormalized, VerificationReport, WorkOrderBuilder,
};
use abp_dialect::{DetectionResult, Dialect, DialectDetector};
use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::{Decision, PolicyEngine};
use abp_protocol::{
    Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version,
    version::{ProtocolVersion, VersionRange, negotiate_version},
};
use abp_receipt::{ReceiptBuilder as ReceiptCrateBuilder, canonicalize, compute_hash, verify_hash};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::Path;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// ── Receipt hashing ────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_hash_is_64_hex_chars() {
    let r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_deterministic() {
    let r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_changes_with_outcome() {
    let r1 = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Failed)
        .build();
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    assert_ne!(h1, h2, "different outcomes must produce different hashes");
}

#[test]
fn receipt_hash_changes_with_backend_id() {
    let r1 = ReceiptCrateBuilder::new("mock-a")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptCrateBuilder::new("mock-b")
        .outcome(Outcome::Complete)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn verify_hash_with_correct_hash() {
    let mut r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_with_tampered_hash() {
    let mut r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_with_none_hash_returns_true() {
    let r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_with_empty_string_hash_fails() {
    let mut r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some(String::new());
    assert!(!verify_hash(&r));
}

#[test]
fn canonicalize_nulls_receipt_sha256() {
    let mut r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some("should_be_nulled".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains(r#""receipt_sha256":null"#));
    assert!(!json.contains("should_be_nulled"));
}

#[test]
fn canonicalize_is_deterministic() {
    let r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn receipt_with_hash_via_builder() {
    let r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_hash_via_core() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn hash_ignores_stored_hash_value() {
    let mut r = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let hash_none = compute_hash(&r).unwrap();
    r.receipt_sha256 = Some("anything".into());
    let hash_with = compute_hash(&r).unwrap();
    assert_eq!(
        hash_none, hash_with,
        "stored hash must not influence computed hash"
    );
}

#[test]
fn receipt_hash_changes_with_events() {
    let r1 = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptCrateBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        })
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════
// ── Policy enforcement ─────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_empty_allows_everything() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_read_path(Path::new("any.txt")).allowed);
    assert!(engine.can_write_path(Path::new("any.txt")).allowed);
}

#[test]
fn policy_disallowed_tool_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(d.reason.is_some());
}

#[test]
fn policy_allowed_tool_not_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_allowlist_blocks_unlisted() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
}

#[test]
fn policy_denylist_overrides_allowlist() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_deny_read_path() {
    let policy = PolicyProfile {
        deny_read: vec!["secret*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("secret.txt")).allowed);
    assert!(engine.can_read_path(Path::new("public.txt")).allowed);
}

#[test]
fn policy_deny_write_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_deny_read_does_not_affect_write() {
    let policy = PolicyProfile {
        deny_read: vec!["secret*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("secret.txt")).allowed);
    assert!(engine.can_write_path(Path::new("secret.txt")).allowed);
}

#[test]
fn policy_deny_write_does_not_affect_read() {
    let policy = PolicyProfile {
        deny_write: vec!["locked*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new("locked.md")).allowed);
    assert!(engine.can_read_path(Path::new("locked.md")).allowed);
}

#[test]
fn policy_decision_allow_fields() {
    let d = Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn policy_decision_deny_fields() {
    let d = Decision::deny("forbidden");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("forbidden"));
}

#[test]
fn policy_glob_pattern_matching() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("BashExec").allowed);
    assert!(!engine.can_use_tool("BashRun").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_deep_nested_deny_write() {
    let policy = PolicyProfile {
        deny_write: vec!["secret/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
    assert!(engine.can_write_path(Path::new("public/data.txt")).allowed);
}

#[test]
fn policy_multiple_deny_read_patterns() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/id_rsa".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// ── Error code classification ──────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_protocol_category() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolMissingRefId.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.category(),
        ErrorCategory::Protocol
    );
}

#[test]
fn error_code_backend_category() {
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
    assert_eq!(
        ErrorCode::BackendRateLimited.category(),
        ErrorCategory::Backend
    );
    assert_eq!(
        ErrorCode::BackendAuthFailed.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
    assert_eq!(
        ErrorCode::BackendUnavailable.category(),
        ErrorCategory::Backend
    );
    assert_eq!(
        ErrorCode::BackendModelNotFound.category(),
        ErrorCategory::Backend
    );
}

#[test]
fn error_code_execution_category() {
    assert_eq!(
        ErrorCode::ExecutionToolFailed.category(),
        ErrorCategory::Execution
    );
    assert_eq!(
        ErrorCode::ExecutionWorkspaceError.category(),
        ErrorCategory::Execution
    );
    assert_eq!(
        ErrorCode::ExecutionPermissionDenied.category(),
        ErrorCategory::Execution
    );
}

#[test]
fn error_code_policy_category() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn error_code_capability_category() {
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.category(),
        ErrorCategory::Capability
    );
}

#[test]
fn error_code_receipt_category() {
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(
        ErrorCode::ReceiptChainBroken.category(),
        ErrorCategory::Receipt
    );
}

#[test]
fn error_code_contract_category() {
    assert_eq!(
        ErrorCode::ContractVersionMismatch.category(),
        ErrorCategory::Contract
    );
    assert_eq!(
        ErrorCode::ContractSchemaViolation.category(),
        ErrorCategory::Contract
    );
    assert_eq!(
        ErrorCode::ContractInvalidReceipt.category(),
        ErrorCategory::Contract
    );
}

#[test]
fn error_code_workspace_category() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.category(),
        ErrorCategory::Workspace
    );
}

#[test]
fn error_code_ir_category() {
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
}

#[test]
fn error_code_dialect_category() {
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(
        ErrorCode::DialectMappingFailed.category(),
        ErrorCategory::Dialect
    );
}

#[test]
fn error_code_mapping_category() {
    assert_eq!(
        ErrorCode::MappingUnsupportedCapability.category(),
        ErrorCategory::Mapping
    );
    assert_eq!(
        ErrorCode::MappingDialectMismatch.category(),
        ErrorCategory::Mapping
    );
    assert_eq!(
        ErrorCode::MappingLossyConversion.category(),
        ErrorCategory::Mapping
    );
    assert_eq!(
        ErrorCode::MappingUnmappableTool.category(),
        ErrorCategory::Mapping
    );
}

#[test]
fn error_code_config_category() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn error_code_internal_category() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn error_code_retryable_backend_errors() {
    assert!(ErrorCode::BackendUnavailable.is_retryable());
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(ErrorCode::BackendCrashed.is_retryable());
}

#[test]
fn error_code_non_retryable_backend_errors() {
    assert!(!ErrorCode::BackendNotFound.is_retryable());
    assert!(!ErrorCode::BackendAuthFailed.is_retryable());
    assert!(!ErrorCode::BackendModelNotFound.is_retryable());
}

#[test]
fn error_code_non_retryable_others() {
    assert!(!ErrorCode::ProtocolInvalidEnvelope.is_retryable());
    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::CapabilityUnsupported.is_retryable());
    assert!(!ErrorCode::ReceiptHashMismatch.is_retryable());
    assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
    assert!(!ErrorCode::Internal.is_retryable());
    assert!(!ErrorCode::IrInvalid.is_retryable());
    assert!(!ErrorCode::ConfigInvalid.is_retryable());
    assert!(!ErrorCode::DialectUnknown.is_retryable());
    assert!(!ErrorCode::MappingDialectMismatch.is_retryable());
    assert!(!ErrorCode::WorkspaceInitFailed.is_retryable());
    assert!(!ErrorCode::ExecutionToolFailed.is_retryable());
}

#[test]
fn error_code_as_str_protocol() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.as_str(),
        "protocol_handshake_failed"
    );
    assert_eq!(
        ErrorCode::ProtocolMissingRefId.as_str(),
        "protocol_missing_ref_id"
    );
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.as_str(),
        "protocol_unexpected_message"
    );
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.as_str(),
        "protocol_version_mismatch"
    );
}

#[test]
fn error_code_as_str_backend() {
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(
        ErrorCode::BackendRateLimited.as_str(),
        "backend_rate_limited"
    );
    assert_eq!(ErrorCode::BackendAuthFailed.as_str(), "backend_auth_failed");
    assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
    assert_eq!(
        ErrorCode::BackendUnavailable.as_str(),
        "backend_unavailable"
    );
    assert_eq!(
        ErrorCode::BackendModelNotFound.as_str(),
        "backend_model_not_found"
    );
}

#[test]
fn error_code_as_str_misc() {
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(
        ErrorCode::CapabilityUnsupported.as_str(),
        "capability_unsupported"
    );
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.as_str(),
        "receipt_hash_mismatch"
    );
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
    assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
}

#[test]
fn error_code_message_not_empty() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::Internal,
    ];
    for code in &codes {
        assert!(!code.message().is_empty(), "{:?} has empty message", code);
    }
}

#[test]
fn error_info_retryable_from_code() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
    assert!(info.is_retryable);
    assert_eq!(info.code, ErrorCode::BackendTimeout);

    let info2 = ErrorInfo::new(ErrorCode::PolicyDenied, "nope");
    assert!(!info2.is_retryable);
}

#[test]
fn abp_error_category_matches_code() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
    assert_eq!(err.category(), ErrorCategory::Backend);
    assert_eq!(err.category(), err.code.category());
}

#[test]
fn abp_error_is_retryable_delegates() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
    assert!(err.is_retryable());

    let err2 = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert!(!err2.is_retryable());
}

// ═══════════════════════════════════════════════════════════════════════════
// ── Capability satisfaction ────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn check_capability_native() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    assert_eq!(
        check_capability(&manifest, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn check_capability_emulated() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Emulated);
    assert!(matches!(
        check_capability(&manifest, &Capability::Streaming),
        SupportLevel::Emulated { .. }
    ));
}

#[test]
fn check_capability_unsupported_explicit() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Unsupported);
    assert!(matches!(
        check_capability(&manifest, &Capability::Streaming),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn check_capability_absent_is_unsupported() {
    let manifest = CapabilityManifest::new();
    assert!(matches!(
        check_capability(&manifest, &Capability::Streaming),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn check_capability_restricted() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    assert!(matches!(
        check_capability(&manifest, &Capability::ToolBash),
        SupportLevel::Restricted { .. }
    ));
}

#[test]
fn negotiate_all_native() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    manifest.insert(Capability::ToolRead, CoreSupportLevel::Native);

    let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolRead], &manifest);
    assert!(result.is_viable());
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 2);
    assert!(result.emulated.is_empty());
    assert!(result.unsupported.is_empty());
}

#[test]
fn negotiate_some_unsupported() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);

    let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
    assert!(!result.is_viable());
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.unsupported.len(), 1);
    assert_eq!(result.unsupported_caps(), vec![Capability::ToolUse]);
}

#[test]
fn negotiate_empty_requirements_is_viable() {
    let manifest = CapabilityManifest::new();
    let result = negotiate_capabilities(&[], &manifest);
    assert!(result.is_viable());
    assert_eq!(result.total(), 0);
}

#[test]
fn negotiation_result_total() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Vision],
    );
    assert_eq!(result.total(), 3);
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.emulated.len(), 1);
    assert_eq!(result.unsupported.len(), 1);
    assert!(!result.is_viable());
}

#[test]
fn negotiation_result_is_viable_no_unsupported() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![],
    );
    assert!(result.is_viable());
}

#[test]
fn negotiation_result_is_not_viable_with_unsupported() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision]);
    assert!(!result.is_viable());
}

#[test]
fn generate_report_compatible() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn generate_report_incompatible() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision]);
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 1);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn support_level_satisfies_native_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_satisfies_native_emulated() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_emulated_does_not_satisfy_native() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_emulated_satisfies_emulated() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_unsupported_satisfies_nothing() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_restricted_satisfies_emulated() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "sandboxed".into(),
    };
    assert!(restricted.satisfies(&MinSupport::Emulated));
    assert!(!restricted.satisfies(&MinSupport::Native));
}

#[test]
fn capability_registry_with_defaults() {
    let reg = CapabilityRegistry::with_defaults();
    assert_eq!(reg.len(), 6);
    assert!(!reg.is_empty());
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.contains("anthropic/claude-3.5-sonnet"));
    assert!(reg.contains("google/gemini-1.5-pro"));
    assert!(reg.contains("moonshot/kimi"));
    assert!(reg.contains("openai/codex"));
    assert!(reg.contains("github/copilot"));
}

#[test]
fn capability_registry_get_missing() {
    let reg = CapabilityRegistry::new();
    assert!(reg.get("nonexistent").is_none());
    assert!(!reg.contains("nonexistent"));
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn capability_registry_register_and_unregister() {
    let mut reg = CapabilityRegistry::new();
    reg.register("test", CapabilityManifest::new());
    assert!(reg.contains("test"));
    assert_eq!(reg.len(), 1);
    assert!(reg.unregister("test"));
    assert!(!reg.contains("test"));
    assert_eq!(reg.len(), 0);
    assert!(!reg.unregister("test"));
}

#[test]
fn emulation_strategy_fidelity_loss() {
    assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
    assert!(!EmulationStrategy::ServerFallback.has_fidelity_loss());
    assert!(EmulationStrategy::Approximate.has_fidelity_loss());
}

#[test]
fn negotiation_warnings_filter_approximate() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    let warnings = result.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].0, Capability::Vision);
}

// ═══════════════════════════════════════════════════════════════════════════
// ── Protocol validation ────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/0.1"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/va.b"), None);
    assert_eq!(parse_version(""), None);
    assert_eq!(parse_version("abp/v0"), None);
}

#[test]
fn is_compatible_version_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[test]
fn is_compatible_version_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn is_compatible_version_invalid_returns_false() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "invalid"));
    assert!(!is_compatible_version("invalid", "invalid"));
}

#[test]
fn protocol_version_parse_and_format() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
    assert_eq!(v.to_string(), "abp/v0.1");
}

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn protocol_version_compatibility() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let v10 = ProtocolVersion { major: 1, minor: 0 };

    assert!(v01.is_compatible(&v02));
    assert!(!v02.is_compatible(&v01)); // remote minor < local minor
    assert!(!v01.is_compatible(&v10));
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
}

#[test]
fn version_range_is_compatible() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
}

#[test]
fn negotiate_version_same_major_picks_min() {
    let local = ProtocolVersion { major: 0, minor: 2 };
    let remote = ProtocolVersion { major: 0, minor: 1 };
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result.minor, 1);
    assert_eq!(result.major, 0);
}

#[test]
fn negotiate_version_different_major_fails() {
    let local = ProtocolVersion { major: 0, minor: 1 };
    let remote = ProtocolVersion { major: 1, minor: 0 };
    assert!(negotiate_version(&local, &remote).is_err());
}

#[test]
fn envelope_encode_decode_roundtrip_hello() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains(r#""t":"hello""#));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn envelope_encode_decode_roundtrip_fatal() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn envelope_fatal_with_code() {
    let fatal =
        Envelope::fatal_with_code(Some("run-1".into()), "timeout", ErrorCode::BackendTimeout);
    assert_eq!(fatal.error_code(), Some(ErrorCode::BackendTimeout));
}

#[test]
fn envelope_error_code_non_fatal_returns_none() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert_eq!(hello.error_code(), None);
}

#[test]
fn decode_invalid_json_returns_error() {
    let err = JsonlCodec::decode("not valid json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_stream_skips_blank_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn protocol_error_error_code_violation() {
    let err = ProtocolError::Violation("bad".into());
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn protocol_error_error_code_unexpected_message() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn contract_version_constant() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn hello_envelope_includes_contract_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    if let Envelope::Hello {
        contract_version, ..
    } = hello
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello envelope");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ── Glob matching ──────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| x.to_string()).collect()
}

#[test]
fn glob_no_patterns_allows_everything() {
    let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(g.decide_str("any/path.txt"), MatchDecision::Allowed);
}

#[test]
fn glob_include_only_gates() {
    let g = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &[]).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn glob_exclude_takes_precedence() {
    let g = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["src/gen/**"])).unwrap();
    assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        g.decide_str("src/gen/out.rs"),
        MatchDecision::DeniedByExclude
    );
}

#[test]
fn glob_match_decision_is_allowed() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn glob_exclude_only_denies_matches() {
    let g = IncludeExcludeGlobs::new(&[], &patterns(&["*.log"])).unwrap();
    assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
    assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
}

#[test]
fn glob_invalid_pattern_returns_error() {
    assert!(IncludeExcludeGlobs::new(&patterns(&["["]), &[]).is_err());
}

#[test]
fn glob_decide_path_consistency() {
    let g =
        IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["src/secret/**"])).unwrap();
    for path in &["src/lib.rs", "src/secret/key.pem", "README.md"] {
        assert_eq!(
            g.decide_str(path),
            g.decide_path(Path::new(path)),
            "mismatch for {path}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ── Dialect detection ──────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_labels() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
    assert_eq!(Dialect::Codex.label(), "Codex");
    assert_eq!(Dialect::Kimi.label(), "Kimi");
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

#[test]
fn dialect_all_returns_six() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_detect_openai() {
    let detector = DialectDetector::new();
    let val = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = detector.detect(&val);
    assert!(result.is_some());
    assert_eq!(result.unwrap().dialect, Dialect::OpenAi);
}

#[test]
fn dialect_detect_gemini() {
    let detector = DialectDetector::new();
    let val = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "generationConfig": {}
    });
    let result = detector.detect(&val);
    assert!(result.is_some());
    assert_eq!(result.unwrap().dialect, Dialect::Gemini);
}

#[test]
fn dialect_detect_non_object_returns_none() {
    let detector = DialectDetector::new();
    assert!(detector.detect(&json!("string")).is_none());
    assert!(detector.detect(&json!(42)).is_none());
    assert!(detector.detect(&json!(null)).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// ── Core type boundary conditions ──────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn outcome_serde_roundtrip() {
    for outcome in &[Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(outcome).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, outcome);
    }
}

#[test]
fn outcome_values_are_distinct() {
    assert_ne!(Outcome::Complete, Outcome::Partial);
    assert_ne!(Outcome::Complete, Outcome::Failed);
    assert_ne!(Outcome::Partial, Outcome::Failed);
}

#[test]
fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
}

#[test]
fn sha256_hex_length() {
    let h = abp_core::sha256_hex(b"hello");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn sha256_hex_deterministic() {
    assert_eq!(abp_core::sha256_hex(b"test"), abp_core::sha256_hex(b"test"));
}

#[test]
fn sha256_hex_different_inputs_different_hashes() {
    assert_ne!(
        abp_core::sha256_hex(b"hello"),
        abp_core::sha256_hex(b"world")
    );
}

#[test]
fn canonical_json_sorts_keys() {
    let json = abp_core::canonical_json(&json!({"z": 1, "a": 2})).unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < z_pos, "keys should be sorted: {json}");
}
