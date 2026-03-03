//! Exhaustive error taxonomy tests covering every `ErrorCode` variant.

use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};

/// All ErrorCode variants in definition order.
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

// =========================================================================
// 1. as_str stability (one test per variant)
// =========================================================================

#[test]
fn as_str_protocol_invalid_envelope() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
}

#[test]
fn as_str_protocol_handshake_failed() {
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.as_str(),
        "protocol_handshake_failed"
    );
}

#[test]
fn as_str_protocol_missing_ref_id() {
    assert_eq!(
        ErrorCode::ProtocolMissingRefId.as_str(),
        "protocol_missing_ref_id"
    );
}

#[test]
fn as_str_protocol_unexpected_message() {
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.as_str(),
        "protocol_unexpected_message"
    );
}

#[test]
fn as_str_protocol_version_mismatch() {
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.as_str(),
        "protocol_version_mismatch"
    );
}

#[test]
fn as_str_mapping_unsupported_capability() {
    assert_eq!(
        ErrorCode::MappingUnsupportedCapability.as_str(),
        "mapping_unsupported_capability"
    );
}

#[test]
fn as_str_mapping_dialect_mismatch() {
    assert_eq!(
        ErrorCode::MappingDialectMismatch.as_str(),
        "mapping_dialect_mismatch"
    );
}

#[test]
fn as_str_mapping_lossy_conversion() {
    assert_eq!(
        ErrorCode::MappingLossyConversion.as_str(),
        "mapping_lossy_conversion"
    );
}

#[test]
fn as_str_mapping_unmappable_tool() {
    assert_eq!(
        ErrorCode::MappingUnmappableTool.as_str(),
        "mapping_unmappable_tool"
    );
}

#[test]
fn as_str_backend_not_found() {
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
}

#[test]
fn as_str_backend_unavailable() {
    assert_eq!(
        ErrorCode::BackendUnavailable.as_str(),
        "backend_unavailable"
    );
}

#[test]
fn as_str_backend_timeout() {
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
}

#[test]
fn as_str_backend_rate_limited() {
    assert_eq!(
        ErrorCode::BackendRateLimited.as_str(),
        "backend_rate_limited"
    );
}

#[test]
fn as_str_backend_auth_failed() {
    assert_eq!(ErrorCode::BackendAuthFailed.as_str(), "backend_auth_failed");
}

#[test]
fn as_str_backend_model_not_found() {
    assert_eq!(
        ErrorCode::BackendModelNotFound.as_str(),
        "backend_model_not_found"
    );
}

#[test]
fn as_str_backend_crashed() {
    assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
}

#[test]
fn as_str_execution_tool_failed() {
    assert_eq!(
        ErrorCode::ExecutionToolFailed.as_str(),
        "execution_tool_failed"
    );
}

#[test]
fn as_str_execution_workspace_error() {
    assert_eq!(
        ErrorCode::ExecutionWorkspaceError.as_str(),
        "execution_workspace_error"
    );
}

#[test]
fn as_str_execution_permission_denied() {
    assert_eq!(
        ErrorCode::ExecutionPermissionDenied.as_str(),
        "execution_permission_denied"
    );
}

#[test]
fn as_str_contract_version_mismatch() {
    assert_eq!(
        ErrorCode::ContractVersionMismatch.as_str(),
        "contract_version_mismatch"
    );
}

#[test]
fn as_str_contract_schema_violation() {
    assert_eq!(
        ErrorCode::ContractSchemaViolation.as_str(),
        "contract_schema_violation"
    );
}

#[test]
fn as_str_contract_invalid_receipt() {
    assert_eq!(
        ErrorCode::ContractInvalidReceipt.as_str(),
        "contract_invalid_receipt"
    );
}

#[test]
fn as_str_capability_unsupported() {
    assert_eq!(
        ErrorCode::CapabilityUnsupported.as_str(),
        "capability_unsupported"
    );
}

#[test]
fn as_str_capability_emulation_failed() {
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.as_str(),
        "capability_emulation_failed"
    );
}

#[test]
fn as_str_policy_denied() {
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
}

#[test]
fn as_str_policy_invalid() {
    assert_eq!(ErrorCode::PolicyInvalid.as_str(), "policy_invalid");
}

#[test]
fn as_str_workspace_init_failed() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.as_str(),
        "workspace_init_failed"
    );
}

#[test]
fn as_str_workspace_staging_failed() {
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.as_str(),
        "workspace_staging_failed"
    );
}

