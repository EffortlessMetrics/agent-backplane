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
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Comprehensive tests for the ABP error taxonomy system.
//!
//! Covers ErrorCode completeness, as_str() format, category grouping,
//! Display impls, serde roundtrips, From conversions, error chaining,
//! ErrorInfo/ErrorResponse construction, pattern matching, and stability.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error as StdError;
use std::io;

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};

// -------------------------------------------------------------------------
// Exhaustive variant list (must stay in sync with the enum)
// -------------------------------------------------------------------------

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

// =========================================================================
// 1. ErrorCode completeness: every variant has as_str(), message(), category()
// =========================================================================

#[test]
fn completeness_every_code_has_non_empty_as_str() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(!s.is_empty(), "{code:?} returned empty as_str()");
    }
}

#[test]
fn completeness_every_code_has_non_empty_message() {
    for &code in ALL_CODES {
        let m = code.message();
        assert!(!m.is_empty(), "{code:?} returned empty message()");
    }
}

#[test]
fn completeness_every_code_has_category() {
    for &code in ALL_CODES {
        let cat = code.category();
        assert!(
            ALL_CATEGORIES.contains(&cat),
            "{code:?} returned unknown category {cat:?}"
        );
    }
}

#[test]
fn completeness_variant_count_guards_against_silent_additions() {
    assert_eq!(ALL_CODES.len(), 36);
}

#[test]
fn completeness_category_count_is_13() {
    assert_eq!(ALL_CATEGORIES.len(), 13);
}

#[test]
fn completeness_every_category_has_at_least_one_code() {
    let covered: HashSet<ErrorCategory> = ALL_CODES.iter().map(|c| c.category()).collect();
    for &cat in ALL_CATEGORIES {
        assert!(covered.contains(&cat), "category {cat:?} has no codes");
    }
}

// =========================================================================
// 2. as_str() format: all return snake_case strings
// =========================================================================

#[test]
fn as_str_format_all_lowercase_and_underscores() {
    for &code in ALL_CODES {
        let s = code.as_str();
        for ch in s.chars() {
            assert!(
                ch.is_ascii_lowercase() || ch == '_' || ch.is_ascii_digit(),
                "{s} contains invalid char '{ch}'"
            );
        }
    }
}

#[test]
fn as_str_format_no_leading_or_trailing_underscores() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(!s.starts_with('_'), "{s} starts with underscore");
        assert!(!s.ends_with('_'), "{s} ends with underscore");
    }
}

#[test]
fn as_str_format_no_double_underscores() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(!s.contains("__"), "{s} contains double underscore");
    }
}

#[test]
fn as_str_format_all_values_unique() {
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
fn as_str_format_prefix_matches_category() {
    for &code in ALL_CODES {
        let s = code.as_str();
        let cat_str = code.category().to_string();
        // Every code except "internal" should start with its category prefix
        if code != ErrorCode::Internal {
            assert!(
                s.starts_with(&cat_str),
                "{s} does not start with category prefix '{cat_str}'"
            );
        }
    }
}

#[test]
fn as_str_format_serde_serializes_to_quoted_as_str() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "serde mismatch for {code:?}");
    }
}

// =========================================================================
// 3. Category grouping: each ErrorCode maps to the correct category
// =========================================================================

#[test]
fn category_protocol_codes() {
    let expected = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in expected {
        assert_eq!(code.category(), ErrorCategory::Protocol, "{code:?}");
    }
}

#[test]
fn category_backend_codes() {
    let expected = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
    ];
    for code in expected {
        assert_eq!(code.category(), ErrorCategory::Backend, "{code:?}");
    }
}

#[test]
fn category_mapping_codes() {
    let expected = [
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ];
    for code in expected {
        assert_eq!(code.category(), ErrorCategory::Mapping, "{code:?}");
    }
}

#[test]
fn category_execution_codes() {
    let expected = [
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
    ];
    for code in expected {
        assert_eq!(code.category(), ErrorCategory::Execution, "{code:?}");
    }
}

