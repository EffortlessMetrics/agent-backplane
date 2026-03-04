#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive error taxonomy tests covering ErrorCode, ErrorCategory,
//! CatalogCode (abp-core), AbpError, ErrorInfo, classification, conversion,
//! and cross-crate interop.

use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;
use std::io;

use abp_core::error::{
    ErrorCatalog, ErrorCode as CatalogCode, ErrorInfo as CoreErrorInfo, MappingError,
    MappingErrorKind,
};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_error_taxonomy::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
    RecoverySuggestion,
};
use abp_protocol::ProtocolError;
use abp_runtime::RuntimeError;

// ─── Complete list of abp_error::ErrorCode variants ────────────────────────

const ALL_ERROR_CODES: &[ErrorCode] = &[
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

const ALL_CATEGORIES: &[ErrorCategory] = &[
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

// ═══════════════════════════════════════════════════════════════════════════
// 1. ErrorCode as_str() exhaustive coverage
// ═══════════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════════
// 2. ErrorCode category() mapping (exhaustive)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_category_protocol_codes() {
    let protocol = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in protocol {
        assert_eq!(code.category(), ErrorCategory::Protocol, "{code:?}");
    }
}

#[test]
fn error_code_category_mapping_codes() {
    let mapping = [
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ];
    for code in mapping {
        assert_eq!(code.category(), ErrorCategory::Mapping, "{code:?}");
    }
}

#[test]
fn error_code_category_backend_codes() {
    let backend = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
    ];
    for code in backend {
        assert_eq!(code.category(), ErrorCategory::Backend, "{code:?}");
    }
}

