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
#![allow(clippy::useless_vec)]
//! Deep tests for the `abp-error` crate.
//!
//! Focuses on edge cases, trait compliance, serialization subtleties,
//! error chain propagation, classification boundaries, and Display/Debug
//! formatting that existing tests do not fully exercise.

use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;
use std::io;

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};

// -------------------------------------------------------------------------
// Exhaustive variant list
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
// 1. std::error::Error trait compliance
// =========================================================================

#[test]
fn abp_error_implements_std_error() {
    let err = AbpError::new(ErrorCode::Internal, "test");
    let _: &dyn StdError = &err;
}

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
fn abp_error_source_returns_none_without_source() {
    let err = AbpError::new(ErrorCode::Internal, "no cause");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn abp_error_source_returns_some_with_source() {
    let inner = io::Error::new(io::ErrorKind::Other, "inner");
    let err = AbpError::new(ErrorCode::Internal, "outer").with_source(inner);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "inner");
}

#[test]
fn abp_error_two_level_source_chain() {
    let level0 = io::Error::new(io::ErrorKind::NotFound, "root cause");
    let level1 = AbpError::new(ErrorCode::WorkspaceInitFailed, "mid").with_source(level0);
    let level2 = AbpError::new(ErrorCode::Internal, "top").with_source(level1);

    let src1 = StdError::source(&level2).unwrap();
    assert!(src1.to_string().contains("workspace_init_failed"));
    let src0 = StdError::source(src1).unwrap();
    assert_eq!(src0.to_string(), "root cause");
}

#[test]
fn abp_error_source_chain_terminates() {
    let err = AbpError::new(ErrorCode::Internal, "leaf");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn all_error_codes_satisfy_std_error_via_abp_error() {
    for &code in ALL_CODES {
        let err = AbpError::new(code, "test message");
        let dyn_err: &dyn StdError = &err;
        assert!(!dyn_err.to_string().is_empty());
    }
}

// =========================================================================
// 2. ErrorCode as_str format — snake_case invariants
// =========================================================================

#[test]
fn as_str_never_contains_uppercase() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(
            !s.chars().any(|c| c.is_uppercase()),
            "{s} contains uppercase"
        );
    }
}

#[test]
fn as_str_never_starts_or_ends_with_underscore() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(!s.starts_with('_'), "{s} starts with underscore");
        assert!(!s.ends_with('_'), "{s} ends with underscore");
    }
}

#[test]
fn as_str_never_has_consecutive_underscores() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(!s.contains("__"), "{s} has consecutive underscores");
    }
}

#[test]
fn as_str_only_ascii_lowercase_and_underscores() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
            "{s} has disallowed characters"
        );
    }
}

#[test]
fn as_str_prefix_matches_category_name() {
    for &code in ALL_CODES {
        let s = code.as_str();
        let cat = code.category().to_string();
        assert!(
            s.starts_with(&cat) || code == ErrorCode::Internal,
            "{s} does not start with category prefix {cat}"
        );
    }
}

#[test]
fn as_str_matches_serde_serialization() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "serde mismatch for {code:?}");
    }
}

// =========================================================================
// 3. ErrorCode Display vs as_str — Display is human-readable
// =========================================================================

#[test]
fn display_differs_from_as_str_for_all_codes() {
    for &code in ALL_CODES {
        let display = code.to_string();
        let as_str = code.as_str();
        assert_ne!(
            display, as_str,
            "{code:?}: Display should differ from as_str"
        );
    }
}

#[test]
fn display_contains_spaces_for_multi_word_messages() {
    for &code in ALL_CODES {
        let msg = code.message();
        // All messages except possibly very short ones have spaces
        assert!(msg.contains(' '), "{code:?} message has no spaces: {msg}");
    }
}

#[test]
fn display_equals_message_for_all_codes() {
    for &code in ALL_CODES {
        assert_eq!(code.to_string(), code.message());
    }
}

// =========================================================================
// 4. ErrorCode message stability — regression guards
// =========================================================================

#[test]
fn message_stability_protocol() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.message(),
        "envelope failed to parse or has invalid fields"
    );
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.message(),
        "sidecar handshake failed"
    );
    assert_eq!(
        ErrorCode::ProtocolMissingRefId.message(),
        "ref_id field is missing from the envelope"
    );
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.message(),
        "message arrived in unexpected order"
    );
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.message(),
        "protocol version mismatch between host and sidecar"
    );
}

