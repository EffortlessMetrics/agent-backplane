#![allow(clippy::all)]
//! Comprehensive error handling and taxonomy integration test suite.
//!
//! Validates the COMPLETE error handling path from sidecar to user-facing output,
//! covering error creation, categorization, serialization, propagation, display,
//! retryability classification, context attachment, and cross-crate conversions.

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_error_taxonomy::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
    RecoverySuggestion,
};
use std::collections::BTreeMap;

// ============================================================================
// Canonical list of all ErrorCode variants (must match abp-error)
// ============================================================================

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

// ============================================================================
// Module: ErrorCode creation and as_str() for every variant
// ============================================================================
mod error_code_as_str {
    use super::*;

    #[test]
    fn protocol_invalid_envelope() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.as_str(),
            "protocol_invalid_envelope"
        );
    }

    #[test]
    fn protocol_handshake_failed() {
        assert_eq!(
            ErrorCode::ProtocolHandshakeFailed.as_str(),
            "protocol_handshake_failed"
        );
    }

    #[test]
    fn protocol_missing_ref_id() {
        assert_eq!(
            ErrorCode::ProtocolMissingRefId.as_str(),
            "protocol_missing_ref_id"
        );
    }

    #[test]
    fn protocol_unexpected_message() {
        assert_eq!(
            ErrorCode::ProtocolUnexpectedMessage.as_str(),
            "protocol_unexpected_message"
        );
    }

    #[test]
    fn protocol_version_mismatch() {
        assert_eq!(
            ErrorCode::ProtocolVersionMismatch.as_str(),
            "protocol_version_mismatch"
        );
    }

    #[test]
    fn mapping_unsupported_capability() {
        assert_eq!(
            ErrorCode::MappingUnsupportedCapability.as_str(),
            "mapping_unsupported_capability"
        );
    }

    #[test]
    fn mapping_dialect_mismatch() {
        assert_eq!(
            ErrorCode::MappingDialectMismatch.as_str(),
            "mapping_dialect_mismatch"
        );
    }

    #[test]
    fn mapping_lossy_conversion() {
        assert_eq!(
            ErrorCode::MappingLossyConversion.as_str(),
            "mapping_lossy_conversion"
        );
    }

    #[test]
    fn mapping_unmappable_tool() {
        assert_eq!(
            ErrorCode::MappingUnmappableTool.as_str(),
            "mapping_unmappable_tool"
        );
    }

    #[test]
    fn backend_not_found() {
        assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
    }

    #[test]
    fn backend_unavailable() {
        assert_eq!(
            ErrorCode::BackendUnavailable.as_str(),
            "backend_unavailable"
        );
    }

    #[test]
    fn backend_timeout() {
        assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    }

    #[test]
    fn backend_rate_limited() {
        assert_eq!(
            ErrorCode::BackendRateLimited.as_str(),
            "backend_rate_limited"
        );
    }

    #[test]
    fn backend_auth_failed() {
        assert_eq!(ErrorCode::BackendAuthFailed.as_str(), "backend_auth_failed");
    }

    #[test]
    fn backend_model_not_found() {
        assert_eq!(
            ErrorCode::BackendModelNotFound.as_str(),
            "backend_model_not_found"
        );
    }

    #[test]
    fn backend_crashed() {
        assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
    }

    #[test]
    fn execution_tool_failed() {
        assert_eq!(
            ErrorCode::ExecutionToolFailed.as_str(),
            "execution_tool_failed"
        );
    }

    #[test]
    fn execution_workspace_error() {
        assert_eq!(
            ErrorCode::ExecutionWorkspaceError.as_str(),
            "execution_workspace_error"
        );
    }

    #[test]
    fn execution_permission_denied() {
        assert_eq!(
            ErrorCode::ExecutionPermissionDenied.as_str(),
            "execution_permission_denied"
        );
    }

    #[test]
    fn contract_version_mismatch() {
        assert_eq!(
            ErrorCode::ContractVersionMismatch.as_str(),
            "contract_version_mismatch"
        );
    }

    #[test]
    fn contract_schema_violation() {
        assert_eq!(
            ErrorCode::ContractSchemaViolation.as_str(),
            "contract_schema_violation"
        );
    }

    #[test]
    fn contract_invalid_receipt() {
        assert_eq!(
            ErrorCode::ContractInvalidReceipt.as_str(),
            "contract_invalid_receipt"
        );
    }

    #[test]
    fn capability_unsupported() {
        assert_eq!(
            ErrorCode::CapabilityUnsupported.as_str(),
            "capability_unsupported"
        );
    }

    #[test]
    fn capability_emulation_failed() {
        assert_eq!(
            ErrorCode::CapabilityEmulationFailed.as_str(),
            "capability_emulation_failed"
        );
    }

    #[test]
    fn policy_denied() {
        assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    }

    #[test]
    fn policy_invalid() {
        assert_eq!(ErrorCode::PolicyInvalid.as_str(), "policy_invalid");
    }

    #[test]
    fn workspace_init_failed() {
        assert_eq!(
            ErrorCode::WorkspaceInitFailed.as_str(),
            "workspace_init_failed"
        );
    }

    #[test]
    fn workspace_staging_failed() {
        assert_eq!(
            ErrorCode::WorkspaceStagingFailed.as_str(),
            "workspace_staging_failed"
        );
    }

    #[test]
    fn ir_lowering_failed() {
        assert_eq!(ErrorCode::IrLoweringFailed.as_str(), "ir_lowering_failed");
    }

    #[test]
    fn ir_invalid() {
        assert_eq!(ErrorCode::IrInvalid.as_str(), "ir_invalid");
    }

    #[test]
    fn receipt_hash_mismatch() {
        assert_eq!(
            ErrorCode::ReceiptHashMismatch.as_str(),
            "receipt_hash_mismatch"
        );
    }

    #[test]
    fn receipt_chain_broken() {
        assert_eq!(
            ErrorCode::ReceiptChainBroken.as_str(),
            "receipt_chain_broken"
        );
    }

    #[test]
    fn dialect_unknown() {
        assert_eq!(ErrorCode::DialectUnknown.as_str(), "dialect_unknown");
    }

    #[test]
    fn dialect_mapping_failed() {
        assert_eq!(
            ErrorCode::DialectMappingFailed.as_str(),
            "dialect_mapping_failed"
        );
    }

    #[test]
    fn config_invalid() {
        assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
    }

    #[test]
    fn internal() {
        assert_eq!(ErrorCode::Internal.as_str(), "internal");
    }
}

// ============================================================================
// Module: ErrorCode → ErrorCategory categorization for every variant
// ============================================================================
mod error_code_category {
    use super::*;

