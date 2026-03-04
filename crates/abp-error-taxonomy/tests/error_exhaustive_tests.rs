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
//! Exhaustive tests for the ABP error taxonomy.
//!
//! Covers: ErrorCode coverage (unique codes, categories, display messages,
//! snake_case serialization, serde roundtrips), ErrorCategory coverage
//! (descriptions, expected codes, serde roundtrips), error conversions
//! (io::Error, serde_json::Error, String, Box<dyn Error>), message quality
//! checks (no empty messages, no duplicate codes, correct categories),
//! and JSON schema validation via schemars.

use abp_error_taxonomy::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use std::collections::HashSet;

// =========================================================================
// Helpers
// =========================================================================

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

/// Maps each category to the expected set of error code `as_str()` prefixes.
fn expected_prefix_for_category(cat: ErrorCategory) -> &'static str {
    match cat {
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
        ErrorCategory::Internal => "internal",
    }
}

// =========================================================================
// 1. ErrorCode coverage — unique codes
// =========================================================================

#[test]
fn error_code_as_str_values_are_all_unique() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        assert!(
            seen.insert(code.as_str()),
            "duplicate as_str: {}",
            code.as_str()
        );
    }
}

#[test]
fn error_code_debug_representations_are_all_unique() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        let dbg = format!("{code:?}");
        assert!(seen.insert(dbg.clone()), "duplicate Debug repr: {dbg}");
    }
}

#[test]
fn error_code_messages_are_all_unique() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        let msg = code.message();
        assert!(seen.insert(msg), "duplicate message: {msg}");
    }
}

#[test]
fn error_code_variant_count_is_36() {
    assert_eq!(ALL_CODES.len(), 36, "ErrorCode variant count changed");
}

// =========================================================================
// 2. ErrorCode — every variant has a category
// =========================================================================

#[test]
fn every_error_code_maps_to_a_known_category() {
    let known: HashSet<ErrorCategory> = ALL_CATEGORIES.iter().copied().collect();
    for code in ALL_CODES {
        assert!(
            known.contains(&code.category()),
            "{:?} maps to unknown category {:?}",
            code,
            code.category()
        );
    }
}

#[test]
fn error_code_category_is_deterministic() {
    for code in ALL_CODES {
        let c1 = code.category();
        let c2 = code.category();
        let c3 = code.category();
        assert_eq!(c1, c2);
        assert_eq!(c2, c3);
    }
}

// =========================================================================
// 3. ErrorCode — non-empty display messages
// =========================================================================

#[test]
fn every_error_code_has_nonempty_message() {
    for code in ALL_CODES {
        let msg = code.message();
        assert!(!msg.is_empty(), "{:?} has an empty message() return", code);
    }
}

#[test]
fn every_error_code_display_is_nonempty() {
    for code in ALL_CODES {
        let display = code.to_string();
        assert!(
            !display.is_empty(),
            "{:?} has an empty Display output",
            code
        );
    }
}

#[test]
fn error_code_display_equals_message() {
    for code in ALL_CODES {
        assert_eq!(
            code.to_string(),
            code.message(),
            "{:?}: Display and message() diverge",
            code
        );
    }
}

// =========================================================================
// 4. ErrorCode — serializes to snake_case JSON
// =========================================================================

#[test]
fn every_error_code_serializes_to_snake_case_json() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let inner = json.trim_matches('"');
        assert!(
            inner.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{:?} serialized to non-snake_case: {inner}",
            code
        );
    }
}

#[test]
fn error_code_json_matches_as_str() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "JSON mismatch for {:?}", code);
    }
}

// =========================================================================
// 5. ErrorCode — serde roundtrips
// =========================================================================

#[test]
fn every_error_code_survives_json_roundtrip() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *code, "roundtrip failed for {:?}", code);
    }
}

#[test]
fn error_code_rejects_unknown_variant() {
    let result = serde_json::from_str::<ErrorCode>("\"totally_bogus_code\"");
    assert!(result.is_err());
}

#[test]
fn error_code_rejects_numeric_json() {
    let result = serde_json::from_str::<ErrorCode>("42");
    assert!(result.is_err());
}

