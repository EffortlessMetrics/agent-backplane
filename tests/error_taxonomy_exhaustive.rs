#![allow(clippy::all)]
#![allow(dead_code)]
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
//! Exhaustive tests for the ABP error taxonomy crate.
//!
//! Verifies every error code, category, and classification exported by
//! `abp_error_taxonomy` (re-exports from `abp_error`).
//!
//! Structure:
//!   A) Error code coverage (10 tests)
//!   B) Error category (10 tests)
//!   C) AbpError integration (10 tests)
//!   D) Extra coverage — ErrorInfo, From conversions, edge cases (10+ tests)

use abp_error_taxonomy::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Exhaustive list of all ErrorCode variants — must stay in sync with the enum.
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
    // RateLimit
    ErrorCode::RateLimitExceeded,
    ErrorCode::CircuitBreakerOpen,
    // Stream
    ErrorCode::StreamClosed,
    // ReceiptStore
    ErrorCode::ReceiptStoreFailed,
    // Validation
    ErrorCode::ValidationFailed,
    // Sidecar
    ErrorCode::SidecarSpawnFailed,
    // Backend (extended)
    ErrorCode::BackendContentFiltered,
    ErrorCode::BackendContextLength,
    // Internal
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
    ErrorCategory::RateLimit,
    ErrorCategory::Stream,
    ErrorCategory::Validation,
    ErrorCategory::Sidecar,
    ErrorCategory::Internal,
];

// =========================================================================
// A) Error code coverage (10 tests)
// =========================================================================

#[test]
fn ec01_every_error_code_variant_is_constructible() {
    assert_eq!(ALL_CODES.len(), 36);
    for &code in ALL_CODES {
        let _copy = code;
    }
}

#[test]
fn ec02_error_code_display_includes_human_readable_message() {
    for &code in ALL_CODES {
        let display = code.to_string();
        assert_eq!(
            display,
            code.message(),
            "{:?}: Display diverges from message()",
            code
        );
        assert_ne!(
            display,
            code.as_str(),
            "{:?}: Display should not be the code string",
            code
        );
    }
}

#[test]
fn ec03_error_code_as_str_returns_snake_case() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{:?}: as_str() is not snake_case: {s}",
            code
        );
        assert!(!s.is_empty());
    }
}

#[test]
fn ec04_error_code_serde_roundtrip() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "roundtrip failed for {:?}", code);
    }
}

#[test]
fn ec05_error_code_deserializes_from_as_str_string() {
    for &code in ALL_CODES {
        let json_str = format!("\"{}\"", code.as_str());
        let parsed: ErrorCode = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed, code, "parsing from as_str failed for {:?}", code);
    }
}

#[test]
fn ec06_all_codes_have_unique_as_str_values() {
    let mut seen = HashSet::new();
    for &code in ALL_CODES {
        assert!(
            seen.insert(code.as_str()),
            "duplicate as_str: {}",
            code.as_str()
        );
    }
    assert_eq!(seen.len(), ALL_CODES.len());
}

#[test]
fn ec07_error_codes_categorize_correctly() {
    let expected_category: &[(ErrorCode, ErrorCategory)] = &[
        (ErrorCode::ProtocolInvalidEnvelope, ErrorCategory::Protocol),
        (ErrorCode::BackendTimeout, ErrorCategory::Backend),
        (ErrorCode::MappingDialectMismatch, ErrorCategory::Mapping),
        (ErrorCode::ExecutionToolFailed, ErrorCategory::Execution),
        (ErrorCode::ContractSchemaViolation, ErrorCategory::Contract),
        (ErrorCode::CapabilityUnsupported, ErrorCategory::Capability),
        (ErrorCode::PolicyDenied, ErrorCategory::Policy),
        (ErrorCode::WorkspaceInitFailed, ErrorCategory::Workspace),
        (ErrorCode::IrLoweringFailed, ErrorCategory::Ir),
        (ErrorCode::ReceiptHashMismatch, ErrorCategory::Receipt),
        (ErrorCode::DialectUnknown, ErrorCategory::Dialect),
        (ErrorCode::ConfigInvalid, ErrorCategory::Config),
        (ErrorCode::Internal, ErrorCategory::Internal),
    ];
    for &(code, cat) in expected_category {
        assert_eq!(code.category(), cat, "{:?} category mismatch", code);
    }
}

#[test]
fn ec08_retryable_codes_identified_correctly() {
    let retryable: Vec<ErrorCode> = ALL_CODES
        .iter()
        .copied()
        .filter(|c| c.is_retryable())
        .collect();
    assert_eq!(retryable.len(), 4);
    assert!(retryable.contains(&ErrorCode::BackendUnavailable));
    assert!(retryable.contains(&ErrorCode::BackendTimeout));
    assert!(retryable.contains(&ErrorCode::BackendRateLimited));
    assert!(retryable.contains(&ErrorCode::BackendCrashed));
}

