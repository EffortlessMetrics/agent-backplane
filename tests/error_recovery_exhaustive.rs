#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive error recovery tests covering all error paths and recovery strategies.

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_protocol::ProtocolError;
use abp_runtime::RuntimeError;
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;
use std::io;

// =========================================================================
// Helpers
// =========================================================================

/// Every `ErrorCode` variant, kept in sync with the source.
const ALL_CODES: &[ErrorCode] = &[
    // Protocol
    ErrorCode::ProtocolInvalidEnvelope,
    ErrorCode::ProtocolHandshakeFailed,
    ErrorCode::ProtocolMissingRefId,
    ErrorCode::ProtocolUnexpectedMessage,
    ErrorCode::ProtocolVersionMismatch,
    // Mapping
    ErrorCode::MappingUnsupportedCapability,
    ErrorCode::MappingDialectMismatch,
    ErrorCode::MappingLossyConversion,
    ErrorCode::MappingUnmappableTool,
    // Backend
    ErrorCode::BackendNotFound,
    ErrorCode::BackendUnavailable,
    ErrorCode::BackendTimeout,
    ErrorCode::BackendRateLimited,
    ErrorCode::BackendAuthFailed,
    ErrorCode::BackendModelNotFound,
    ErrorCode::BackendCrashed,
    // Execution
    ErrorCode::ExecutionToolFailed,
    ErrorCode::ExecutionWorkspaceError,
    ErrorCode::ExecutionPermissionDenied,
    // Contract
    ErrorCode::ContractVersionMismatch,
    ErrorCode::ContractSchemaViolation,
    ErrorCode::ContractInvalidReceipt,
    // Capability
    ErrorCode::CapabilityUnsupported,
    ErrorCode::CapabilityEmulationFailed,
    // Policy
    ErrorCode::PolicyDenied,
    ErrorCode::PolicyInvalid,
    // Workspace
    ErrorCode::WorkspaceInitFailed,
    ErrorCode::WorkspaceStagingFailed,
    // IR
    ErrorCode::IrLoweringFailed,
    ErrorCode::IrInvalid,
    // Receipt
    ErrorCode::ReceiptHashMismatch,
    ErrorCode::ReceiptChainBroken,
    // Dialect
    ErrorCode::DialectUnknown,
    ErrorCode::DialectMappingFailed,
    // Config
    ErrorCode::ConfigInvalid,
    // Internal
    ErrorCode::Internal,
];

/// Simulate a retry loop. Returns Ok(()) if the attempt function succeeds
/// within `max_retries`, or the last error otherwise.
fn retry_on_retryable<F>(max_retries: usize, mut attempt: F) -> Result<(), AbpError>
where
    F: FnMut(usize) -> Result<(), AbpError>,
{
    let mut last_err = None;
    for i in 0..=max_retries {
        match attempt(i) {
            Ok(()) => return Ok(()),
            Err(e) if e.is_retryable() && i < max_retries => {
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap())
}

// =========================================================================
// 1. ErrorCode::as_str() — every variant
// =========================================================================

#[test]
fn error_code_as_str_protocol_invalid_envelope() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
}

#[test]
fn error_code_as_str_protocol_handshake_failed() {
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.as_str(),
        "protocol_handshake_failed"
    );
}

#[test]
fn error_code_as_str_protocol_missing_ref_id() {
    assert_eq!(
        ErrorCode::ProtocolMissingRefId.as_str(),
        "protocol_missing_ref_id"
    );
}

#[test]
fn error_code_as_str_protocol_unexpected_message() {
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.as_str(),
        "protocol_unexpected_message"
    );
}

#[test]
fn error_code_as_str_protocol_version_mismatch() {
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.as_str(),
        "protocol_version_mismatch"
    );
}

#[test]
fn error_code_as_str_mapping_unsupported_capability() {
    assert_eq!(
        ErrorCode::MappingUnsupportedCapability.as_str(),
        "mapping_unsupported_capability"
    );
}

#[test]
fn error_code_as_str_mapping_dialect_mismatch() {
    assert_eq!(
        ErrorCode::MappingDialectMismatch.as_str(),
        "mapping_dialect_mismatch"
    );
}

#[test]
fn error_code_as_str_mapping_lossy_conversion() {
    assert_eq!(
        ErrorCode::MappingLossyConversion.as_str(),
        "mapping_lossy_conversion"
    );
}

#[test]
fn error_code_as_str_mapping_unmappable_tool() {
    assert_eq!(
        ErrorCode::MappingUnmappableTool.as_str(),
        "mapping_unmappable_tool"
    );
}