#[test]
fn error_code_category_execution() {
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
fn error_code_category_contract() {
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
fn error_code_category_capability() {
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
fn error_code_category_policy() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn error_code_category_workspace() {
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
fn error_code_category_ir() {
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
}

#[test]
fn error_code_category_receipt() {
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
fn error_code_category_dialect() {
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(
        ErrorCode::DialectMappingFailed.category(),
        ErrorCategory::Dialect
    );
}

#[test]
fn error_code_category_config_and_internal() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. ErrorCode Display — human-readable message
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_display_is_message_not_code_string() {
    assert_eq!(ErrorCode::BackendTimeout.to_string(), "backend timed out");
    assert_ne!(ErrorCode::BackendTimeout.to_string(), "backend_timeout");
}

#[test]
fn error_code_display_matches_message_for_all_variants() {
    for code in ALL_ERROR_CODES {
        assert_eq!(code.to_string(), code.message(), "mismatch for {code:?}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. ErrorCode serialization / deserialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_serde_roundtrip_all_variants() {
    for &code in ALL_ERROR_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "roundtrip failed for {code:?}");
    }
}

#[test]
fn error_code_serializes_as_snake_case_string() {
    let json = serde_json::to_string(&ErrorCode::ProtocolInvalidEnvelope).unwrap();
    assert_eq!(json, r#""protocol_invalid_envelope""#);

    let json = serde_json::to_string(&ErrorCode::BackendRateLimited).unwrap();
    assert_eq!(json, r#""backend_rate_limited""#);
}

#[test]
fn error_code_serialization_matches_as_str() {
    for code in ALL_ERROR_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!(r#""{}""#, code.as_str());
        assert_eq!(json, expected, "mismatch for {code:?}");
    }
}

#[test]
fn error_code_deserialize_from_snake_case() {
    let code: ErrorCode = serde_json::from_str(r#""backend_timeout""#).unwrap();
    assert_eq!(code, ErrorCode::BackendTimeout);
}

#[test]
fn error_code_deserialize_unknown_rejects() {
    let result = serde_json::from_str::<ErrorCode>(r#""not_a_real_code""#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. ErrorCode uniqueness and count
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_count_is_36() {
    assert_eq!(ALL_ERROR_CODES.len(), 36);
}

#[test]
fn error_code_as_str_all_unique() {
    let mut seen = HashSet::new();
    for code in ALL_ERROR_CODES {
        assert!(
            seen.insert(code.as_str()),
            "duplicate as_str: {}",
            code.as_str()
        );
    }
}

#[test]
fn error_code_messages_all_unique() {
    let mut seen = HashSet::new();
    for code in ALL_ERROR_CODES {
        assert!(
            seen.insert(code.message()),
            "duplicate message: {}",
            code.message()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. CatalogCode (abp_core::error::ErrorCode) coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn catalog_code_contract_codes() {
    assert_eq!(CatalogCode::InvalidContractVersion.code(), "ABP-C001");
    assert_eq!(CatalogCode::MalformedWorkOrder.code(), "ABP-C002");
    assert_eq!(CatalogCode::MalformedReceipt.code(), "ABP-C003");
    assert_eq!(CatalogCode::InvalidHash.code(), "ABP-C004");
    assert_eq!(CatalogCode::MissingRequiredField.code(), "ABP-C005");
    assert_eq!(CatalogCode::InvalidWorkOrderId.code(), "ABP-C006");
    assert_eq!(CatalogCode::InvalidRunId.code(), "ABP-C007");
    assert_eq!(CatalogCode::DuplicateWorkOrderId.code(), "ABP-C008");
    assert_eq!(CatalogCode::ContractVersionMismatch.code(), "ABP-C009");
    assert_eq!(CatalogCode::InvalidOutcome.code(), "ABP-C010");
    assert_eq!(CatalogCode::InvalidExecutionLane.code(), "ABP-C011");
    assert_eq!(CatalogCode::InvalidExecutionMode.code(), "ABP-C012");
}

#[test]
fn catalog_code_protocol_codes() {
    assert_eq!(CatalogCode::InvalidEnvelope.code(), "ABP-P001");
    assert_eq!(CatalogCode::HandshakeFailed.code(), "ABP-P002");
    assert_eq!(CatalogCode::UnexpectedMessage.code(), "ABP-P003");
    assert_eq!(CatalogCode::VersionMismatch.code(), "ABP-P004");
    assert_eq!(CatalogCode::MalformedJsonl.code(), "ABP-P005");
    assert_eq!(CatalogCode::InvalidRefId.code(), "ABP-P006");
    assert_eq!(CatalogCode::EnvelopeTooLarge.code(), "ABP-P007");
    assert_eq!(CatalogCode::MissingEnvelopeField.code(), "ABP-P008");
    assert_eq!(CatalogCode::InvalidEnvelopeTag.code(), "ABP-P009");
    assert_eq!(CatalogCode::ProtocolTimeout.code(), "ABP-P010");
    assert_eq!(CatalogCode::DuplicateHello.code(), "ABP-P011");
    assert_eq!(CatalogCode::UnexpectedFinal.code(), "ABP-P012");
}

#[test]
fn catalog_code_policy_codes() {
    assert_eq!(CatalogCode::ToolDenied.code(), "ABP-L001");
    assert_eq!(CatalogCode::ReadDenied.code(), "ABP-L002");
    assert_eq!(CatalogCode::WriteDenied.code(), "ABP-L003");
    assert_eq!(CatalogCode::PolicyCompilationFailed.code(), "ABP-L004");
    assert_eq!(CatalogCode::CapabilityNotSupported.code(), "ABP-L005");
    assert_eq!(CatalogCode::NetworkDenied.code(), "ABP-L006");
    assert_eq!(CatalogCode::ApprovalRequired.code(), "ABP-L007");
    assert_eq!(CatalogCode::PolicyViolation.code(), "ABP-L008");
    assert_eq!(CatalogCode::InvalidGlobPattern.code(), "ABP-L009");
    assert_eq!(CatalogCode::ToolNotRegistered.code(), "ABP-L010");
    assert_eq!(CatalogCode::PathTraversal.code(), "ABP-L011");
}

#[test]
fn catalog_code_runtime_codes() {
    assert_eq!(CatalogCode::BackendUnavailable.code(), "ABP-R001");
    assert_eq!(CatalogCode::BackendTimeout.code(), "ABP-R002");
    assert_eq!(CatalogCode::WorkspaceStagingFailed.code(), "ABP-R003");
    assert_eq!(CatalogCode::EventStreamClosed.code(), "ABP-R004");
    assert_eq!(CatalogCode::RunCancelled.code(), "ABP-R005");
    assert_eq!(CatalogCode::SidecarCrashed.code(), "ABP-R006");
    assert_eq!(CatalogCode::SidecarSpawnFailed.code(), "ABP-R007");
    assert_eq!(CatalogCode::WorkspaceCleanupFailed.code(), "ABP-R008");
    assert_eq!(CatalogCode::MaxTurnsExceeded.code(), "ABP-R009");
    assert_eq!(CatalogCode::BudgetExceeded.code(), "ABP-R010");
    assert_eq!(CatalogCode::BackendMismatch.code(), "ABP-R011");
    assert_eq!(CatalogCode::RunAlreadyCompleted.code(), "ABP-R012");
    assert_eq!(CatalogCode::NoBackendRegistered.code(), "ABP-R013");
}

#[test]
fn catalog_code_system_codes() {
    assert_eq!(CatalogCode::IoError.code(), "ABP-S001");
    assert_eq!(CatalogCode::SerializationError.code(), "ABP-S002");
    assert_eq!(CatalogCode::InternalError.code(), "ABP-S003");
    assert_eq!(CatalogCode::ConfigurationError.code(), "ABP-S004");
    assert_eq!(CatalogCode::ResourceExhausted.code(), "ABP-S005");
    assert_eq!(CatalogCode::Utf8Error.code(), "ABP-S006");
    assert_eq!(CatalogCode::TaskJoinError.code(), "ABP-S007");
    assert_eq!(CatalogCode::ChannelClosed.code(), "ABP-S008");
    assert_eq!(CatalogCode::InvalidArgument.code(), "ABP-S009");
    assert_eq!(CatalogCode::PermissionDenied.code(), "ABP-S010");
    assert_eq!(CatalogCode::NotImplemented.code(), "ABP-S011");
}

#[test]
fn catalog_code_category_mapping() {
    assert_eq!(CatalogCode::InvalidContractVersion.category(), "contract");
    assert_eq!(CatalogCode::InvalidEnvelope.category(), "protocol");
    assert_eq!(CatalogCode::ToolDenied.category(), "policy");
    assert_eq!(CatalogCode::BackendUnavailable.category(), "runtime");
    assert_eq!(CatalogCode::IoError.category(), "system");
}

#[test]
fn catalog_code_description_non_empty() {
    for code in ErrorCatalog::all() {
        assert!(
            !code.description().is_empty(),
            "empty description for {:?}",
            code
        );
    }
}

#[test]
fn catalog_code_display_is_code_string() {
    assert_eq!(CatalogCode::InvalidContractVersion.to_string(), "ABP-C001");
    assert_eq!(CatalogCode::IoError.to_string(), "ABP-S001");
}

#[test]
fn catalog_code_serde_roundtrip() {
    for code in ErrorCatalog::all() {
        let json = serde_json::to_string(&code).unwrap();
        let back: CatalogCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "roundtrip failed for {code:?}");
    }
}

#[test]
fn catalog_all_codes_unique_code_strings() {
    let all = ErrorCatalog::all();
    let mut seen = HashSet::new();
    for code in &all {
        assert!(seen.insert(code.code()), "duplicate code: {}", code.code());
    }
}

#[test]
fn catalog_lookup_by_code_string() {
    assert_eq!(
        ErrorCatalog::lookup("ABP-C001"),
        Some(CatalogCode::InvalidContractVersion)
    );
    assert_eq!(
        ErrorCatalog::lookup("ABP-P012"),
        Some(CatalogCode::UnexpectedFinal)
    );
    assert_eq!(ErrorCatalog::lookup("ABP-Z999"), None);
}

#[test]
fn catalog_by_category_contract() {
    let contract = ErrorCatalog::by_category("contract");
    assert_eq!(contract.len(), 12);
    for code in &contract {
        assert_eq!(code.category(), "contract");
    }
}

#[test]
fn catalog_by_category_protocol() {
    let protocol = ErrorCatalog::by_category("protocol");
    assert_eq!(protocol.len(), 12);
}

#[test]
fn catalog_by_category_policy() {
    let policy = ErrorCatalog::by_category("policy");
    assert_eq!(policy.len(), 11);
}

#[test]
fn catalog_by_category_runtime() {
    let runtime = ErrorCatalog::by_category("runtime");
    assert_eq!(runtime.len(), 13);
}

#[test]
fn catalog_by_category_system() {
    let system = ErrorCatalog::by_category("system");
    assert_eq!(system.len(), 11);
}

#[test]
fn catalog_total_code_count() {
    assert_eq!(ErrorCatalog::all().len(), 59);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. ErrorCategory coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn category_count_is_13() {
    assert_eq!(ALL_CATEGORIES.len(), 13);
}

#[test]
fn category_display_protocol() {
    assert_eq!(ErrorCategory::Protocol.to_string(), "protocol");
}

#[test]
fn category_display_backend() {
    assert_eq!(ErrorCategory::Backend.to_string(), "backend");
}

#[test]
fn category_display_capability() {
    assert_eq!(ErrorCategory::Capability.to_string(), "capability");
}

#[test]
fn category_display_policy() {
    assert_eq!(ErrorCategory::Policy.to_string(), "policy");
}

#[test]
fn category_display_workspace() {
    assert_eq!(ErrorCategory::Workspace.to_string(), "workspace");
}

#[test]
fn category_display_ir() {
    assert_eq!(ErrorCategory::Ir.to_string(), "ir");
}

#[test]
fn category_display_receipt() {
    assert_eq!(ErrorCategory::Receipt.to_string(), "receipt");
}

#[test]
fn category_display_dialect() {
    assert_eq!(ErrorCategory::Dialect.to_string(), "dialect");
}

#[test]
fn category_display_config() {
    assert_eq!(ErrorCategory::Config.to_string(), "config");
}

#[test]
fn category_display_mapping() {
    assert_eq!(ErrorCategory::Mapping.to_string(), "mapping");
}

#[test]
fn category_display_execution() {
    assert_eq!(ErrorCategory::Execution.to_string(), "execution");
}

#[test]
fn category_display_contract() {
    assert_eq!(ErrorCategory::Contract.to_string(), "contract");
}

#[test]
fn category_display_internal() {
    assert_eq!(ErrorCategory::Internal.to_string(), "internal");
}

#[test]
fn category_serde_roundtrip_all() {
    for &cat in ALL_CATEGORIES {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat, "roundtrip failed for {cat:?}");
    }
}

#[test]
fn category_every_error_code_maps_to_known_category() {
    for code in ALL_ERROR_CODES {
        let cat = code.category();
        assert!(
            ALL_CATEGORIES.contains(&cat),
            "code {:?} maps to unknown category {:?}",
            code,
            cat
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. AbpError builder pattern
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_new_basic() {
    let err = AbpError::new(ErrorCode::Internal, "boom");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(err.message, "boom");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_with_context_single() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("backend", "openai");
    assert_eq!(err.context.len(), 1);
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
}

#[test]
fn abp_error_with_context_multiple() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    assert_eq!(err.context.len(), 3);
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_with_context_nested_json() {
    let err = AbpError::new(ErrorCode::Internal, "nested")
        .with_context("details", serde_json::json!({"a": 1, "b": [2, 3]}));
    assert_eq!(
        err.context["details"],
        serde_json::json!({"a": 1, "b": [2, 3]})
    );
}

#[test]
fn abp_error_with_source_io() {
    let src = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied").with_source(src);
    assert!(err.source.is_some());
    assert_eq!(err.source.as_ref().unwrap().to_string(), "access denied");
}

#[test]
fn abp_error_with_source_chain_traversal() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "not found");
    let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(inner);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "not found");
}

#[test]
fn abp_error_no_source_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn abp_error_builder_chaining_all() {
    let src = io::Error::other("underlying");
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml")
        .with_source(src);
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.context["file"], serde_json::json!("backplane.toml"));
    assert!(err.source.is_some());
}

#[test]
fn abp_error_display_without_context() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    assert_eq!(err.to_string(), "[backend_not_found] no such backend");
}

#[test]
fn abp_error_display_with_context_includes_json() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 5000);
    let s = err.to_string();
    assert!(s.starts_with("[backend_timeout] timed out"));
    assert!(s.contains("backend"));
    assert!(s.contains("timeout_ms"));
    assert!(s.contains("5000"));
}

#[test]
fn abp_error_display_context_deterministic_order() {
    let err = AbpError::new(ErrorCode::Internal, "err")
        .with_context("z_key", "last")
        .with_context("a_key", "first");
    let s = err.to_string();
    let a_pos = s.find("a_key").unwrap();
    let z_pos = s.find("z_key").unwrap();
    assert!(a_pos < z_pos, "BTreeMap should order a < z");
}

#[test]
fn abp_error_debug_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "nope");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("PolicyDenied"));
    assert!(dbg.contains("nope"));
}

#[test]
fn abp_error_debug_includes_source() {
    let src = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("file missing"));
}

#[test]
fn abp_error_debug_includes_context() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("k", "v");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
}

#[test]
fn abp_error_category_shorthand() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unknown dialect");
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

#[test]
fn abp_error_is_retryable_shorthand() {
    assert!(AbpError::new(ErrorCode::BackendTimeout, "t").is_retryable());
    assert!(!AbpError::new(ErrorCode::PolicyDenied, "d").is_retryable());
}

#[test]
fn abp_error_send_sync_bounds() {
    fn assert_send_sync<T: Send + Sync>() {}
    // AbpError is not Clone (has Box<dyn Error>), but is Send + Sync
    assert_send_sync::<AbpError>();
}

#[test]
fn abp_error_to_info_conversion() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("ms", 3000);
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "timeout");
    assert!(info.is_retryable);
    assert_eq!(info.details["ms"], serde_json::json!(3000));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. ErrorInfo (abp_error) tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_info_construction_retryable() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert!(info.is_retryable);
    assert!(info.details.is_empty());
}

#[test]
fn error_info_construction_non_retryable() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
    assert!(!info.is_retryable);
}