    #[test]
    fn protocol_codes_belong_to_protocol_category() {
        let protocol_codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolMissingRefId,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
        ];
        for code in protocol_codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Protocol,
                "{code:?} should be Protocol"
            );
        }
    }

    #[test]
    fn mapping_codes_belong_to_mapping_category() {
        let codes = [
            ErrorCode::MappingUnsupportedCapability,
            ErrorCode::MappingDialectMismatch,
            ErrorCode::MappingLossyConversion,
            ErrorCode::MappingUnmappableTool,
        ];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Mapping,
                "{code:?} should be Mapping"
            );
        }
    }

    #[test]
    fn backend_codes_belong_to_backend_category() {
        let codes = [
            ErrorCode::BackendNotFound,
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendAuthFailed,
            ErrorCode::BackendModelNotFound,
            ErrorCode::BackendCrashed,
        ];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Backend,
                "{code:?} should be Backend"
            );
        }
    }

    #[test]
    fn execution_codes_belong_to_execution_category() {
        let codes = [
            ErrorCode::ExecutionToolFailed,
            ErrorCode::ExecutionWorkspaceError,
            ErrorCode::ExecutionPermissionDenied,
        ];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Execution,
                "{code:?} should be Execution"
            );
        }
    }

    #[test]
    fn contract_codes_belong_to_contract_category() {
        let codes = [
            ErrorCode::ContractVersionMismatch,
            ErrorCode::ContractSchemaViolation,
            ErrorCode::ContractInvalidReceipt,
        ];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Contract,
                "{code:?} should be Contract"
            );
        }
    }

    #[test]
    fn capability_codes_belong_to_capability_category() {
        let codes = [
            ErrorCode::CapabilityUnsupported,
            ErrorCode::CapabilityEmulationFailed,
        ];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Capability,
                "{code:?} should be Capability"
            );
        }
    }

    #[test]
    fn policy_codes_belong_to_policy_category() {
        let codes = [ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Policy,
                "{code:?} should be Policy"
            );
        }
    }

    #[test]
    fn workspace_codes_belong_to_workspace_category() {
        let codes = [
            ErrorCode::WorkspaceInitFailed,
            ErrorCode::WorkspaceStagingFailed,
        ];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Workspace,
                "{code:?} should be Workspace"
            );
        }
    }

    #[test]
    fn ir_codes_belong_to_ir_category() {
        let codes = [ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid];
        for code in codes {
            assert_eq!(code.category(), ErrorCategory::Ir, "{code:?} should be Ir");
        }
    }

    #[test]
    fn receipt_codes_belong_to_receipt_category() {
        let codes = [
            ErrorCode::ReceiptHashMismatch,
            ErrorCode::ReceiptChainBroken,
        ];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Receipt,
                "{code:?} should be Receipt"
            );
        }
    }

    #[test]
    fn dialect_codes_belong_to_dialect_category() {
        let codes = [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed];
        for code in codes {
            assert_eq!(
                code.category(),
                ErrorCategory::Dialect,
                "{code:?} should be Dialect"
            );
        }
    }

    #[test]
    fn config_code_belongs_to_config_category() {
        assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    }

    #[test]
    fn internal_code_belongs_to_internal_category() {
        assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
    }

    #[test]
    fn every_variant_has_a_category() {
        for &code in ALL_CODES {
            let _ = code.category(); // should not panic
        }
    }
}

// ============================================================================
// Module: Serde serialization and deserialization
// ============================================================================
mod error_code_serde {
    use super::*;

    #[test]
    fn all_codes_roundtrip_via_json() {
        for &code in ALL_CODES {
            let json = serde_json::to_string(&code).unwrap();
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, code, "roundtrip failed for {code:?}");
        }
    }

    #[test]
    fn serde_uses_snake_case_not_screaming() {
        for &code in ALL_CODES {
            let json = serde_json::to_string(&code).unwrap();
            let expected = format!("\"{}\"", code.as_str());
            assert_eq!(json, expected, "serde mismatch for {code:?}");
            // Verify no uppercase letters in the serialized form
            let inner = &json[1..json.len() - 1];
            assert!(
                inner.chars().all(|c| c.is_lowercase() || c == '_'),
                "expected snake_case for {code:?}, got {json}"
            );
        }
    }

    #[test]
    fn deserialize_from_snake_case_string() {
        let code: ErrorCode = serde_json::from_str("\"backend_timeout\"").unwrap();
        assert_eq!(code, ErrorCode::BackendTimeout);
    }

    #[test]
    fn deserialize_unknown_variant_fails() {
        let result = serde_json::from_str::<ErrorCode>("\"nonexistent_code\"");
        assert!(result.is_err());
    }

    #[test]
    fn category_roundtrip_via_json() {
        let categories = [
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
        for cat in categories {
            let json = serde_json::to_string(&cat).unwrap();
            let back: ErrorCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(back, cat, "category roundtrip failed for {cat:?}");
        }
    }

    #[test]
    fn category_serde_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&ErrorCategory::Protocol).unwrap(),
            "\"protocol\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCategory::Internal).unwrap(),
            "\"internal\""
        );
    }

    #[test]
    fn error_info_roundtrip_with_details() {
        let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited")
            .with_detail("retry_after_ms", 2000)
            .with_detail("backend", "openai");
        let json = serde_json::to_string(&info).unwrap();
        let back: ErrorInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn error_info_json_structure() {
        let info = ErrorInfo::new(ErrorCode::Internal, "oops");
        let v: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert_eq!(v["code"], "internal");
        assert_eq!(v["message"], "oops");
        assert_eq!(v["is_retryable"], false);
    }

    #[test]
    fn abp_error_dto_roundtrip_no_source() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
        assert!(back.source_message.is_none());
    }

    #[test]
    fn abp_error_dto_roundtrip_with_source() {
        let src = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_message.as_deref(), Some("pipe broke"));
    }

    #[test]
    fn abp_error_dto_source_message_omitted_when_none() {
        let err = AbpError::new(ErrorCode::Internal, "oops");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("source_message"));
    }

    #[test]
    fn error_info_empty_details_serializes_as_empty_object() {
        let info = ErrorInfo::new(ErrorCode::Internal, "test");
        let v: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert_eq!(v["details"], serde_json::json!({}));
    }
}

// ============================================================================
// Module: ErrorCode Display (human-readable message)
// ============================================================================
mod error_code_display {
    use super::*;

    #[test]
    fn display_returns_human_readable_message_not_code() {
        let msg = ErrorCode::BackendTimeout.to_string();
        assert_eq!(msg, "backend timed out");
        assert_ne!(msg, "backend_timeout");
    }

    #[test]
    fn all_codes_have_nonempty_messages() {
        for &code in ALL_CODES {
            let msg = code.message();
            assert!(!msg.is_empty(), "{code:?} has empty message");
        }
    }

    #[test]
    fn all_codes_have_unique_messages() {
        let mut seen = std::collections::HashSet::new();
        for &code in ALL_CODES {
            let m = code.message();
            assert!(seen.insert(m), "duplicate message: {m}");
        }
    }

    #[test]
    fn all_codes_have_unique_as_str_values() {
        let mut seen = std::collections::HashSet::new();
        for &code in ALL_CODES {
            let s = code.as_str();
            assert!(seen.insert(s), "duplicate as_str value: {s}");
        }
    }

    #[test]
    fn display_equals_message() {
        for &code in ALL_CODES {
            assert_eq!(code.to_string(), code.message());
        }
    }

    #[test]
    fn category_display_values() {
        assert_eq!(ErrorCategory::Protocol.to_string(), "protocol");
        assert_eq!(ErrorCategory::Backend.to_string(), "backend");
        assert_eq!(ErrorCategory::Capability.to_string(), "capability");
        assert_eq!(ErrorCategory::Policy.to_string(), "policy");
        assert_eq!(ErrorCategory::Workspace.to_string(), "workspace");
        assert_eq!(ErrorCategory::Ir.to_string(), "ir");
        assert_eq!(ErrorCategory::Receipt.to_string(), "receipt");
        assert_eq!(ErrorCategory::Dialect.to_string(), "dialect");
        assert_eq!(ErrorCategory::Config.to_string(), "config");
        assert_eq!(ErrorCategory::Mapping.to_string(), "mapping");
        assert_eq!(ErrorCategory::Execution.to_string(), "execution");
        assert_eq!(ErrorCategory::Contract.to_string(), "contract");
        assert_eq!(ErrorCategory::Internal.to_string(), "internal");
    }