#[test]
fn category_contract_codes() {
    let expected = [
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ];
    for code in expected {
        assert_eq!(code.category(), ErrorCategory::Contract, "{code:?}");
    }
}

#[test]
fn category_capability_codes() {
    let expected = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in expected {
        assert_eq!(code.category(), ErrorCategory::Capability, "{code:?}");
    }
}

#[test]
fn category_policy_codes() {
    for code in [ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid] {
        assert_eq!(code.category(), ErrorCategory::Policy, "{code:?}");
    }
}

#[test]
fn category_workspace_codes() {
    for code in [
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
    ] {
        assert_eq!(code.category(), ErrorCategory::Workspace, "{code:?}");
    }
}

#[test]
fn category_ir_codes() {
    for code in [ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid] {
        assert_eq!(code.category(), ErrorCategory::Ir, "{code:?}");
    }
}

#[test]
fn category_receipt_codes() {
    for code in [
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ] {
        assert_eq!(code.category(), ErrorCategory::Receipt, "{code:?}");
    }
}

#[test]
fn category_dialect_codes() {
    for code in [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed] {
        assert_eq!(code.category(), ErrorCategory::Dialect, "{code:?}");
    }
}

#[test]
fn category_config_code() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn category_internal_code() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn category_grouping_correct_cardinalities() {
    let mut counts: HashMap<ErrorCategory, usize> = HashMap::new();
    for &code in ALL_CODES {
        *counts.entry(code.category()).or_default() += 1;
    }
    assert_eq!(counts[&ErrorCategory::Protocol], 5);
    assert_eq!(counts[&ErrorCategory::Backend], 7);
    assert_eq!(counts[&ErrorCategory::Mapping], 4);
    assert_eq!(counts[&ErrorCategory::Execution], 3);
    assert_eq!(counts[&ErrorCategory::Contract], 3);
    assert_eq!(counts[&ErrorCategory::Capability], 2);
    assert_eq!(counts[&ErrorCategory::Policy], 2);
    assert_eq!(counts[&ErrorCategory::Workspace], 2);
    assert_eq!(counts[&ErrorCategory::Ir], 2);
    assert_eq!(counts[&ErrorCategory::Receipt], 2);
    assert_eq!(counts[&ErrorCategory::Dialect], 2);
    assert_eq!(counts[&ErrorCategory::Config], 1);
    assert_eq!(counts[&ErrorCategory::Internal], 1);
}

// =========================================================================
// 4. Display impl: human-readable error messages
// =========================================================================

#[test]
fn display_error_code_returns_message_not_as_str() {
    for &code in ALL_CODES {
        let display = code.to_string();
        let as_str = code.as_str();
        assert_ne!(
            display, as_str,
            "{code:?}: Display should be human text, not code string"
        );
    }
}

#[test]
fn display_error_code_equals_message() {
    for &code in ALL_CODES {
        assert_eq!(code.to_string(), code.message(), "{code:?}");
    }
}

#[test]
fn display_error_category_all_lowercase() {
    for &cat in ALL_CATEGORIES {
        let s = cat.to_string();
        assert_eq!(s, s.to_lowercase(), "{cat:?} display is not lowercase");
    }
}

#[test]
fn display_abp_error_format_bracket_code_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "access denied");
    assert_eq!(err.to_string(), "[policy_denied] access denied");
}

#[test]
fn display_abp_error_includes_context_json() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow").with_context("timeout_ms", 30_000);
    let s = err.to_string();
    assert!(s.starts_with("[backend_timeout] slow"));
    assert!(s.contains("30000"));
}

#[test]
fn display_abp_error_no_trailing_brace_without_context() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let s = err.to_string();
    assert!(!s.contains('{'));
}

#[test]
fn display_error_info_format_bracket_code_message() {
    let info = ErrorInfo::new(ErrorCode::DialectUnknown, "unknown dialect xyz");
    assert_eq!(info.to_string(), "[dialect_unknown] unknown dialect xyz");
}