#[test]
fn message_stability_mapping() {
    assert_eq!(
        ErrorCode::MappingUnsupportedCapability.message(),
        "required capability is not supported by the target dialect"
    );
    assert_eq!(
        ErrorCode::MappingDialectMismatch.message(),
        "source and target dialects are incompatible"
    );
    assert_eq!(
        ErrorCode::MappingLossyConversion.message(),
        "translation succeeded but information was lost"
    );
    assert_eq!(
        ErrorCode::MappingUnmappableTool.message(),
        "tool call cannot be represented in the target dialect"
    );
}

#[test]
fn message_stability_backend() {
    assert_eq!(
        ErrorCode::BackendNotFound.message(),
        "requested backend does not exist"
    );
    assert_eq!(
        ErrorCode::BackendUnavailable.message(),
        "backend is temporarily unavailable"
    );
    assert_eq!(ErrorCode::BackendTimeout.message(), "backend timed out");
    assert_eq!(
        ErrorCode::BackendRateLimited.message(),
        "backend rejected the request due to rate limiting"
    );
    assert_eq!(
        ErrorCode::BackendAuthFailed.message(),
        "authentication with the backend failed"
    );
    assert_eq!(
        ErrorCode::BackendModelNotFound.message(),
        "requested model was not found on the backend"
    );
    assert_eq!(
        ErrorCode::BackendCrashed.message(),
        "backend process exited unexpectedly"
    );
}

#[test]
fn message_stability_remaining() {
    assert_eq!(
        ErrorCode::ExecutionToolFailed.message(),
        "tool invocation failed during execution"
    );
    assert_eq!(
        ErrorCode::ExecutionWorkspaceError.message(),
        "an error occurred in the staged workspace"
    );
    assert_eq!(
        ErrorCode::ExecutionPermissionDenied.message(),
        "operation denied due to insufficient permissions"
    );
    assert_eq!(
        ErrorCode::ContractVersionMismatch.message(),
        "contract version does not match the expected version"
    );
    assert_eq!(
        ErrorCode::ContractSchemaViolation.message(),
        "payload violates the contract schema"
    );
    assert_eq!(
        ErrorCode::ContractInvalidReceipt.message(),
        "receipt is structurally invalid or cannot be verified"
    );
    assert_eq!(
        ErrorCode::CapabilityUnsupported.message(),
        "required capability is not supported by the backend"
    );
    assert_eq!(
        ErrorCode::CapabilityEmulationFailed.message(),
        "capability emulation layer failed"
    );
    assert_eq!(
        ErrorCode::PolicyDenied.message(),
        "policy rule denied the operation"
    );
    assert_eq!(
        ErrorCode::PolicyInvalid.message(),
        "policy definition is malformed"
    );
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.message(),
        "failed to initialise the staged workspace"
    );
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.message(),
        "failed to stage files into the workspace"
    );
    assert_eq!(ErrorCode::IrLoweringFailed.message(), "IR lowering failed");
    assert_eq!(
        ErrorCode::IrInvalid.message(),
        "IR structure is invalid or inconsistent"
    );
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.message(),
        "computed receipt hash does not match the declared hash"
    );
    assert_eq!(
        ErrorCode::ReceiptChainBroken.message(),
        "receipt chain has a gap or out-of-order entry"
    );
    assert_eq!(
        ErrorCode::DialectUnknown.message(),
        "dialect identifier is not recognised"
    );
    assert_eq!(
        ErrorCode::DialectMappingFailed.message(),
        "mapping between dialects failed"
    );
    assert_eq!(
        ErrorCode::ConfigInvalid.message(),
        "configuration file or value is invalid"
    );
    assert_eq!(ErrorCode::Internal.message(), "unexpected internal error");
}

// =========================================================================
// 5. ErrorCode retryable boundary — exact set
// =========================================================================

#[test]
fn exactly_four_retryable_codes() {
    let retryable: Vec<_> = ALL_CODES.iter().filter(|c| c.is_retryable()).collect();
    assert_eq!(retryable.len(), 4);
}

#[test]
fn retryable_set_is_exactly() {
    let expected: HashSet<ErrorCode> = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ]
    .into_iter()
    .collect();
    let actual: HashSet<ErrorCode> = ALL_CODES
        .iter()
        .copied()
        .filter(|c| c.is_retryable())
        .collect();
    assert_eq!(expected, actual);
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

