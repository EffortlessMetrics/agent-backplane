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
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_borrows_for_generic_args)]
//! End-to-end error taxonomy tests verifying error propagation through the
//! entire ABP pipeline: construction → classification → propagation →
//! serialization → receipt.

use std::error::Error as StdError;
use std::io;

use abp_config::ConfigError;
use abp_core::{AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_error_taxonomy::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
};
use abp_host::HostError;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_runtime::RuntimeError;
use chrono::Utc;

// ─── helpers ────────────────────────────────────────────────────────────────

/// All error codes in the taxonomy.
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

fn make_error_event(msg: &str, code: Option<ErrorCode>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: code,
        },
        ext: None,
    }
}

fn make_receipt_with_error(code: ErrorCode, msg: &str) -> Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Failed)
        .add_trace_event(make_error_event(msg, Some(code)))
        .build()
}

fn extract_error_code_from_receipt(receipt: &Receipt) -> Option<ErrorCode> {
    receipt.trace.iter().find_map(|e| match &e.kind {
        AgentEventKind::Error { error_code, .. } => *error_code,
        _ => None,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Backend errors propagate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_not_found_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::BackendNotFound, "no such backend");
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::BackendNotFound)
    );
}

#[test]
fn backend_unavailable_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::BackendUnavailable, "backend down");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::BackendUnavailable)
    );
}

#[test]
fn backend_timeout_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::BackendTimeout, "timed out");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::BackendTimeout)
    );
}

#[test]
fn backend_crashed_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::BackendCrashed, "exited unexpectedly");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::BackendCrashed)
    );
}

#[test]
fn backend_model_not_found_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::BackendModelNotFound, "model missing");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::BackendModelNotFound)
    );
}

#[test]
fn runtime_unknown_backend_maps_to_backend_not_found() {
    let err = RuntimeError::UnknownBackend {
        name: "nonexistent".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_backend_failed_maps_to_backend_crashed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Policy errors propagate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_denied_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::PolicyDenied, "tool blocked by policy");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::PolicyDenied)
    );
}

#[test]
fn policy_invalid_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::PolicyInvalid, "malformed policy");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::PolicyInvalid)
    );
}

#[test]
fn runtime_policy_failed_maps_to_policy_invalid() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn policy_denied_category_is_policy() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
}

#[test]
fn policy_invalid_category_is_policy() {
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Workspace errors propagate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_init_failed_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::WorkspaceInitFailed, "cannot create temp dir");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::WorkspaceInitFailed)
    );
}

#[test]
fn workspace_staging_failed_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::WorkspaceStagingFailed, "copy failed");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::WorkspaceStagingFailed)
    );
}

#[test]
fn runtime_workspace_failed_maps_to_workspace_init_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("tmp error"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn workspace_errors_belong_to_workspace_category() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.category(),
        ErrorCategory::Workspace
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Protocol errors propagate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_json_parse_error_produces_json_variant() {
    let err = JsonlCodec::decode("not valid json {{{");
    assert!(matches!(err, Err(ProtocolError::Json(_))));
}

#[test]
fn protocol_violation_maps_to_invalid_envelope() {
    let pe = ProtocolError::Violation("bad envelope".into());
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn protocol_unexpected_message_maps_to_error_code() {
    let pe = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn protocol_io_error_has_no_error_code() {
    let pe = ProtocolError::Io(io::Error::other("pipe broke"));
    assert_eq!(pe.error_code(), None);
}

#[test]
fn protocol_error_invalid_envelope_propagates_to_receipt() {
    let receipt = make_receipt_with_error(
        ErrorCode::ProtocolInvalidEnvelope,
        "failed to parse envelope",
    );
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_handshake_failed_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::ProtocolHandshakeFailed, "no hello received");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::ProtocolHandshakeFailed)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Capability errors propagate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_unsupported_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::CapabilityUnsupported, "no streaming support");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::CapabilityUnsupported)
    );
}