#[test]
fn display_error_code_messages_are_lowercase_sentences() {
    for &code in ALL_CODES {
        let msg = code.message();
        let first = msg.chars().next().unwrap();
        assert!(
            first.is_ascii_lowercase() || first == 'I',
            "{code:?}: message '{msg}' should start lowercase (except IR)"
        );
    }
}

// =========================================================================
// 5. Serde roundtrip: serialize/deserialize all variants
// =========================================================================

#[test]
fn serde_roundtrip_all_error_codes() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "roundtrip failed for {code:?}");
    }
}

#[test]
fn serde_roundtrip_all_error_categories() {
    for &cat in ALL_CATEGORIES {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat, "roundtrip failed for {cat:?}");
    }
}

#[test]
fn serde_roundtrip_error_info_with_details() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited")
        .with_detail("retry_after_ms", 5000)
        .with_detail("backend", "openai");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn serde_roundtrip_error_info_empty_details() {
    let info = ErrorInfo::new(ErrorCode::Internal, "bare");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn serde_roundtrip_abp_error_dto_with_context() {
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed")
        .with_context("path", "/tmp/ws");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn serde_roundtrip_abp_error_dto_with_source_message() {
    let src = io::Error::new(io::ErrorKind::NotFound, "missing file");
    let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging failed").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_message.as_deref(), Some("missing file"));
}

#[test]
fn serde_error_code_deserialization_from_string() {
    let code: ErrorCode = serde_json::from_str("\"backend_timeout\"").unwrap();
    assert_eq!(code, ErrorCode::BackendTimeout);
}

#[test]
fn serde_error_code_unknown_variant_rejected() {
    let result = serde_json::from_str::<ErrorCode>("\"nonexistent_code\"");
    assert!(result.is_err());
}

#[test]
fn serde_error_category_deserialization_from_string() {
    let cat: ErrorCategory = serde_json::from_str("\"protocol\"").unwrap();
    assert_eq!(cat, ErrorCategory::Protocol);
}

#[test]
fn serde_dto_omits_source_message_when_none() {
    let err = AbpError::new(ErrorCode::Internal, "no source");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

// =========================================================================
// 6. From conversions: test From impls
// =========================================================================

#[test]
fn from_io_error_maps_to_internal_code() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
}

#[test]
fn from_io_error_preserves_message() {
    let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let abp: AbpError = io_err.into();
    assert!(abp.message.contains("access denied"));
}

#[test]
fn from_io_error_chains_source() {
    let io_err = io::Error::new(io::ErrorKind::TimedOut, "timed out");
    let abp: AbpError = io_err.into();
    assert!(abp.source.is_some());
    let src = StdError::source(&abp).unwrap();
    assert_eq!(src.to_string(), "timed out");
}

#[test]
fn from_io_error_various_error_kinds() {
    let kinds = [
        io::ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::ConnectionRefused,
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::AddrInUse,
    ];
    for kind in kinds {
        let io_err = io::Error::new(kind, "test");
        let abp: AbpError = io_err.into();
        assert_eq!(abp.code, ErrorCode::Internal);
    }
}

#[test]
fn from_serde_json_error_maps_to_protocol_invalid_envelope() {
    let json_err = serde_json::from_str::<serde_json::Value>("not valid json").unwrap_err();
    let abp: AbpError = json_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
}

#[test]
fn from_serde_json_error_chains_source() {
    let json_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
    let abp: AbpError = json_err.into();
    assert!(abp.source.is_some());
}

#[test]
fn from_serde_json_error_message_is_descriptive() {
    let json_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let abp: AbpError = json_err.into();
    assert!(!abp.message.is_empty());
}

#[test]
fn from_abp_error_dto_to_abp_error() {
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad toml".into(),
        context: BTreeMap::new(),
        source_message: Some("parse error at line 3".into()),
        location: None,
        cause_chain: Vec::new(),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.message, "bad toml");
    // Source is lost: DTO cannot carry opaque Box<dyn Error>
    assert!(err.source.is_none());
}

#[test]
fn from_abp_error_ref_to_dto() {
    let err =
        AbpError::new(ErrorCode::ReceiptChainBroken, "gap at index 5").with_context("index", 5);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::ReceiptChainBroken);
    assert_eq!(dto.context["index"], serde_json::json!(5));
}

