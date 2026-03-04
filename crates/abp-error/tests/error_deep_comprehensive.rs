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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Deep comprehensive tests for the `abp-error` crate.
//!
//! Covers ErrorCode, ErrorCategory, ErrorInfo, AbpError, AbpErrorDto,
//! serialization determinism, error conversion, source chaining,
//! display/debug formatting, and regression guards for stable string values.

use std::collections::{BTreeMap, HashSet};
use std::io;

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};

// -------------------------------------------------------------------------
// Helper: exhaustive list of all ErrorCode variants
// -------------------------------------------------------------------------

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

// =========================================================================
// 1. ErrorCode — unique as_str(), non-empty message(), Display
// =========================================================================

#[test]
fn error_code_as_str_values_are_all_unique() {
    let mut seen = HashSet::new();
    for &code in ALL_CODES {
        assert!(
            seen.insert(code.as_str()),
            "duplicate as_str: {}",
            code.as_str()
        );
    }
}

#[test]
fn error_code_messages_are_all_non_empty() {
    for &code in ALL_CODES {
        let msg = code.message();
        assert!(!msg.is_empty(), "{:?} has empty message", code);
    }
}

#[test]
fn error_code_display_equals_message() {
    for &code in ALL_CODES {
        assert_eq!(
            code.to_string(),
            code.message(),
            "Display != message for {:?}",
            code
        );
    }
}

#[test]
fn error_code_as_str_is_snake_case() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
            "{s} is not snake_case"
        );
    }
}

#[test]
fn error_code_variant_count_is_36() {
    assert_eq!(ALL_CODES.len(), 36, "expected 36 ErrorCode variants");
}

#[test]
fn error_code_debug_contains_variant_name() {
    let dbg = format!("{:?}", ErrorCode::BackendTimeout);
    assert!(dbg.contains("BackendTimeout"));
}

// =========================================================================
// 2. ErrorCategory — Display, serde roundtrip, exhaustive coverage
// =========================================================================

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

#[test]
fn error_category_display_all_non_empty() {
    for &cat in ALL_CATEGORIES {
        assert!(!cat.to_string().is_empty());
    }
}

#[test]
fn error_category_serde_roundtrip_all() {
    for &cat in ALL_CATEGORIES {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat);
    }
}

#[test]
fn error_category_display_values_stable() {
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
fn error_category_count_is_13() {
    assert_eq!(ALL_CATEGORIES.len(), 13);
}

#[test]
fn every_code_maps_to_a_known_category() {
    for &code in ALL_CODES {
        let cat = code.category();
        assert!(
            ALL_CATEGORIES.contains(&cat),
            "{:?} maps to unknown category {:?}",
            code,
            cat
        );
    }
}

// =========================================================================
// 3. ErrorCode → category mapping correctness
// =========================================================================

#[test]
fn protocol_codes_map_to_protocol_category() {
    let protocol_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in protocol_codes {
        assert_eq!(code.category(), ErrorCategory::Protocol, "{code:?}");
    }
}

#[test]
fn backend_codes_map_to_backend_category() {
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
        assert_eq!(code.category(), ErrorCategory::Backend, "{code:?}");
    }
}

#[test]
fn mapping_codes_map_to_mapping_category() {
    let codes = [
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Mapping, "{code:?}");
    }
}

#[test]
fn execution_codes_map_to_execution_category() {
    let codes = [
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
    ];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Execution, "{code:?}");
    }
}

#[test]
fn contract_codes_map_to_contract_category() {
    let codes = [
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Contract, "{code:?}");
    }
}

#[test]
fn capability_codes_map_to_capability_category() {
    let codes = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Capability, "{code:?}");
    }
}

#[test]
fn policy_codes_map_to_policy_category() {
    for code in [ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid] {
        assert_eq!(code.category(), ErrorCategory::Policy, "{code:?}");
    }
}

#[test]
fn workspace_codes_map_to_workspace_category() {
    for code in [
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
    ] {
        assert_eq!(code.category(), ErrorCategory::Workspace, "{code:?}");
    }
}

#[test]
fn ir_codes_map_to_ir_category() {
    for code in [ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid] {
        assert_eq!(code.category(), ErrorCategory::Ir, "{code:?}");
    }
}

#[test]
fn receipt_codes_map_to_receipt_category() {
    for code in [
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ] {
        assert_eq!(code.category(), ErrorCategory::Receipt, "{code:?}");
    }
}