// =========================================================================
// 6. ErrorCategory — serde, Display, Hash, Eq
// =========================================================================

#[test]
fn category_serializes_to_snake_case() {
    for &cat in ALL_CATEGORIES {
        let json = serde_json::to_string(&cat).unwrap();
        let inner = json.trim_matches('"');
        assert_eq!(inner, cat.to_string());
    }
}

#[test]
fn category_display_matches_serde() {
    for &cat in ALL_CATEGORIES {
        let json = serde_json::to_string(&cat).unwrap();
        let expected = format!("\"{}\"", cat);
        assert_eq!(json, expected);
    }
}

#[test]
fn category_hash_eq_consistent() {
    let mut set = HashSet::new();
    for &cat in ALL_CATEGORIES {
        set.insert(cat);
    }
    assert_eq!(set.len(), ALL_CATEGORIES.len());
    // inserting duplicates should not change length
    for &cat in ALL_CATEGORIES {
        set.insert(cat);
    }
    assert_eq!(set.len(), ALL_CATEGORIES.len());
}

#[test]
fn category_deserialize_from_string() {
    let cat: ErrorCategory = serde_json::from_str("\"protocol\"").unwrap();
    assert_eq!(cat, ErrorCategory::Protocol);
    let cat: ErrorCategory = serde_json::from_str("\"internal\"").unwrap();
    assert_eq!(cat, ErrorCategory::Internal);
}

#[test]
fn category_deserialize_rejects_uppercase() {
    let result = serde_json::from_str::<ErrorCategory>("\"PROTOCOL\"");
    assert!(result.is_err());
}

#[test]
fn category_deserialize_rejects_unknown() {
    let result = serde_json::from_str::<ErrorCategory>("\"nonexistent\"");
    assert!(result.is_err());
}

// =========================================================================
// 7. ErrorCode serde — deserialization edge cases
// =========================================================================

#[test]
fn error_code_deserialize_rejects_screaming_snake() {
    let result = serde_json::from_str::<ErrorCode>("\"BACKEND_TIMEOUT\"");
    assert!(result.is_err());
}

#[test]
fn error_code_deserialize_rejects_camel_case() {
    let result = serde_json::from_str::<ErrorCode>("\"backendTimeout\"");
    assert!(result.is_err());
}

#[test]
fn error_code_deserialize_rejects_empty_string() {
    let result = serde_json::from_str::<ErrorCode>("\"\"");
    assert!(result.is_err());
}

#[test]
fn error_code_deserialize_rejects_unknown_code() {
    let result = serde_json::from_str::<ErrorCode>("\"totally_unknown_code\"");
    assert!(result.is_err());
}

#[test]
fn error_code_deserialize_rejects_number() {
    let result = serde_json::from_str::<ErrorCode>("42");
    assert!(result.is_err());
}

#[test]
fn error_code_roundtrip_all() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code);
    }
}

// =========================================================================
// 8. AbpError Display formatting — detailed checks
// =========================================================================

#[test]
fn display_format_bracket_code_space_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let s = err.to_string();
    assert!(s.starts_with('['));
    assert!(s.contains("] "));
    assert_eq!(s, "[policy_denied] denied");
}

#[test]
fn display_with_multiple_context_keys_shows_json() {
    let err = AbpError::new(ErrorCode::Internal, "msg")
        .with_context("alpha", 1)
        .with_context("beta", 2);
    let s = err.to_string();
    assert!(s.contains('{'));
    assert!(s.contains('}'));
    assert!(s.contains("alpha"));
    assert!(s.contains("beta"));
}

#[test]
fn display_context_is_valid_json() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("key", "value")
        .with_context("num", 42);
    let s = err.to_string();
    // Extract the JSON portion after the message
    let json_start = s.find('{').unwrap();
    let json_str = &s[json_start..];
    let parsed: serde_json::Value = serde_json::from_str(json_str).unwrap();
    assert_eq!(parsed["key"], serde_json::json!("value"));
    assert_eq!(parsed["num"], serde_json::json!(42));
}

#[test]
fn display_no_context_no_braces() {
    let err = AbpError::new(ErrorCode::Internal, "clean");
    let s = err.to_string();
    assert!(!s.contains('{'));
    assert!(!s.contains('}'));
}

// =========================================================================
// 9. AbpError Debug formatting
// =========================================================================

#[test]
fn debug_contains_abp_error_struct_name() {
    let err = AbpError::new(ErrorCode::Internal, "test");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("AbpError"));
}