#[test]
fn capability_emulation_failed_propagates_to_receipt() {
    let receipt = make_receipt_with_error(
        ErrorCode::CapabilityEmulationFailed,
        "emulation layer crashed",
    );
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::CapabilityEmulationFailed)
    );
}

#[test]
fn runtime_capability_check_maps_to_capability_unsupported() {
    let err = RuntimeError::CapabilityCheckFailed("streaming required".into());
    assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn capability_errors_belong_to_capability_category() {
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.category(),
        ErrorCategory::Capability
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Config errors propagate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_invalid_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::ConfigInvalid, "bad toml");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::ConfigInvalid)
    );
}

#[test]
fn config_error_file_not_found_display() {
    let err = ConfigError::FileNotFound {
        path: "/missing.toml".into(),
    };
    assert!(err.to_string().contains("/missing.toml"));
}

#[test]
fn config_error_parse_error_display() {
    let err = ConfigError::ParseError {
        reason: "invalid toml".into(),
    };
    assert!(err.to_string().contains("invalid toml"));
}

#[test]
fn config_error_validation_display() {
    let err = ConfigError::ValidationError {
        reasons: vec!["missing field".into()],
    };
    assert!(err.to_string().contains("missing field"));
}

#[test]
fn config_error_merge_conflict_display() {
    let err = ConfigError::MergeConflict {
        reason: "conflicting timeout".into(),
    };
    assert!(err.to_string().contains("conflicting timeout"));
}

#[test]
fn config_invalid_category_is_config() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Timeout errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_timeout_is_retryable() {
    assert!(ErrorCode::BackendTimeout.is_retryable());
}

#[test]
fn backend_timeout_classifier_suggests_retry() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendTimeout);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::TimeoutError);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.is_some());
}

#[test]
fn timeout_error_propagates_through_receipt_trace() {
    let receipt = make_receipt_with_error(ErrorCode::BackendTimeout, "30s deadline exceeded");
    let code = extract_error_code_from_receipt(&receipt).unwrap();
    assert_eq!(code, ErrorCode::BackendTimeout);
    assert!(code.is_retryable());
}

#[test]
fn timeout_error_in_abp_error_preserves_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_context("timeout_ms", 30_000)
        .with_context("backend", "openai");
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.context.len(), 2);
    assert!(err.is_retryable());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Auth errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_auth_failed_not_retryable() {
    assert!(!ErrorCode::BackendAuthFailed.is_retryable());
}

#[test]
fn auth_failed_classifier_suggests_contact_admin() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    assert!(cl.recovery.delay_ms.is_none());
}

#[test]
fn auth_error_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::BackendAuthFailed, "invalid API key");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::BackendAuthFailed)
    );
}

#[test]
fn execution_permission_denied_is_auth_category() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::ExecutionPermissionDenied);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Rate limit errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_rate_limited_is_retryable() {
    assert!(ErrorCode::BackendRateLimited.is_retryable());
}

#[test]
fn rate_limited_classifier_suggests_retry_with_delay() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::RateLimit);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.unwrap() >= 1000);
}

#[test]
fn rate_limit_propagates_to_receipt() {
    let receipt = make_receipt_with_error(ErrorCode::BackendRateLimited, "429 Too Many Requests");
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::BackendRateLimited)
    );
}

#[test]
fn rate_limit_error_info_marks_retryable() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited");
    assert!(info.is_retryable);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Error chaining — root cause preserved through layers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_chains_io_source() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let abp_err =
        AbpError::new(ErrorCode::WorkspaceInitFailed, "workspace prep failed").with_source(io_err);
    let source = abp_err.source().unwrap();
    assert!(source.to_string().contains("file missing"));
}

#[test]
fn abp_error_chains_custom_source() {
    let inner = AbpError::new(ErrorCode::BackendTimeout, "inner timeout");
    let outer = AbpError::new(ErrorCode::Internal, "wrapper").with_source(inner);
    assert!(outer.source().is_some());
    assert!(
        outer
            .source()
            .unwrap()
            .to_string()
            .contains("inner timeout")
    );
}