#[test]
fn error_code_rejects_null_json() {
    let result = serde_json::from_str::<ErrorCode>("null");
    assert!(result.is_err());
}

// =========================================================================
// 6. ErrorCategory — description via Display
// =========================================================================

#[test]
fn every_category_has_nonempty_display() {
    for cat in ALL_CATEGORIES {
        let display = cat.to_string();
        assert!(!display.is_empty(), "{:?} has empty Display output", cat);
    }
}

#[test]
fn category_display_is_lowercase_ascii() {
    for cat in ALL_CATEGORIES {
        let display = cat.to_string();
        assert!(
            display.chars().all(|c| c.is_ascii_lowercase()),
            "{:?} display is not lowercase ASCII: {display}",
            cat
        );
    }
}

#[test]
fn category_display_values_are_unique() {
    let mut seen = HashSet::new();
    for cat in ALL_CATEGORIES {
        let display = cat.to_string();
        assert!(
            seen.insert(display.clone()),
            "duplicate category display: {display}"
        );
    }
}

// =========================================================================
// 7. ErrorCategory — contains expected error codes
// =========================================================================

#[test]
fn every_category_has_at_least_one_error_code() {
    for cat in ALL_CATEGORIES {
        let count = ALL_CODES.iter().filter(|c| c.category() == *cat).count();
        assert!(count >= 1, "category {:?} has zero error codes", cat);
    }
}

#[test]
fn error_code_prefix_matches_category() {
    for code in ALL_CODES {
        let cat = code.category();
        let prefix = expected_prefix_for_category(cat);
        let as_str = code.as_str();
        assert!(
            as_str.starts_with(prefix),
            "{:?} (as_str={as_str}) should start with prefix {prefix} for category {:?}",
            code,
            cat
        );
    }
}

#[test]
fn category_code_counts_sum_to_total() {
    let total: usize = ALL_CATEGORIES
        .iter()
        .map(|cat| ALL_CODES.iter().filter(|c| c.category() == *cat).count())
        .sum();
    assert_eq!(total, ALL_CODES.len());
}

#[test]
fn backend_category_has_seven_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Backend)
        .count();
    assert_eq!(count, 7);
}

#[test]
fn protocol_category_has_five_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Protocol)
        .count();
    assert_eq!(count, 5);
}

// =========================================================================
// 8. ErrorCategory — serde roundtrips
// =========================================================================

#[test]
fn every_category_survives_json_roundtrip() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *cat, "roundtrip failed for {:?}", cat);
    }
}

#[test]
fn category_serializes_to_snake_case() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let inner = json.trim_matches('"');
        assert!(
            inner.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{:?} serialized to non-snake_case: {inner}",
            cat
        );
    }
}

#[test]
fn category_rejects_unknown_variant() {
    let result = serde_json::from_str::<ErrorCategory>("\"nonexistent\"");
    assert!(result.is_err());
}

#[test]
fn category_count_is_13() {
    assert_eq!(
        ALL_CATEGORIES.len(),
        13,
        "ErrorCategory variant count changed"
    );
}

// =========================================================================
// 9. Error conversion — From<io::Error>
// =========================================================================

#[test]
fn from_io_error_produces_internal_code() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert!(abp.message.contains("file missing"));
}

#[test]
fn from_io_error_preserves_source_chain() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access");
    let abp: AbpError = io_err.into();
    let src = std::error::Error::source(&abp).expect("should have a source");
    assert_eq!(src.to_string(), "no access");
}

#[test]
fn from_io_error_source_is_downcastable_to_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "deadline");
    let abp: AbpError = io_err.into();
    let boxed = abp.source.as_ref().unwrap();
    let io_ref = boxed.downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(io_ref.kind(), std::io::ErrorKind::TimedOut);
}

// =========================================================================
// 10. Error conversion — From<serde_json::Error>
// =========================================================================