#[test]
fn debug_omits_source_field_when_none() {
    let err = AbpError::new(ErrorCode::Internal, "no cause");
    let dbg = format!("{err:?}");
    // Debug struct should not have a "source:" field when source is None
    assert!(!dbg.contains("source:"));
}

#[test]
fn debug_includes_source_field_when_present() {
    let src = io::Error::new(io::ErrorKind::Other, "the cause");
    let err = AbpError::new(ErrorCode::Internal, "with source").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("the cause"));
}

#[test]
fn debug_omits_context_field_when_empty() {
    let err = AbpError::new(ErrorCode::Internal, "no ctx");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("context"));
}

#[test]
fn debug_includes_context_field_when_present() {
    let err = AbpError::new(ErrorCode::Internal, "has ctx").with_context("k", "v");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
}

// =========================================================================
// 10. AbpError builder — edge cases
// =========================================================================

#[test]
fn empty_message() {
    let err = AbpError::new(ErrorCode::Internal, "");
    assert_eq!(err.message, "");
    assert_eq!(err.to_string(), "[internal] ");
}

#[test]
fn very_long_message() {
    let long_msg = "x".repeat(10_000);
    let err = AbpError::new(ErrorCode::Internal, &long_msg);
    assert_eq!(err.message.len(), 10_000);
    assert!(err.to_string().contains(&long_msg));
}

#[test]
fn message_with_special_characters() {
    let msg = r#"error: "quotes" & <angles> \backslash"#;
    let err = AbpError::new(ErrorCode::Internal, msg);
    assert_eq!(err.message, msg);
}

#[test]
fn message_with_newlines() {
    let msg = "line1\nline2\nline3";
    let err = AbpError::new(ErrorCode::Internal, msg);
    assert!(err.to_string().contains('\n'));
}

#[test]
fn message_with_unicode() {
    let msg = "エラー: 失敗 🔥 café résumé";
    let err = AbpError::new(ErrorCode::Internal, msg);
    assert_eq!(err.message, msg);
    assert!(err.to_string().contains("🔥"));
}

#[test]
fn message_with_null_byte_in_string() {
    let msg = "before\0after";
    let err = AbpError::new(ErrorCode::Internal, msg);
    assert_eq!(err.message, msg);
}

#[test]
fn context_key_overwrite() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("key", "first")
        .with_context("key", "second");
    assert_eq!(err.context.len(), 1);
    assert_eq!(err.context["key"], serde_json::json!("second"));
}

#[test]
fn context_with_empty_key() {
    let err = AbpError::new(ErrorCode::Internal, "test").with_context("", "val");
    assert_eq!(err.context[""], serde_json::json!("val"));
}

#[test]
fn context_with_null_value() {
    let err = AbpError::new(ErrorCode::Internal, "test").with_context("k", serde_json::Value::Null);
    assert_eq!(err.context["k"], serde_json::Value::Null);
}

#[test]
fn context_with_nested_object() {
    let nested = serde_json::json!({"a": {"b": {"c": 1}}});
    let err = AbpError::new(ErrorCode::Internal, "test").with_context("deep", nested.clone());
    assert_eq!(err.context["deep"], nested);
}

#[test]
fn context_with_array_value() {
    let arr = vec![1, 2, 3];
    let err = AbpError::new(ErrorCode::Internal, "test").with_context("arr", arr);
    assert_eq!(err.context["arr"], serde_json::json!([1, 2, 3]));
}

#[test]
fn context_preserves_btreemap_ordering() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("z", 1)
        .with_context("a", 2)
        .with_context("m", 3);
    let keys: Vec<_> = err.context.keys().collect();
    assert_eq!(keys, vec!["a", "m", "z"]);
}

#[test]
fn with_source_replaces_previous_source() {
    let src1 = io::Error::new(io::ErrorKind::NotFound, "first");
    let src2 = io::Error::new(io::ErrorKind::Other, "second");
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_source(src1)
        .with_source(src2);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "second");
}

// =========================================================================
// 11. ErrorInfo — construction and edge cases
// =========================================================================

#[test]
fn error_info_retryable_inferred_from_retryable_code() {
    let info = ErrorInfo::new(ErrorCode::BackendUnavailable, "unavailable");
    assert!(info.is_retryable);
}

#[test]
fn error_info_not_retryable_for_non_retryable_code() {
    let info = ErrorInfo::new(ErrorCode::ProtocolInvalidEnvelope, "bad");
    assert!(!info.is_retryable);
}