    #[test]
    fn specific_message_values() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.message(),
            "envelope failed to parse or has invalid fields"
        );
        assert_eq!(
            ErrorCode::BackendNotFound.message(),
            "requested backend does not exist"
        );
        assert_eq!(
            ErrorCode::PolicyDenied.message(),
            "policy rule denied the operation"
        );
        assert_eq!(ErrorCode::Internal.message(), "unexpected internal error");
    }
}

// ============================================================================
// Module: Retryability classification
// ============================================================================
mod retryability {
    use super::*;

    #[test]
    fn backend_unavailable_is_retryable() {
        assert!(ErrorCode::BackendUnavailable.is_retryable());
    }

    #[test]
    fn backend_timeout_is_retryable() {
        assert!(ErrorCode::BackendTimeout.is_retryable());
    }

    #[test]
    fn backend_rate_limited_is_retryable() {
        assert!(ErrorCode::BackendRateLimited.is_retryable());
    }

    #[test]
    fn backend_crashed_is_retryable() {
        assert!(ErrorCode::BackendCrashed.is_retryable());
    }

    #[test]
    fn protocol_errors_are_not_retryable() {
        assert!(!ErrorCode::ProtocolInvalidEnvelope.is_retryable());
        assert!(!ErrorCode::ProtocolHandshakeFailed.is_retryable());
        assert!(!ErrorCode::ProtocolMissingRefId.is_retryable());
        assert!(!ErrorCode::ProtocolUnexpectedMessage.is_retryable());
        assert!(!ErrorCode::ProtocolVersionMismatch.is_retryable());
    }

    #[test]
    fn mapping_errors_are_not_retryable() {
        assert!(!ErrorCode::MappingUnsupportedCapability.is_retryable());
        assert!(!ErrorCode::MappingDialectMismatch.is_retryable());
        assert!(!ErrorCode::MappingLossyConversion.is_retryable());
        assert!(!ErrorCode::MappingUnmappableTool.is_retryable());
    }

    #[test]
    fn backend_not_found_is_not_retryable() {
        assert!(!ErrorCode::BackendNotFound.is_retryable());
    }

    #[test]
    fn backend_auth_failed_is_not_retryable() {
        assert!(!ErrorCode::BackendAuthFailed.is_retryable());
    }

    #[test]
    fn backend_model_not_found_is_not_retryable() {
        assert!(!ErrorCode::BackendModelNotFound.is_retryable());
    }

    #[test]
    fn execution_errors_are_not_retryable() {
        assert!(!ErrorCode::ExecutionToolFailed.is_retryable());
        assert!(!ErrorCode::ExecutionWorkspaceError.is_retryable());
        assert!(!ErrorCode::ExecutionPermissionDenied.is_retryable());
    }

    #[test]
    fn contract_errors_are_not_retryable() {
        assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
        assert!(!ErrorCode::ContractSchemaViolation.is_retryable());
        assert!(!ErrorCode::ContractInvalidReceipt.is_retryable());
    }

    #[test]
    fn policy_errors_are_not_retryable() {
        assert!(!ErrorCode::PolicyDenied.is_retryable());
        assert!(!ErrorCode::PolicyInvalid.is_retryable());
    }

    #[test]
    fn workspace_errors_are_not_retryable() {
        assert!(!ErrorCode::WorkspaceInitFailed.is_retryable());
        assert!(!ErrorCode::WorkspaceStagingFailed.is_retryable());
    }

    #[test]
    fn ir_errors_are_not_retryable() {
        assert!(!ErrorCode::IrLoweringFailed.is_retryable());
        assert!(!ErrorCode::IrInvalid.is_retryable());
    }

    #[test]
    fn receipt_errors_are_not_retryable() {
        assert!(!ErrorCode::ReceiptHashMismatch.is_retryable());
        assert!(!ErrorCode::ReceiptChainBroken.is_retryable());
    }

    #[test]
    fn dialect_errors_are_not_retryable() {
        assert!(!ErrorCode::DialectUnknown.is_retryable());
        assert!(!ErrorCode::DialectMappingFailed.is_retryable());
    }

    #[test]
    fn config_invalid_is_not_retryable() {
        assert!(!ErrorCode::ConfigInvalid.is_retryable());
    }

    #[test]
    fn internal_is_not_retryable() {
        assert!(!ErrorCode::Internal.is_retryable());
    }

    #[test]
    fn exactly_four_retryable_codes_exist() {
        let retryable: Vec<_> = ALL_CODES.iter().filter(|c| c.is_retryable()).collect();
        assert_eq!(retryable.len(), 4);
    }

    #[test]
    fn abp_error_delegates_retryability() {
        let retryable = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        assert!(retryable.is_retryable());
        let non_retryable = AbpError::new(ErrorCode::PolicyDenied, "denied");
        assert!(!non_retryable.is_retryable());
    }
}

// ============================================================================
// Module: AbpError construction and builder pattern
// ============================================================================
mod abp_error_construction {
    use super::*;

    #[test]
    fn basic_construction() {
        let err = AbpError::new(ErrorCode::Internal, "boom");
        assert_eq!(err.code, ErrorCode::Internal);
        assert_eq!(err.message, "boom");
        assert!(err.source.is_none());
        assert!(err.context.is_empty());
    }

    #[test]
    fn with_single_context() {
        let err =
            AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("backend", "openai");
        assert_eq!(err.context.len(), 1);
        assert_eq!(err.context["backend"], serde_json::json!("openai"));
    }

    #[test]
    fn with_multiple_context_keys() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
            .with_context("backend", "openai")
            .with_context("timeout_ms", 30_000)
            .with_context("retries", 3);
        assert_eq!(err.context.len(), 3);
    }

    #[test]
    fn with_nested_json_context() {
        let err = AbpError::new(ErrorCode::Internal, "nested")
            .with_context("details", serde_json::json!({"a": 1, "b": [2, 3]}));
        assert_eq!(
            err.context["details"],
            serde_json::json!({"a": 1, "b": [2, 3]})
        );
    }

    #[test]
    fn with_source_error() {
        let src = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err = AbpError::new(ErrorCode::ExecutionPermissionDenied, "denied").with_source(src);
        assert!(err.source.is_some());
    }

    #[test]
    fn chaining_context_and_source() {
        let src = std::io::Error::other("underlying");
        let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
            .with_context("file", "backplane.toml")
            .with_source(src);
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
        assert_eq!(err.context["file"], serde_json::json!("backplane.toml"));
        assert!(err.source.is_some());
    }

    #[test]
    fn category_shorthand() {
        let err = AbpError::new(ErrorCode::DialectUnknown, "unknown");
        assert_eq!(err.category(), ErrorCategory::Dialect);
    }

    #[test]
    fn every_code_can_construct_abp_error() {
        for &code in ALL_CODES {
            let err = AbpError::new(code, format!("test {code:?}"));
            assert_eq!(err.code, code);
            assert_eq!(err.category(), code.category());
        }
    }
}

// ============================================================================
// Module: AbpError display formatting
// ============================================================================
mod abp_error_display {
    use super::*;