#[test]
fn from_serde_json_error_produces_protocol_code() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{{bad").unwrap_err();
    let abp: AbpError = serde_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
}

#[test]
fn from_serde_json_error_preserves_source() {
    let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let abp: AbpError = serde_err.into();
    assert!(std::error::Error::source(&abp).is_some());
}

#[test]
fn from_serde_json_error_message_is_descriptive() {
    let serde_err = serde_json::from_str::<Vec<i32>>("\"wrong type\"").unwrap_err();
    let abp: AbpError = serde_err.into();
    assert!(!abp.message.is_empty());
}

// =========================================================================
// 11. Error conversion — from String (via AbpError::new)
// =========================================================================

#[test]
fn abp_error_new_accepts_string_owned() {
    let msg = String::from("dynamic error message");
    let err = AbpError::new(ErrorCode::ConfigInvalid, msg);
    assert_eq!(err.message, "dynamic error message");
}

#[test]
fn abp_error_new_accepts_str_ref() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "static str");
    assert_eq!(err.message, "static str");
}

#[test]
fn abp_error_new_accepts_format_string() {
    let backend = "openai";
    let err = AbpError::new(
        ErrorCode::BackendNotFound,
        format!("backend '{backend}' not found"),
    );
    assert!(err.message.contains("openai"));
}

// =========================================================================
// 12. Error conversion — into Box<dyn Error> (compatible with anyhow-style usage)
// =========================================================================

#[test]
fn abp_error_into_boxed_dyn_error() {
    let err = AbpError::new(ErrorCode::Internal, "boxed");
    let boxed: Box<dyn std::error::Error> = Box::new(err);
    assert!(boxed.to_string().contains("internal"));
}

#[test]
fn abp_error_into_boxed_dyn_error_send_sync() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout");
    let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(err);
    assert!(boxed.to_string().contains("backend_timeout"));
}

#[test]
fn abp_error_usable_as_result_err() {
    fn fallible() -> Result<(), AbpError> {
        Err(AbpError::new(ErrorCode::PolicyDenied, "denied"))
    }
    let result = fallible();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::PolicyDenied);
}

// =========================================================================
// 13. Error messages — quality checks
// =========================================================================

#[test]
fn no_error_message_starts_or_ends_with_whitespace() {
    for code in ALL_CODES {
        let msg = code.message();
        assert_eq!(
            msg,
            msg.trim(),
            "{:?} message has leading/trailing whitespace",
            code
        );
    }
}

#[test]
fn no_error_message_is_exact_copy_of_code_string() {
    // Messages should be human-readable, not identical to the machine code.
    for code in ALL_CODES {
        let msg = code.message();
        let as_str = code.as_str();
        assert_ne!(
            msg, as_str,
            "{:?} message is identical to its code string",
            code
        );
    }
}

#[test]
fn all_error_messages_start_lowercase() {
    // Convention: messages start lowercase (sentence fragment style).
    for code in ALL_CODES {
        let msg = code.message();
        let first = msg.chars().next().unwrap();
        // Allow "IR" as a special case (acronym).
        if !msg.starts_with("IR") {
            assert!(
                first.is_ascii_lowercase(),
                "{:?} message starts uppercase: {msg}",
                code
            );
        }
    }
}

#[test]
fn no_error_message_ends_with_period() {
    for code in ALL_CODES {
        let msg = code.message();
        assert!(
            !msg.ends_with('.'),
            "{:?} message ends with period: {msg}",
            code
        );
    }
}

// =========================================================================
// 14. Category assignments correctness
// =========================================================================

#[test]
fn only_backend_codes_are_retryable() {
    for code in ALL_CODES {
        if code.is_retryable() {
            assert_eq!(
                code.category(),
                ErrorCategory::Backend,
                "retryable code {:?} is not in Backend category",
                code
            );
        }
    }
}

#[test]
fn retryable_set_is_exactly_four() {
    let retryable: Vec<_> = ALL_CODES.iter().filter(|c| c.is_retryable()).collect();
    assert_eq!(retryable.len(), 4);
}

#[test]
fn non_retryable_backend_codes_exist() {
    let non_retryable_backend: Vec<_> = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Backend && !c.is_retryable())
        .collect();
    assert_eq!(
        non_retryable_backend.len(),
        3,
        "expected BackendNotFound, BackendAuthFailed, BackendModelNotFound"
    );
}

#[test]
fn error_info_retryable_mirrors_code() {
    for code in ALL_CODES {
        let info = ErrorInfo::new(*code, "test");
        assert_eq!(
            info.is_retryable,
            code.is_retryable(),
            "{:?}: ErrorInfo.is_retryable != ErrorCode.is_retryable()",
            code
        );
    }
}