#[test]
fn error_info_display_uses_code_as_str() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
    let s = info.to_string();
    assert!(s.starts_with("[backend_timeout]"));
    assert!(s.ends_with("timed out"));
}

#[test]
fn error_info_with_detail_complex_types() {
    let info = ErrorInfo::new(ErrorCode::Internal, "x")
        .with_detail("map", serde_json::json!({"nested": true}))
        .with_detail("list", vec!["a", "b"])
        .with_detail("num", 42.5_f64)
        .with_detail("flag", false)
        .with_detail("null", serde_json::Value::Null);
    assert_eq!(info.details.len(), 5);
}

#[test]
fn error_info_detail_key_overwrite() {
    let info = ErrorInfo::new(ErrorCode::Internal, "x")
        .with_detail("key", "first")
        .with_detail("key", "second");
    assert_eq!(info.details.len(), 1);
    assert_eq!(info.details["key"], serde_json::json!("second"));
}

#[test]
fn error_info_empty_message() {
    let info = ErrorInfo::new(ErrorCode::Internal, "");
    assert_eq!(info.message, "");
    assert_eq!(info.to_string(), "[internal] ");
}

#[test]
fn error_info_clone_independence() {
    let info = ErrorInfo::new(ErrorCode::Internal, "original").with_detail("k", "v");
    let mut cloned = info.clone();
    cloned.message = "modified".to_string();
    cloned
        .details
        .insert("k2".to_string(), serde_json::json!("v2"));
    assert_eq!(info.message, "original");
    assert_eq!(info.details.len(), 1);
}

#[test]
fn error_info_serde_roundtrip_with_many_details() {
    let mut info = ErrorInfo::new(ErrorCode::Internal, "many");
    for i in 0..20 {
        info = info.with_detail(format!("key_{i:02}"), i);
    }
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
    assert_eq!(back.details.len(), 20);
}

#[test]
fn error_info_details_deterministic_serialization() {
    let info = ErrorInfo::new(ErrorCode::Internal, "det")
        .with_detail("z", 3)
        .with_detail("a", 1)
        .with_detail("m", 2);
    let json1 = serde_json::to_string(&info).unwrap();
    let json2 = serde_json::to_string(&info).unwrap();
    assert_eq!(json1, json2);
    let a_pos = json1.find("\"a\"").unwrap();
    let m_pos = json1.find("\"m\"").unwrap();
    let z_pos = json1.find("\"z\"").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

// =========================================================================
// 12. AbpError → ErrorInfo conversion
// =========================================================================

#[test]
fn to_info_preserves_code() {
    for &code in ALL_CODES {
        let err = AbpError::new(code, "msg");
        let info = err.to_info();
        assert_eq!(info.code, code);
    }
}

#[test]
fn to_info_preserves_message() {
    let err = AbpError::new(ErrorCode::Internal, "hello world");
    let info = err.to_info();
    assert_eq!(info.message, "hello world");
}

#[test]
fn to_info_preserves_context_as_details() {
    let err = AbpError::new(ErrorCode::Internal, "x")
        .with_context("a", 1)
        .with_context("b", "two");
    let info = err.to_info();
    assert_eq!(info.details.len(), 2);
    assert_eq!(info.details["a"], serde_json::json!(1));
    assert_eq!(info.details["b"], serde_json::json!("two"));
}

#[test]
fn to_info_infers_retryable_from_code() {
    let retryable = AbpError::new(ErrorCode::BackendTimeout, "t").to_info();
    assert!(retryable.is_retryable);
    let not = AbpError::new(ErrorCode::Internal, "x").to_info();
    assert!(!not.is_retryable);
}

#[test]
fn to_info_discards_source() {
    let src = io::Error::new(io::ErrorKind::Other, "cause");
    let err = AbpError::new(ErrorCode::Internal, "x").with_source(src);
    let _info = err.to_info();
    // ErrorInfo has no source — just confirm it doesn't panic
}

// =========================================================================
// 13. AbpErrorDto — conversion and serialization
// =========================================================================

#[test]
fn dto_from_ref_captures_all_fields() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "anthropic")
        .with_context("ms", 5000);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::BackendTimeout);
    assert_eq!(dto.message, "slow");
    assert_eq!(dto.context.len(), 2);
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_from_ref_with_source_captures_source_message() {
    let src = io::Error::new(io::ErrorKind::ConnectionRefused, "connection refused");
    let err = AbpError::new(ErrorCode::BackendUnavailable, "down").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("connection refused"));
}