#[test]
fn error_info_with_details() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited")
        .with_detail("retry_after_ms", 5000)
        .with_detail("backend", "openai");
    assert_eq!(info.details.len(), 2);
}

#[test]
fn error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::ExecutionToolFailed, "tool crashed");
    assert_eq!(info.to_string(), "[execution_tool_failed] tool crashed");
}

#[test]
fn error_info_serde_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::ContractSchemaViolation, "bad schema")
        .with_detail("field", "work_order.task");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. AbpErrorDto serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dto_from_abp_error_without_source() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::IrInvalid);
    assert_eq!(dto.message, "bad IR");
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_from_abp_error_with_source() {
    let src = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn dto_roundtrip_json() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_to_abp_error_drops_source() {
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

// ═══════════════════════════════════════════════════════════════════════════
// 11. Error conversion and interop
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn from_io_error_to_abp_error() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let abp_err: AbpError = io_err.into();
    assert_eq!(abp_err.code, ErrorCode::Internal);
    assert!(abp_err.message.contains("file not found"));
    assert!(abp_err.source.is_some());
}

#[test]
fn from_serde_json_error_to_abp_error() {
    let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let abp_err: AbpError = json_err.into();
    assert_eq!(abp_err.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(abp_err.source.is_some());
}

#[test]
fn runtime_error_unknown_backend_to_error_code() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_workspace_failed_to_error_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("inner"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_policy_failed_to_error_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("inner"));
    assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_backend_failed_to_error_code() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("inner"));
    assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_capability_check_to_error_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing tool".into());
    assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_no_projection_match_to_error_code() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no match".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_into_abp_error_preserves_code() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_classified_passthrough() {
    let inner = AbpError::new(ErrorCode::PolicyDenied, "no");
    let err = RuntimeError::Classified(inner);
    assert_eq!(err.error_code(), ErrorCode::PolicyDenied);
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::PolicyDenied);
    assert_eq!(abp.message, "no");
}