// =========================================================================
// 7. Error chain: errors can wrap source errors
// =========================================================================

#[test]
fn error_chain_with_source_via_std_trait() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "disk missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(inner);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "disk missing");
}

#[test]
fn error_chain_source_is_none_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "standalone");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn error_chain_nested_two_levels() {
    let level0 = io::Error::new(io::ErrorKind::Other, "root cause");
    let level1 = AbpError::new(ErrorCode::BackendCrashed, "sidecar died").with_source(level0);
    // Verify the chain exists
    let src = StdError::source(&level1).unwrap();
    assert_eq!(src.to_string(), "root cause");
}

#[test]
fn error_chain_source_preserved_in_debug() {
    let inner = io::Error::new(io::ErrorKind::Other, "underlying");
    let err = AbpError::new(ErrorCode::Internal, "outer").with_source(inner);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("underlying"));
}

#[test]
fn error_chain_source_not_shown_in_display() {
    let inner = io::Error::new(io::ErrorKind::Other, "hidden");
    let err = AbpError::new(ErrorCode::Internal, "visible").with_source(inner);
    let disp = err.to_string();
    assert!(!disp.contains("hidden"));
    assert!(disp.contains("visible"));
}

// =========================================================================
// 8. Error response construction: build error responses from ErrorCode
// =========================================================================

#[test]
fn error_response_error_info_from_code() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out after 30s");
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "timed out after 30s");
    assert!(info.is_retryable);
}

#[test]
fn error_response_error_info_with_detail_chain() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited")
        .with_detail("retry_after_ms", 5000)
        .with_detail("backend", "anthropic")
        .with_detail("model", "claude-3");
    assert_eq!(info.details.len(), 3);
}

#[test]
fn error_response_error_info_retryable_inferred() {
    assert!(ErrorInfo::new(ErrorCode::BackendTimeout, "t").is_retryable);
    assert!(ErrorInfo::new(ErrorCode::BackendUnavailable, "u").is_retryable);
    assert!(!ErrorInfo::new(ErrorCode::PolicyDenied, "d").is_retryable);
    assert!(!ErrorInfo::new(ErrorCode::Internal, "i").is_retryable);
}

#[test]
fn error_response_abp_error_to_info_preserves_context() {
    let err = AbpError::new(ErrorCode::ContractSchemaViolation, "schema error")
        .with_context("field", "work_order.task")
        .with_context("expected", "string");
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::ContractSchemaViolation);
    assert_eq!(info.details.len(), 2);
    assert_eq!(info.details["field"], serde_json::json!("work_order.task"));
}

#[test]
fn error_response_abp_error_to_info_retryable_propagates() {
    let retryable = AbpError::new(ErrorCode::BackendCrashed, "crash");
    assert!(retryable.to_info().is_retryable);

    let not_retryable = AbpError::new(ErrorCode::IrInvalid, "bad ir");
    assert!(!not_retryable.to_info().is_retryable);
}

#[test]
fn error_response_dto_from_abp_error_captures_source_message() {
    let src = io::Error::new(io::ErrorKind::Other, "root cause detail");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crashed").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("root cause detail"));
}

#[test]
fn error_response_dto_serializes_to_json_object() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied by policy")
        .with_context("rule", "no_shell_exec");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["code"], "policy_denied");
    assert_eq!(parsed["message"], "denied by policy");
    assert_eq!(parsed["context"]["rule"], "no_shell_exec");
}

