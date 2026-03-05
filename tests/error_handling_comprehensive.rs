#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive error handling test suite for the Agent Backplane workspace.
//!
//! Covers: abp-error, abp-error-taxonomy, abp-core error types,
//! Display impls, Error source chains, code stability, serde roundtrips,
//! From conversions, ErrorClassifier, and more.

use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;
use std::io;

// ── abp-error types ──────────────────────────────────────────────────────
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode as AbpErrorCode, ErrorInfo};

// ── abp-error-taxonomy types ─────────────────────────────────────────────
use abp_error_taxonomy::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
    RecoverySuggestion,
};

// ── abp-core error types ─────────────────────────────────────────────────
use abp_core::chain::ChainError;
use abp_core::error::{
    ErrorCatalog, ErrorCode as CoreErrorCode, ErrorInfo as CoreErrorInfo,
    MappingError as CoreMappingError, MappingErrorKind,
};
use abp_core::validate::ValidationError;
use abp_core::ContractError;

// =========================================================================
// Helpers
// =========================================================================

/// All 36 abp_error::ErrorCode variants.
const ABP_ALL_CODES: &[AbpErrorCode] = &[
    AbpErrorCode::ProtocolInvalidEnvelope,
    AbpErrorCode::ProtocolHandshakeFailed,
    AbpErrorCode::ProtocolMissingRefId,
    AbpErrorCode::ProtocolUnexpectedMessage,
    AbpErrorCode::ProtocolVersionMismatch,
    AbpErrorCode::MappingUnsupportedCapability,
    AbpErrorCode::MappingDialectMismatch,
    AbpErrorCode::MappingLossyConversion,
    AbpErrorCode::MappingUnmappableTool,
    AbpErrorCode::BackendNotFound,
    AbpErrorCode::BackendUnavailable,
    AbpErrorCode::BackendTimeout,
    AbpErrorCode::BackendRateLimited,
    AbpErrorCode::BackendAuthFailed,
    AbpErrorCode::BackendModelNotFound,
    AbpErrorCode::BackendCrashed,
    AbpErrorCode::ExecutionToolFailed,
    AbpErrorCode::ExecutionWorkspaceError,
    AbpErrorCode::ExecutionPermissionDenied,
    AbpErrorCode::ContractVersionMismatch,
    AbpErrorCode::ContractSchemaViolation,
    AbpErrorCode::ContractInvalidReceipt,
    AbpErrorCode::CapabilityUnsupported,
    AbpErrorCode::CapabilityEmulationFailed,
    AbpErrorCode::PolicyDenied,
    AbpErrorCode::PolicyInvalid,
    AbpErrorCode::WorkspaceInitFailed,
    AbpErrorCode::WorkspaceStagingFailed,
    AbpErrorCode::IrLoweringFailed,
    AbpErrorCode::IrInvalid,
    AbpErrorCode::ReceiptHashMismatch,
    AbpErrorCode::ReceiptChainBroken,
    AbpErrorCode::DialectUnknown,
    AbpErrorCode::DialectMappingFailed,
    AbpErrorCode::ConfigInvalid,
    AbpErrorCode::Internal,
];

// =========================================================================
// Module: abp_error – ErrorCode basics
// =========================================================================

#[test]
fn abp_error_code_count_is_36() {
    assert_eq!(ABP_ALL_CODES.len(), 36);
}

#[test]
fn abp_error_codes_have_unique_as_str() {
    let mut seen = HashSet::new();
    for code in ABP_ALL_CODES {
        assert!(seen.insert(code.as_str()), "duplicate: {}", code.as_str());
    }
}

#[test]
fn abp_error_codes_have_unique_messages() {
    let mut seen = HashSet::new();
    for code in ABP_ALL_CODES {
        assert!(
            seen.insert(code.message()),
            "dup message: {}",
            code.message()
        );
    }
}

#[test]
fn abp_error_code_as_str_matches_serde() {
    for code in ABP_ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        assert_eq!(json, format!("\"{}\"", code.as_str()));
    }
}

#[test]
fn abp_error_code_display_returns_message() {
    for code in ABP_ALL_CODES {
        assert_eq!(code.to_string(), code.message());
    }
}

#[test]
fn abp_error_code_serde_roundtrip_all() {
    for &code in ABP_ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let back: AbpErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code);
    }
}

#[test]
fn abp_error_code_debug_contains_variant_name() {
    let dbg = format!("{:?}", AbpErrorCode::BackendTimeout);
    assert!(dbg.contains("BackendTimeout"));
}

// =========================================================================
// Module: abp_error – ErrorCategory
// =========================================================================

#[test]
fn error_category_protocol_codes() {
    let codes = [
        AbpErrorCode::ProtocolInvalidEnvelope,
        AbpErrorCode::ProtocolHandshakeFailed,
        AbpErrorCode::ProtocolMissingRefId,
        AbpErrorCode::ProtocolUnexpectedMessage,
        AbpErrorCode::ProtocolVersionMismatch,
    ];
    for c in &codes {
        assert_eq!(c.category(), ErrorCategory::Protocol);
    }
}