#[test]
fn error_code_as_str_backend_not_found() {
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
}

#[test]
fn error_code_as_str_backend_unavailable() {
    assert_eq!(
        ErrorCode::BackendUnavailable.as_str(),
        "backend_unavailable"
    );
}

#[test]
fn error_code_as_str_backend_timeout() {
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
}

#[test]
fn error_code_as_str_backend_rate_limited() {
    assert_eq!(
        ErrorCode::BackendRateLimited.as_str(),
        "backend_rate_limited"
    );
}

#[test]
fn error_code_as_str_backend_auth_failed() {
    assert_eq!(ErrorCode::BackendAuthFailed.as_str(), "backend_auth_failed");
}

#[test]
fn error_code_as_str_backend_model_not_found() {
    assert_eq!(
        ErrorCode::BackendModelNotFound.as_str(),
        "backend_model_not_found"
    );
}

#[test]
fn error_code_as_str_backend_crashed() {
    assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
}

#[test]
fn error_code_as_str_execution_tool_failed() {
    assert_eq!(
        ErrorCode::ExecutionToolFailed.as_str(),
        "execution_tool_failed"
    );
}

#[test]
fn error_code_as_str_execution_workspace_error() {
    assert_eq!(
        ErrorCode::ExecutionWorkspaceError.as_str(),
        "execution_workspace_error"
    );
}

#[test]
fn error_code_as_str_execution_permission_denied() {
    assert_eq!(
        ErrorCode::ExecutionPermissionDenied.as_str(),
        "execution_permission_denied"
    );
}

#[test]
fn error_code_as_str_contract_version_mismatch() {
    assert_eq!(
        ErrorCode::ContractVersionMismatch.as_str(),
        "contract_version_mismatch"
    );
}

#[test]
fn error_code_as_str_contract_schema_violation() {
    assert_eq!(
        ErrorCode::ContractSchemaViolation.as_str(),
        "contract_schema_violation"
    );
}

#[test]
fn error_code_as_str_contract_invalid_receipt() {
    assert_eq!(
        ErrorCode::ContractInvalidReceipt.as_str(),
        "contract_invalid_receipt"
    );
}

#[test]
fn error_code_as_str_capability_unsupported() {
    assert_eq!(
        ErrorCode::CapabilityUnsupported.as_str(),
        "capability_unsupported"
    );
}

#[test]
fn error_code_as_str_capability_emulation_failed() {
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.as_str(),
        "capability_emulation_failed"
    );
}

#[test]
fn error_code_as_str_policy_denied() {
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
}

#[test]
fn error_code_as_str_policy_invalid() {
    assert_eq!(ErrorCode::PolicyInvalid.as_str(), "policy_invalid");
}

#[test]
fn error_code_as_str_workspace_init_failed() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.as_str(),
        "workspace_init_failed"
    );
}

#[test]
fn error_code_as_str_workspace_staging_failed() {
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.as_str(),
        "workspace_staging_failed"
    );
}

#[test]
fn error_code_as_str_ir_lowering_failed() {
    assert_eq!(ErrorCode::IrLoweringFailed.as_str(), "ir_lowering_failed");
}

#[test]
fn error_code_as_str_ir_invalid() {
    assert_eq!(ErrorCode::IrInvalid.as_str(), "ir_invalid");
}

#[test]
fn error_code_as_str_receipt_hash_mismatch() {
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.as_str(),
        "receipt_hash_mismatch"
    );
}

#[test]
fn error_code_as_str_receipt_chain_broken() {
    assert_eq!(
        ErrorCode::ReceiptChainBroken.as_str(),
        "receipt_chain_broken"
    );
}

#[test]
fn error_code_as_str_dialect_unknown() {
    assert_eq!(ErrorCode::DialectUnknown.as_str(), "dialect_unknown");
}

#[test]
fn error_code_as_str_dialect_mapping_failed() {
    assert_eq!(
        ErrorCode::DialectMappingFailed.as_str(),
        "dialect_mapping_failed"
    );
}

#[test]
fn error_code_as_str_config_invalid() {
    assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
}

#[test]
fn error_code_as_str_internal() {
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
}

// =========================================================================
// 2. ErrorCode::category() — every variant
// =========================================================================

#[test]
fn category_protocol_codes() {
    for code in &[
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ] {
        assert_eq!(code.category(), ErrorCategory::Protocol, "{code:?}");
    }
}

#[test]
fn category_mapping_codes() {
    for code in &[
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ] {
        assert_eq!(code.category(), ErrorCategory::Mapping, "{code:?}");
    }
}