#[test]
fn dialect_codes_map_to_dialect_category() {
    for code in [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed] {
        assert_eq!(code.category(), ErrorCategory::Dialect, "{code:?}");
    }
}

#[test]
fn config_code_maps_to_config_category() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn internal_code_maps_to_internal_category() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

// =========================================================================
// 4. BackplaneError (AbpError) — construction, context, source chaining
// =========================================================================

#[test]
fn abp_error_new_sets_fields() {
    let err = AbpError::new(ErrorCode::Internal, "boom");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(err.message, "boom");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_with_context_adds_entries() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    assert_eq!(err.context.len(), 2);
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_with_context_overwrites_same_key() {
    let err = AbpError::new(ErrorCode::Internal, "x")
        .with_context("k", "v1")
        .with_context("k", "v2");
    assert_eq!(err.context.len(), 1);
    assert_eq!(err.context["k"], serde_json::json!("v2"));
}

#[test]
fn abp_error_with_source_sets_source() {
    let src = io::Error::new(io::ErrorKind::BrokenPipe, "pipe");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crashed").with_source(src);
    assert!(err.source.is_some());
}

#[test]
fn abp_error_source_chain_via_std_error_trait() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init").with_source(inner);
    let src = std::error::Error::source(&err).unwrap();
    assert_eq!(src.to_string(), "missing");
}

#[test]
fn abp_error_source_is_none_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "x");
    assert!(std::error::Error::source(&err).is_none());
}

#[test]
fn abp_error_category_delegates_to_code() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert_eq!(err.category(), ErrorCategory::Policy);
}

#[test]
fn abp_error_is_retryable_delegates_to_code() {
    assert!(AbpError::new(ErrorCode::BackendTimeout, "t").is_retryable());
    assert!(!AbpError::new(ErrorCode::PolicyDenied, "d").is_retryable());
}

#[test]
fn abp_error_to_info_preserves_fields() {
    let err = AbpError::new(ErrorCode::BackendRateLimited, "rl").with_context("retry_after", 2000);
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendRateLimited);
    assert_eq!(info.message, "rl");
    assert!(info.is_retryable);
    assert_eq!(info.details["retry_after"], serde_json::json!(2000));
}

#[test]
fn abp_error_to_info_discards_source() {
    let src = io::Error::new(io::ErrorKind::Other, "inner");
    let err = AbpError::new(ErrorCode::Internal, "x").with_source(src);
    let info = err.to_info();
    // ErrorInfo has no source field — just verify it doesn't panic
    assert_eq!(info.code, ErrorCode::Internal);
}

#[test]
fn abp_error_with_context_nested_json() {
    let err = AbpError::new(ErrorCode::Internal, "nested")
        .with_context("data", serde_json::json!({"a": [1, 2], "b": true}));
    assert_eq!(
        err.context["data"],
        serde_json::json!({"a": [1, 2], "b": true})
    );
}

#[test]
fn abp_error_many_context_entries() {
    let mut err = AbpError::new(ErrorCode::Internal, "m");
    for i in 0..50 {
        err = err.with_context(format!("key_{i}"), i);
    }
    assert_eq!(err.context.len(), 50);
}

// =========================================================================
// 5. Error recovery — retryable vs non-retryable classification
// =========================================================================

#[test]
fn retryable_codes_are_exactly_four() {
    let retryable: Vec<&ErrorCode> = ALL_CODES.iter().filter(|c| c.is_retryable()).collect();
    assert_eq!(retryable.len(), 4);
}

#[test]
fn retryable_codes_are_backend_transients() {
    let expected = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ];
    for code in &expected {
        assert!(code.is_retryable(), "{code:?} should be retryable");
    }
}

#[test]
fn non_retryable_codes_exhaustive() {
    let non_retryable: Vec<&ErrorCode> = ALL_CODES.iter().filter(|c| !c.is_retryable()).collect();
    assert_eq!(non_retryable.len(), ALL_CODES.len() - 4);
    for code in non_retryable {
        assert!(
            !matches!(
                code,
                ErrorCode::BackendUnavailable
                    | ErrorCode::BackendTimeout
                    | ErrorCode::BackendRateLimited
                    | ErrorCode::BackendCrashed
            ),
            "{code:?} should not be in non-retryable set"
        );
    }
}

#[test]
fn auth_errors_are_not_retryable() {
    assert!(!ErrorCode::BackendAuthFailed.is_retryable());
    assert!(!ErrorCode::ExecutionPermissionDenied.is_retryable());
}