#[test]
fn as_str_ir_lowering_failed() {
    assert_eq!(ErrorCode::IrLoweringFailed.as_str(), "ir_lowering_failed");
}

#[test]
fn as_str_ir_invalid() {
    assert_eq!(ErrorCode::IrInvalid.as_str(), "ir_invalid");
}

#[test]
fn as_str_receipt_hash_mismatch() {
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.as_str(),
        "receipt_hash_mismatch"
    );
}

#[test]
fn as_str_receipt_chain_broken() {
    assert_eq!(
        ErrorCode::ReceiptChainBroken.as_str(),
        "receipt_chain_broken"
    );
}

#[test]
fn as_str_dialect_unknown() {
    assert_eq!(ErrorCode::DialectUnknown.as_str(), "dialect_unknown");
}

#[test]
fn as_str_dialect_mapping_failed() {
    assert_eq!(
        ErrorCode::DialectMappingFailed.as_str(),
        "dialect_mapping_failed"
    );
}

#[test]
fn as_str_config_invalid() {
    assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
}

#[test]
fn as_str_internal() {
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
}

// =========================================================================
// 2. message() coverage — every variant has a non-empty human-readable msg
// =========================================================================

#[test]
fn message_protocol_invalid_envelope() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.message(),
        "envelope failed to parse or has invalid fields"
    );
}

#[test]
fn message_protocol_handshake_failed() {
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.message(),
        "sidecar handshake failed"
    );
}

#[test]
fn message_protocol_missing_ref_id() {
    assert_eq!(
        ErrorCode::ProtocolMissingRefId.message(),
        "ref_id field is missing from the envelope"
    );
}

#[test]
fn message_protocol_unexpected_message() {
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.message(),
        "message arrived in unexpected order"
    );
}

#[test]
fn message_protocol_version_mismatch() {
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.message(),
        "protocol version mismatch between host and sidecar"
    );
}

#[test]
fn message_mapping_unsupported_capability() {
    assert_eq!(
        ErrorCode::MappingUnsupportedCapability.message(),
        "required capability is not supported by the target dialect"
    );
}

#[test]
fn message_mapping_dialect_mismatch() {
    assert_eq!(
        ErrorCode::MappingDialectMismatch.message(),
        "source and target dialects are incompatible"
    );
}

#[test]
fn message_mapping_lossy_conversion() {
    assert_eq!(
        ErrorCode::MappingLossyConversion.message(),
        "translation succeeded but information was lost"
    );
}

#[test]
fn message_mapping_unmappable_tool() {
    assert_eq!(
        ErrorCode::MappingUnmappableTool.message(),
        "tool call cannot be represented in the target dialect"
    );
}

#[test]
fn message_backend_not_found() {
    assert_eq!(
        ErrorCode::BackendNotFound.message(),
        "requested backend does not exist"
    );
}

#[test]
fn message_backend_unavailable() {
    assert_eq!(
        ErrorCode::BackendUnavailable.message(),
        "backend is temporarily unavailable"
    );
}

#[test]
fn message_backend_timeout() {
    assert_eq!(ErrorCode::BackendTimeout.message(), "backend timed out");
}

#[test]
fn message_backend_rate_limited() {
    assert_eq!(
        ErrorCode::BackendRateLimited.message(),
        "backend rejected the request due to rate limiting"
    );
}

#[test]
fn message_backend_auth_failed() {
    assert_eq!(
        ErrorCode::BackendAuthFailed.message(),
        "authentication with the backend failed"
    );
}

#[test]
fn message_backend_model_not_found() {
    assert_eq!(
        ErrorCode::BackendModelNotFound.message(),
        "requested model was not found on the backend"
    );
}

#[test]
fn message_backend_crashed() {
    assert_eq!(
        ErrorCode::BackendCrashed.message(),
        "backend process exited unexpectedly"
    );
}

#[test]
fn message_all_non_empty() {
    for code in ALL_CODES {
        let msg = code.message();
        assert!(!msg.is_empty(), "{:?} has an empty message", code);
    }
}

#[test]
fn display_uses_message_not_as_str() {
    for code in ALL_CODES {
        let display = format!("{code}");
        assert_eq!(
            display,
            code.message(),
            "{:?} Display should equal message()",
            code
        );
        assert_ne!(
            display,
            code.as_str(),
            "{:?} Display must differ from as_str",
            code
        );
    }
}

// =========================================================================
// 3. Category mapping — every variant maps to the correct category
// =========================================================================

#[test]
fn category_protocol_variants() {
    let protocol = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in &protocol {
        assert_eq!(code.category(), ErrorCategory::Protocol, "{code:?}");
    }
}