#[test]
fn runtime_error_display_messages() {
    assert_eq!(
        RuntimeError::UnknownBackend { name: "x".into() }.to_string(),
        "unknown backend: x"
    );
    assert_eq!(
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("inner")).to_string(),
        "workspace preparation failed"
    );
    assert_eq!(
        RuntimeError::CapabilityCheckFailed("reason".into()).to_string(),
        "capability check failed: reason"
    );
}

#[test]
fn protocol_error_violation() {
    let err = ProtocolError::Violation("bad envelope".into());
    assert!(err.to_string().contains("bad envelope"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Retryability tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn retryable_backend_codes() {
    assert!(ErrorCode::BackendUnavailable.is_retryable());
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(ErrorCode::BackendCrashed.is_retryable());
}

#[test]
fn non_retryable_protocol_codes() {
    assert!(!ErrorCode::ProtocolInvalidEnvelope.is_retryable());
    assert!(!ErrorCode::ProtocolHandshakeFailed.is_retryable());
    assert!(!ErrorCode::ProtocolMissingRefId.is_retryable());
    assert!(!ErrorCode::ProtocolUnexpectedMessage.is_retryable());
    assert!(!ErrorCode::ProtocolVersionMismatch.is_retryable());
}

#[test]
fn non_retryable_policy_codes() {
    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::PolicyInvalid.is_retryable());
}

#[test]
fn non_retryable_contract_codes() {
    assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
    assert!(!ErrorCode::ContractSchemaViolation.is_retryable());
    assert!(!ErrorCode::ContractInvalidReceipt.is_retryable());
}

#[test]
fn non_retryable_mapping_codes() {
    assert!(!ErrorCode::MappingUnsupportedCapability.is_retryable());
    assert!(!ErrorCode::MappingDialectMismatch.is_retryable());
    assert!(!ErrorCode::MappingLossyConversion.is_retryable());
    assert!(!ErrorCode::MappingUnmappableTool.is_retryable());
}

#[test]
fn non_retryable_misc_codes() {
    assert!(!ErrorCode::BackendNotFound.is_retryable());
    assert!(!ErrorCode::BackendAuthFailed.is_retryable());
    assert!(!ErrorCode::BackendModelNotFound.is_retryable());
    assert!(!ErrorCode::ExecutionToolFailed.is_retryable());
    assert!(!ErrorCode::Internal.is_retryable());
    assert!(!ErrorCode::ConfigInvalid.is_retryable());
}

#[test]
fn exactly_four_retryable_codes() {
    let retryable: Vec<_> = ALL_ERROR_CODES
        .iter()
        .filter(|c| c.is_retryable())
        .collect();
    assert_eq!(retryable.len(), 4);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. ErrorClassifier (abp-error-taxonomy) coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn classifier_rate_limited() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::RateLimit);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.is_some());
}