#[test]
fn policy_errors_are_not_retryable() {
    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::PolicyInvalid.is_retryable());
}

// =========================================================================
// 6. Error conversion — from std::io::Error, from serde_json::Error
// =========================================================================

#[test]
fn from_io_error_maps_to_internal() {
    let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert!(abp.message.contains("denied"));
    assert!(abp.source.is_some());
}

#[test]
fn from_io_error_preserves_source_message() {
    let io_err = io::Error::new(io::ErrorKind::TimedOut, "timed out");
    let abp: AbpError = io_err.into();
    let src = std::error::Error::source(&abp).unwrap();
    assert_eq!(src.to_string(), "timed out");
}

#[test]
fn from_serde_json_error_maps_to_protocol_invalid_envelope() {
    let json_err = serde_json::from_str::<serde_json::Value>("{invalid").unwrap_err();
    let abp: AbpError = json_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(abp.source.is_some());
}

#[test]
fn from_serde_json_error_message_is_descriptive() {
    let json_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let abp: AbpError = json_err.into();
    assert!(!abp.message.is_empty());
}

// =========================================================================
// 7. Error display — Display, Debug, alternate formatting
// =========================================================================

#[test]
fn abp_error_display_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    let s = err.to_string();
    assert!(s.contains("backend_not_found"));
    assert!(s.contains("no such backend"));
}

#[test]
fn abp_error_display_with_context_includes_json() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow").with_context("ms", 5000);
    let s = err.to_string();
    assert!(s.contains("5000"));
    assert!(s.contains("ms"));
}

#[test]
fn abp_error_display_without_context_no_trailing_json() {
    let err = AbpError::new(ErrorCode::Internal, "plain");
    let s = err.to_string();
    assert_eq!(s, "[internal] plain");
    assert!(!s.contains('{'));
}

#[test]
fn abp_error_debug_contains_struct_name() {
    let err = AbpError::new(ErrorCode::Internal, "x");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("AbpError"));
}

#[test]
fn abp_error_debug_includes_source_when_present() {
    let src = io::Error::new(io::ErrorKind::Other, "inner cause");
    let err = AbpError::new(ErrorCode::Internal, "outer").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("inner cause"));
}

#[test]
fn abp_error_debug_omits_context_when_empty() {
    let err = AbpError::new(ErrorCode::Internal, "x");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("context"));
}

#[test]
fn abp_error_debug_includes_context_when_present() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("k", "v");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
}

#[test]
fn error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::ConfigInvalid, "bad config");
    assert_eq!(info.to_string(), "[config_invalid] bad config");
}

#[test]
fn error_code_display_is_human_readable_not_snake_case() {
    for &code in ALL_CODES {
        let display = code.to_string();
        let as_str = code.as_str();
        // Display should NOT be the snake_case code string
        assert_ne!(
            display, as_str,
            "{:?}: Display should differ from as_str",
            code
        );
    }
}

// =========================================================================
// 8. ErrorInfo — construction, details, serde
// =========================================================================

#[test]
fn error_info_new_infers_retryable_from_code() {
    let retryable = ErrorInfo::new(ErrorCode::BackendTimeout, "t");
    assert!(retryable.is_retryable);

    let not_retryable = ErrorInfo::new(ErrorCode::PolicyDenied, "d");
    assert!(!not_retryable.is_retryable);
}

#[test]
fn error_info_with_detail_adds_entries() {
    let info = ErrorInfo::new(ErrorCode::Internal, "x")
        .with_detail("a", 1)
        .with_detail("b", "two");
    assert_eq!(info.details.len(), 2);
}

#[test]
fn error_info_serde_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::ReceiptHashMismatch, "mismatch")
        .with_detail("expected", "abc")
        .with_detail("actual", "def");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

// =========================================================================
// 9. AbpErrorDto — conversion, serde, source preservation
// =========================================================================

#[test]
fn dto_from_abp_error_preserves_code_and_message() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad ir");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::IrInvalid);
    assert_eq!(dto.message, "bad ir");
}

#[test]
fn dto_from_abp_error_captures_source_message() {
    let src = io::Error::new(io::ErrorKind::Other, "root cause");
    let err = AbpError::new(ErrorCode::Internal, "wrapper").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("root cause"));
}