#[test]
fn error_category_mapping_codes() {
    let codes = [
        AbpErrorCode::MappingUnsupportedCapability,
        AbpErrorCode::MappingDialectMismatch,
        AbpErrorCode::MappingLossyConversion,
        AbpErrorCode::MappingUnmappableTool,
    ];
    for c in &codes {
        assert_eq!(c.category(), ErrorCategory::Mapping);
    }
}

#[test]
fn error_category_backend_codes() {
    let codes = [
        AbpErrorCode::BackendNotFound,
        AbpErrorCode::BackendUnavailable,
        AbpErrorCode::BackendTimeout,
        AbpErrorCode::BackendRateLimited,
        AbpErrorCode::BackendAuthFailed,
        AbpErrorCode::BackendModelNotFound,
        AbpErrorCode::BackendCrashed,
    ];
    for c in &codes {
        assert_eq!(c.category(), ErrorCategory::Backend);
    }
}

#[test]
fn error_category_execution_codes() {
    let codes = [
        AbpErrorCode::ExecutionToolFailed,
        AbpErrorCode::ExecutionWorkspaceError,
        AbpErrorCode::ExecutionPermissionDenied,
    ];
    for c in &codes {
        assert_eq!(c.category(), ErrorCategory::Execution);
    }
}

#[test]
fn error_category_contract_codes() {
    let codes = [
        AbpErrorCode::ContractVersionMismatch,
        AbpErrorCode::ContractSchemaViolation,
        AbpErrorCode::ContractInvalidReceipt,
    ];
    for c in &codes {
        assert_eq!(c.category(), ErrorCategory::Contract);
    }
}