#[test]
fn category_mapping_variants() {
    let mapping = [
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ];
    for code in &mapping {
        assert_eq!(code.category(), ErrorCategory::Mapping, "{code:?}");
    }
}

#[test]
fn category_backend_variants() {
    let backend = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
    ];
    for code in &backend {
        assert_eq!(code.category(), ErrorCategory::Backend, "{code:?}");
    }
}

#[test]
fn category_execution_variants() {
    let execution = [
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
    ];
    for code in &execution {
        assert_eq!(code.category(), ErrorCategory::Execution, "{code:?}");
    }
}

#[test]
fn category_contract_variants() {
    let contract = [
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ];
    for code in &contract {
        assert_eq!(code.category(), ErrorCategory::Contract, "{code:?}");
    }
}

#[test]
fn category_capability_variants() {
    let cap = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in &cap {
        assert_eq!(code.category(), ErrorCategory::Capability, "{code:?}");
    }
}

#[test]
fn category_policy_variants() {
    let policy = [ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid];
    for code in &policy {
        assert_eq!(code.category(), ErrorCategory::Policy, "{code:?}");
    }
}

#[test]
fn category_workspace_variants() {
    let ws = [
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
    ];
    for code in &ws {
        assert_eq!(code.category(), ErrorCategory::Workspace, "{code:?}");
    }
}

#[test]
fn category_ir_receipt_dialect_config_internal() {
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(
        ErrorCode::ReceiptChainBroken.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(
        ErrorCode::DialectMappingFailed.category(),
        ErrorCategory::Dialect
    );
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn category_every_variant_covered() {
    // Ensures ALL_CODES covers every variant by checking the total count
    // matches the number of as_str() unique strings.
    let unique: std::collections::HashSet<&str> = ALL_CODES.iter().map(|c| c.as_str()).collect();
    assert_eq!(unique.len(), ALL_CODES.len(), "ALL_CODES has duplicates");
    assert_eq!(ALL_CODES.len(), 36, "expected 36 ErrorCode variants");
}

// =========================================================================
// 4. Serde roundtrip — every variant survives JSON serialization
// =========================================================================

#[test]
fn serde_roundtrip_all_codes() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back, "roundtrip failed for {code:?}");
    }
}

#[test]
fn serde_serializes_to_snake_case() {
    let json = serde_json::to_string(&ErrorCode::BackendTimeout).unwrap();
    assert_eq!(json, r#""backend_timeout""#);
}

#[test]
fn serde_deserializes_from_snake_case() {
    let code: ErrorCode = serde_json::from_str(r#""backend_timeout""#).unwrap();
    assert_eq!(code, ErrorCode::BackendTimeout);
}

#[test]
fn serde_rejects_unknown_variant() {
    let result = serde_json::from_str::<ErrorCode>(r#""nonexistent_code""#);
    assert!(result.is_err());
}

#[test]
fn serde_as_str_matches_json() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "serde JSON != as_str for {code:?}");
    }
}

#[test]
fn serde_roundtrip_error_info() {
    let info =
        ErrorInfo::new(ErrorCode::PolicyDenied, "denied by rule X").with_detail("rule", "no-write");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn serde_error_info_code_field() {
    let info = ErrorInfo::new(ErrorCode::BackendNotFound, "not found");
    let val: serde_json::Value = serde_json::to_value(&info).unwrap();
    assert_eq!(val["code"], "backend_not_found");
}

#[test]
fn serde_error_info_details_preserved() {
    let info = ErrorInfo::new(ErrorCode::Internal, "boom")
        .with_detail("key1", "value1")
        .with_detail("key2", 42);
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.details["key1"], serde_json::json!("value1"));
    assert_eq!(back.details["key2"], serde_json::json!(42));
}

#[test]
fn serde_category_roundtrip() {
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
    for cat in &cats {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back, "category roundtrip failed for {cat:?}");
    }
}

#[test]
fn serde_category_snake_case() {
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Workspace).unwrap(),
        r#""workspace""#
    );
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Internal).unwrap(),
        r#""internal""#
    );
}

// =========================================================================
// 5. ErrorInfo construction — every code, details, retryability
// =========================================================================

#[test]
fn error_info_retryable_codes() {
    let retryable = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ];
    for code in &retryable {
        let info = ErrorInfo::new(*code, "test");
        assert!(info.is_retryable, "{code:?} should be retryable");
    }
}