#[test]
fn category_backend_codes() {
    for code in &[
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
    ] {
        assert_eq!(code.category(), ErrorCategory::Backend, "{code:?}");
    }
}

#[test]
fn category_execution_codes() {
    for code in &[
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
    ] {
        assert_eq!(code.category(), ErrorCategory::Execution, "{code:?}");
    }
}

#[test]
fn category_contract_codes() {
    for code in &[
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ] {
        assert_eq!(code.category(), ErrorCategory::Contract, "{code:?}");
    }
}

#[test]
fn category_capability_codes() {
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
fn category_policy_codes() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn category_workspace_codes() {
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
fn category_ir_codes() {
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
}

#[test]
fn category_receipt_codes() {
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
fn category_dialect_codes() {
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(
        ErrorCode::DialectMappingFailed.category(),
        ErrorCategory::Dialect
    );
}

#[test]
fn category_config_code() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn category_internal_code() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

// =========================================================================
// 3. ErrorCode::is_retryable() — exhaustive
// =========================================================================

#[test]
fn retryable_backend_unavailable() {
    assert!(ErrorCode::BackendUnavailable.is_retryable());
}

#[test]
fn retryable_backend_timeout() {
    assert!(ErrorCode::BackendTimeout.is_retryable());
}

#[test]
fn retryable_backend_rate_limited() {
    assert!(ErrorCode::BackendRateLimited.is_retryable());
}

#[test]
fn retryable_backend_crashed() {
    assert!(ErrorCode::BackendCrashed.is_retryable());
}

#[test]
fn non_retryable_codes_exhaustive() {
    let retryable: HashSet<ErrorCode> = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ]
    .into_iter()
    .collect();

    for code in ALL_CODES {
        if !retryable.contains(code) {
            assert!(
                !code.is_retryable(),
                "{code:?} should NOT be retryable but is"
            );
        }
    }
}

// =========================================================================
// 4. ErrorCode::message() — non-empty for every variant
// =========================================================================

#[test]
fn message_non_empty_for_all_codes() {
    for code in ALL_CODES {
        let msg = code.message();
        assert!(!msg.is_empty(), "{code:?} has empty message");
    }
}

#[test]
fn display_uses_message() {
    for code in ALL_CODES {
        assert_eq!(format!("{code}"), code.message(), "{code:?}");
    }
}

// =========================================================================
// 5. ErrorCode serde round-trip
// =========================================================================

#[test]
fn error_code_serde_round_trip_all() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        // serde output must be the snake_case string quoted
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "{code:?}");
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *code);
    }
}

// =========================================================================
// 6. ErrorCategory display
// =========================================================================

#[test]
fn error_category_display() {
    let cases = vec![
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
    for (cat, expected) in cases {
        assert_eq!(format!("{cat}"), expected);
    }
}

// =========================================================================
// 7. AbpError creation and Display
// =========================================================================

#[test]
fn abp_error_new_basic() {
    let err = AbpError::new(ErrorCode::Internal, "boom");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(err.message, "boom");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_display_no_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let display = format!("{err}");
    assert_eq!(display, "[backend_timeout] timed out");
}

#[test]
fn abp_error_display_with_context() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    let display = format!("{err}");
    assert!(display.starts_with("[backend_timeout] timed out"));
    assert!(display.contains("\"backend\""));
    assert!(display.contains("\"openai\""));
}

#[test]
fn abp_error_with_multiple_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30000);
    assert_eq!(err.context.len(), 2);
    assert_eq!(err.context["backend"], json!("openai"));
    assert_eq!(err.context["timeout_ms"], json!(30000));
}

#[test]
fn abp_error_with_source() {
    let io_err = io::Error::new(io::ErrorKind::ConnectionRefused, "refused");
    let err = AbpError::new(ErrorCode::BackendUnavailable, "cannot connect").with_source(io_err);
    assert!(err.source.is_some());
    let source = err.source().unwrap();
    assert!(source.to_string().contains("refused"));
}

#[test]
fn abp_error_category_delegates() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "nope");
    assert_eq!(err.category(), ErrorCategory::Policy);
}

#[test]
fn abp_error_is_retryable_delegates() {
    assert!(AbpError::new(ErrorCode::BackendTimeout, "t").is_retryable());
    assert!(!AbpError::new(ErrorCode::PolicyDenied, "no").is_retryable());
}

#[test]
fn abp_error_debug_format() {
    let err = AbpError::new(ErrorCode::Internal, "debug me");
    let debug = format!("{err:?}");
    assert!(debug.contains("AbpError"));
    assert!(debug.contains("Internal"));
    assert!(debug.contains("debug me"));
}