#[test]
fn dto_from_abp_error_source_none_when_absent() {
    let err = AbpError::new(ErrorCode::Internal, "no source");
    let dto: AbpErrorDto = (&err).into();
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_to_abp_error_loses_opaque_source() {
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad".into(),
        context: BTreeMap::new(),
        source_message: Some("inner".into()),
    };
    let err: AbpError = dto.into();
    assert!(err.source.is_none());
}

#[test]
fn dto_serde_roundtrip_with_source() {
    let src = io::Error::new(io::ErrorKind::Other, "cause");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash")
        .with_source(src)
        .with_context("pid", 1234);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
    assert!(json.contains("cause"));
    assert!(json.contains("1234"));
}

#[test]
fn dto_serde_omits_source_message_when_none() {
    let err = AbpError::new(ErrorCode::Internal, "x");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

// =========================================================================
// 10. Error codes stability — as_str regression guards
// =========================================================================

#[test]
fn as_str_protocol_codes_stable() {
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
fn as_str_mapping_codes_stable() {
    assert_eq!(
        ErrorCode::MappingUnsupportedCapability.as_str(),
        "mapping_unsupported_capability"
    );
    assert_eq!(
        ErrorCode::MappingDialectMismatch.as_str(),
        "mapping_dialect_mismatch"
    );
    assert_eq!(
        ErrorCode::MappingLossyConversion.as_str(),
        "mapping_lossy_conversion"
    );
    assert_eq!(
        ErrorCode::MappingUnmappableTool.as_str(),
        "mapping_unmappable_tool"
    );
}

#[test]
fn as_str_backend_codes_stable() {
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
    assert_eq!(
        ErrorCode::BackendUnavailable.as_str(),
        "backend_unavailable"
    );
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(
        ErrorCode::BackendRateLimited.as_str(),
        "backend_rate_limited"
    );
    assert_eq!(ErrorCode::BackendAuthFailed.as_str(), "backend_auth_failed");
    assert_eq!(
        ErrorCode::BackendModelNotFound.as_str(),
        "backend_model_not_found"
    );
    assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
}

#[test]
fn as_str_remaining_codes_stable() {
    assert_eq!(
        ErrorCode::ExecutionToolFailed.as_str(),
        "execution_tool_failed"
    );
    assert_eq!(
        ErrorCode::ExecutionWorkspaceError.as_str(),
        "execution_workspace_error"
    );
    assert_eq!(
        ErrorCode::ExecutionPermissionDenied.as_str(),
        "execution_permission_denied"
    );
    assert_eq!(
        ErrorCode::ContractVersionMismatch.as_str(),
        "contract_version_mismatch"
    );
    assert_eq!(
        ErrorCode::ContractSchemaViolation.as_str(),
        "contract_schema_violation"
    );
    assert_eq!(
        ErrorCode::ContractInvalidReceipt.as_str(),
        "contract_invalid_receipt"
    );
    assert_eq!(
        ErrorCode::CapabilityUnsupported.as_str(),
        "capability_unsupported"
    );
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.as_str(),
        "capability_emulation_failed"
    );
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(ErrorCode::PolicyInvalid.as_str(), "policy_invalid");
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.as_str(),
        "workspace_init_failed"
    );
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.as_str(),
        "workspace_staging_failed"
    );
    assert_eq!(ErrorCode::IrLoweringFailed.as_str(), "ir_lowering_failed");
    assert_eq!(ErrorCode::IrInvalid.as_str(), "ir_invalid");
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.as_str(),
        "receipt_hash_mismatch"
    );
    assert_eq!(
        ErrorCode::ReceiptChainBroken.as_str(),
        "receipt_chain_broken"
    );
    assert_eq!(ErrorCode::DialectUnknown.as_str(), "dialect_unknown");
    assert_eq!(
        ErrorCode::DialectMappingFailed.as_str(),
        "dialect_mapping_failed"
    );
    assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
}

// =========================================================================
// 11. Deterministic serialization — JSON output is stable
// =========================================================================

#[test]
fn error_code_serde_roundtrip_all_variants() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "roundtrip failed for {code:?}");
    }
}

#[test]
fn error_code_json_matches_as_str() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "JSON mismatch for {code:?}");
    }
}