#[test]
fn error_response_error_info_serializes_to_json_object() {
    let info = ErrorInfo::new(ErrorCode::CapabilityUnsupported, "no streaming")
        .with_detail("capability", "streaming");
    let json = serde_json::to_string(&info).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["code"], "capability_unsupported");
    assert_eq!(parsed["details"]["capability"], "streaming");
}

// =========================================================================
// 9. Error matching: pattern match on categories for error handling
// =========================================================================

fn classify_error(err: &AbpError) -> &'static str {
    match err.category() {
        ErrorCategory::Backend if err.is_retryable() => "retry",
        ErrorCategory::Backend => "backend_fatal",
        ErrorCategory::Policy => "forbidden",
        ErrorCategory::Protocol => "bad_request",
        ErrorCategory::Contract => "contract_violation",
        _ => "other",
    }
}

#[test]
fn match_retryable_backend_error() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
    assert_eq!(classify_error(&err), "retry");
}

#[test]
fn match_fatal_backend_error() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "not found");
    assert_eq!(classify_error(&err), "backend_fatal");
}

#[test]
fn match_policy_error() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert_eq!(classify_error(&err), "forbidden");
}

#[test]
fn match_protocol_error() {
    let err = AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "bad envelope");
    assert_eq!(classify_error(&err), "bad_request");
}

#[test]
fn match_contract_error() {
    let err = AbpError::new(ErrorCode::ContractSchemaViolation, "violation");
    assert_eq!(classify_error(&err), "contract_violation");
}

#[test]
fn match_other_category_error() {
    let err = AbpError::new(ErrorCode::IrLoweringFailed, "lowering");
    assert_eq!(classify_error(&err), "other");
}

#[test]
fn match_category_based_filtering() {
    let errors = vec![
        AbpError::new(ErrorCode::BackendTimeout, "t1"),
        AbpError::new(ErrorCode::PolicyDenied, "p1"),
        AbpError::new(ErrorCode::BackendRateLimited, "t2"),
        AbpError::new(ErrorCode::Internal, "i1"),
    ];
    let retryable: Vec<_> = errors.iter().filter(|e| e.is_retryable()).collect();
    assert_eq!(retryable.len(), 2);
}

#[test]
fn match_category_grouping_for_metrics() {
    let errors = vec![
        AbpError::new(ErrorCode::BackendTimeout, "a"),
        AbpError::new(ErrorCode::BackendCrashed, "b"),
        AbpError::new(ErrorCode::PolicyDenied, "c"),
        AbpError::new(ErrorCode::ProtocolMissingRefId, "d"),
    ];
    let mut by_category: HashMap<String, usize> = HashMap::new();
    for err in &errors {
        *by_category.entry(err.category().to_string()).or_default() += 1;
    }
    assert_eq!(by_category["backend"], 2);
    assert_eq!(by_category["policy"], 1);
    assert_eq!(by_category["protocol"], 1);
}

// =========================================================================
// 10. Stability: as_str() values are stable (breaking change if they change)
// =========================================================================

#[test]
fn stability_protocol_as_str_values() {
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
fn stability_mapping_as_str_values() {
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
fn stability_backend_as_str_values() {
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
fn stability_execution_as_str_values() {
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
}

#[test]
fn stability_contract_as_str_values() {
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
}

#[test]
fn stability_capability_as_str_values() {
    assert_eq!(
        ErrorCode::CapabilityUnsupported.as_str(),
        "capability_unsupported"
    );
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.as_str(),
        "capability_emulation_failed"
    );
}

#[test]
fn stability_policy_as_str_values() {
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(ErrorCode::PolicyInvalid.as_str(), "policy_invalid");
}

#[test]
fn stability_workspace_as_str_values() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.as_str(),
        "workspace_init_failed"
    );
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.as_str(),
        "workspace_staging_failed"
    );
}