#[test]
fn error_category_capability_codes() {
    assert_eq!(
        AbpErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(
        AbpErrorCode::CapabilityEmulationFailed.category(),
        ErrorCategory::Capability
    );
}

#[test]
fn error_category_policy_codes() {
    assert_eq!(AbpErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(
        AbpErrorCode::PolicyInvalid.category(),
        ErrorCategory::Policy
    );
}

#[test]
fn error_category_workspace_codes() {
    assert_eq!(
        AbpErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
    assert_eq!(
        AbpErrorCode::WorkspaceStagingFailed.category(),
        ErrorCategory::Workspace
    );
}

#[test]
fn error_category_ir_codes() {
    assert_eq!(AbpErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(AbpErrorCode::IrInvalid.category(), ErrorCategory::Ir);
}

#[test]
fn error_category_receipt_codes() {
    assert_eq!(
        AbpErrorCode::ReceiptHashMismatch.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(
        AbpErrorCode::ReceiptChainBroken.category(),
        ErrorCategory::Receipt
    );
}

#[test]
fn error_category_dialect_codes() {
    assert_eq!(
        AbpErrorCode::DialectUnknown.category(),
        ErrorCategory::Dialect
    );
    assert_eq!(
        AbpErrorCode::DialectMappingFailed.category(),
        ErrorCategory::Dialect
    );
}

#[test]
fn error_category_config_code() {
    assert_eq!(
        AbpErrorCode::ConfigInvalid.category(),
        ErrorCategory::Config
    );
}

#[test]
fn error_category_internal_code() {
    assert_eq!(AbpErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn error_category_display_all_variants() {
    let pairs = [
        (ErrorCategory::Protocol, "protocol"),
        (ErrorCategory::Backend, "backend"),
        (ErrorCategory::Capability, "capability"),
        (ErrorCategory::Policy, "policy"),
        (ErrorCategory::Workspace, "workspace"),
        (ErrorCategory::Ir, "ir"),
        (ErrorCategory::Receipt, "receipt"),
        (ErrorCategory::Dialect, "dialect"),
        (ErrorCategory::Config, "config"),
        (ErrorCategory::Mapping, "mapping"),
        (ErrorCategory::Execution, "execution"),
        (ErrorCategory::Contract, "contract"),
        (ErrorCategory::Internal, "internal"),
    ];
    for (cat, expected) in &pairs {
        assert_eq!(cat.to_string(), *expected);
    }
}

#[test]
fn error_category_serde_roundtrip() {
    let cats = [
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Mapping,
        ErrorCategory::Execution,
        ErrorCategory::Contract,
        ErrorCategory::Internal,
    ];
    for cat in cats {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat);
    }
}

// =========================================================================
// Module: abp_error – Retryability
// =========================================================================

#[test]
fn retryable_codes_are_exactly_four() {
    let retryable: Vec<_> = ABP_ALL_CODES.iter().filter(|c| c.is_retryable()).collect();
    assert_eq!(retryable.len(), 4);
}

#[test]
fn retryable_backend_unavailable() {
    assert!(AbpErrorCode::BackendUnavailable.is_retryable());
}

#[test]
fn retryable_backend_timeout() {
    assert!(AbpErrorCode::BackendTimeout.is_retryable());
}

#[test]
fn retryable_backend_rate_limited() {
    assert!(AbpErrorCode::BackendRateLimited.is_retryable());
}

#[test]
fn retryable_backend_crashed() {
    assert!(AbpErrorCode::BackendCrashed.is_retryable());
}

#[test]
fn non_retryable_protocol_codes() {
    assert!(!AbpErrorCode::ProtocolInvalidEnvelope.is_retryable());
    assert!(!AbpErrorCode::ProtocolHandshakeFailed.is_retryable());
}

#[test]
fn non_retryable_policy_codes() {
    assert!(!AbpErrorCode::PolicyDenied.is_retryable());
    assert!(!AbpErrorCode::PolicyInvalid.is_retryable());
}

// =========================================================================
// Module: abp_error – AbpError construction & Display
// =========================================================================

#[test]
fn abp_error_basic_construction() {
    let err = AbpError::new(AbpErrorCode::Internal, "boom");
    assert_eq!(err.code, AbpErrorCode::Internal);
    assert_eq!(err.message, "boom");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_display_without_context() {
    let err = AbpError::new(AbpErrorCode::BackendNotFound, "no such backend");
    assert_eq!(err.to_string(), "[backend_not_found] no such backend");
}

#[test]
fn abp_error_display_with_context() {
    let err =
        AbpError::new(AbpErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 5000);
    let s = err.to_string();
    assert!(s.starts_with("[backend_timeout] timed out"));
    assert!(s.contains("timeout_ms"));
}

#[test]
fn abp_error_debug_format() {
    let err = AbpError::new(AbpErrorCode::PolicyDenied, "denied");
    let dbg = format!("{:?}", err);
    assert!(dbg.contains("PolicyDenied"));
    assert!(dbg.contains("denied"));
}

#[test]
fn abp_error_debug_with_source() {
    let src = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(AbpErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    let dbg = format!("{:?}", err);
    assert!(dbg.contains("source"));
    assert!(dbg.contains("file missing"));
}

#[test]
fn abp_error_with_context_multiple_keys() {
    let err = AbpError::new(AbpErrorCode::BackendTimeout, "timeout")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    assert_eq!(err.context.len(), 3);
}

#[test]
fn abp_error_with_source() {
    let src = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let err = AbpError::new(AbpErrorCode::PolicyDenied, "denied").with_source(src);
    assert!(err.source.is_some());
}

#[test]
fn abp_error_category_shorthand() {
    let err = AbpError::new(AbpErrorCode::DialectUnknown, "unknown");
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

#[test]
fn abp_error_is_retryable_delegates() {
    assert!(AbpError::new(AbpErrorCode::BackendTimeout, "t").is_retryable());
    assert!(!AbpError::new(AbpErrorCode::PolicyDenied, "d").is_retryable());
}

// =========================================================================
// Module: abp_error – Error source chain (std::error::Error)
// =========================================================================

#[test]
fn abp_error_source_chain_present() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "not found");
    let err = AbpError::new(AbpErrorCode::WorkspaceStagingFailed, "staging").with_source(inner);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "not found");
}

#[test]
fn abp_error_source_none_by_default() {
    let err = AbpError::new(AbpErrorCode::Internal, "oops");
    assert!(StdError::source(&err).is_none());
}

// =========================================================================
// Module: abp_error – From conversions
// =========================================================================

#[test]
fn from_io_error_to_abp_error() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let abp_err: AbpError = io_err.into();
    assert_eq!(abp_err.code, AbpErrorCode::Internal);
    assert!(abp_err.message.contains("file not found"));
    assert!(abp_err.source.is_some());
}

#[test]
fn from_serde_json_error_to_abp_error() {
    let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let abp_err: AbpError = json_err.into();
    assert_eq!(abp_err.code, AbpErrorCode::ProtocolInvalidEnvelope);
    assert!(abp_err.source.is_some());
}

#[test]
fn question_mark_operator_io_error() {
    fn inner() -> Result<(), AbpError> {
        let _: Vec<u8> = std::fs::read("nonexistent_path_that_does_not_exist_12345")?;
        Ok(())
    }
    let result = inner();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, AbpErrorCode::Internal);
}

#[test]
fn question_mark_operator_serde_error() {
    fn inner() -> Result<serde_json::Value, AbpError> {
        Ok(serde_json::from_str("{{bad json")?)
    }
    let result = inner();
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().code,
        AbpErrorCode::ProtocolInvalidEnvelope
    );
}

// =========================================================================
// Module: abp_error – AbpErrorDto
// =========================================================================

#[test]
fn dto_from_abp_error_without_source() {
    let err = AbpError::new(AbpErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, AbpErrorCode::IrInvalid);
    assert_eq!(dto.message, "bad IR");
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_from_abp_error_with_source() {
    let src = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(AbpErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn dto_serde_roundtrip() {
    let err =
        AbpError::new(AbpErrorCode::ConfigInvalid, "bad config").with_context("file", "bp.toml");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_to_abp_error_loses_source() {
    let dto = AbpErrorDto {
        code: AbpErrorCode::ConfigInvalid,
        message: "bad".into(),
        context: BTreeMap::new(),
        source_message: Some("inner".into()),
        location: None,
        cause_chain: Vec::new(),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, AbpErrorCode::ConfigInvalid);
    assert!(err.source.is_none());
}

#[test]
fn dto_context_preserved() {
    let err = AbpError::new(AbpErrorCode::Internal, "e")
        .with_context("k1", "v1")
        .with_context("k2", 42);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.context.len(), 2);
}

// =========================================================================
// Module: abp_error – ErrorInfo
// =========================================================================

#[test]
fn error_info_construction_retryable() {
    let info = ErrorInfo::new(AbpErrorCode::BackendTimeout, "timed out");
    assert_eq!(info.code, AbpErrorCode::BackendTimeout);
    assert!(info.is_retryable);
    assert!(info.details.is_empty());
}

#[test]
fn error_info_construction_non_retryable() {
    let info = ErrorInfo::new(AbpErrorCode::PolicyDenied, "denied");
    assert!(!info.is_retryable);
}

#[test]
fn error_info_with_details() {
    let info = ErrorInfo::new(AbpErrorCode::BackendRateLimited, "rate limited")
        .with_detail("retry_after_ms", 5000)
        .with_detail("backend", "openai");
    assert_eq!(info.details.len(), 2);
}

#[test]
fn error_info_display() {
    let info = ErrorInfo::new(AbpErrorCode::ExecutionToolFailed, "tool crashed");
    assert_eq!(info.to_string(), "[execution_tool_failed] tool crashed");
}

#[test]
fn error_info_serde_roundtrip() {
    let info = ErrorInfo::new(AbpErrorCode::ContractSchemaViolation, "bad schema")
        .with_detail("field", "work_order.task");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn error_info_deterministic_order() {
    let info = ErrorInfo::new(AbpErrorCode::Internal, "err")
        .with_detail("z_key", "last")
        .with_detail("a_key", "first");
    let json = serde_json::to_string(&info).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn abp_error_to_info_conversion() {
    let err = AbpError::new(AbpErrorCode::BackendTimeout, "timeout").with_context("ms", 3000);
    let info = err.to_info();
    assert_eq!(info.code, AbpErrorCode::BackendTimeout);
    assert!(info.is_retryable);
    assert_eq!(info.details["ms"], serde_json::json!(3000));
}

// =========================================================================
// Module: abp_error – Error code stability (codes must not change)
// =========================================================================

#[test]
fn error_code_stability_protocol() {
    assert_eq!(
        AbpErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
    assert_eq!(
        AbpErrorCode::ProtocolHandshakeFailed.as_str(),
        "protocol_handshake_failed"
    );
    assert_eq!(
        AbpErrorCode::ProtocolMissingRefId.as_str(),
        "protocol_missing_ref_id"
    );
    assert_eq!(
        AbpErrorCode::ProtocolUnexpectedMessage.as_str(),
        "protocol_unexpected_message"
    );
    assert_eq!(
        AbpErrorCode::ProtocolVersionMismatch.as_str(),
        "protocol_version_mismatch"
    );
}

#[test]
fn error_code_stability_mapping() {
    assert_eq!(
        AbpErrorCode::MappingUnsupportedCapability.as_str(),
        "mapping_unsupported_capability"
    );
    assert_eq!(
        AbpErrorCode::MappingDialectMismatch.as_str(),
        "mapping_dialect_mismatch"
    );
    assert_eq!(
        AbpErrorCode::MappingLossyConversion.as_str(),
        "mapping_lossy_conversion"
    );
    assert_eq!(
        AbpErrorCode::MappingUnmappableTool.as_str(),
        "mapping_unmappable_tool"
    );
}

#[test]
fn error_code_stability_backend() {
    assert_eq!(AbpErrorCode::BackendNotFound.as_str(), "backend_not_found");
    assert_eq!(
        AbpErrorCode::BackendUnavailable.as_str(),
        "backend_unavailable"
    );
    assert_eq!(AbpErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(
        AbpErrorCode::BackendRateLimited.as_str(),
        "backend_rate_limited"
    );
    assert_eq!(
        AbpErrorCode::BackendAuthFailed.as_str(),
        "backend_auth_failed"
    );
    assert_eq!(
        AbpErrorCode::BackendModelNotFound.as_str(),
        "backend_model_not_found"
    );
    assert_eq!(AbpErrorCode::BackendCrashed.as_str(), "backend_crashed");
}

#[test]
fn error_code_stability_misc() {
    assert_eq!(AbpErrorCode::Internal.as_str(), "internal");
    assert_eq!(AbpErrorCode::ConfigInvalid.as_str(), "config_invalid");
    assert_eq!(AbpErrorCode::DialectUnknown.as_str(), "dialect_unknown");
    assert_eq!(AbpErrorCode::IrInvalid.as_str(), "ir_invalid");
}

// =========================================================================
// Module: abp_error – ErrorCode message stability
// =========================================================================

#[test]
fn message_stability_selected() {
    assert_eq!(AbpErrorCode::BackendTimeout.message(), "backend timed out");
    assert_eq!(
        AbpErrorCode::PolicyDenied.message(),
        "policy rule denied the operation"
    );
    assert_eq!(
        AbpErrorCode::Internal.message(),
        "unexpected internal error"
    );
    assert_eq!(
        AbpErrorCode::ProtocolInvalidEnvelope.message(),
        "envelope failed to parse or has invalid fields"
    );
}

// =========================================================================
// Module: abp-error-taxonomy – ErrorClassifier
// =========================================================================

#[test]
fn classifier_default_trait() {
    let c = ErrorClassifier::default();
    let cl = c.classify(&AbpErrorCode::Internal);
    assert_eq!(cl.code, AbpErrorCode::Internal);
}

#[test]
fn classifier_rate_limited_is_retriable() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::BackendRateLimited);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::RateLimit);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.is_some());
}

#[test]
fn classifier_auth_failed_is_fatal() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::BackendAuthFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn classifier_backend_timeout_is_retriable() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::BackendTimeout);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::TimeoutError);
}

#[test]
fn classifier_backend_unavailable_is_retriable() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::BackendUnavailable);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
}

#[test]
fn classifier_protocol_errors_are_fatal() {
    let c = ErrorClassifier::new();
    for code in [
        AbpErrorCode::ProtocolInvalidEnvelope,
        AbpErrorCode::ProtocolHandshakeFailed,
        AbpErrorCode::ProtocolVersionMismatch,
    ] {
        let cl = c.classify(&code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Fatal,
            "expected fatal for {:?}",
            code
        );
    }
}

#[test]
fn classifier_mapping_lossy_is_degraded() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    assert_eq!(cl.category, ClassificationCategory::MappingFailure);
}