#[test]
fn error_info_non_retryable_codes() {
    let non_retryable = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ConfigInvalid,
    ];
    for code in &non_retryable {
        let info = ErrorInfo::new(*code, "test");
        assert!(!info.is_retryable, "{code:?} should NOT be retryable");
    }
}

#[test]
fn error_info_message_preserved() {
    let info = ErrorInfo::new(ErrorCode::Internal, "custom message here");
    assert_eq!(info.message, "custom message here");
}

#[test]
fn error_info_code_preserved() {
    let info = ErrorInfo::new(ErrorCode::DialectUnknown, "test");
    assert_eq!(info.code, ErrorCode::DialectUnknown);
}

#[test]
fn error_info_empty_details_by_default() {
    let info = ErrorInfo::new(ErrorCode::Internal, "test");
    assert!(info.details.is_empty());
}

#[test]
fn error_info_with_detail_chaining() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "slow")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 30_000)
        .with_detail("retries", 3);
    assert_eq!(info.details.len(), 3);
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
    assert_eq!(info.details["timeout_ms"], serde_json::json!(30_000));
    assert_eq!(info.details["retries"], serde_json::json!(3));
}

#[test]
fn error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "write denied");
    let display = format!("{info}");
    assert_eq!(display, "[policy_denied] write denied");
}

#[test]
fn error_info_display_uses_as_str() {
    for code in ALL_CODES {
        let info = ErrorInfo::new(*code, "msg");
        let display = format!("{info}");
        assert!(
            display.starts_with(&format!("[{}]", code.as_str())),
            "{code:?}"
        );
    }
}

#[test]
fn error_info_with_complex_detail() {
    let info = ErrorInfo::new(ErrorCode::ExecutionToolFailed, "fail")
        .with_detail("nested", serde_json::json!({"a": 1, "b": [2, 3]}));
    assert_eq!(info.details["nested"]["a"], 1);
    assert_eq!(info.details["nested"]["b"][0], 2);
}

#[test]
fn error_info_for_every_code() {
    for code in ALL_CODES {
        let info = ErrorInfo::new(*code, format!("test for {:?}", code));
        assert_eq!(info.code, *code);
        assert_eq!(info.is_retryable, code.is_retryable());
    }
}

// =========================================================================
// 6. AbpError builder — construction, context, source chaining
// =========================================================================

#[test]
fn abp_error_basic_construction() {
    let err = AbpError::new(ErrorCode::Internal, "something broke");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(err.message, "something broke");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_with_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    assert_eq!(err.context.len(), 2);
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_with_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(io_err);
    assert!(err.source.is_some());
    let src = std::error::Error::source(&err).unwrap();
    assert!(src.to_string().contains("file missing"));
}

#[test]
fn abp_error_category_shorthand() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert_eq!(err.category(), ErrorCategory::Policy);
}

#[test]
fn abp_error_is_retryable_shorthand() {
    let err_yes = AbpError::new(ErrorCode::BackendTimeout, "slow");
    assert!(err_yes.is_retryable());
    let err_no = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert!(!err_no.is_retryable());
}

#[test]
fn abp_error_display_format() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let display = format!("{err}");
    assert_eq!(display, "[internal] oops");
}

#[test]
fn abp_error_display_with_context() {
    let err =
        AbpError::new(ErrorCode::BackendNotFound, "gone").with_context("name", "test-backend");
    let display = format!("{err}");
    assert!(display.contains("[backend_not_found]"));
    assert!(display.contains("gone"));
    assert!(display.contains("test-backend"));
}

#[test]
fn abp_error_debug_format() {
    let err =
        AbpError::new(ErrorCode::ConfigInvalid, "bad config").with_context("file", "config.toml");
    let debug = format!("{err:?}");
    assert!(debug.contains("AbpError"));
    assert!(debug.contains("ConfigInvalid"));
    assert!(debug.contains("bad config"));
}

#[test]
fn abp_error_to_info() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "timed out");
    assert!(info.is_retryable);
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
}

#[test]
fn abp_error_from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access");
    let err: AbpError = io_err.into();
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("no access"));
}

#[test]
fn abp_error_from_serde_json_error() {
    let json_err = serde_json::from_str::<serde_json::Value>("not valid json").unwrap_err();
    let err: AbpError = json_err.into();
    assert_eq!(err.code, ErrorCode::ProtocolInvalidEnvelope);
}

// =========================================================================
// 7. Category grouping — verify grouping completeness
// =========================================================================

fn codes_for_category(cat: ErrorCategory) -> Vec<ErrorCode> {
    ALL_CODES
        .iter()
        .copied()
        .filter(|c| c.category() == cat)
        .collect()
}