#[test]
fn ec09_non_retryable_codes_identified_correctly() {
    let non_retryable: Vec<ErrorCode> = ALL_CODES
        .iter()
        .copied()
        .filter(|c| !c.is_retryable())
        .collect();
    assert_eq!(non_retryable.len(), ALL_CODES.len() - 4);
    assert!(non_retryable.contains(&ErrorCode::BackendNotFound));
    assert!(non_retryable.contains(&ErrorCode::BackendAuthFailed));
    assert!(non_retryable.contains(&ErrorCode::PolicyDenied));
    assert!(non_retryable.contains(&ErrorCode::Internal));
    assert!(non_retryable.contains(&ErrorCode::ConfigInvalid));
    assert!(non_retryable.contains(&ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn ec10_unknown_error_code_string_rejected() {
    let result = serde_json::from_str::<ErrorCode>("\"unknown_error_xyz\"");
    assert!(result.is_err(), "unknown code string should fail to parse");
    let result2 = serde_json::from_str::<ErrorCode>("42");
    assert!(result2.is_err(), "numeric value should fail to parse");
    let result3 = serde_json::from_str::<ErrorCode>("null");
    assert!(result3.is_err(), "null should fail to parse");
}

// =========================================================================
// B) Error category (10 tests)
// =========================================================================

#[test]
fn cat01_every_error_category_variant_exists() {
    assert_eq!(ALL_CATEGORIES.len(), 17);
    for &cat in ALL_CATEGORIES {
        let _copy = cat;
    }
}

#[test]
fn cat02_categories_group_related_codes_correctly() {
    for &code in ALL_CODES {
        let prefix = match code.category() {
            ErrorCategory::Protocol => "protocol_",
            ErrorCategory::Backend => "backend_",
            ErrorCategory::Capability => "capability_",
            ErrorCategory::Policy => "policy_",
            ErrorCategory::Workspace => "workspace_",
            ErrorCategory::Ir => "ir_",
            ErrorCategory::Receipt => "receipt_",
            ErrorCategory::Dialect => "dialect_",
            ErrorCategory::Config => "config_",
            ErrorCategory::Mapping => "mapping_",
            ErrorCategory::Execution => "execution_",
            ErrorCategory::Contract => "contract_",
            ErrorCategory::RateLimit => "rate_limit_",
            ErrorCategory::Stream => "stream_",
            ErrorCategory::Validation => "validation_",
            ErrorCategory::Sidecar => "sidecar_",
            ErrorCategory::Internal => "internal",
        };
        assert!(
            code.as_str().starts_with(prefix),
            "{:?}: as_str '{}' does not start with '{}'",
            code,
            code.as_str(),
            prefix
        );
    }
}

#[test]
fn cat03_category_display_formatting() {
    let expected = [
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
        (ErrorCategory::RateLimit, "rate_limit"),
        (ErrorCategory::Stream, "stream"),
        (ErrorCategory::Validation, "validation"),
        (ErrorCategory::Sidecar, "sidecar"),
        (ErrorCategory::Internal, "internal"),
    ];
    for (cat, s) in expected {
        assert_eq!(cat.to_string(), s);
    }
}

#[test]
fn cat04_category_serde_roundtrip() {
    for &cat in ALL_CATEGORIES {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat, "roundtrip failed for {:?}", cat);
    }
}

#[test]
fn cat05_category_from_error_code_derivation() {
    for &code in ALL_CODES {
        let c1 = code.category();
        let c2 = code.category();
        assert_eq!(c1, c2);
        assert!(ALL_CATEGORIES.contains(&c1));
    }
}

#[test]
fn cat06_protocol_errors_category() {
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
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Protocol)
        .count();
    assert_eq!(count, 5);
}

#[test]
fn cat07_runtime_backend_errors_category() {
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
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Backend)
        .count();
    assert_eq!(count, 7);
}

#[test]
fn cat08_validation_contract_errors_category() {
    let contract_codes = [
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ];
    for code in &contract_codes {
        assert_eq!(code.category(), ErrorCategory::Contract);
    }
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Contract)
        .count();
    assert_eq!(count, 3);
}

#[test]
fn cat09_execution_and_workspace_categories() {
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
fn cat10_every_category_has_at_least_one_code() {
    for &cat in ALL_CATEGORIES {
        let count = ALL_CODES.iter().filter(|c| c.category() == cat).count();
        assert!(count >= 1, "category {:?} has zero error codes", cat);
    }
    let total: usize = ALL_CATEGORIES
        .iter()
        .map(|cat| ALL_CODES.iter().filter(|c| c.category() == *cat).count())
        .sum();
    assert_eq!(total, ALL_CODES.len());
}

// =========================================================================
// C) AbpError integration (10 tests)
// =========================================================================

#[test]
fn abp01_create_with_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30 s");
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.message, "timed out after 30 s");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp02_with_source_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(io_err);
    assert!(err.source.is_some());
    let src = std::error::Error::source(&err).unwrap();
    assert_eq!(src.to_string(), "file missing");
}

#[test]
fn abp03_downcast_source_to_original() {
    let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "deadline");
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout").with_source(io_err);
    let boxed = err.source.as_ref().unwrap();
    let io_ref = boxed.downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(io_ref.kind(), std::io::ErrorKind::TimedOut);
}