#[test]
fn abp_error_debug_with_source_shows_source() {
    let inner = io::Error::new(io::ErrorKind::Other, "inner cause");
    let err = AbpError::new(ErrorCode::Internal, "outer").with_source(inner);
    let debug = format!("{err:?}");
    assert!(debug.contains("inner cause"));
}

#[test]
fn abp_error_debug_with_context_shows_context() {
    let err = AbpError::new(ErrorCode::Internal, "ctx test").with_context("key", "value");
    let debug = format!("{err:?}");
    assert!(debug.contains("context"));
    assert!(debug.contains("key"));
}

// =========================================================================
// 8. AbpError::to_info()
// =========================================================================

#[test]
fn to_info_preserves_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendRateLimited, "slow down");
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendRateLimited);
    assert_eq!(info.message, "slow down");
    assert!(info.is_retryable);
}

#[test]
fn to_info_preserves_context() {
    let err = AbpError::new(ErrorCode::Internal, "msg").with_context("k", "v");
    let info = err.to_info();
    assert_eq!(info.details["k"], json!("v"));
}

#[test]
fn to_info_non_retryable() {
    let info = AbpError::new(ErrorCode::PolicyDenied, "denied").to_info();
    assert!(!info.is_retryable);
}

// =========================================================================
// 9. ErrorInfo creation and display
// =========================================================================

#[test]
fn error_info_new() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out after 30s");
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert!(info.is_retryable);
    assert!(info.details.is_empty());
}

#[test]
fn error_info_with_detail() {
    let info =
        ErrorInfo::new(ErrorCode::BackendTimeout, "timeout").with_detail("backend", "openai");
    assert_eq!(info.details["backend"], json!("openai"));
}

#[test]
fn error_info_display() {
    let info = ErrorInfo::new(ErrorCode::Internal, "oops");
    assert_eq!(format!("{info}"), "[internal] oops");
}

#[test]
fn error_info_serde_round_trip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timeout").with_detail("retry_after", 5);
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

// =========================================================================
// 10. From conversions into AbpError
// =========================================================================

#[test]
fn from_io_error() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file gone");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert!(abp.message.contains("file gone"));
    assert!(abp.source.is_some());
}

#[test]
fn from_serde_json_error() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{{bad}}").unwrap_err();
    let abp: AbpError = serde_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(abp.source.is_some());
}

// =========================================================================
// 11. AbpErrorDto round-trip
// =========================================================================

#[test]
fn abp_error_dto_from_abp_error() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "blocked").with_context("rule", "no_write");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::PolicyDenied);
    assert_eq!(dto.message, "blocked");
    assert_eq!(dto.context["rule"], json!("no_write"));
    assert!(dto.source_message.is_none());
}

#[test]
fn abp_error_dto_captures_source_message() {
    let inner = io::Error::new(io::ErrorKind::Other, "disk full");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init fail").with_source(inner);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message, Some("disk full".to_string()));
}

#[test]
fn abp_error_dto_back_to_abp_error() {
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad config".into(),
        context: BTreeMap::new(),
        source_message: Some("parse error".into()),
        location: None,
        cause_chain: Vec::new(),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.message, "bad config");
    // source is lost in DTO round-trip (only message preserved)
    assert!(err.source.is_none());
}

#[test]
fn abp_error_dto_serde_round_trip() {
    let err = AbpError::new(ErrorCode::Internal, "test").with_context("detail", 42);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn abp_error_dto_skips_none_source_in_json() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "x".into(),
        context: BTreeMap::new(),
        source_message: None,
        location: None,
        cause_chain: Vec::new(),
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

// =========================================================================
// 12. ProtocolError variants and error_code()
// =========================================================================

#[test]
fn protocol_error_json_variant() {
    let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let pe = ProtocolError::Json(serde_err);
    assert!(pe.error_code().is_none());
    assert!(format!("{pe}").contains("invalid JSON"));
}

#[test]
fn protocol_error_io_variant() {
    let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe");
    let pe = ProtocolError::Io(io_err);
    assert!(pe.error_code().is_none());
    assert!(format!("{pe}").contains("I/O error"));
}

#[test]
fn protocol_error_violation_variant() {
    let pe = ProtocolError::Violation("missing hello".into());
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
    assert!(format!("{pe}").contains("protocol violation"));
}

#[test]
fn protocol_error_unexpected_message_variant() {
    let pe = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
    let display = format!("{pe}");
    assert!(display.contains("hello"));
    assert!(display.contains("run"));
}

#[test]
fn protocol_error_abp_variant() {
    let abp = AbpError::new(ErrorCode::ProtocolHandshakeFailed, "handshake fail");
    let pe = ProtocolError::Abp(abp);
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolHandshakeFailed));
}