#[test]
fn error_info_deterministic_key_order() {
    let info = ErrorInfo::new(ErrorCode::Internal, "err")
        .with_detail("z_last", "z")
        .with_detail("a_first", "a")
        .with_detail("m_mid", "m");
    let json = serde_json::to_string(&info).unwrap();
    let a_pos = json.find("a_first").unwrap();
    let m_pos = json.find("m_mid").unwrap();
    let z_pos = json.find("z_last").unwrap();
    assert!(a_pos < m_pos, "a_first should precede m_mid");
    assert!(m_pos < z_pos, "m_mid should precede z_last");
}

#[test]
fn abp_error_display_context_deterministic_order() {
    let err = AbpError::new(ErrorCode::Internal, "msg")
        .with_context("z_key", "z")
        .with_context("a_key", "a");
    let s = err.to_string();
    let a_pos = s.find("a_key").unwrap();
    let z_pos = s.find("z_key").unwrap();
    assert!(a_pos < z_pos, "BTreeMap should order a_key before z_key");
}

#[test]
fn dto_deterministic_context_order() {
    let err = AbpError::new(ErrorCode::Internal, "msg")
        .with_context("zulu", 26)
        .with_context("alpha", 1)
        .with_context("mike", 13);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let alpha_pos = json.find("alpha").unwrap();
    let mike_pos = json.find("mike").unwrap();
    let zulu_pos = json.find("zulu").unwrap();
    assert!(alpha_pos < mike_pos);
    assert!(mike_pos < zulu_pos);
}

#[test]
fn serialization_is_repeatable() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
        .with_context("backend", "openai")
        .with_context("ms", 5000);
    let dto: AbpErrorDto = (&err).into();
    let json1 = serde_json::to_string(&dto).unwrap();
    let json2 = serde_json::to_string(&dto).unwrap();
    assert_eq!(json1, json2);
}

// =========================================================================
// 12. Edge cases and misc
// =========================================================================

#[test]
fn error_code_clone_and_copy() {
    let code = ErrorCode::BackendTimeout;
    let cloned = code.clone();
    let copied = code;
    assert_eq!(code, cloned);
    assert_eq!(code, copied);
}

#[test]
fn error_code_eq_and_hash_consistent() {
    let mut set = HashSet::new();
    set.insert(ErrorCode::Internal);
    set.insert(ErrorCode::Internal);
    assert_eq!(set.len(), 1);
}

#[test]
fn error_category_clone_and_copy() {
    let cat = ErrorCategory::Backend;
    let cloned = cat.clone();
    let copied = cat;
    assert_eq!(cat, cloned);
    assert_eq!(cat, copied);
}

#[test]
fn error_info_clone() {
    let info = ErrorInfo::new(ErrorCode::Internal, "x").with_detail("k", "v");
    let cloned = info.clone();
    assert_eq!(info, cloned);
}

#[test]
fn abp_error_empty_message() {
    let err = AbpError::new(ErrorCode::Internal, "");
    assert_eq!(err.message, "");
    assert_eq!(err.to_string(), "[internal] ");
}

#[test]
fn abp_error_unicode_message() {
    let err = AbpError::new(ErrorCode::Internal, "エラー: 失敗しました 🔥");
    assert!(err.to_string().contains("🔥"));
}

#[test]
fn error_info_with_bool_detail() {
    let info = ErrorInfo::new(ErrorCode::Internal, "x").with_detail("flag", true);
    assert_eq!(info.details["flag"], serde_json::json!(true));
}

#[test]
fn error_info_with_null_detail() {
    let info =
        ErrorInfo::new(ErrorCode::Internal, "x").with_detail("empty", serde_json::Value::Null);
    assert_eq!(info.details["empty"], serde_json::Value::Null);
}

#[test]
fn error_info_with_array_detail() {
    let info = ErrorInfo::new(ErrorCode::Internal, "x").with_detail("items", vec![1, 2, 3]);
    assert_eq!(info.details["items"], serde_json::json!([1, 2, 3]));
}

#[test]
fn abp_error_context_with_float() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("latency_ms", 12.5_f64);
    assert_eq!(err.context["latency_ms"], serde_json::json!(12.5));
}

#[test]
fn from_io_error_various_kinds() {
    let kinds = [
        io::ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::ConnectionRefused,
        io::ErrorKind::TimedOut,
        io::ErrorKind::BrokenPipe,
    ];
    for kind in kinds {
        let io_err = io::Error::new(kind, format!("{kind:?}"));
        let abp: AbpError = io_err.into();
        assert_eq!(abp.code, ErrorCode::Internal, "io {kind:?} → Internal");
    }
}