#[test]
fn dto_to_abp_error_preserves_code_message_context() {
    let mut ctx = BTreeMap::new();
    ctx.insert("k".to_string(), serde_json::json!("v"));
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad config".into(),
        context: ctx,
        source_message: Some("inner".into()),
        location: None,
        cause_chain: Vec::new(),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.message, "bad config");
    assert_eq!(err.context.len(), 1);
    assert!(err.source.is_none()); // source is lost
}

#[test]
fn dto_serialization_skip_source_when_none() {
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

#[test]
fn dto_serialization_includes_source_when_some() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "x".into(),
        context: BTreeMap::new(),
        source_message: Some("cause".into()),
        location: None,
        cause_chain: Vec::new(),
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("source_message"));
    assert!(json.contains("cause"));
}

#[test]
fn dto_roundtrip_preserves_all_fields() {
    let src = io::Error::new(io::ErrorKind::Other, "root cause");
    let err = AbpError::new(ErrorCode::ReceiptHashMismatch, "hash mismatch")
        .with_source(src)
        .with_context("expected", "abc123")
        .with_context("actual", "def456");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_deserialization_with_missing_optional_source() {
    let json = r#"{"code":"internal","message":"test","context":{}}"#;
    let dto: AbpErrorDto = serde_json::from_str(json).unwrap();
    assert_eq!(dto.code, ErrorCode::Internal);
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_deserialization_with_explicit_null_source() {
    let json = r#"{"code":"internal","message":"test","context":{},"source_message":null}"#;
    let dto: AbpErrorDto = serde_json::from_str(json).unwrap();
    assert!(dto.source_message.is_none());
}

// =========================================================================
// 14. From conversions — io::Error and serde_json::Error
// =========================================================================

#[test]
fn from_io_error_sets_internal_code() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
}

#[test]
fn from_io_error_message_contains_description() {
    let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let abp: AbpError = io_err.into();
    assert!(abp.message.contains("access denied"));
}

#[test]
fn from_io_error_preserves_source() {
    let io_err = io::Error::new(io::ErrorKind::TimedOut, "timeout");
    let abp: AbpError = io_err.into();
    let src = StdError::source(&abp).unwrap();
    assert_eq!(src.to_string(), "timeout");
}

#[test]
fn from_io_error_various_kinds_all_map_to_internal() {
    let kinds = [
        io::ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::ConnectionRefused,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::TimedOut,
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::AlreadyExists,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::InvalidInput,
        io::ErrorKind::InvalidData,
    ];
    for kind in kinds {
        let io_err = io::Error::new(kind, "msg");
        let abp: AbpError = io_err.into();
        assert_eq!(
            abp.code,
            ErrorCode::Internal,
            "io {kind:?} should map to Internal"
        );
    }
}

#[test]
fn from_serde_json_error_sets_protocol_invalid_envelope() {
    let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let abp: AbpError = json_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
}

#[test]
fn from_serde_json_error_preserves_source() {
    let json_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let abp: AbpError = json_err.into();
    assert!(StdError::source(&abp).is_some());
}

#[test]
fn from_serde_json_error_message_is_descriptive() {
    let json_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
    let abp: AbpError = json_err.into();
    assert!(!abp.message.is_empty());
}

// =========================================================================
// 15. Category grouping completeness
// =========================================================================

#[test]
fn every_category_has_at_least_one_code() {
    for &cat in ALL_CATEGORIES {
        let count = ALL_CODES.iter().filter(|c| c.category() == cat).count();
        assert!(count >= 1, "category {cat:?} has no codes");
    }
}

#[test]
fn every_code_maps_to_a_known_category() {
    let known: HashSet<ErrorCategory> = ALL_CATEGORIES.iter().copied().collect();
    for &code in ALL_CODES {
        assert!(
            known.contains(&code.category()),
            "{code:?} maps to unknown category"
        );
    }
}

#[test]
fn protocol_category_has_five_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Protocol)
        .count();
    assert_eq!(count, 5);
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
fn mapping_category_has_four_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Mapping)
        .count();
    assert_eq!(count, 4);
}

#[test]
fn internal_category_has_one_code() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Internal)
        .count();
    assert_eq!(count, 1);
}

// =========================================================================
// 16. ErrorCode Clone, Copy, Eq, Hash
// =========================================================================