#[test]
fn protocol_error_chains_serde_source() {
    let serde_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let pe = ProtocolError::Json(serde_err);
    assert!(pe.source().is_some());
}

#[test]
fn protocol_error_chains_io_source() {
    let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "broken");
    let pe = ProtocolError::Io(io_err);
    assert!(pe.source().is_some());
}

#[test]
fn host_error_chains_protocol_error() {
    let pe = ProtocolError::Violation("bad".into());
    let he: HostError = pe.into();
    assert!(he.to_string().contains("bad"));
}

#[test]
fn runtime_error_classified_preserves_abp_error_code() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "timeout");
    let re = RuntimeError::Classified(abp);
    assert_eq!(re.error_code(), ErrorCode::BackendTimeout);
}

#[test]
fn runtime_error_into_abp_error_preserves_code() {
    let re = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    let abp = re.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
}

#[test]
fn error_chain_three_layers_deep() {
    let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let abp_inner =
        AbpError::new(ErrorCode::ExecutionPermissionDenied, "perm denied").with_source(io_err);
    let abp_outer = AbpError::new(ErrorCode::Internal, "execution failed").with_source(abp_inner);
    // Walk chain: outer → inner → io
    let source1 = abp_outer.source().unwrap();
    assert!(source1.to_string().contains("perm denied"));
    let source2 = source1.source().unwrap();
    assert!(source2.to_string().contains("access denied"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Error display — human-readable messages
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn every_error_code_has_nonempty_message() {
    for code in ALL_CODES {
        let msg = code.message();
        assert!(!msg.is_empty(), "empty message for {code:?}");
    }
}

#[test]
fn every_error_code_has_nonempty_as_str() {
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(!s.is_empty(), "empty as_str for {code:?}");
        assert!(s.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
    }
}

#[test]
fn abp_error_display_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
    let display = err.to_string();
    assert!(display.contains("backend_timeout"), "got: {display}");
    assert!(display.contains("timed out after 30s"), "got: {display}");
}

#[test]
fn abp_error_display_includes_context() {
    let err = AbpError::new(ErrorCode::Internal, "boom").with_context("key", "value");
    let display = err.to_string();
    assert!(display.contains("key"), "got: {display}");
}

#[test]
fn error_info_display_includes_code_and_message() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "tool blocked");
    let display = info.to_string();
    assert!(display.contains("policy_denied"), "got: {display}");
    assert!(display.contains("tool blocked"), "got: {display}");
}

#[test]
fn error_code_display_uses_message_not_as_str() {
    let code = ErrorCode::BackendTimeout;
    let display = code.to_string();
    assert_eq!(display, code.message());
    assert_ne!(display, code.as_str());
}

#[test]
fn protocol_error_display_variants() {
    let pe = ProtocolError::Violation("test violation".into());
    assert!(pe.to_string().contains("test violation"));

    let pe = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert!(pe.to_string().contains("hello"));
    assert!(pe.to_string().contains("event"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Error serde — round-trip serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_serde_roundtrip_all_codes() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back, "serde roundtrip failed for {code:?}");
    }
}

#[test]
fn error_info_serde_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 30_000);
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn abp_error_dto_serde_roundtrip() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "blocked").with_context("tool", "bash");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn abp_error_dto_preserves_source_message() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "workspace error").with_source(inner);
    let dto: AbpErrorDto = (&err).into();
    assert!(
        dto.source_message
            .as_ref()
            .unwrap()
            .contains("file not found")
    );
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn error_code_serializes_to_snake_case() {
    let json = serde_json::to_string(&ErrorCode::BackendTimeout).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
}

#[test]
fn error_category_serializes_to_snake_case() {
    let json = serde_json::to_string(&ErrorCategory::Protocol).unwrap();
    assert_eq!(json, r#""protocol""#);
}

#[test]
fn error_classification_serde_roundtrip() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendRateLimited);
    let json = serde_json::to_string(&cl).unwrap();
    let back: ErrorClassification = serde_json::from_str(&json).unwrap();
    assert_eq!(cl, back);
}