#[test]
fn classifier_capability_emulation_is_degraded() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::CapabilityEmulationFailed);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
}

#[test]
fn classifier_model_not_found() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::BackendModelNotFound);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ModelNotFound);
    assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
}

#[test]
fn classifier_policy_denied_is_content_filter() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::PolicyDenied);
    assert_eq!(cl.category, ClassificationCategory::ContentFilter);
}

#[test]
fn classifier_internal_is_fatal_server() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::Internal);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
}

#[test]
fn classifier_suggest_recovery_matches_classify() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::BackendRateLimited);
    let suggestion = c.suggest_recovery(&cl);
    assert_eq!(suggestion.action, cl.recovery.action);
}

// =========================================================================
// Module: abp-error-taxonomy – Serde for taxonomy types
// =========================================================================

#[test]
fn error_severity_serde_roundtrip() {
    let sevs = [
        ErrorSeverity::Fatal,
        ErrorSeverity::Retriable,
        ErrorSeverity::Degraded,
        ErrorSeverity::Informational,
    ];
    for sev in sevs {
        let json = serde_json::to_string(&sev).unwrap();
        let back: ErrorSeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sev);
    }
}

#[test]
fn classification_category_serde_roundtrip() {
    let cats = [
        ClassificationCategory::Authentication,
        ClassificationCategory::RateLimit,
        ClassificationCategory::ModelNotFound,
        ClassificationCategory::InvalidRequest,
        ClassificationCategory::ContentFilter,
        ClassificationCategory::ContextLength,
        ClassificationCategory::ServerError,
        ClassificationCategory::NetworkError,
        ClassificationCategory::ProtocolError,
        ClassificationCategory::CapabilityUnsupported,
        ClassificationCategory::MappingFailure,
        ClassificationCategory::TimeoutError,
    ];
    for cat in cats {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ClassificationCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat);
    }
}