#[test]
fn protocol_error_from_abp_error() {
    let abp = AbpError::new(ErrorCode::Internal, "converted");
    let pe: ProtocolError = abp.into();
    assert!(matches!(pe, ProtocolError::Abp(_)));
    assert_eq!(pe.error_code(), Some(ErrorCode::Internal));
}

// =========================================================================
// 13. RuntimeError variants
// =========================================================================

#[test]
fn runtime_error_unknown_backend() {
    let re = RuntimeError::UnknownBackend {
        name: "nonexistent".into(),
    };
    assert_eq!(re.error_code(), ErrorCode::BackendNotFound);
    assert!(!re.is_retryable());
    assert!(format!("{re}").contains("nonexistent"));
}

#[test]
fn runtime_error_workspace_failed() {
    let re = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(re.error_code(), ErrorCode::WorkspaceInitFailed);
    assert!(re.is_retryable()); // transient
    assert!(format!("{re}").contains("workspace preparation failed"));
}

#[test]
fn runtime_error_policy_failed() {
    let re = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(re.error_code(), ErrorCode::PolicyInvalid);
    assert!(!re.is_retryable()); // permanent
}

#[test]
fn runtime_error_backend_failed() {
    let re = RuntimeError::BackendFailed(anyhow::anyhow!("crashed"));
    assert_eq!(re.error_code(), ErrorCode::BackendCrashed);
    assert!(re.is_retryable()); // transient
}

#[test]
fn runtime_error_capability_check_failed() {
    let re = RuntimeError::CapabilityCheckFailed("no streaming".into());
    assert_eq!(re.error_code(), ErrorCode::CapabilityUnsupported);
    assert!(!re.is_retryable());
    assert!(format!("{re}").contains("no streaming"));
}

#[test]
fn runtime_error_classified_retryable() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "slow");
    let re = RuntimeError::Classified(abp);
    assert_eq!(re.error_code(), ErrorCode::BackendTimeout);
    assert!(re.is_retryable());
}

#[test]
fn runtime_error_classified_non_retryable() {
    let abp = AbpError::new(ErrorCode::PolicyDenied, "nope");
    let re = RuntimeError::Classified(abp);
    assert_eq!(re.error_code(), ErrorCode::PolicyDenied);
    assert!(!re.is_retryable());
}

#[test]
fn runtime_error_no_projection_match() {
    let re = RuntimeError::NoProjectionMatch {
        reason: "no suitable backend".into(),
    };
    assert_eq!(re.error_code(), ErrorCode::BackendNotFound);
    assert!(!re.is_retryable());
    assert!(format!("{re}").contains("projection failed"));
}

#[test]
fn runtime_error_into_abp_error_classified() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("ms", 5000);
    let re = RuntimeError::Classified(abp);
    let converted = re.into_abp_error();
    assert_eq!(converted.code, ErrorCode::BackendTimeout);
    assert_eq!(converted.context["ms"], json!(5000));
}

#[test]
fn runtime_error_into_abp_error_non_classified() {
    let re = RuntimeError::UnknownBackend { name: "xyz".into() };
    let converted = re.into_abp_error();
    assert_eq!(converted.code, ErrorCode::BackendNotFound);
    assert!(converted.message.contains("xyz"));
}

// =========================================================================
// 14. Error recovery strategies (retry logic)
// =========================================================================