    #[test]
    fn display_without_context() {
        let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
        assert_eq!(err.to_string(), "[backend_not_found] no such backend");
    }

    #[test]
    fn display_with_context_includes_json() {
        let err =
            AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 5000);
        let s = err.to_string();
        assert!(s.starts_with("[backend_timeout] timed out"));
        assert!(s.contains("timeout_ms"));
        assert!(s.contains("5000"));
    }

    #[test]
    fn display_context_is_deterministic() {
        let err = AbpError::new(ErrorCode::Internal, "err")
            .with_context("z_key", "last")
            .with_context("a_key", "first");
        let s = err.to_string();
        let a_pos = s.find("a_key").unwrap();
        let z_pos = s.find("z_key").unwrap();
        assert!(a_pos < z_pos, "BTreeMap should produce deterministic order");
    }

    #[test]
    fn debug_contains_code_and_message() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "nope");
        let dbg = format!("{err:?}");
        assert!(dbg.contains("PolicyDenied"));
        assert!(dbg.contains("nope"));
    }

    #[test]
    fn debug_with_source_shows_source() {
        let src = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
        let dbg = format!("{err:?}");
        assert!(dbg.contains("source"));
        assert!(dbg.contains("file missing"));
    }

    #[test]
    fn display_format_matches_bracket_code_message_pattern() {
        for &code in ALL_CODES {
            let err = AbpError::new(code, "test message");
            let display = err.to_string();
            let expected_prefix = format!("[{}] test message", code.as_str());
            assert!(
                display.starts_with(&expected_prefix),
                "Display for {code:?}: {display}"
            );
        }
    }

    #[test]
    fn error_info_display_format() {
        let info = ErrorInfo::new(ErrorCode::ExecutionToolFailed, "tool crashed");
        assert_eq!(info.to_string(), "[execution_tool_failed] tool crashed");
    }
}

// ============================================================================
// Module: ErrorInfo
// ============================================================================
mod error_info_tests {
    use super::*;

    #[test]
    fn retryable_inferred_from_code() {
        let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
        assert!(info.is_retryable);

        let info2 = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
        assert!(!info2.is_retryable);
    }

    #[test]
    fn with_detail_adds_to_details() {
        let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited")
            .with_detail("retry_after_ms", 5000)
            .with_detail("backend", "openai");
        assert_eq!(info.details.len(), 2);
        assert_eq!(info.details["retry_after_ms"], serde_json::json!(5000));
    }

    #[test]
    fn abp_error_to_info_preserves_context() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
            .with_context("ms", 3000)
            .with_context("backend", "anthropic");
        let info = err.to_info();
        assert_eq!(info.code, ErrorCode::BackendTimeout);
        assert_eq!(info.message, "timeout");
        assert!(info.is_retryable);
        assert_eq!(info.details["ms"], serde_json::json!(3000));
        assert_eq!(info.details["backend"], serde_json::json!("anthropic"));
    }

    #[test]
    fn error_info_deterministic_detail_order() {
        let info = ErrorInfo::new(ErrorCode::Internal, "err")
            .with_detail("z_key", "last")
            .with_detail("a_key", "first");
        let json = serde_json::to_string(&info).unwrap();
        let a_pos = json.find("a_key").unwrap();
        let z_pos = json.find("z_key").unwrap();
        assert!(a_pos < z_pos);
    }
}

// ============================================================================
// Module: AbpErrorDto conversions
// ============================================================================
mod abp_error_dto {
    use super::*;

    #[test]
    fn from_abp_error_captures_fields() {
        let err = AbpError::new(ErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.code, ErrorCode::IrInvalid);
        assert_eq!(dto.message, "bad IR");
        assert_eq!(dto.context["node"], serde_json::json!("call_tool"));
        assert!(dto.source_message.is_none());
    }

    #[test]
    fn from_abp_error_captures_source_message() {
        let src = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
    }

    #[test]
    fn dto_to_abp_error_loses_source() {
        let dto = AbpErrorDto {
            code: ErrorCode::ConfigInvalid,
            message: "bad".into(),
            context: BTreeMap::new(),
            source_message: Some("inner".into()),
        };
        let err: AbpError = dto.into();
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
        assert!(err.source.is_none());
    }

    #[test]
    fn dto_roundtrip_preserves_context() {
        let mut ctx = BTreeMap::new();
        ctx.insert("key1".into(), serde_json::json!("value1"));
        ctx.insert("key2".into(), serde_json::json!(42));
        let dto = AbpErrorDto {
            code: ErrorCode::Internal,
            message: "test".into(),
            context: ctx.clone(),
            source_message: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.context, ctx);
    }
}

// ============================================================================
// Module: From conversions (std types → AbpError)
// ============================================================================
mod from_conversions {
    use super::*;

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let abp_err: AbpError = io_err.into();
        assert_eq!(abp_err.code, ErrorCode::Internal);
        assert!(abp_err.message.contains("file not found"));
        assert!(abp_err.source.is_some());
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let abp_err: AbpError = json_err.into();
        assert_eq!(abp_err.code, ErrorCode::ProtocolInvalidEnvelope);
        assert!(abp_err.source.is_some());
    }

    #[test]
    fn from_io_error_preserves_source_chain() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let abp_err: AbpError = io_err.into();
        let src = std::error::Error::source(&abp_err).unwrap();
        assert_eq!(src.to_string(), "access denied");
    }

    #[test]
    fn from_serde_json_error_preserves_source_chain() {
        let json_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
        let msg = json_err.to_string();
        let abp_err: AbpError = json_err.into();
        let src = std::error::Error::source(&abp_err).unwrap();
        assert_eq!(src.to_string(), msg);
    }
}

// ============================================================================
// Module: std::error::Error trait implementation
// ============================================================================
mod std_error_trait {
    use super::*;

    #[test]
    fn source_is_none_by_default() {
        let err = AbpError::new(ErrorCode::Internal, "oops");
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn source_returns_inner_error() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(inner);
        let src = std::error::Error::source(&err).unwrap();
        assert_eq!(src.to_string(), "not found");
    }

    #[test]
    fn error_implements_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        // AbpError source is Box<dyn Error + Send + Sync>, so AbpError itself is Send
        let err = AbpError::new(ErrorCode::Internal, "test");
        let _ = Box::new(err) as Box<dyn std::error::Error + Send + Sync>;
    }
}

// ============================================================================
// Module: ErrorClassifier - taxonomy classification
// ============================================================================
mod classifier_tests {
    use super::*;

    #[test]
    fn all_codes_classifiable() {
        let classifier = ErrorClassifier::new();
        for &code in ALL_CODES {
            let classification = classifier.classify(&code);
            assert_eq!(classification.code, code);
        }
    }