#[test]
fn recovery_action_serde_roundtrip() {
    let actions = [
        RecoveryAction::Retry,
        RecoveryAction::Fallback,
        RecoveryAction::ReduceContext,
        RecoveryAction::ChangeModel,
        RecoveryAction::ContactAdmin,
        RecoveryAction::None,
    ];
    for action in actions {
        let json = serde_json::to_string(&action).unwrap();
        let back: RecoveryAction = serde_json::from_str(&json).unwrap();
        assert_eq!(back, action);
    }
}

#[test]
fn recovery_suggestion_serde_roundtrip() {
    let rs = RecoverySuggestion {
        action: RecoveryAction::Retry,
        description: "retry after delay".into(),
        delay_ms: Some(1000),
    };
    let json = serde_json::to_string(&rs).unwrap();
    let back: RecoverySuggestion = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rs);
}

#[test]
fn error_classification_serde_roundtrip() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&AbpErrorCode::BackendTimeout);
    let json = serde_json::to_string(&cl).unwrap();
    let back: ErrorClassification = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cl);
}

// =========================================================================
// Module: abp-core – ErrorCode (ABP-X### codes)
// =========================================================================

#[test]
fn core_error_code_count() {
    let all = ErrorCatalog::all();
    assert!(!all.is_empty());
    // 12 contract + 12 protocol + 11 policy + 13 runtime + 11 system = 59
    assert_eq!(all.len(), 59);
}

#[test]
fn core_error_code_unique_codes() {
    let all = ErrorCatalog::all();
    let mut seen = HashSet::new();
    for c in &all {
        assert!(seen.insert(c.code()), "dup code: {}", c.code());
    }
}