#[test]
fn error_event_in_receipt_survives_serde() {
    let receipt = make_receipt_with_error(ErrorCode::BackendCrashed, "process died");
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    let code = extract_error_code_from_receipt(&back);
    assert_eq!(code, Some(ErrorCode::BackendCrashed));
}

#[test]
fn fatal_envelope_serde_roundtrip_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "auth failed",
        ErrorCode::BackendAuthFailed,
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(&encoded).unwrap();
    assert_eq!(decoded.error_code(), Some(ErrorCode::BackendAuthFailed));
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: classification exhaustiveness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn every_error_code_classifiable() {
    let c = ErrorClassifier::new();
    for code in ALL_CODES {
        let cl = c.classify(code);
        assert_eq!(cl.code, *code);
        assert!(!cl.recovery.description.is_empty());
    }
}

#[test]
fn every_error_code_has_a_category() {
    for code in ALL_CODES {
        let _ = code.category();
    }
}

#[test]
fn retryable_codes_match_classifier_retriable_severity() {
    let c = ErrorClassifier::new();
    for code in ALL_CODES {
        if code.is_retryable() {
            let cl = c.classify(code);
            assert_eq!(
                cl.severity,
                ErrorSeverity::Retriable,
                "{code:?} is retryable but classified as {:?}",
                cl.severity
            );
        }
    }
}

#[test]
fn backend_errors_all_have_backend_category() {
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
        assert_eq!(code.category(), ErrorCategory::Backend);
    }
}

#[test]
fn protocol_errors_all_have_protocol_category() {
    let protocol_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in &protocol_codes {
        assert_eq!(code.category(), ErrorCategory::Protocol);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: runtime error propagation pipeline
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_error_all_variants_have_error_codes() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("fail")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("fail")),
        RuntimeError::BackendFailed(anyhow::anyhow!("fail")),
        RuntimeError::CapabilityCheckFailed("fail".into()),
        RuntimeError::Classified(AbpError::new(ErrorCode::Internal, "fail")),
        RuntimeError::NoProjectionMatch {
            reason: "none".into(),
        },
    ];
    for err in &variants {
        let _ = err.error_code();
    }
}

#[test]
fn runtime_error_into_abp_error_all_variants() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("w")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("p")),
        RuntimeError::BackendFailed(anyhow::anyhow!("b")),
        RuntimeError::CapabilityCheckFailed("c".into()),
        RuntimeError::NoProjectionMatch { reason: "n".into() },
    ];
    for err in variants {
        let code = err.error_code();
        let abp = err.into_abp_error();
        assert_eq!(abp.code, code);
    }
}