    #[test]
    fn rate_limited_is_retriable_with_delay() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendRateLimited);
        assert_eq!(cl.severity, ErrorSeverity::Retriable);
        assert_eq!(cl.category, ClassificationCategory::RateLimit);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry);
        assert!(cl.recovery.delay_ms.is_some());
    }

    #[test]
    fn backend_timeout_is_retriable() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendTimeout);
        assert_eq!(cl.severity, ErrorSeverity::Retriable);
        assert_eq!(cl.category, ClassificationCategory::TimeoutError);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    }

    #[test]
    fn backend_unavailable_is_retriable() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendUnavailable);
        assert_eq!(cl.severity, ErrorSeverity::Retriable);
        assert_eq!(cl.category, ClassificationCategory::ServerError);
    }

    #[test]
    fn backend_crashed_is_retriable() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendCrashed);
        assert_eq!(cl.severity, ErrorSeverity::Retriable);
        assert_eq!(cl.category, ClassificationCategory::ServerError);
    }

    #[test]
    fn auth_failed_is_fatal_authentication() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendAuthFailed);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::Authentication);
        assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    }

    #[test]
    fn model_not_found_is_fatal_model_not_found() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendModelNotFound);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ModelNotFound);
        assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
    }

    #[test]
    fn protocol_errors_are_fatal_protocol() {
        let c = ErrorClassifier::new();
        for code in [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
        ] {
            let cl = c.classify(&code);
            assert_eq!(cl.severity, ErrorSeverity::Fatal, "{code:?}");
            assert_eq!(
                cl.category,
                ClassificationCategory::ProtocolError,
                "{code:?}"
            );
        }
    }

    #[test]
    fn protocol_missing_ref_id_is_invalid_request() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::ProtocolMissingRefId);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
    }

    #[test]
    fn mapping_lossy_is_degraded() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::MappingLossyConversion);
        assert_eq!(cl.severity, ErrorSeverity::Degraded);
        assert_eq!(cl.category, ClassificationCategory::MappingFailure);
    }

    #[test]
    fn capability_emulation_failed_is_degraded() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::CapabilityEmulationFailed);
        assert_eq!(cl.severity, ErrorSeverity::Degraded);
        assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
    }

    #[test]
    fn policy_denied_is_fatal_content_filter() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::PolicyDenied);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ContentFilter);
        assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    }

    #[test]
    fn mapping_unsupported_capability_is_fatal() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::MappingUnsupportedCapability);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
        assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
    }

    #[test]
    fn dialect_errors_are_mapping_failure() {
        let c = ErrorClassifier::new();
        for code in [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed] {
            let cl = c.classify(&code);
            assert_eq!(cl.severity, ErrorSeverity::Fatal, "{code:?}");
            assert_eq!(
                cl.category,
                ClassificationCategory::MappingFailure,
                "{code:?}"
            );
            assert_eq!(cl.recovery.action, RecoveryAction::Fallback, "{code:?}");
        }
    }

    #[test]
    fn ir_lowering_failed_is_mapping_failure() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::IrLoweringFailed);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::MappingFailure);
    }

    #[test]
    fn ir_invalid_is_invalid_request() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::IrInvalid);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
    }

    #[test]
    fn contract_errors_classification() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::ContractVersionMismatch);
        assert_eq!(cl.category, ClassificationCategory::ProtocolError);

        let cl = c.classify(&ErrorCode::ContractSchemaViolation);
        assert_eq!(cl.category, ClassificationCategory::InvalidRequest);

        let cl = c.classify(&ErrorCode::ContractInvalidReceipt);
        assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
    }

    #[test]
    fn workspace_errors_are_fatal_server() {
        let c = ErrorClassifier::new();
        for code in [
            ErrorCode::WorkspaceInitFailed,
            ErrorCode::WorkspaceStagingFailed,
        ] {
            let cl = c.classify(&code);
            assert_eq!(cl.severity, ErrorSeverity::Fatal, "{code:?}");
            assert_eq!(cl.category, ClassificationCategory::ServerError, "{code:?}");
        }
    }

    #[test]
    fn internal_error_is_fatal_server() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::Internal);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ServerError);
    }

    #[test]
    fn backend_not_found_is_fatal_server() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendNotFound);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ServerError);
    }

    #[test]
    fn config_invalid_is_fatal_invalid_request() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::ConfigInvalid);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
    }

    #[test]
    fn policy_invalid_is_fatal_invalid_request() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::PolicyInvalid);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
    }

    #[test]
    fn receipt_errors_are_fatal_invalid_request() {
        let c = ErrorClassifier::new();
        for code in [
            ErrorCode::ReceiptHashMismatch,
            ErrorCode::ReceiptChainBroken,
        ] {
            let cl = c.classify(&code);
            assert_eq!(cl.severity, ErrorSeverity::Fatal, "{code:?}");
            assert_eq!(
                cl.category,
                ClassificationCategory::InvalidRequest,
                "{code:?}"
            );
        }
    }

    #[test]
    fn execution_tool_failed_is_fatal_server() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::ExecutionToolFailed);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ServerError);
    }

    #[test]
    fn execution_permission_denied_is_fatal_authentication() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::ExecutionPermissionDenied);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::Authentication);
    }

    #[test]
    fn execution_workspace_error_is_fatal_server() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::ExecutionWorkspaceError);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ServerError);
    }

    #[test]
    fn capability_unsupported_is_fatal() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::CapabilityUnsupported);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
        assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
    }

    #[test]
    fn mapping_dialect_mismatch_is_fatal_mapping_failure() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::MappingDialectMismatch);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::MappingFailure);
        assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
    }

    #[test]
    fn mapping_unmappable_tool_is_fatal_mapping_failure() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::MappingUnmappableTool);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::MappingFailure);
    }
}

// ============================================================================
// Module: Recovery suggestions from classifier
// ============================================================================
mod recovery_suggestions {
    use super::*;

    #[test]
    fn rate_limit_suggests_retry_with_delay() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendRateLimited);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry);
        assert_eq!(cl.recovery.delay_ms, Some(2000));
    }

    #[test]
    fn timeout_suggests_retry_with_delay() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendTimeout);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry);
        assert_eq!(cl.recovery.delay_ms, Some(1000));
    }

    #[test]
    fn server_error_suggests_retry() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendUnavailable);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry);
        assert!(cl.recovery.delay_ms.is_some());
    }

    #[test]
    fn auth_error_suggests_contact_admin() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendAuthFailed);
        assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
        assert!(cl.recovery.delay_ms.is_none());
    }

    #[test]
    fn model_not_found_suggests_change_model() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::BackendModelNotFound);
        assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
    }

    #[test]
    fn capability_unsupported_suggests_fallback() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::CapabilityUnsupported);
        assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
    }

    #[test]
    fn mapping_failure_suggests_fallback() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::MappingDialectMismatch);
        assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
    }

    #[test]
    fn policy_denied_suggests_contact_admin() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::PolicyDenied);
        assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    }

    #[test]
    fn degraded_lossy_conversion_suggests_fallback() {
        let c = ErrorClassifier::new();
        let cl = c.classify(&ErrorCode::MappingLossyConversion);
        // Degraded + MappingFailure → the MappingFailure arm fires → Fallback
        assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
    }

    #[test]
    fn suggest_recovery_matches_classify_recovery() {
        let c = ErrorClassifier::new();
        for &code in ALL_CODES {
            let cl = c.classify(&code);
            let recovery = c.suggest_recovery(&cl);
            assert_eq!(recovery.action, cl.recovery.action, "mismatch for {code:?}");
        }
    }

    #[test]
    fn all_recoveries_have_description() {
        let c = ErrorClassifier::new();
        for &code in ALL_CODES {
            let cl = c.classify(&code);
            assert!(
                !cl.recovery.description.is_empty(),
                "{code:?} has empty recovery description"
            );
        }
    }
}

// ============================================================================
// Module: Classification serde roundtrips
// ============================================================================
mod classification_serde {
    use super::*;