#[test]
fn error_code_copy_semantics() {
    let code = ErrorCode::BackendTimeout;
    let copied = code;
    assert_eq!(code, copied);
}

#[test]
fn error_code_clone_equals_original() {
    for &code in ALL_CODES {
        assert_eq!(code, code.clone());
    }
}

#[test]
fn error_code_hash_set_deduplication() {
    let mut set = HashSet::new();
    set.insert(ErrorCode::Internal);
    set.insert(ErrorCode::Internal);
    set.insert(ErrorCode::BackendTimeout);
    assert_eq!(set.len(), 2);
}

#[test]
fn error_code_ne_for_different_variants() {
    assert_ne!(ErrorCode::Internal, ErrorCode::BackendTimeout);
    assert_ne!(ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid);
}

// =========================================================================
// 17. ErrorInfo serde — deserialization from raw JSON
// =========================================================================

#[test]
fn error_info_deserialize_from_raw_json() {
    let json = r#"{
        "code": "backend_timeout",
        "message": "timed out",
        "details": {"ms": 5000},
        "is_retryable": true
    }"#;
    let info: ErrorInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "timed out");
    assert!(info.is_retryable);
    assert_eq!(info.details["ms"], serde_json::json!(5000));
}

#[test]
fn error_info_deserialize_overrides_retryable() {
    // Even though BackendTimeout is retryable, the JSON can set is_retryable to false
    let json = r#"{
        "code": "backend_timeout",
        "message": "forced non-retryable",
        "details": {},
        "is_retryable": false
    }"#;
    let info: ErrorInfo = serde_json::from_str(json).unwrap();
    assert!(!info.is_retryable);
}

#[test]
fn error_info_roundtrip_empty_details() {
    let info = ErrorInfo::new(ErrorCode::Internal, "simple");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
    assert!(back.details.is_empty());
}

// =========================================================================
// 18. Cross-type consistency
// =========================================================================

#[test]
fn abp_error_display_matches_error_info_display() {
    // When no context, AbpError and ErrorInfo should display the same
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
    assert_eq!(err.to_string(), info.to_string());
}

#[test]
fn dto_code_matches_original_error_code() {
    for &code in ALL_CODES {
        let err = AbpError::new(code, "test");
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.code, code);
    }
}

#[test]
fn dto_back_to_abp_error_display_matches() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config");
    let dto: AbpErrorDto = (&err).into();
    let back: AbpError = dto.into();
    assert_eq!(err.to_string(), back.to_string());
}

// =========================================================================
// 19. Serialization stability and JSON structure
// =========================================================================

#[test]
fn dto_json_structure_has_expected_keys() {
    let err = AbpError::new(ErrorCode::Internal, "test").with_context("k", "v");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("code").is_some());
    assert!(parsed.get("message").is_some());
    assert!(parsed.get("context").is_some());
}

#[test]
fn error_info_json_structure_has_expected_keys() {
    let info = ErrorInfo::new(ErrorCode::Internal, "test").with_detail("k", "v");
    let json = serde_json::to_string(&info).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("code").is_some());
    assert!(parsed.get("message").is_some());
    assert!(parsed.get("details").is_some());
    assert!(parsed.get("is_retryable").is_some());
}

#[test]
fn repeated_serialization_is_identical() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
        .with_context("backend", "openai")
        .with_context("ms", 5000);
    let dto: AbpErrorDto = (&err).into();
    let runs: Vec<String> = (0..5)
        .map(|_| serde_json::to_string(&dto).unwrap())
        .collect();
    for json in &runs {
        assert_eq!(json, &runs[0]);
    }
}

// =========================================================================
// 20. Comprehensive construction — every code through AbpError
// =========================================================================

#[test]
fn construct_abp_error_for_every_code() {
    for &code in ALL_CODES {
        let err = AbpError::new(code, format!("testing {}", code.as_str()));
        assert_eq!(err.code, code);
        assert!(err.to_string().contains(code.as_str()));
    }
}

#[test]
fn construct_error_info_for_every_code() {
    for &code in ALL_CODES {
        let info = ErrorInfo::new(code, format!("testing {}", code.as_str()));
        assert_eq!(info.code, code);
        assert_eq!(info.is_retryable, code.is_retryable());
    }
}

#[test]
fn construct_dto_for_every_code() {
    for &code in ALL_CODES {
        let err = AbpError::new(code, "test");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, code);
    }
}