#[test]
fn runtime_error_retryability_matches_error_code() {
    let transient = RuntimeError::BackendFailed(anyhow::anyhow!("retry me"));
    let permanent = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(transient.is_retryable());
    assert!(!permanent.is_retryable());
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: envelope error code propagation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_fatal_with_code_propagates() {
    let env = Envelope::fatal_with_code(Some("r1".into()), "test error", ErrorCode::BackendTimeout);
    assert_eq!(env.error_code(), Some(ErrorCode::BackendTimeout));
}

#[test]
fn envelope_fatal_from_abp_error_propagates() {
    let abp = AbpError::new(ErrorCode::PolicyDenied, "no tools allowed");
    let env = Envelope::fatal_from_abp_error(Some("r2".into()), &abp);
    assert_eq!(env.error_code(), Some(ErrorCode::PolicyDenied));
}

#[test]
fn envelope_non_fatal_has_no_error_code() {
    let env = Envelope::hello(
        abp_core::BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        abp_core::CapabilityManifest::new(),
    );
    assert_eq!(env.error_code(), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: AbpError → ErrorInfo → DTO pipeline
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_to_info_preserves_fields() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "timed out");
    assert!(info.is_retryable);
    assert!(info.details.contains_key("backend"));
}

#[test]
fn abp_error_from_io_implicit() {
    let io_err = io::Error::new(io::ErrorKind::Other, "io boom");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
}

#[test]
fn abp_error_from_serde_json_implicit() {
    let serde_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let abp: AbpError = serde_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
}

#[test]
fn abp_error_dto_from_roundtrip_loses_source() {
    let inner = io::Error::new(io::ErrorKind::Other, "inner");
    let err = AbpError::new(ErrorCode::Internal, "outer").with_source(inner);
    let dto: AbpErrorDto = (&err).into();
    assert!(dto.source_message.is_some());
    let restored: AbpError = dto.into();
    // DTO→AbpError loses the opaque source
    assert!(restored.source.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: multiple error events in a receipt trace
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_multiple_error_events() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Failed)
        .add_trace_event(make_error_event(
            "first error",
            Some(ErrorCode::BackendTimeout),
        ))
        .add_trace_event(make_error_event(
            "second error",
            Some(ErrorCode::BackendCrashed),
        ))
        .build();
    let codes: Vec<ErrorCode> = receipt
        .trace
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::Error { error_code, .. } => *error_code,
            _ => None,
        })
        .collect();
    assert_eq!(codes.len(), 2);
    assert!(codes.contains(&ErrorCode::BackendTimeout));
    assert!(codes.contains(&ErrorCode::BackendCrashed));
}

#[test]
fn receipt_with_mixed_events_preserves_error_codes() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Failed)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        })
        .add_trace_event(make_error_event("timeout", Some(ErrorCode::BackendTimeout)))
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "degraded".into(),
            },
            ext: None,
        })
        .build();
    assert_eq!(
        extract_error_code_from_receipt(&receipt),
        Some(ErrorCode::BackendTimeout)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: ErrorInfo with_detail chaining
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_info_with_detail_chaining() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timeout")
        .with_detail("backend", "anthropic")
        .with_detail("timeout_ms", 5000)
        .with_detail("retries", 3);
    assert_eq!(info.details.len(), 3);
}

#[test]
fn error_info_retryable_flag_from_code() {
    let retryable = ErrorInfo::new(ErrorCode::BackendTimeout, "timeout");
    assert!(retryable.is_retryable);

    let non_retryable = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
    assert!(!non_retryable.is_retryable);
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional propagation: mapping / dialect / IR / receipt / contract codes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_codes_propagate_to_receipt() {
    for code in &[
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ] {
        let receipt = make_receipt_with_error(*code, "mapping issue");
        assert_eq!(extract_error_code_from_receipt(&receipt), Some(*code));
    }
}

#[test]
fn dialect_codes_propagate_to_receipt() {
    for code in &[ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed] {
        let receipt = make_receipt_with_error(*code, "dialect issue");
        assert_eq!(extract_error_code_from_receipt(&receipt), Some(*code));
    }
}

#[test]
fn ir_codes_propagate_to_receipt() {
    for code in &[ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid] {
        let receipt = make_receipt_with_error(*code, "ir issue");
        assert_eq!(extract_error_code_from_receipt(&receipt), Some(*code));
    }
}

#[test]
fn receipt_integrity_codes_propagate() {
    for code in &[
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ] {
        let receipt = make_receipt_with_error(*code, "receipt issue");
        assert_eq!(extract_error_code_from_receipt(&receipt), Some(*code));
    }
}

#[test]
fn contract_codes_propagate_to_receipt() {
    for code in &[
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ] {
        let receipt = make_receipt_with_error(*code, "contract issue");
        assert_eq!(extract_error_code_from_receipt(&receipt), Some(*code));
    }
}

#[test]
fn execution_codes_propagate_to_receipt() {
    for code in &[
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
    ] {
        let receipt = make_receipt_with_error(*code, "exec issue");
        assert_eq!(extract_error_code_from_receipt(&receipt), Some(*code));
    }
}