#[test]
fn abp_error_retryable_mirrors_code() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, "test");
        assert_eq!(
            err.is_retryable(),
            code.is_retryable(),
            "{:?}: AbpError.is_retryable() != ErrorCode.is_retryable()",
            code
        );
    }
}

#[test]
fn abp_error_category_mirrors_code() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, "test");
        assert_eq!(
            err.category(),
            code.category(),
            "{:?}: AbpError.category() != ErrorCode.category()",
            code
        );
    }
}

// =========================================================================
// 15. Display output matches expectations
// =========================================================================

#[test]
fn abp_error_display_format_without_context() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "access denied");
    assert_eq!(err.to_string(), "[policy_denied] access denied");
}

#[test]
fn abp_error_display_format_with_context_includes_json() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow").with_context("backend", "anthropic");
    let display = err.to_string();
    assert!(display.starts_with("[backend_timeout] slow"));
    assert!(display.contains("anthropic"));
}

#[test]
fn error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::WorkspaceInitFailed, "init failed");
    assert_eq!(info.to_string(), "[workspace_init_failed] init failed");
}

#[test]
fn dto_roundtrip_preserves_all_fields() {
    let err = AbpError::new(ErrorCode::BackendRateLimited, "rate limited")
        .with_context("retry_after_ms", 5000)
        .with_context("backend", "openai");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();

    assert_eq!(back.code, ErrorCode::BackendRateLimited);
    assert_eq!(back.message, "rate limited");
    assert_eq!(back.context["retry_after_ms"], serde_json::json!(5000));
    assert_eq!(back.context["backend"], serde_json::json!("openai"));
    assert!(back.source_message.is_none());
}

#[test]
fn dto_with_source_message_roundtrips() {
    let src = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();

    assert_eq!(back.source_message.as_deref(), Some("pipe broke"));
}

// =========================================================================
// 16. Schema validation — ErrorCode JSON schema
// =========================================================================

#[test]
fn error_code_json_schema_is_valid() {
    let schema = schemars::schema_for!(ErrorCode);
    let schema_json = serde_json::to_value(&schema).unwrap();

    // The schema should be a valid JSON object with expected top-level keys.
    assert!(schema_json.is_object());
    let obj = schema_json.as_object().unwrap();
    assert!(
        obj.contains_key("$schema") || obj.contains_key("oneOf") || obj.contains_key("enum"),
        "schema should contain expected keys"
    );
}

#[test]
fn error_code_schema_enumerates_all_variants() {
    let schema = schemars::schema_for!(ErrorCode);
    let schema_str = serde_json::to_string(&schema).unwrap();

    // Every error code's as_str representation should appear in the schema.
    for code in ALL_CODES {
        assert!(
            schema_str.contains(code.as_str()),
            "{:?} (as_str={}) not found in JSON schema",
            code,
            code.as_str()
        );
    }
}

#[test]
fn error_code_schema_has_correct_variant_count() {
    let schema = schemars::schema_for!(ErrorCode);
    let schema_str = serde_json::to_string(&schema).unwrap();

    // Count how many of our known codes appear in the schema.
    let found = ALL_CODES
        .iter()
        .filter(|code| schema_str.contains(code.as_str()))
        .count();
    assert_eq!(found, ALL_CODES.len());
}

#[test]
fn error_code_schema_serializes_to_valid_json() {
    let schema = schemars::schema_for!(ErrorCode);
    let json_str = serde_json::to_string_pretty(&schema).unwrap();
    // Ensure it round-trips as valid JSON.
    let _: serde_json::Value = serde_json::from_str(&json_str).unwrap();
}

#[test]
fn error_code_all_serialized_values_validate_against_schema() {
    let schema = schemars::schema_for!(ErrorCode);
    let schema_json = serde_json::to_string(&schema).unwrap();

    // Each code should serialize to a value that appears in the schema.
    for code in ALL_CODES {
        let serialized = serde_json::to_string(code).unwrap();
        let value_str = serialized.trim_matches('"');
        assert!(
            schema_json.contains(value_str),
            "serialized value {value_str} for {:?} not in schema",
            code
        );
    }
}