#[test]
fn core_error_code_lookup() {
    assert_eq!(
        ErrorCatalog::lookup("ABP-C001"),
        Some(CoreErrorCode::InvalidContractVersion)
    );
    assert_eq!(
        ErrorCatalog::lookup("ABP-P001"),
        Some(CoreErrorCode::InvalidEnvelope)
    );
    assert_eq!(
        ErrorCatalog::lookup("ABP-L001"),
        Some(CoreErrorCode::ToolDenied)
    );
    assert_eq!(
        ErrorCatalog::lookup("ABP-R001"),
        Some(CoreErrorCode::BackendUnavailable)
    );
    assert_eq!(
        ErrorCatalog::lookup("ABP-S001"),
        Some(CoreErrorCode::IoError)
    );
    assert_eq!(ErrorCatalog::lookup("ABP-Z999"), None);
}

#[test]
fn core_error_code_by_category_contract() {
    let contracts = ErrorCatalog::by_category("contract");
    assert_eq!(contracts.len(), 12);
    for c in &contracts {
        assert_eq!(c.category(), "contract");
    }
}

#[test]
fn core_error_code_by_category_protocol() {
    let protos = ErrorCatalog::by_category("protocol");
    assert_eq!(protos.len(), 12);
    for c in &protos {
        assert_eq!(c.category(), "protocol");
    }
}

#[test]
fn core_error_code_by_category_policy() {
    let policies = ErrorCatalog::by_category("policy");
    assert_eq!(policies.len(), 11);
}

#[test]
fn core_error_code_by_category_runtime() {
    let runtimes = ErrorCatalog::by_category("runtime");
    assert_eq!(runtimes.len(), 13);
}

#[test]
fn core_error_code_by_category_system() {
    let systems = ErrorCatalog::by_category("system");
    assert_eq!(systems.len(), 11);
}

#[test]
fn core_error_code_display_shows_code_string() {
    assert_eq!(
        CoreErrorCode::InvalidContractVersion.to_string(),
        "ABP-C001"
    );
    assert_eq!(CoreErrorCode::IoError.to_string(), "ABP-S001");
}

#[test]
fn core_error_code_description_non_empty() {
    for c in ErrorCatalog::all() {
        assert!(!c.description().is_empty(), "empty desc for {}", c.code());
    }
}

#[test]
fn core_error_code_implements_std_error() {
    let code = CoreErrorCode::InternalError;
    let _: &dyn StdError = &code;
}

#[test]
fn core_error_code_serde_roundtrip() {
    for code in ErrorCatalog::all() {
        let json = serde_json::to_string(&code).unwrap();
        let back: CoreErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code);
    }
}

// =========================================================================
// Module: abp-core – ErrorInfo
// =========================================================================

#[test]
fn core_error_info_construction() {
    let info = CoreErrorInfo::new(CoreErrorCode::BackendTimeout, "timed out");
    assert_eq!(info.code, CoreErrorCode::BackendTimeout);
    assert_eq!(info.message, "timed out");
    assert!(info.context.is_empty());
    assert!(info.source.is_none());
}

#[test]
fn core_error_info_with_context() {
    let info =
        CoreErrorInfo::new(CoreErrorCode::IoError, "read failed").with_context("path", "/tmp/file");
    assert_eq!(info.context["path"], "/tmp/file");
}

#[test]
fn core_error_info_with_source() {
    let src = io::Error::new(io::ErrorKind::NotFound, "not found");
    let info = CoreErrorInfo::new(CoreErrorCode::IoError, "read failed").with_source(src);
    assert!(info.source.is_some());
}

#[test]
fn core_error_info_display_without_context() {
    let info = CoreErrorInfo::new(CoreErrorCode::InternalError, "oops");
    assert_eq!(info.to_string(), "[ABP-S003] oops");
}

#[test]
fn core_error_info_display_with_context() {
    let info = CoreErrorInfo::new(CoreErrorCode::IoError, "failed").with_context("path", "/tmp");
    let s = info.to_string();
    assert!(s.contains("[ABP-S001]"));
    assert!(s.contains("path=/tmp"));
}

#[test]
fn core_error_info_source_chain() {
    let inner = io::Error::new(io::ErrorKind::BrokenPipe, "pipe");
    let info = CoreErrorInfo::new(CoreErrorCode::ChannelClosed, "closed").with_source(inner);
    let src = StdError::source(&info).unwrap();
    assert_eq!(src.to_string(), "pipe");
}

#[test]
fn core_error_info_debug_format() {
    let info = CoreErrorInfo::new(CoreErrorCode::ConfigurationError, "bad");
    let dbg = format!("{:?}", info);
    assert!(dbg.contains("ConfigurationError"));
}

// =========================================================================
// Module: abp-core – MappingError
// =========================================================================

#[test]
fn mapping_error_fidelity_loss() {
    let err = CoreMappingError::FidelityLoss {
        field: "temperature".into(),
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        detail: "range differs".into(),
    };
    assert_eq!(err.code(), CoreMappingError::FIDELITY_LOSS_CODE);
    assert_eq!(err.kind(), MappingErrorKind::Degraded);
    assert!(err.is_degraded());
    assert!(!err.is_fatal());
    assert!(!err.is_emulated());
}