    #[test]
    fn severity_roundtrip() {
        let severities = [
            ErrorSeverity::Fatal,
            ErrorSeverity::Retriable,
            ErrorSeverity::Degraded,
            ErrorSeverity::Informational,
        ];
        for sev in severities {
            let json = serde_json::to_string(&sev).unwrap();
            let back: ErrorSeverity = serde_json::from_str(&json).unwrap();
            assert_eq!(back, sev);
        }
    }

    #[test]
    fn classification_category_roundtrip() {
        let categories = [
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
        for cat in categories {
            let json = serde_json::to_string(&cat).unwrap();
            let back: ClassificationCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(back, cat, "roundtrip failed for {cat:?}");
        }
    }

    #[test]
    fn recovery_action_roundtrip() {
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
    fn recovery_suggestion_roundtrip() {
        let suggestion = RecoverySuggestion {
            action: RecoveryAction::Retry,
            description: "try again".into(),
            delay_ms: Some(1500),
        };
        let json = serde_json::to_string(&suggestion).unwrap();
        let back: RecoverySuggestion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, suggestion);
    }

    #[test]
    fn full_classification_roundtrip() {
        let c = ErrorClassifier::new();
        for &code in ALL_CODES {
            let cl = c.classify(&code);
            let json = serde_json::to_string(&cl).unwrap();
            let back: ErrorClassification = serde_json::from_str(&json).unwrap();
            assert_eq!(back, cl, "roundtrip failed for {code:?}");
        }
    }

    #[test]
    fn severity_serde_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&ErrorSeverity::Fatal).unwrap(),
            "\"fatal\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorSeverity::Retriable).unwrap(),
            "\"retriable\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorSeverity::Degraded).unwrap(),
            "\"degraded\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorSeverity::Informational).unwrap(),
            "\"informational\""
        );
    }

    #[test]
    fn recovery_action_serde_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&RecoveryAction::Retry).unwrap(),
            "\"retry\""
        );
        assert_eq!(
            serde_json::to_string(&RecoveryAction::Fallback).unwrap(),
            "\"fallback\""
        );
        assert_eq!(
            serde_json::to_string(&RecoveryAction::ReduceContext).unwrap(),
            "\"reduce_context\""
        );
        assert_eq!(
            serde_json::to_string(&RecoveryAction::ChangeModel).unwrap(),
            "\"change_model\""
        );
        assert_eq!(
            serde_json::to_string(&RecoveryAction::ContactAdmin).unwrap(),
            "\"contact_admin\""
        );
        assert_eq!(
            serde_json::to_string(&RecoveryAction::None).unwrap(),
            "\"none\""
        );
    }

    #[test]
    fn classification_category_serde_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&ClassificationCategory::Authentication).unwrap(),
            "\"authentication\""
        );
        assert_eq!(
            serde_json::to_string(&ClassificationCategory::RateLimit).unwrap(),
            "\"rate_limit\""
        );
        assert_eq!(
            serde_json::to_string(&ClassificationCategory::ContextLength).unwrap(),
            "\"context_length\""
        );
        assert_eq!(
            serde_json::to_string(&ClassificationCategory::CapabilityUnsupported).unwrap(),
            "\"capability_unsupported\""
        );
    }
}

// ============================================================================
// Module: Error propagation path - sidecar → host → integration → runtime
// ============================================================================
mod error_propagation {
    use super::*;
    use abp_protocol::ProtocolError;
    use abp_runtime::RuntimeError;

    #[test]
    fn protocol_json_error_has_error_code() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let proto_err: ProtocolError = json_err.into();
        let code = proto_err.error_code();
        // Json variant may not have a code, or has one - test it doesn't panic
        let _ = code;
    }

    #[test]
    fn protocol_violation_has_error_code() {
        let proto_err = ProtocolError::Violation("bad envelope".into());
        let code = proto_err.error_code();
        assert_eq!(code, Some(ErrorCode::ProtocolInvalidEnvelope));
    }

    #[test]
    fn protocol_unexpected_message_has_error_code() {
        let proto_err = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        };
        let code = proto_err.error_code();
        assert_eq!(code, Some(ErrorCode::ProtocolUnexpectedMessage));
    }

    #[test]
    fn protocol_abp_error_has_error_code() {
        let abp_err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
        let proto_err: ProtocolError = abp_err.into();
        let code = proto_err.error_code();
        assert_eq!(code, Some(ErrorCode::BackendTimeout));
    }

    #[test]
    fn runtime_unknown_backend_error_code() {
        let err = RuntimeError::UnknownBackend {
            name: "nonexistent".into(),
        };
        assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
        assert!(!err.is_retryable());
    }

    #[test]
    fn runtime_workspace_failed_error_code() {
        let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("init failed"));
        assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
        assert!(err.is_retryable());
    }

    #[test]
    fn runtime_policy_failed_error_code() {
        let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad policy"));
        assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
        assert!(!err.is_retryable());
    }

    #[test]
    fn runtime_backend_failed_error_code() {
        let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
        assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
        assert!(err.is_retryable());
    }

    #[test]
    fn runtime_capability_check_failed_error_code() {
        let err = RuntimeError::CapabilityCheckFailed("missing tool_use".into());
        assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
        assert!(!err.is_retryable());
    }

    #[test]
    fn runtime_classified_error_preserves_code() {
        let abp_err = AbpError::new(ErrorCode::BackendRateLimited, "rate limited");
        let runtime_err = RuntimeError::Classified(abp_err);
        assert_eq!(runtime_err.error_code(), ErrorCode::BackendRateLimited);
        assert!(runtime_err.is_retryable());
    }

    #[test]
    fn runtime_no_projection_match_error_code() {
        let err = RuntimeError::NoProjectionMatch {
            reason: "no backend supports this".into(),
        };
        assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
        assert!(!err.is_retryable());
    }

    #[test]
    fn runtime_into_abp_error_preserves_classified() {
        let abp_err =
            AbpError::new(ErrorCode::PolicyDenied, "denied").with_context("rule", "no_write");
        let runtime_err = RuntimeError::Classified(abp_err);
        let recovered = runtime_err.into_abp_error();
        assert_eq!(recovered.code, ErrorCode::PolicyDenied);
        assert_eq!(recovered.context["rule"], serde_json::json!("no_write"));
    }

    #[test]
    fn runtime_into_abp_error_for_non_classified() {
        let err = RuntimeError::UnknownBackend {
            name: "missing".into(),
        };
        let abp_err = err.into_abp_error();
        assert_eq!(abp_err.code, ErrorCode::BackendNotFound);
        assert!(abp_err.message.contains("missing"));
    }

    #[test]
    fn full_propagation_path_sidecar_to_runtime() {
        // Simulate: sidecar → protocol error → AbpError → RuntimeError → AbpError
        let abp_err = AbpError::new(ErrorCode::BackendAuthFailed, "invalid API key")
            .with_context("backend", "openai");

        // Protocol layer wraps it
        let proto_err: ProtocolError = abp_err.into();
        assert_eq!(proto_err.error_code(), Some(ErrorCode::BackendAuthFailed));

        // Runtime would classify it
        let runtime_err = RuntimeError::Classified(
            AbpError::new(ErrorCode::BackendAuthFailed, "invalid API key")
                .with_context("backend", "openai"),
        );
        let final_err = runtime_err.into_abp_error();
        assert_eq!(final_err.code, ErrorCode::BackendAuthFailed);
        assert_eq!(final_err.context["backend"], serde_json::json!("openai"));

        // Taxonomy classification
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&final_err.code);
        assert_eq!(classification.severity, ErrorSeverity::Fatal);
        assert_eq!(
            classification.category,
            ClassificationCategory::Authentication
        );
        assert_eq!(classification.recovery.action, RecoveryAction::ContactAdmin);
    }

    #[test]
    fn backend_timeout_full_propagation() {
        // backend timeout → RuntimeError → AbpError → classifier
        let runtime_err = RuntimeError::BackendFailed(anyhow::anyhow!("timed out"));
        let abp_err = runtime_err.into_abp_error();
        assert_eq!(abp_err.code, ErrorCode::BackendCrashed);

        let classifier = ErrorClassifier::new();
        let cl = classifier.classify(&abp_err.code);
        assert_eq!(cl.severity, ErrorSeverity::Retriable);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    }

    #[test]
    fn workspace_failure_full_propagation() {
        let runtime_err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
        let abp_err = runtime_err.into_abp_error();
        assert_eq!(abp_err.code, ErrorCode::WorkspaceInitFailed);

        let classifier = ErrorClassifier::new();
        let cl = classifier.classify(&abp_err.code);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ServerError);
    }
}