#[test]
fn classifier_auth_failed() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn classifier_backend_timeout() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendTimeout);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::TimeoutError);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
}

#[test]
fn classifier_model_not_found() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendModelNotFound);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ModelNotFound);
    assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
}

#[test]
fn classifier_protocol_errors_fatal() {
    let c = ErrorClassifier::new();
    for &code in &[
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
fn classifier_protocol_missing_ref_id_is_invalid_request() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::ProtocolMissingRefId);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

#[test]
fn classifier_mapping_dialect_mismatch() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::MappingDialectMismatch);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::MappingFailure);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn classifier_lossy_conversion_degraded() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    assert_eq!(cl.category, ClassificationCategory::MappingFailure);
}

#[test]
fn classifier_capability_unsupported() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::CapabilityUnsupported);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn classifier_capability_emulation_degraded() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::CapabilityEmulationFailed);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
}

#[test]
fn classifier_policy_denied_content_filter() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::PolicyDenied);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ContentFilter);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn classifier_policy_invalid() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::PolicyInvalid);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

#[test]
fn classifier_backend_unavailable_retriable() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendUnavailable);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
}

#[test]
fn classifier_backend_crashed_retriable() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendCrashed);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
}

#[test]
fn classifier_internal_fatal_server_error() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::Internal);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
}