#[test]
fn mapping_error_unsupported_capability() {
    let err = CoreMappingError::UnsupportedCapability {
        capability: "tool_use".into(),
        dialect: "basic".into(),
    };
    assert_eq!(err.code(), CoreMappingError::UNSUPPORTED_CAP_CODE);
    assert_eq!(err.kind(), MappingErrorKind::Fatal);
    assert!(err.is_fatal());
}

#[test]
fn mapping_error_emulation_required() {
    let err = CoreMappingError::EmulationRequired {
        feature: "streaming".into(),
        detail: "will buffer".into(),
    };
    assert_eq!(err.code(), CoreMappingError::EMULATION_REQUIRED_CODE);
    assert_eq!(err.kind(), MappingErrorKind::Emulated);
    assert!(err.is_emulated());
}

#[test]
fn mapping_error_incompatible_model() {
    let err = CoreMappingError::IncompatibleModel {
        requested: "gpt-4".into(),
        dialect: "claude".into(),
        suggestion: Some("claude-3".into()),
    };
    assert_eq!(err.code(), CoreMappingError::INCOMPATIBLE_MODEL_CODE);
    assert!(err.is_fatal());
    let s = err.to_string();
    assert!(s.contains("gpt-4"));
    assert!(s.contains("try claude-3"));
}

#[test]
fn mapping_error_incompatible_model_no_suggestion() {
    let err = CoreMappingError::IncompatibleModel {
        requested: "gpt-4".into(),
        dialect: "claude".into(),
        suggestion: None,
    };
    let s = err.to_string();
    assert!(!s.contains("try"));
}

#[test]
fn mapping_error_parameter_not_mappable() {
    let err = CoreMappingError::ParameterNotMappable {
        parameter: "top_k".into(),
        value: "40".into(),
        dialect: "openai".into(),
    };
    assert_eq!(err.code(), CoreMappingError::PARAM_NOT_MAPPABLE_CODE);
    assert!(err.is_degraded());
}

#[test]
fn mapping_error_streaming_unsupported() {
    let err = CoreMappingError::StreamingUnsupported {
        dialect: "batch-only".into(),
    };
    assert_eq!(err.code(), CoreMappingError::STREAMING_UNSUPPORTED_CODE);
    assert!(err.is_fatal());
}

#[test]
fn mapping_error_display_contains_code() {
    let err = CoreMappingError::FidelityLoss {
        field: "f".into(),
        source_dialect: "a".into(),
        target_dialect: "b".into(),
        detail: "d".into(),
    };
    let s = err.to_string();
    assert!(s.contains(CoreMappingError::FIDELITY_LOSS_CODE));
}

#[test]
fn mapping_error_kind_display() {
    assert_eq!(MappingErrorKind::Fatal.to_string(), "fatal");
    assert_eq!(MappingErrorKind::Degraded.to_string(), "degraded");
    assert_eq!(MappingErrorKind::Emulated.to_string(), "emulated");
}