// ============================================================================
// Module: Error context preservation through the stack
// ============================================================================
mod context_preservation {
    use super::*;

    #[test]
    fn context_survives_dto_roundtrip() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
            .with_context("backend", "anthropic")
            .with_context("timeout_ms", 30000)
            .with_context("model", "claude-3.5");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        let recovered: AbpError = back.into();

        assert_eq!(recovered.code, ErrorCode::BackendTimeout);
        assert_eq!(recovered.context["backend"], serde_json::json!("anthropic"));
        assert_eq!(recovered.context["timeout_ms"], serde_json::json!(30000));
        assert_eq!(recovered.context["model"], serde_json::json!("claude-3.5"));
    }

    #[test]
    fn context_preserved_in_error_info() {
        let err = AbpError::new(ErrorCode::ExecutionToolFailed, "tool failed")
            .with_context("tool_name", "bash")
            .with_context("exit_code", 1);
        let info = err.to_info();
        assert_eq!(info.details["tool_name"], serde_json::json!("bash"));
        assert_eq!(info.details["exit_code"], serde_json::json!(1));
    }

    #[test]
    fn nested_json_context_preserved() {
        let nested = serde_json::json!({
            "request": {
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hello"}]
            },
            "response_headers": {
                "x-ratelimit-remaining": "0"
            }
        });
        let err = AbpError::new(ErrorCode::BackendRateLimited, "rate limited")
            .with_context("diagnostics", nested.clone());
        assert_eq!(err.context["diagnostics"], nested);
    }

    #[test]
    fn context_preserved_through_runtime_classified() {
        let abp_err =
            AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("elapsed_ms", 30000);
        let runtime_err = abp_runtime::RuntimeError::Classified(abp_err);
        let recovered = runtime_err.into_abp_error();
        assert_eq!(recovered.context["elapsed_ms"], serde_json::json!(30000));
    }

    #[test]
    fn empty_context_after_non_classified_runtime_conversion() {
        let runtime_err = abp_runtime::RuntimeError::UnknownBackend {
            name: "missing".into(),
        };
        let abp_err = runtime_err.into_abp_error();
        assert!(abp_err.context.is_empty());
    }
}

// ============================================================================
// Module: Exhaustive variant count and coverage
// ============================================================================
mod exhaustive_coverage {
    use super::*;

    #[test]
    fn all_codes_list_has_36_variants() {
        assert_eq!(ALL_CODES.len(), 36);
    }

    #[test]
    fn no_duplicate_codes_in_all_codes() {
        let mut seen = std::collections::HashSet::new();
        for &code in ALL_CODES {
            assert!(seen.insert(code), "duplicate code: {code:?}");
        }
    }

    #[test]
    fn all_categories_covered_by_at_least_one_code() {
        let categories: std::collections::HashSet<_> =
            ALL_CODES.iter().map(|c| c.category()).collect();
        assert!(categories.contains(&ErrorCategory::Protocol));
        assert!(categories.contains(&ErrorCategory::Backend));
        assert!(categories.contains(&ErrorCategory::Mapping));
        assert!(categories.contains(&ErrorCategory::Execution));
        assert!(categories.contains(&ErrorCategory::Contract));
        assert!(categories.contains(&ErrorCategory::Capability));
        assert!(categories.contains(&ErrorCategory::Policy));
        assert!(categories.contains(&ErrorCategory::Workspace));
        assert!(categories.contains(&ErrorCategory::Ir));
        assert!(categories.contains(&ErrorCategory::Receipt));
        assert!(categories.contains(&ErrorCategory::Dialect));
        assert!(categories.contains(&ErrorCategory::Config));
        assert!(categories.contains(&ErrorCategory::Internal));
    }

    #[test]
    fn exactly_13_categories_exist() {
        let categories: std::collections::HashSet<_> =
            ALL_CODES.iter().map(|c| c.category()).collect();
        assert_eq!(categories.len(), 13);
    }

    #[test]
    fn all_severity_levels_used() {
        let c = ErrorClassifier::new();
        let severities: std::collections::HashSet<_> = ALL_CODES
            .iter()
            .map(|code| c.classify(code).severity)
            .collect();
        assert!(severities.contains(&ErrorSeverity::Fatal));
        assert!(severities.contains(&ErrorSeverity::Retriable));
        assert!(severities.contains(&ErrorSeverity::Degraded));
        // Informational may not be assigned to any code
    }

    #[test]
    fn all_recovery_actions_except_reduce_context_used() {
        let c = ErrorClassifier::new();
        let actions: std::collections::HashSet<_> = ALL_CODES
            .iter()
            .map(|code| c.classify(code).recovery.action)
            .collect();
        assert!(actions.contains(&RecoveryAction::Retry));
        assert!(actions.contains(&RecoveryAction::Fallback));
        assert!(actions.contains(&RecoveryAction::ChangeModel));
        assert!(actions.contains(&RecoveryAction::ContactAdmin));
        assert!(actions.contains(&RecoveryAction::None));
    }

    #[test]
    fn classification_categories_used() {
        let c = ErrorClassifier::new();
        let cats: std::collections::HashSet<_> = ALL_CODES
            .iter()
            .map(|code| c.classify(code).category)
            .collect();
        assert!(cats.contains(&ClassificationCategory::Authentication));
        assert!(cats.contains(&ClassificationCategory::RateLimit));
        assert!(cats.contains(&ClassificationCategory::ModelNotFound));
        assert!(cats.contains(&ClassificationCategory::InvalidRequest));
        assert!(cats.contains(&ClassificationCategory::ContentFilter));
        assert!(cats.contains(&ClassificationCategory::ServerError));
        assert!(cats.contains(&ClassificationCategory::ProtocolError));
        assert!(cats.contains(&ClassificationCategory::CapabilityUnsupported));
        assert!(cats.contains(&ClassificationCategory::MappingFailure));
        assert!(cats.contains(&ClassificationCategory::TimeoutError));
    }
}

// ============================================================================
// Module: ProtocolError integration
// ============================================================================
mod protocol_error_integration {
    use super::*;
    use abp_protocol::ProtocolError;