#[test]
fn retry_succeeds_on_second_attempt() {
    let result = retry_on_retryable(3, |attempt| {
        if attempt < 1 {
            Err(AbpError::new(ErrorCode::BackendTimeout, "timeout"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_ok());
}

#[test]
fn retry_exhausts_all_attempts_then_fails() {
    let result = retry_on_retryable(2, |_| {
        Err(AbpError::new(ErrorCode::BackendUnavailable, "down"))
    });
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, ErrorCode::BackendUnavailable);
}

#[test]
fn retry_stops_immediately_for_non_retryable() {
    let mut attempts = 0;
    let result = retry_on_retryable(5, |_| {
        attempts += 1;
        Err(AbpError::new(ErrorCode::PolicyDenied, "permanent"))
    });
    assert!(result.is_err());
    assert_eq!(attempts, 1, "should not retry non-retryable errors");
}

#[test]
fn retry_strategy_rate_limited() {
    let mut attempts = 0;
    let result = retry_on_retryable(3, |attempt| {
        attempts += 1;
        if attempt < 2 {
            Err(AbpError::new(ErrorCode::BackendRateLimited, "429"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_ok());
    assert_eq!(attempts, 3);
}

#[test]
fn retry_strategy_backend_crashed_recovery() {
    let result = retry_on_retryable(1, |attempt| {
        if attempt == 0 {
            Err(AbpError::new(ErrorCode::BackendCrashed, "segfault"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_ok());
}

#[test]
fn retry_non_retryable_auth_fails_immediately() {
    let mut calls = 0;
    let result = retry_on_retryable(10, |_| {
        calls += 1;
        Err(AbpError::new(ErrorCode::BackendAuthFailed, "bad key"))
    });
    assert!(result.is_err());
    assert_eq!(calls, 1);
}

#[test]
fn retry_non_retryable_contract_violation() {
    let mut calls = 0;
    let result = retry_on_retryable(10, |_| {
        calls += 1;
        Err(AbpError::new(ErrorCode::ContractSchemaViolation, "bad"))
    });
    assert!(result.is_err());
    assert_eq!(calls, 1);
}

// =========================================================================
// 15. Error propagation through pipeline
// =========================================================================

fn pipeline_stage_one() -> Result<String, AbpError> {
    Err(AbpError::new(ErrorCode::BackendTimeout, "stage 1 timeout"))
}

fn pipeline_stage_two(input: &str) -> Result<String, AbpError> {
    Ok(format!("processed: {input}"))
}

#[test]
fn pipeline_error_short_circuits() {
    let result = pipeline_stage_one().and_then(|v| pipeline_stage_two(&v));
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert!(err.message.contains("stage 1"));
}

fn protocol_to_runtime(pe: ProtocolError) -> RuntimeError {
    match pe {
        ProtocolError::Abp(e) => RuntimeError::Classified(e),
        other => {
            let code = other
                .error_code()
                .unwrap_or(ErrorCode::ProtocolInvalidEnvelope);
            RuntimeError::Classified(AbpError::new(code, other.to_string()))
        }
    }
}

#[test]
fn error_propagation_protocol_to_runtime() {
    let pe = ProtocolError::Violation("bad envelope".into());
    let re = protocol_to_runtime(pe);
    assert_eq!(re.error_code(), ErrorCode::ProtocolInvalidEnvelope);
}

#[test]
fn error_propagation_abp_through_protocol_to_runtime() {
    let abp = AbpError::new(ErrorCode::BackendAuthFailed, "401");
    let pe: ProtocolError = abp.into();
    let re = protocol_to_runtime(pe);
    assert_eq!(re.error_code(), ErrorCode::BackendAuthFailed);
}

// =========================================================================
// 16. Error context enrichment
// =========================================================================

#[test]
fn context_with_string_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("key", "val");
    assert_eq!(err.context["key"], json!("val"));
}

#[test]
fn context_with_numeric_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("retries", 3);
    assert_eq!(err.context["retries"], json!(3));
}

#[test]
fn context_with_boolean_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("fatal", true);
    assert_eq!(err.context["fatal"], json!(true));
}

#[test]
fn context_with_nested_json() {
    let nested = json!({"a": 1, "b": [2, 3]});
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("data", nested.clone());
    assert_eq!(err.context["data"], nested);
}

#[test]
fn context_deterministic_ordering() {
    let err = AbpError::new(ErrorCode::Internal, "x")
        .with_context("z_key", "last")
        .with_context("a_key", "first")
        .with_context("m_key", "middle");
    let keys: Vec<&String> = err.context.keys().collect();
    assert_eq!(keys, vec!["a_key", "m_key", "z_key"]);
}

#[test]
fn context_appears_in_display() {
    let err = AbpError::new(ErrorCode::Internal, "msg").with_context("trace_id", "abc123");
    let display = format!("{err}");
    assert!(display.contains("trace_id"));
    assert!(display.contains("abc123"));
}

// =========================================================================
// 17. Error chain building (source chain via std::error::Error)
// =========================================================================

#[test]
fn error_chain_single_source() {
    let root = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let err = AbpError::new(ErrorCode::ExecutionPermissionDenied, "cannot write").with_source(root);
    let src = err.source().unwrap();
    assert!(src.to_string().contains("access denied"));
}

#[test]
fn error_chain_no_source_returns_none() {
    let err = AbpError::new(ErrorCode::Internal, "standalone");
    assert!(err.source().is_none());
}

#[test]
fn error_chain_source_preserved_after_context() {
    let root = io::Error::new(io::ErrorKind::Other, "root cause");
    let err = AbpError::new(ErrorCode::Internal, "wrapper")
        .with_source(root)
        .with_context("extra", "info");
    assert!(err.source().is_some());
    assert!(err.source().unwrap().to_string().contains("root cause"));
}

// =========================================================================
// 18. as_str() uniqueness
// =========================================================================

#[test]
fn all_as_str_values_are_unique() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str: {s}");
    }
}

#[test]
fn all_as_str_are_snake_case() {
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{code:?} as_str is not snake_case: {s}"
        );
    }
}

// =========================================================================
// 19. all_codes coverage sanity — we have exactly 36 codes
// =========================================================================

#[test]
fn all_codes_count() {
    assert_eq!(
        ALL_CODES.len(),
        36,
        "update ALL_CODES if variants added/removed"
    );
}

// =========================================================================
// 20. Mixed error-category classification tests
// =========================================================================

#[test]
fn classify_all_codes_have_known_category() {
    let known: HashSet<ErrorCategory> = [
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
    ]
    .into_iter()
    .collect();

    for code in ALL_CODES {
        assert!(
            known.contains(&code.category()),
            "{code:?} has unknown category"
        );
    }
}

#[test]
fn authentication_errors_are_backend_category() {
    assert_eq!(
        ErrorCode::BackendAuthFailed.category(),
        ErrorCategory::Backend
    );
}

#[test]
fn validation_style_errors() {
    assert_eq!(
        ErrorCode::ContractSchemaViolation.category(),
        ErrorCategory::Contract
    );
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn runtime_execution_errors() {
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

// =========================================================================
// 21. ProtocolError from conversions
// =========================================================================

#[test]
fn protocol_error_from_io() {
    let io_err = io::Error::new(io::ErrorKind::UnexpectedEof, "eof");
    let pe: ProtocolError = io_err.into();
    assert!(matches!(pe, ProtocolError::Io(_)));
}

#[test]
fn protocol_error_from_serde() {
    let serde_err = serde_json::from_str::<serde_json::Value>("nope").unwrap_err();
    let pe: ProtocolError = serde_err.into();
    assert!(matches!(pe, ProtocolError::Json(_)));
}

// =========================================================================
// 22. RuntimeError retryable vs non-retryable boundaries
// =========================================================================

#[test]
fn runtime_error_retryable_matrix() {
    let cases: Vec<(RuntimeError, bool)> = vec![
        (RuntimeError::UnknownBackend { name: "x".into() }, false),
        (RuntimeError::WorkspaceFailed(anyhow::anyhow!("tmp")), true),
        (RuntimeError::PolicyFailed(anyhow::anyhow!("bad")), false),
        (RuntimeError::BackendFailed(anyhow::anyhow!("crash")), true),
        (RuntimeError::CapabilityCheckFailed("missing".into()), false),
        (
            RuntimeError::NoProjectionMatch {
                reason: "none".into(),
            },
            false,
        ),
    ];
    for (err, expected) in cases {
        assert_eq!(
            err.is_retryable(),
            expected,
            "RuntimeError::{:?} retryable mismatch",
            err.error_code()
        );
    }
}

// =========================================================================
// 23. ErrorInfo retryable inference
// =========================================================================

#[test]
fn error_info_retryable_inferred_from_code() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "t");
    assert!(info.is_retryable);

    let info2 = ErrorInfo::new(ErrorCode::PolicyDenied, "d");
    assert!(!info2.is_retryable);
}

// =========================================================================
// 24. Display consistency across types
// =========================================================================

#[test]
fn abp_error_and_error_info_display_bracket_format() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, "msg");
        let info = ErrorInfo::new(*code, "msg");
        let err_display = format!("{err}");
        let info_display = format!("{info}");
        // Both use [code_str] prefix
        let prefix = format!("[{}]", code.as_str());
        assert!(
            err_display.starts_with(&prefix),
            "{code:?} AbpError display"
        );
        assert!(
            info_display.starts_with(&prefix),
            "{code:?} ErrorInfo display"
        );
    }
}

// =========================================================================
// 25. Error code equality and hashing
// =========================================================================

#[test]
fn error_code_equality() {
    assert_eq!(ErrorCode::Internal, ErrorCode::Internal);
    assert_ne!(ErrorCode::Internal, ErrorCode::BackendTimeout);
}

#[test]
fn error_code_hashable() {
    let mut set = HashSet::new();
    set.insert(ErrorCode::Internal);
    set.insert(ErrorCode::Internal);
    assert_eq!(set.len(), 1);
    set.insert(ErrorCode::BackendTimeout);
    assert_eq!(set.len(), 2);
}

#[test]
fn error_category_equality() {
    assert_eq!(ErrorCategory::Backend, ErrorCategory::Backend);
    assert_ne!(ErrorCategory::Backend, ErrorCategory::Protocol);
}

// =========================================================================
// 26. Edge cases
// =========================================================================

#[test]
fn abp_error_empty_message() {
    let err = AbpError::new(ErrorCode::Internal, "");
    assert_eq!(err.message, "");
    assert_eq!(format!("{err}"), "[internal] ");
}

#[test]
fn abp_error_unicode_message() {
    let err = AbpError::new(ErrorCode::Internal, "错误消息 — résumé 🚀");
    assert!(format!("{err}").contains("错误消息"));
}

#[test]
fn abp_error_large_context() {
    let mut err = AbpError::new(ErrorCode::Internal, "big context");
    for i in 0..50 {
        err = err.with_context(format!("key_{i}"), i);
    }
    assert_eq!(err.context.len(), 50);
}

#[test]
fn error_info_multiple_details() {
    let info = ErrorInfo::new(ErrorCode::Internal, "x")
        .with_detail("a", 1)
        .with_detail("b", "two")
        .with_detail("c", true);
    assert_eq!(info.details.len(), 3);
}

// =========================================================================
// 27. RuntimeError error_code mapping completeness
// =========================================================================

#[test]
fn runtime_error_code_mapping_all_variants() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "a".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("b")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("c")),
        RuntimeError::BackendFailed(anyhow::anyhow!("d")),
        RuntimeError::CapabilityCheckFailed("e".into()),
        RuntimeError::Classified(AbpError::new(ErrorCode::DialectUnknown, "f")),
        RuntimeError::NoProjectionMatch { reason: "g".into() },
    ];
    let expected = [
        ErrorCode::BackendNotFound,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::PolicyInvalid,
        ErrorCode::BackendCrashed,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::DialectUnknown,
        ErrorCode::BackendNotFound,
    ];
    for (re, exp) in variants.into_iter().zip(expected.iter()) {
        assert_eq!(re.error_code(), *exp);
    }
}