#[test]
fn group_protocol_count() {
    assert_eq!(codes_for_category(ErrorCategory::Protocol).len(), 5);
}

#[test]
fn group_mapping_count() {
    assert_eq!(codes_for_category(ErrorCategory::Mapping).len(), 4);
}

#[test]
fn group_backend_count() {
    assert_eq!(codes_for_category(ErrorCategory::Backend).len(), 7);
}

#[test]
fn group_execution_count() {
    assert_eq!(codes_for_category(ErrorCategory::Execution).len(), 3);
}

#[test]
fn group_contract_count() {
    assert_eq!(codes_for_category(ErrorCategory::Contract).len(), 3);
}

#[test]
fn group_capability_count() {
    assert_eq!(codes_for_category(ErrorCategory::Capability).len(), 2);
}

#[test]
fn group_policy_count() {
    assert_eq!(codes_for_category(ErrorCategory::Policy).len(), 2);
}

#[test]
fn group_workspace_count() {
    assert_eq!(codes_for_category(ErrorCategory::Workspace).len(), 2);
}

#[test]
fn group_small_categories() {
    assert_eq!(codes_for_category(ErrorCategory::Ir).len(), 2);
    assert_eq!(codes_for_category(ErrorCategory::Receipt).len(), 2);
    assert_eq!(codes_for_category(ErrorCategory::Dialect).len(), 2);
    assert_eq!(codes_for_category(ErrorCategory::Config).len(), 1);
    assert_eq!(codes_for_category(ErrorCategory::Internal).len(), 1);
}

#[test]
fn groups_cover_all_variants() {
    let all_categories = [
        ErrorCategory::Protocol,
        ErrorCategory::Mapping,
        ErrorCategory::Backend,
        ErrorCategory::Execution,
        ErrorCategory::Contract,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Internal,
    ];
    let total: usize = all_categories
        .iter()
        .map(|c| codes_for_category(*c).len())
        .sum();
    assert_eq!(
        total,
        ALL_CODES.len(),
        "category groups must cover all codes"
    );
}

#[test]
fn category_display_matches_snake_case() {
    let cases = [
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
    for (cat, expected) in &cases {
        assert_eq!(format!("{cat}"), *expected);
    }
}

// =========================================================================
// Retryability exhaustive
// =========================================================================

#[test]
fn is_retryable_exhaustive() {
    let retryable: std::collections::HashSet<ErrorCode> = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ]
    .into_iter()
    .collect();

    for code in ALL_CODES {
        if retryable.contains(code) {
            assert!(code.is_retryable(), "{code:?} should be retryable");
        } else {
            assert!(!code.is_retryable(), "{code:?} should NOT be retryable");
        }
    }
}

// =========================================================================
// as_str uniqueness and format
// =========================================================================

#[test]
fn as_str_all_unique() {
    let strs: Vec<&str> = ALL_CODES.iter().map(|c| c.as_str()).collect();
    let unique: std::collections::HashSet<&&str> = strs.iter().collect();
    assert_eq!(unique.len(), strs.len(), "as_str values must be unique");
}

#[test]
fn as_str_all_snake_case() {
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{code:?} as_str '{}' is not snake_case",
            s
        );
    }
}

// =========================================================================
// ErrorInfo + AbpError combined
// =========================================================================

#[test]
fn abp_error_to_info_context_becomes_details() {
    let err = AbpError::new(ErrorCode::ExecutionPermissionDenied, "denied")
        .with_context("path", "/secret")
        .with_context("user", "nobody");
    let info = err.to_info();
    assert_eq!(info.details.len(), 2);
    assert_eq!(info.details["path"], serde_json::json!("/secret"));
    assert_eq!(info.details["user"], serde_json::json!("nobody"));
}

#[test]
fn error_info_serde_roundtrip_all_codes() {
    for code in ALL_CODES {
        let info = ErrorInfo::new(*code, format!("msg for {}", code.as_str()))
            .with_detail("variant", code.as_str());
        let json = serde_json::to_string(&info).unwrap();
        let back: ErrorInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back, "ErrorInfo roundtrip failed for {code:?}");
    }
}

#[test]
fn btreemap_context_deterministic_display() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("aaa", 1)
        .with_context("bbb", 2)
        .with_context("ccc", 3);
    let display = format!("{err}");
    // BTreeMap guarantees alphabetical order.
    assert!(display.contains(r#""aaa":1"#));
    assert!(display.contains(r#""bbb":2"#));
    assert!(display.contains(r#""ccc":3"#));
}