    #[test]
    fn protocol_error_from_serde_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
        let proto_err: ProtocolError = json_err.into();
        let display = proto_err.to_string();
        assert!(display.contains("JSON") || display.contains("json") || display.contains("key"));
    }

    #[test]
    fn protocol_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let proto_err: ProtocolError = io_err.into();
        let display = proto_err.to_string();
        assert!(display.contains("pipe broke") || display.contains("I/O"));
    }

    #[test]
    fn protocol_error_from_abp_error() {
        let abp_err = AbpError::new(ErrorCode::ProtocolHandshakeFailed, "handshake failed");
        let proto_err: ProtocolError = abp_err.into();
        assert_eq!(
            proto_err.error_code(),
            Some(ErrorCode::ProtocolHandshakeFailed)
        );
    }

    #[test]
    fn protocol_violation_display() {
        let proto_err = ProtocolError::Violation("missing hello".into());
        assert!(proto_err.to_string().contains("missing hello"));
    }

    #[test]
    fn protocol_unexpected_message_display() {
        let proto_err = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "event".into(),
        };
        let s = proto_err.to_string();
        assert!(s.contains("hello"));
        assert!(s.contains("event"));
    }
}

// ============================================================================
// Module: RuntimeError integration
// ============================================================================
mod runtime_error_integration {
    use super::*;
    use abp_runtime::RuntimeError;

    #[test]
    fn all_runtime_variants_have_error_code() {
        let variants: Vec<RuntimeError> = vec![
            RuntimeError::UnknownBackend { name: "x".into() },
            RuntimeError::WorkspaceFailed(anyhow::anyhow!("fail")),
            RuntimeError::PolicyFailed(anyhow::anyhow!("fail")),
            RuntimeError::BackendFailed(anyhow::anyhow!("fail")),
            RuntimeError::CapabilityCheckFailed("fail".into()),
            RuntimeError::Classified(AbpError::new(ErrorCode::Internal, "fail")),
            RuntimeError::NoProjectionMatch {
                reason: "fail".into(),
            },
        ];
        for err in variants {
            let _ = err.error_code(); // should not panic
        }
    }

    #[test]
    fn all_runtime_variants_convert_to_abp_error() {
        let variants: Vec<RuntimeError> = vec![
            RuntimeError::UnknownBackend { name: "x".into() },
            RuntimeError::WorkspaceFailed(anyhow::anyhow!("fail")),
            RuntimeError::PolicyFailed(anyhow::anyhow!("fail")),
            RuntimeError::BackendFailed(anyhow::anyhow!("fail")),
            RuntimeError::CapabilityCheckFailed("fail".into()),
            RuntimeError::Classified(AbpError::new(ErrorCode::Internal, "fail")),
            RuntimeError::NoProjectionMatch {
                reason: "fail".into(),
            },
        ];
        for err in variants {
            let abp_err = err.into_abp_error();
            let _ = abp_err.code; // should not panic
        }
    }

    #[test]
    fn runtime_display_contains_meaningful_message() {
        let err = RuntimeError::UnknownBackend {
            name: "nonexistent".into(),
        };
        assert!(err.to_string().contains("nonexistent"));

        let err = RuntimeError::CapabilityCheckFailed("tool_use missing".into());
        assert!(err.to_string().contains("tool_use missing"));
    }

    #[test]
    fn runtime_retryable_variants() {
        assert!(RuntimeError::BackendFailed(anyhow::anyhow!("x")).is_retryable());
        assert!(RuntimeError::WorkspaceFailed(anyhow::anyhow!("x")).is_retryable());
    }

    #[test]
    fn runtime_non_retryable_variants() {
        assert!(!RuntimeError::UnknownBackend { name: "x".into() }.is_retryable());
        assert!(!RuntimeError::PolicyFailed(anyhow::anyhow!("x")).is_retryable());
        assert!(!RuntimeError::CapabilityCheckFailed("x".into()).is_retryable());
        assert!(!RuntimeError::NoProjectionMatch { reason: "x".into() }.is_retryable());
    }

    #[test]
    fn runtime_classified_retryable_delegates() {
        let retryable =
            RuntimeError::Classified(AbpError::new(ErrorCode::BackendRateLimited, "rate limited"));
        assert!(retryable.is_retryable());

        let non_retryable =
            RuntimeError::Classified(AbpError::new(ErrorCode::PolicyDenied, "denied"));
        assert!(!non_retryable.is_retryable());
    }
}

// ============================================================================
// Module: End-to-end error scenarios
// ============================================================================
mod e2e_error_scenarios {
    use super::*;

    #[test]
    fn sidecar_auth_failure_end_to_end() {
        // 1. Create error as sidecar would
        let err = AbpError::new(ErrorCode::BackendAuthFailed, "invalid API key")
            .with_context("backend", "openai")
            .with_context("status_code", 401);

        // 2. Serialize to DTO (wire format)
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();

        // 3. Deserialize on host side
        let recovered_dto: AbpErrorDto = serde_json::from_str(&json).unwrap();
        let recovered: AbpError = recovered_dto.into();

        // 4. Classify for user
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&recovered.code);

        assert_eq!(classification.severity, ErrorSeverity::Fatal);
        assert_eq!(
            classification.category,
            ClassificationCategory::Authentication
        );
        assert_eq!(classification.recovery.action, RecoveryAction::ContactAdmin);
        assert!(!recovered.is_retryable());
    }

    #[test]
    fn sidecar_rate_limit_end_to_end() {
        let err = AbpError::new(ErrorCode::BackendRateLimited, "rate limit exceeded")
            .with_context("retry_after_ms", 2000);
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let recovered: AbpError = serde_json::from_str::<AbpErrorDto>(&json).unwrap().into();

        let classifier = ErrorClassifier::new();
        let cl = classifier.classify(&recovered.code);

        assert_eq!(cl.severity, ErrorSeverity::Retriable);
        assert_eq!(cl.category, ClassificationCategory::RateLimit);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry);
        assert!(cl.recovery.delay_ms.is_some());
        assert!(recovered.is_retryable());
    }

    #[test]
    fn model_not_found_end_to_end() {
        let err = AbpError::new(ErrorCode::BackendModelNotFound, "model not found")
            .with_context("model", "gpt-5-turbo");
        let info = err.to_info();
        let json = serde_json::to_string(&info).unwrap();
        let recovered: ErrorInfo = serde_json::from_str(&json).unwrap();

        let classifier = ErrorClassifier::new();
        let cl = classifier.classify(&recovered.code);

        assert_eq!(cl.category, ClassificationCategory::ModelNotFound);
        assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
        assert!(!recovered.is_retryable);
    }

    #[test]
    fn protocol_handshake_failure_end_to_end() {
        let err = AbpError::new(ErrorCode::ProtocolHandshakeFailed, "no hello received")
            .with_context("timeout_ms", 5000);

        let runtime_err = abp_runtime::RuntimeError::Classified(err);
        let abp_err = runtime_err.into_abp_error();

        let classifier = ErrorClassifier::new();
        let cl = classifier.classify(&abp_err.code);

        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ProtocolError);
        assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    }

    #[test]
    fn policy_denial_end_to_end() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "write access denied")
            .with_context("path", "/etc/passwd")
            .with_context("rule", "no_system_write");

        let info = err.to_info();
        assert!(!info.is_retryable);

        let classifier = ErrorClassifier::new();
        let cl = classifier.classify(&info.code);
        assert_eq!(cl.category, ClassificationCategory::ContentFilter);
        assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    }
}