// =========================================================================
// 28. Error category serde
// =========================================================================

#[test]
fn error_category_serde_round_trip() {
    let cats = vec![
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
    for cat in &cats {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back);
    }
}

// =========================================================================
// 29. Complex recovery scenarios
// =========================================================================

#[test]
fn recovery_scenario_fallback_on_non_retryable() {
    // Simulate: primary fails permanently, fallback succeeds
    let primary = Err::<(), _>(AbpError::new(ErrorCode::BackendNotFound, "no primary"));
    let fallback = Ok::<(), AbpError>(());

    let result = primary.or(fallback);
    assert!(result.is_ok());
}

#[test]
fn recovery_scenario_classify_and_decide() {
    let errors: Vec<AbpError> = vec![
        AbpError::new(ErrorCode::BackendTimeout, "t"),
        AbpError::new(ErrorCode::PolicyDenied, "p"),
        AbpError::new(ErrorCode::BackendRateLimited, "r"),
    ];
    let retryable: Vec<_> = errors.iter().filter(|e| e.is_retryable()).collect();
    assert_eq!(retryable.len(), 2);
    let non_retryable: Vec<_> = errors.iter().filter(|e| !e.is_retryable()).collect();
    assert_eq!(non_retryable.len(), 1);
    assert_eq!(non_retryable[0].code, ErrorCode::PolicyDenied);
}

#[test]
fn recovery_scenario_context_enrichment_on_retry() {
    let mut attempt = 0;
    let result = retry_on_retryable(2, |i| {
        attempt = i;
        if i < 2 {
            Err(AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("attempt", i))
        } else {
            Ok(())
        }
    });
    assert!(result.is_ok());
    assert_eq!(attempt, 2);
}

#[test]
fn recovery_scenario_max_retries_preserves_last_error() {
    let result = retry_on_retryable(3, |attempt| {
        Err(AbpError::new(
            ErrorCode::BackendUnavailable,
            format!("attempt {attempt}"),
        ))
    });
    let err = result.unwrap_err();
    assert!(err.message.contains("attempt 3"));
}

// =========================================================================
// 30. std::error::Error trait compliance
// =========================================================================

#[test]
fn abp_error_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<AbpError>();
}

#[test]
fn abp_error_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<AbpError>();
}

#[test]
fn protocol_error_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<ProtocolError>();
}

#[test]
fn runtime_error_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<RuntimeError>();
}