#[test]
fn mapping_error_serde_roundtrip() {
    let err = CoreMappingError::StreamingUnsupported {
        dialect: "basic".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: CoreMappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

#[test]
fn mapping_error_kind_serde_roundtrip() {
    for kind in [
        MappingErrorKind::Fatal,
        MappingErrorKind::Degraded,
        MappingErrorKind::Emulated,
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        let back: MappingErrorKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }
}

#[test]
fn mapping_error_stable_codes() {
    assert_eq!(CoreMappingError::FIDELITY_LOSS_CODE, "ABP_E_FIDELITY_LOSS");
    assert_eq!(
        CoreMappingError::UNSUPPORTED_CAP_CODE,
        "ABP_E_UNSUPPORTED_CAP"
    );
    assert_eq!(
        CoreMappingError::EMULATION_REQUIRED_CODE,
        "ABP_E_EMULATION_REQUIRED"
    );
    assert_eq!(
        CoreMappingError::INCOMPATIBLE_MODEL_CODE,
        "ABP_E_INCOMPATIBLE_MODEL"
    );
    assert_eq!(
        CoreMappingError::PARAM_NOT_MAPPABLE_CODE,
        "ABP_E_PARAM_NOT_MAPPABLE"
    );
    assert_eq!(
        CoreMappingError::STREAMING_UNSUPPORTED_CODE,
        "ABP_E_STREAMING_UNSUPPORTED"
    );
}

// =========================================================================
// Module: abp-core – ContractError
// =========================================================================

#[test]
fn contract_error_from_serde_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("bad json").unwrap_err();
    let err: ContractError = json_err.into();
    let s = err.to_string();
    assert!(s.contains("serialize JSON"));
}

#[test]
fn contract_error_debug_format() {
    let json_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
    let err: ContractError = json_err.into();
    let dbg = format!("{:?}", err);
    assert!(dbg.contains("Json"));
}

// =========================================================================
// Module: abp-core – ValidationError
// =========================================================================

#[test]
fn validation_error_missing_field() {
    let err = ValidationError::MissingField { field: "task" };
    assert_eq!(err.to_string(), "missing required field: task");
}

#[test]
fn validation_error_invalid_hash() {
    let err = ValidationError::InvalidHash {
        expected: "abc".into(),
        actual: "xyz".into(),
    };
    let s = err.to_string();
    assert!(s.contains("hash mismatch"));
    assert!(s.contains("abc"));
    assert!(s.contains("xyz"));
}

#[test]
fn validation_error_empty_backend_id() {
    let err = ValidationError::EmptyBackendId;
    assert_eq!(err.to_string(), "backend.id must not be empty");
}

#[test]
fn validation_error_invalid_outcome() {
    let err = ValidationError::InvalidOutcome {
        reason: "unknown status".into(),
    };
    assert!(err.to_string().contains("invalid outcome"));
}

#[test]
fn validation_error_implements_std_error() {
    let err = ValidationError::EmptyBackendId;
    let _: &dyn StdError = &err;
}

// =========================================================================
// Module: abp-core – ChainError
// =========================================================================

#[test]
fn chain_error_invalid_hash() {
    let err = ChainError::InvalidHash { index: 3 };
    assert!(err.to_string().contains("invalid hash"));
    assert!(err.to_string().contains("3"));
}

#[test]
fn chain_error_empty_chain() {
    let err = ChainError::EmptyChain;
    assert_eq!(err.to_string(), "chain is empty");
}

#[test]
fn chain_error_duplicate_id() {
    let id = uuid::Uuid::nil();
    let err = ChainError::DuplicateId { id };
    assert!(err.to_string().contains("duplicate"));
}

#[test]
fn chain_error_implements_std_error() {
    let err = ChainError::EmptyChain;
    let _: &dyn StdError = &err;
}

// =========================================================================
// Module: abp-core – ErrorCode stability (ABP-X### pattern)
// =========================================================================

#[test]
fn core_error_code_stability_contract() {
    assert_eq!(CoreErrorCode::InvalidContractVersion.code(), "ABP-C001");
    assert_eq!(CoreErrorCode::MalformedWorkOrder.code(), "ABP-C002");
    assert_eq!(CoreErrorCode::MalformedReceipt.code(), "ABP-C003");
    assert_eq!(CoreErrorCode::InvalidHash.code(), "ABP-C004");
    assert_eq!(CoreErrorCode::ContractVersionMismatch.code(), "ABP-C009");
    assert_eq!(CoreErrorCode::InvalidExecutionMode.code(), "ABP-C012");
}

#[test]
fn core_error_code_stability_protocol() {
    assert_eq!(CoreErrorCode::InvalidEnvelope.code(), "ABP-P001");
    assert_eq!(CoreErrorCode::HandshakeFailed.code(), "ABP-P002");
    assert_eq!(CoreErrorCode::ProtocolTimeout.code(), "ABP-P010");
    assert_eq!(CoreErrorCode::UnexpectedFinal.code(), "ABP-P012");
}

#[test]
fn core_error_code_stability_policy() {
    assert_eq!(CoreErrorCode::ToolDenied.code(), "ABP-L001");
    assert_eq!(CoreErrorCode::PathTraversal.code(), "ABP-L011");
}

#[test]
fn core_error_code_stability_runtime() {
    assert_eq!(CoreErrorCode::BackendUnavailable.code(), "ABP-R001");
    assert_eq!(CoreErrorCode::NoBackendRegistered.code(), "ABP-R013");
}

#[test]
fn core_error_code_stability_system() {
    assert_eq!(CoreErrorCode::IoError.code(), "ABP-S001");
    assert_eq!(CoreErrorCode::NotImplemented.code(), "ABP-S011");
}

// =========================================================================
// Module: Cross-crate – abp_error::ErrorCode vs abp_core::ErrorCode coexist
// =========================================================================

#[test]
fn both_error_code_enums_independent() {
    // They are different types but share some conceptual overlap.
    let abp = AbpErrorCode::BackendTimeout;
    let core = CoreErrorCode::BackendTimeout;
    // They display differently (abp-error shows message, core shows ABP-R002)
    assert_ne!(abp.to_string(), core.to_string());
}

// =========================================================================
// Module: Classifier classifies all abp_error codes
// =========================================================================

#[test]
fn classifier_classifies_every_abp_error_code() {
    let c = ErrorClassifier::new();
    for &code in ABP_ALL_CODES {
        let cl = c.classify(&code);
        // Every classification must have a non-empty description
        assert!(
            !cl.recovery.description.is_empty(),
            "empty desc for {:?}",
            code
        );
    }
}

#[test]
fn classifier_all_workspace_errors_are_fatal() {
    let c = ErrorClassifier::new();
    for code in [
        AbpErrorCode::WorkspaceInitFailed,
        AbpErrorCode::WorkspaceStagingFailed,
    ] {
        let cl = c.classify(&code);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
    }
}

#[test]
fn classifier_ir_errors_are_fatal() {
    let c = ErrorClassifier::new();
    for code in [AbpErrorCode::IrLoweringFailed, AbpErrorCode::IrInvalid] {
        let cl = c.classify(&code);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
    }
}