#[test]
fn abp04_display_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "write to /etc blocked");
    let display = err.to_string();
    assert!(display.contains("policy_denied"), "missing code in display");
    assert!(
        display.contains("write to /etc blocked"),
        "missing message in display"
    );
    assert_eq!(display, "[policy_denied] write to /etc blocked");
}

#[test]
fn abp05_error_trait_implementation() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let as_std_err: &dyn std::error::Error = &err;
    assert!(as_std_err.source().is_none());
    assert!(as_std_err.to_string().contains("internal"));
}

#[test]
fn abp06_send_and_sync_bounds() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AbpError>();
    let err = AbpError::new(ErrorCode::Internal, "threadsafe");
    let _boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(err);
}

#[test]
fn abp07_with_context_details() {
    let err = AbpError::new(ErrorCode::BackendRateLimited, "rate limited")
        .with_context("backend", "openai")
        .with_context("retry_after_ms", 5000)
        .with_context("nested", serde_json::json!({"a": 1}));
    assert_eq!(err.context.len(), 3);
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
    assert_eq!(err.context["retry_after_ms"], serde_json::json!(5000));
    assert_eq!(err.context["nested"], serde_json::json!({"a": 1}));
    let display = err.to_string();
    assert!(display.contains("openai"));
    assert!(display.contains("5000"));
}

#[test]
fn abp08_chain_multiple_abp_errors() {
    let inner = AbpError::new(ErrorCode::BackendTimeout, "upstream timed out");
    let outer =
        AbpError::new(ErrorCode::ExecutionToolFailed, "tool invocation failed").with_source(inner);
    assert_eq!(outer.code, ErrorCode::ExecutionToolFailed);
    let src = std::error::Error::source(&outer).unwrap();
    assert!(src.to_string().contains("backend_timeout"));
}

#[test]
fn abp09_compare_error_codes() {
    let err1 = AbpError::new(ErrorCode::BackendTimeout, "timeout 1");
    let err2 = AbpError::new(ErrorCode::BackendTimeout, "timeout 2");
    let err3 = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert_eq!(err1.code, err2.code);
    assert_ne!(err1.code, err3.code);
    assert_eq!(err1.category(), err2.category());
    assert_ne!(err1.category(), err3.category());
}

#[test]
fn abp10_to_json_serialization_via_dto() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad toml")
        .with_context("file", "backplane.toml")
        .with_source(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "parse error",
        ));
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string_pretty(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, ErrorCode::ConfigInvalid);
    assert_eq!(back.message, "bad toml");
    assert_eq!(back.context["file"], serde_json::json!("backplane.toml"));
    assert_eq!(back.source_message.as_deref(), Some("parse error"));
    let restored: AbpError = back.into();
    assert_eq!(restored.code, ErrorCode::ConfigInvalid);
    assert!(restored.source.is_none());
}

// =========================================================================
// D) Extra coverage — ErrorInfo, From conversions, edge cases
// =========================================================================

#[test]
fn extra_error_info_retryable_mirrors_code() {
    for &code in ALL_CODES {
        let info = ErrorInfo::new(code, "test");
        assert_eq!(
            info.is_retryable,
            code.is_retryable(),
            "{:?}: ErrorInfo mismatch",
            code
        );
    }
}

#[test]
fn extra_error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::IrLoweringFailed, "lowering failed");
    assert_eq!(info.to_string(), "[ir_lowering_failed] lowering failed");
}

#[test]
fn extra_from_io_error_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert!(abp.message.contains("denied"));
    assert!(abp.source.is_some());
}

#[test]
fn extra_from_serde_json_error_conversion() {
    let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let abp: AbpError = serde_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(abp.source.is_some());
}

#[test]
fn extra_abp_error_to_info_preserves_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "anthropic")
        .with_context("ms", 15000);
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "slow");
    assert!(info.is_retryable);
    assert_eq!(info.details["backend"], serde_json::json!("anthropic"));
    assert_eq!(info.details["ms"], serde_json::json!(15000));
}

#[test]
fn extra_error_info_serde_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::DialectMappingFailed, "dialect mapping")
        .with_detail("source", "claude")
        .with_detail("target", "openai");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn extra_dto_omits_null_source_message() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(
        !json.contains("source_message"),
        "null source_message should be skipped"
    );
}

#[test]
fn extra_context_deterministic_serialization() {
    let err = AbpError::new(ErrorCode::Internal, "ordered")
        .with_context("z_key", "last")
        .with_context("a_key", "first");
    let display = err.to_string();
    let a_pos = display.find("a_key").unwrap();
    let z_pos = display.find("z_key").unwrap();
    assert!(a_pos < z_pos, "context keys should be sorted");
}

#[test]
fn extra_error_code_json_matches_as_str() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "JSON mismatch for {:?}", code);
    }
}

#[test]
fn extra_abp_error_debug_impl() {
    let err =
        AbpError::new(ErrorCode::ReceiptChainBroken, "gap at index 3").with_context("index", 3);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("ReceiptChainBroken"));
    assert!(dbg.contains("gap at index 3"));
    assert!(dbg.contains("context"));
}