#[test]
fn stability_ir_as_str_values() {
    assert_eq!(ErrorCode::IrLoweringFailed.as_str(), "ir_lowering_failed");
    assert_eq!(ErrorCode::IrInvalid.as_str(), "ir_invalid");
}

#[test]
fn stability_receipt_as_str_values() {
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.as_str(),
        "receipt_hash_mismatch"
    );
    assert_eq!(
        ErrorCode::ReceiptChainBroken.as_str(),
        "receipt_chain_broken"
    );
}

#[test]
fn stability_dialect_as_str_values() {
    assert_eq!(ErrorCode::DialectUnknown.as_str(), "dialect_unknown");
    assert_eq!(
        ErrorCode::DialectMappingFailed.as_str(),
        "dialect_mapping_failed"
    );
}

#[test]
fn stability_config_as_str_value() {
    assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
}

#[test]
fn stability_internal_as_str_value() {
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
}

#[test]
fn stability_message_values_are_stable() {
    // Spot-check key messages that external consumers may depend on.
    assert_eq!(ErrorCode::BackendTimeout.message(), "backend timed out");
    assert_eq!(
        ErrorCode::PolicyDenied.message(),
        "policy rule denied the operation"
    );
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.message(),
        "envelope failed to parse or has invalid fields"
    );
    assert_eq!(ErrorCode::Internal.message(), "unexpected internal error");
}

#[test]
fn stability_category_display_values() {
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

// =========================================================================
// Bonus: trait impl verification and edge cases
// =========================================================================

#[test]
fn error_code_derives_clone_copy_eq_hash() {
    let code = ErrorCode::BackendTimeout;
    #[allow(clippy::clone_on_copy)]
    let cloned = code.clone();
    let copied = code;
    assert_eq!(code, cloned);
    assert_eq!(code, copied);
    let mut set = HashSet::new();
    set.insert(code);
    set.insert(cloned);
    assert_eq!(set.len(), 1);
}

#[test]
fn error_category_derives_clone_copy_eq_hash() {
    let cat = ErrorCategory::Protocol;
    #[allow(clippy::clone_on_copy)]
    let cloned = cat.clone();
    let copied = cat;
    assert_eq!(cat, cloned);
    assert_eq!(cat, copied);
}

#[test]
fn error_info_derives_clone_partial_eq() {
    let info = ErrorInfo::new(ErrorCode::Internal, "test").with_detail("key", "val");
    let cloned = info.clone();
    assert_eq!(info, cloned);
}

#[test]
fn abp_error_debug_omits_empty_context() {
    let err = AbpError::new(ErrorCode::Internal, "x");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("context"));
}

#[test]
fn abp_error_debug_shows_context_when_present() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("k", "v");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
}

#[test]
fn abp_error_context_deterministic_order() {
    let err = AbpError::new(ErrorCode::Internal, "msg")
        .with_context("z_key", "z")
        .with_context("a_key", "a");
    let s = err.to_string();
    let a_pos = s.find("a_key").unwrap();
    let z_pos = s.find("z_key").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn retryable_exactly_four_codes() {
    let retryable: Vec<_> = ALL_CODES.iter().filter(|c| c.is_retryable()).collect();
    assert_eq!(retryable.len(), 4);
}

#[test]
fn abp_error_is_retryable_delegates() {
    assert!(AbpError::new(ErrorCode::BackendUnavailable, "u").is_retryable());
    assert!(!AbpError::new(ErrorCode::BackendNotFound, "nf").is_retryable());
}

#[test]
fn abp_error_category_delegates() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unknown");
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

#[test]
fn error_info_detail_with_various_types() {
    let info = ErrorInfo::new(ErrorCode::Internal, "x")
        .with_detail("int", 42)
        .with_detail("float", 3.14)
        .with_detail("bool", true)
        .with_detail("null", serde_json::Value::Null)
        .with_detail("array", vec![1, 2, 3])
        .with_detail("string", "hello");
    assert_eq!(info.details.len(), 6);
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