#[test]
fn classifier_contract_version_mismatch_protocol() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::ContractVersionMismatch);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ProtocolError);
}

#[test]
fn classifier_contract_schema_violation_invalid() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::ContractSchemaViolation);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

#[test]
fn classifier_ir_lowering_mapping_failure() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::IrLoweringFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::MappingFailure);
}

#[test]
fn classifier_dialect_unknown_mapping_failure() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::DialectUnknown);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::MappingFailure);
}

#[test]
fn classifier_workspace_init_server_error() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::WorkspaceInitFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
}

#[test]
fn classifier_execution_permission_auth() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::ExecutionPermissionDenied);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
}

#[test]
fn classifier_config_invalid() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::ConfigInvalid);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

#[test]
fn classifier_receipt_hash_mismatch_invalid() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::ReceiptHashMismatch);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

#[test]
fn classifier_suggest_recovery_matches_classify() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendRateLimited);
    let recovery = c.suggest_recovery(&cl);
    assert_eq!(recovery.action, cl.recovery.action);
}

#[test]
fn classifier_classifies_all_codes_without_panic() {
    let c = ErrorClassifier::new();
    for &code in ALL_ERROR_CODES {
        let cl = c.classify(&code);
        // Just verify it doesn't panic and returns sensible data
        assert_eq!(cl.code, code);
        assert!(!cl.recovery.description.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. MappingError (abp_core) coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_fidelity_loss_code() {
    assert_eq!(MappingError::FIDELITY_LOSS_CODE, "ABP_E_FIDELITY_LOSS");
}

#[test]
fn mapping_error_unsupported_cap_code() {
    assert_eq!(MappingError::UNSUPPORTED_CAP_CODE, "ABP_E_UNSUPPORTED_CAP");
}

#[test]
fn mapping_error_emulation_required_code() {
    assert_eq!(
        MappingError::EMULATION_REQUIRED_CODE,
        "ABP_E_EMULATION_REQUIRED"
    );
}

#[test]
fn mapping_error_incompatible_model_code() {
    assert_eq!(
        MappingError::INCOMPATIBLE_MODEL_CODE,
        "ABP_E_INCOMPATIBLE_MODEL"
    );
}

#[test]
fn mapping_error_param_not_mappable_code() {
    assert_eq!(
        MappingError::PARAM_NOT_MAPPABLE_CODE,
        "ABP_E_PARAM_NOT_MAPPABLE"
    );
}

#[test]
fn mapping_error_streaming_unsupported_code() {
    assert_eq!(
        MappingError::STREAMING_UNSUPPORTED_CODE,
        "ABP_E_STREAMING_UNSUPPORTED"
    );
}

#[test]
fn mapping_error_fidelity_loss_kind() {
    let err = MappingError::FidelityLoss {
        field: "temperature".into(),
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        detail: "range differs".into(),
    };
    assert_eq!(err.kind(), MappingErrorKind::Degraded);
    assert!(err.is_degraded());
    assert!(!err.is_fatal());
    assert_eq!(err.code(), MappingError::FIDELITY_LOSS_CODE);
}

#[test]
fn mapping_error_unsupported_capability_kind() {
    let err = MappingError::UnsupportedCapability {
        capability: "tool_use".into(),
        dialect: "basic-llm".into(),
    };
    assert_eq!(err.kind(), MappingErrorKind::Fatal);
    assert!(err.is_fatal());
}

#[test]
fn mapping_error_emulation_required_kind() {
    let err = MappingError::EmulationRequired {
        feature: "streaming".into(),
        detail: "polled".into(),
    };
    assert_eq!(err.kind(), MappingErrorKind::Emulated);
    assert!(err.is_emulated());
}

#[test]
fn mapping_error_incompatible_model_kind() {
    let err = MappingError::IncompatibleModel {
        requested: "gpt-5".into(),
        dialect: "claude".into(),
        suggestion: Some("claude-opus".into()),
    };
    assert!(err.is_fatal());
}

#[test]
fn mapping_error_streaming_unsupported_kind() {
    let err = MappingError::StreamingUnsupported {
        dialect: "batch-only".into(),
    };
    assert!(err.is_fatal());
    assert_eq!(err.code(), MappingError::STREAMING_UNSUPPORTED_CODE);
}

#[test]
fn mapping_error_parameter_not_mappable_kind() {
    let err = MappingError::ParameterNotMappable {
        parameter: "top_k".into(),
        value: "50".into(),
        dialect: "openai".into(),
    };
    assert!(err.is_degraded());
}

#[test]
fn mapping_error_display_includes_code() {
    let err = MappingError::FidelityLoss {
        field: "f".into(),
        source_dialect: "a".into(),
        target_dialect: "b".into(),
        detail: "d".into(),
    };
    assert!(err.to_string().contains("ABP_E_FIDELITY_LOSS"));
}

#[test]
fn mapping_error_serde_roundtrip() {
    let err = MappingError::UnsupportedCapability {
        capability: "vision".into(),
        dialect: "text-only".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. CoreErrorInfo (abp_core::error::ErrorInfo)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn core_error_info_construction() {
    let info = CoreErrorInfo::new(CatalogCode::IoError, "disk full");
    assert_eq!(info.code, CatalogCode::IoError);
    assert_eq!(info.message, "disk full");
    assert!(info.context.is_empty());
    assert!(info.source.is_none());
}

#[test]
fn core_error_info_with_context() {
    let info =
        CoreErrorInfo::new(CatalogCode::BackendTimeout, "slow").with_context("backend", "openai");
    assert_eq!(info.context["backend"], "openai");
}

#[test]
fn core_error_info_with_source() {
    let src = io::Error::new(io::ErrorKind::TimedOut, "timed out");
    let info = CoreErrorInfo::new(CatalogCode::BackendTimeout, "slow").with_source(src);
    assert!(info.source.is_some());
}

#[test]
fn core_error_info_display_format() {
    let info = CoreErrorInfo::new(CatalogCode::IoError, "disk full");
    assert_eq!(info.to_string(), "[ABP-S001] disk full");
}

#[test]
fn core_error_info_display_with_context() {
    let info = CoreErrorInfo::new(CatalogCode::IoError, "disk full").with_context("path", "/tmp");
    let s = info.to_string();
    assert!(s.contains("[ABP-S001]"));
    assert!(s.contains("path=/tmp"));
}

#[test]
fn core_error_info_source_chain() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "missing");
    let info = CoreErrorInfo::new(CatalogCode::IoError, "read failed").with_source(inner);
    let src = StdError::source(&info).unwrap();
    assert_eq!(src.to_string(), "missing");
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. ErrorSeverity / RecoveryAction serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn severity_serde_roundtrip() {
    for sev in [
        ErrorSeverity::Fatal,
        ErrorSeverity::Retriable,
        ErrorSeverity::Degraded,
        ErrorSeverity::Informational,
    ] {
        let json = serde_json::to_string(&sev).unwrap();
        let back: ErrorSeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sev);
    }
}

#[test]
fn recovery_action_serde_roundtrip() {
    for action in [
        RecoveryAction::Retry,
        RecoveryAction::Fallback,
        RecoveryAction::ReduceContext,
        RecoveryAction::ChangeModel,
        RecoveryAction::ContactAdmin,
        RecoveryAction::None,
    ] {
        let json = serde_json::to_string(&action).unwrap();
        let back: RecoveryAction = serde_json::from_str(&json).unwrap();
        assert_eq!(back, action);
    }
}

#[test]
fn classification_category_serde_roundtrip() {
    for cat in [
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
    ] {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ClassificationCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat);
    }
}

#[test]
fn error_classification_serde_roundtrip() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendTimeout);
    let json = serde_json::to_string(&cl).unwrap();
    let back: ErrorClassification = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cl);
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. MappingErrorKind exhaustive
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_kind_display() {
    assert_eq!(MappingErrorKind::Fatal.to_string(), "fatal");
    assert_eq!(MappingErrorKind::Degraded.to_string(), "degraded");
    assert_eq!(MappingErrorKind::Emulated.to_string(), "emulated");
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
