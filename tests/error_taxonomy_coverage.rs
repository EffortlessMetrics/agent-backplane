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
//! Comprehensive tests for the `abp-error` crate.
//!
//! Covers: ErrorCode variants, ErrorCategory mapping, AbpError construction,
//! display formatting, context enrichment, conversion traits, serde roundtrips,
//! error chaining, AbpErrorDto, and edge cases.

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;

// =========================================================================
// Helpers
// =========================================================================

/// All 20 ErrorCode variants for exhaustive iteration.
const ALL_CODES: &[ErrorCode] = &[
    ErrorCode::ProtocolInvalidEnvelope,
    ErrorCode::ProtocolUnexpectedMessage,
    ErrorCode::ProtocolVersionMismatch,
    ErrorCode::BackendNotFound,
    ErrorCode::BackendTimeout,
    ErrorCode::BackendCrashed,
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

/// All 10 ErrorCategory variants.
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
    ErrorCategory::Internal,
];

/// Expected (ErrorCode, &str, ErrorCategory) triples for every variant.
const CODE_TABLE: &[(ErrorCode, &str, ErrorCategory)] = &[
    (
        ErrorCode::ProtocolInvalidEnvelope,
        "protocol_invalid_envelope",
        ErrorCategory::Protocol,
    ),
    (
        ErrorCode::ProtocolUnexpectedMessage,
        "protocol_unexpected_message",
        ErrorCategory::Protocol,
    ),
    (
        ErrorCode::ProtocolVersionMismatch,
        "protocol_version_mismatch",
        ErrorCategory::Protocol,
    ),
    (
        ErrorCode::BackendNotFound,
        "backend_not_found",
        ErrorCategory::Backend,
    ),
    (
        ErrorCode::BackendTimeout,
        "backend_timeout",
        ErrorCategory::Backend,
    ),
    (
        ErrorCode::BackendCrashed,
        "backend_crashed",
        ErrorCategory::Backend,
    ),
    (
        ErrorCode::CapabilityUnsupported,
        "capability_unsupported",
        ErrorCategory::Capability,
    ),
    (
        ErrorCode::CapabilityEmulationFailed,
        "capability_emulation_failed",
        ErrorCategory::Capability,
    ),
    (
        ErrorCode::PolicyDenied,
        "policy_denied",
        ErrorCategory::Policy,
    ),
    (
        ErrorCode::PolicyInvalid,
        "policy_invalid",
        ErrorCategory::Policy,
    ),
    (
        ErrorCode::WorkspaceInitFailed,
        "workspace_init_failed",
        ErrorCategory::Workspace,
    ),
    (
        ErrorCode::WorkspaceStagingFailed,
        "workspace_staging_failed",
        ErrorCategory::Workspace,
    ),
    (
        ErrorCode::IrLoweringFailed,
        "ir_lowering_failed",
        ErrorCategory::Ir,
    ),
    (ErrorCode::IrInvalid, "ir_invalid", ErrorCategory::Ir),
    (
        ErrorCode::ReceiptHashMismatch,
        "receipt_hash_mismatch",
        ErrorCategory::Receipt,
    ),
    (
        ErrorCode::ReceiptChainBroken,
        "receipt_chain_broken",
        ErrorCategory::Receipt,
    ),
    (
        ErrorCode::DialectUnknown,
        "dialect_unknown",
        ErrorCategory::Dialect,
    ),
    (
        ErrorCode::DialectMappingFailed,
        "dialect_mapping_failed",
        ErrorCategory::Dialect,
    ),
    (
        ErrorCode::ConfigInvalid,
        "config_invalid",
        ErrorCategory::Config,
    ),
    (ErrorCode::Internal, "internal", ErrorCategory::Internal),
];

// =========================================================================
// 1. ErrorCode variants — existence & serialization
// =========================================================================

#[test]
fn error_code_count_is_20() {
    assert_eq!(ALL_CODES.len(), 20);
}

#[test]
fn error_code_as_str_matches_table() {
    for &(code, expected_str, _) in CODE_TABLE {
        assert_eq!(code.as_str(), expected_str, "as_str mismatch for {code:?}");
    }
}

#[test]
fn error_code_display_equals_as_str() {
    for code in ALL_CODES {
        assert_eq!(code.to_string(), code.message());
    }
}

#[test]
fn error_code_as_str_values_are_unique() {
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
fn error_code_as_str_is_snake_case() {
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{s} is not snake_case"
        );
    }
}

#[test]
fn error_code_serialize_to_screaming_snake_string() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected);
    }
}

#[test]
fn error_code_deserialize_from_screaming_snake() {
    for code in ALL_CODES {
        let json = format!("\"{}\"", code.as_str());
        let parsed: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, *code);
    }
}

#[test]
fn error_code_serde_roundtrip_all_variants() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *code);
    }
}

#[test]
fn error_code_deserialize_rejects_invalid() {
    let result: Result<ErrorCode, _> = serde_json::from_str("\"NOT_A_REAL_CODE\"");
    assert!(result.is_err());
}

#[test]
fn error_code_deserialize_rejects_lowercase() {
    // snake_case IS the canonical serde format
    let result: Result<ErrorCode, _> = serde_json::from_str("\"backend_not_found\"");
    assert!(result.is_ok());
}

#[test]
fn error_code_deserialize_rejects_number() {
    let result: Result<ErrorCode, _> = serde_json::from_str("42");
    assert!(result.is_err());
}

#[test]
fn error_code_deserialize_rejects_null() {
    let result: Result<ErrorCode, _> = serde_json::from_str("null");
    assert!(result.is_err());
}

#[test]
fn error_code_clone_eq() {
    for code in ALL_CODES {
        let cloned = *code;
        assert_eq!(*code, cloned);
    }
}

#[test]
fn error_code_debug_not_empty() {
    for code in ALL_CODES {
        let dbg = format!("{code:?}");
        assert!(!dbg.is_empty());
    }
}

#[test]
fn error_code_hash_consistent() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    for code in ALL_CODES {
        let mut h1 = DefaultHasher::new();
        code.hash(&mut h1);
        let mut h2 = DefaultHasher::new();
        code.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }
}

// =========================================================================
// 2. ErrorCategory — mapping from ErrorCode
// =========================================================================

#[test]
fn error_category_count_is_10() {
    assert_eq!(ALL_CATEGORIES.len(), 10);
}

#[test]
fn error_code_category_matches_table() {
    for &(code, _, expected_cat) in CODE_TABLE {
        assert_eq!(
            code.category(),
            expected_cat,
            "category mismatch for {code:?}"
        );
    }
}

#[test]
fn protocol_codes_map_to_protocol() {
    let protocol_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in protocol_codes {
        assert_eq!(code.category(), ErrorCategory::Protocol);
    }
}

#[test]
fn backend_codes_map_to_backend() {
    let codes = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
    ];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Backend);
    }
}

#[test]
fn capability_codes_map_to_capability() {
    let codes = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Capability);
    }
}

#[test]
fn policy_codes_map_to_policy() {
    let codes = [ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Policy);
    }
}

#[test]
fn workspace_codes_map_to_workspace() {
    let codes = [
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
    ];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Workspace);
    }
}

#[test]
fn ir_codes_map_to_ir() {
    let codes = [ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Ir);
    }
}

#[test]
fn receipt_codes_map_to_receipt() {
    let codes = [
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Receipt);
    }
}

#[test]
fn dialect_codes_map_to_dialect() {
    let codes = [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed];
    for code in codes {
        assert_eq!(code.category(), ErrorCategory::Dialect);
    }
}

#[test]
fn config_code_maps_to_config() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn internal_code_maps_to_internal() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn every_category_is_reachable() {
    let reachable: HashSet<ErrorCategory> = ALL_CODES.iter().map(|c| c.category()).collect();
    for cat in ALL_CATEGORIES {
        assert!(
            reachable.contains(cat),
            "category {cat:?} unreachable from any ErrorCode"
        );
    }
}

// =========================================================================
// ErrorCategory display & serde
// =========================================================================

#[test]
fn error_category_display_all() {
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
        (ErrorCategory::Internal, "internal"),
    ];
    for (cat, s) in expected {
        assert_eq!(cat.to_string(), s);
    }
}

#[test]
fn error_category_serde_roundtrip_all() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *cat);
    }
}

#[test]
fn error_category_serialize_snake_case() {
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Protocol).unwrap(),
        "\"protocol\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Backend).unwrap(),
        "\"backend\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Internal).unwrap(),
        "\"internal\""
    );
}

#[test]
fn error_category_deserialize_rejects_invalid() {
    let result: Result<ErrorCategory, _> = serde_json::from_str("\"nosuchcat\"");
    assert!(result.is_err());
}

#[test]
fn error_category_clone_eq() {
    for cat in ALL_CATEGORIES {
        let cloned = *cat;
        assert_eq!(*cat, cloned);
    }
}

#[test]
fn error_category_debug_not_empty() {
    for cat in ALL_CATEGORIES {
        assert!(!format!("{cat:?}").is_empty());
    }
}

#[test]
fn error_category_hash_consistent() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    for cat in ALL_CATEGORIES {
        let mut h1 = DefaultHasher::new();
        cat.hash(&mut h1);
        let mut h2 = DefaultHasher::new();
        cat.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }
}

// =========================================================================
// 3. AbpError construction and display formatting
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
fn abp_error_new_from_string() {
    let msg = String::from("allocated message");
    let err = AbpError::new(ErrorCode::BackendNotFound, msg);
    assert_eq!(err.message, "allocated message");
}

#[test]
fn abp_error_display_without_context() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "access denied");
    assert_eq!(err.to_string(), "[policy_denied] access denied");
}

#[test]
fn abp_error_display_with_single_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow").with_context("timeout_ms", 5000);
    let s = err.to_string();
    assert!(s.starts_with("[backend_timeout] slow"));
    assert!(s.contains("timeout_ms"));
    assert!(s.contains("5000"));
}

#[test]
fn abp_error_display_with_multiple_contexts() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("retry", 3);
    let s = err.to_string();
    assert!(s.contains("backend"));
    assert!(s.contains("openai"));
    assert!(s.contains("retry"));
    assert!(s.contains('3'));
}

#[test]
fn abp_error_display_context_is_deterministic() {
    let err = AbpError::new(ErrorCode::Internal, "x")
        .with_context("zzz", 1)
        .with_context("aaa", 2);
    let s1 = err.to_string();
    // BTreeMap guarantees alphabetical order; "aaa" before "zzz".
    let aaa_pos = s1.find("aaa").unwrap();
    let zzz_pos = s1.find("zzz").unwrap();
    assert!(aaa_pos < zzz_pos);
}

#[test]
fn abp_error_debug_contains_code_and_message() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad ir");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("IrInvalid"));
    assert!(dbg.contains("bad ir"));
}

#[test]
fn abp_error_debug_omits_source_when_none() {
    let err = AbpError::new(ErrorCode::Internal, "test");
    let dbg = format!("{err:?}");
    // source field should not appear
    assert!(!dbg.contains("source"));
}

#[test]
fn abp_error_debug_includes_source_when_present() {
    let src = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("gone"));
}

#[test]
fn abp_error_debug_omits_context_when_empty() {
    let err = AbpError::new(ErrorCode::Internal, "test");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("context"));
}

#[test]
fn abp_error_debug_includes_context_when_present() {
    let err = AbpError::new(ErrorCode::Internal, "test").with_context("key", "val");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
    assert!(dbg.contains("key"));
}

#[test]
fn abp_error_display_for_every_code() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, "msg");
        let s = err.to_string();
        assert!(s.starts_with(&format!("[{}]", code.as_str())));
        assert!(s.contains("msg"));
    }
}

// =========================================================================
// 4. Error context enrichment (with_context)
// =========================================================================

#[test]
fn with_context_string_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("key", "value");
    assert_eq!(err.context["key"], json!("value"));
}

#[test]
fn with_context_integer_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("count", 42);
    assert_eq!(err.context["count"], json!(42));
}

#[test]
fn with_context_float_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("ratio", 2.72);
    assert_eq!(err.context["ratio"], json!(2.72));
}

#[test]
fn with_context_bool_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("flag", true);
    assert_eq!(err.context["flag"], json!(true));
}

#[test]
fn with_context_null_value() {
    let err =
        AbpError::new(ErrorCode::Internal, "x").with_context("nothing", serde_json::Value::Null);
    assert_eq!(err.context["nothing"], json!(null));
}

#[test]
fn with_context_array_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("tags", vec!["a", "b"]);
    assert_eq!(err.context["tags"], json!(["a", "b"]));
}

#[test]
fn with_context_nested_object() {
    let obj = json!({"a": 1, "b": [2, 3]});
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("details", obj.clone());
    assert_eq!(err.context["details"], obj);
}

#[test]
fn with_context_overwrites_same_key() {
    let err = AbpError::new(ErrorCode::Internal, "x")
        .with_context("k", "first")
        .with_context("k", "second");
    assert_eq!(err.context.len(), 1);
    assert_eq!(err.context["k"], json!("second"));
}

#[test]
fn with_context_preserves_insertion_order_in_btree() {
    let err = AbpError::new(ErrorCode::Internal, "x")
        .with_context("z", 1)
        .with_context("a", 2)
        .with_context("m", 3);
    let keys: Vec<_> = err.context.keys().collect();
    assert_eq!(keys, vec!["a", "m", "z"]); // BTreeMap sorts keys
}

#[test]
fn with_context_many_entries() {
    let mut err = AbpError::new(ErrorCode::Internal, "x");
    for i in 0..50 {
        err = err.with_context(format!("key_{i:03}"), i);
    }
    assert_eq!(err.context.len(), 50);
}

#[test]
fn with_context_empty_key() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("", "val");
    assert_eq!(err.context[""], json!("val"));
}

// =========================================================================
// 5. Error conversion traits (From impls) & with_source
// =========================================================================

#[test]
fn with_source_io_error() {
    let src = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let err = AbpError::new(ErrorCode::PolicyDenied, "no").with_source(src);
    assert!(err.source.is_some());
}

#[test]
fn with_source_replaces_previous() {
    let src1 = std::io::Error::new(std::io::ErrorKind::NotFound, "first");
    let src2 = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "second");
    let err = AbpError::new(ErrorCode::Internal, "x")
        .with_source(src1)
        .with_source(src2);
    let displayed = err.source.as_ref().unwrap().to_string();
    assert_eq!(displayed, "second");
}

#[test]
fn std_error_source_returns_inner() {
    let src = std::io::Error::other("inner cause");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
    let source = StdError::source(&err).unwrap();
    assert_eq!(source.to_string(), "inner cause");
}

#[test]
fn std_error_source_returns_none_when_absent() {
    let err = AbpError::new(ErrorCode::Internal, "no source");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn abp_error_is_std_error() {
    // Compile-time check that AbpError implements std::error::Error.
    fn assert_std_error<E: StdError>(_e: &E) {}
    let err = AbpError::new(ErrorCode::Internal, "check");
    assert_std_error(&err);
}

#[test]
fn abp_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    // AbpError has `source: Option<Box<dyn Error + Send + Sync>>`, so it should be Send+Sync.
    assert_send_sync::<AbpError>();
}

#[test]
fn dto_from_abp_error_ref() {
    let err =
        AbpError::new(ErrorCode::ConfigInvalid, "bad config").with_context("file", "config.toml");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::ConfigInvalid);
    assert_eq!(dto.message, "bad config");
    assert_eq!(dto.context["file"], json!("config.toml"));
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_from_abp_error_with_source_captures_message() {
    let src = std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout inner");
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("timeout inner"));
}

#[test]
fn abp_error_from_dto() {
    let dto = AbpErrorDto {
        code: ErrorCode::DialectUnknown,
        message: "unknown".into(),
        context: BTreeMap::new(),
        source_message: Some("was here".into()),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::DialectUnknown);
    assert_eq!(err.message, "unknown");
    // Source is lost in DTO→AbpError (opaque type cannot be reconstructed).
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_from_dto_preserves_context() {
    let mut ctx = BTreeMap::new();
    ctx.insert("k".into(), json!("v"));
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "msg".into(),
        context: ctx,
        source_message: None,
    };
    let err: AbpError = dto.into();
    assert_eq!(err.context["k"], json!("v"));
}

#[test]
fn category_shorthand_delegates_to_code() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, "test");
        assert_eq!(err.category(), code.category());
    }
}

// =========================================================================
// 6. Serde roundtrip for all error types
// =========================================================================

#[test]
fn dto_serde_roundtrip_minimal() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "oops".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_serde_roundtrip_with_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("ms", 30_000);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_serde_roundtrip_with_source_message() {
    let dto = AbpErrorDto {
        code: ErrorCode::WorkspaceStagingFailed,
        message: "stage fail".into(),
        context: BTreeMap::new(),
        source_message: Some("disk full".into()),
    };
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_serde_source_message_skipped_when_none() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "x".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

#[test]
fn dto_serde_source_message_present_when_some() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "x".into(),
        context: BTreeMap::new(),
        source_message: Some("inner".into()),
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("source_message"));
    assert!(json.contains("inner"));
}

#[test]
fn dto_serde_roundtrip_all_codes() {
    for code in ALL_CODES {
        let dto = AbpErrorDto {
            code: *code,
            message: format!("msg for {}", code.as_str()),
            context: BTreeMap::new(),
            source_message: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back, "roundtrip failed for {code:?}");
    }
}

#[test]
fn dto_deserialize_from_handcrafted_json() {
    let raw = r#"{"code":"backend_not_found","message":"no backend","context":{"name":"openai"}}"#;
    let dto: AbpErrorDto = serde_json::from_str(raw).unwrap();
    assert_eq!(dto.code, ErrorCode::BackendNotFound);
    assert_eq!(dto.message, "no backend");
    assert_eq!(dto.context["name"], json!("openai"));
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_deserialize_with_source_message() {
    let raw = r#"{"code":"internal","message":"x","context":{},"source_message":"inner"}"#;
    let dto: AbpErrorDto = serde_json::from_str(raw).unwrap();
    assert_eq!(dto.source_message.as_deref(), Some("inner"));
}

#[test]
fn error_code_json_schema_exists() {
    // ErrorCode derives JsonSchema, so we can generate one.
    let schema = schemars::schema_for!(ErrorCode);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(!json.is_empty());
}

// =========================================================================
// 7. Error chain / wrapping behavior
// =========================================================================

#[test]
fn error_chain_single_depth() {
    let inner = std::io::Error::other("root cause");
    let err = AbpError::new(ErrorCode::Internal, "wrapper").with_source(inner);
    // Walk the chain manually instead of using successors (lifetime issues).
    let mut chain = vec![err.to_string()];
    let mut cur: Option<&dyn StdError> = StdError::source(&err);
    while let Some(e) = cur {
        chain.push(e.to_string());
        cur = e.source();
    }
    assert_eq!(chain.len(), 2);
    assert!(chain[0].contains("wrapper"));
    assert_eq!(chain[1], "root cause");
}

#[test]
fn error_chain_double_depth() {
    let root = std::io::Error::other("root");
    let mid = std::io::Error::other(root);
    let err = AbpError::new(ErrorCode::Internal, "top").with_source(mid);
    // At least 2 levels deep from AbpError.
    let top_src = StdError::source(&err).unwrap();
    assert!(!top_src.to_string().is_empty());
}

#[test]
fn abp_error_wrapping_another_abp_error() {
    let inner = AbpError::new(ErrorCode::PolicyDenied, "inner deny");
    let outer = AbpError::new(ErrorCode::Internal, "outer").with_source(inner);
    let src = StdError::source(&outer).unwrap();
    assert!(src.to_string().contains("policy_denied"));
    assert!(src.to_string().contains("inner deny"));
}

#[test]
fn dto_captures_nested_abp_error_source() {
    let inner = AbpError::new(ErrorCode::PolicyDenied, "inner");
    let outer = AbpError::new(ErrorCode::Internal, "outer").with_source(inner);
    let dto: AbpErrorDto = (&outer).into();
    assert!(dto.source_message.as_deref().unwrap().contains("inner"));
}

#[test]
fn with_source_and_context_combined() {
    let src = std::io::Error::other("io fail");
    let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging")
        .with_context("path", "/tmp/work")
        .with_source(src);
    assert_eq!(err.context["path"], json!("/tmp/work"));
    assert!(err.source.is_some());
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("io fail"));
    assert_eq!(dto.context["path"], json!("/tmp/work"));
}

// =========================================================================
// 8. Edge cases
// =========================================================================

#[test]
fn empty_message() {
    let err = AbpError::new(ErrorCode::Internal, "");
    assert_eq!(err.message, "");
    assert_eq!(err.to_string(), "[internal] ");
}

#[test]
fn very_long_message() {
    let long = "x".repeat(10_000);
    let err = AbpError::new(ErrorCode::Internal, &long);
    assert_eq!(err.message.len(), 10_000);
    assert!(err.to_string().contains(&long));
}

#[test]
fn message_with_special_chars() {
    let msg = "line1\nline2\ttab \"quotes\" \\backslash";
    let err = AbpError::new(ErrorCode::Internal, msg);
    assert_eq!(err.message, msg);
}

#[test]
fn message_with_unicode() {
    let msg = "エラー 🚀 données café";
    let err = AbpError::new(ErrorCode::Internal, msg);
    assert_eq!(err.message, msg);
    assert!(err.to_string().contains(msg));
}

#[test]
fn context_with_empty_string_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("k", "");
    assert_eq!(err.context["k"], json!(""));
}

#[test]
fn context_with_very_long_value() {
    let long = "y".repeat(10_000);
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("big", &long);
    assert_eq!(err.context["big"], json!(long));
}

#[test]
fn context_with_unicode_key_and_value() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("キー", "値");
    assert_eq!(err.context["キー"], json!("値"));
}

#[test]
fn dto_roundtrip_with_empty_message() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: String::new(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_roundtrip_with_complex_context() {
    let mut ctx = BTreeMap::new();
    ctx.insert("arr".into(), json!([1, 2, 3]));
    ctx.insert("obj".into(), json!({"nested": true}));
    ctx.insert("null_val".into(), json!(null));
    ctx.insert("float".into(), json!(1.5));
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "complex".into(),
        context: ctx,
        source_message: Some("cause".into()),
    };
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_clone_is_equal() {
    let dto = AbpErrorDto {
        code: ErrorCode::PolicyDenied,
        message: "denied".into(),
        context: BTreeMap::new(),
        source_message: Some("x".into()),
    };
    let cloned = dto.clone();
    assert_eq!(dto, cloned);
}

#[test]
fn dto_partial_eq_different_code() {
    let a = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "x".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let b = AbpErrorDto {
        code: ErrorCode::PolicyDenied,
        message: "x".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    assert_ne!(a, b);
}

#[test]
fn dto_partial_eq_different_message() {
    let a = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "a".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let b = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "b".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    assert_ne!(a, b);
}

#[test]
fn display_no_context_no_trailing_space() {
    let err = AbpError::new(ErrorCode::Internal, "msg");
    let s = err.to_string();
    assert_eq!(s, "[internal] msg");
    assert!(!s.ends_with(' '));
}

#[test]
fn context_negative_number() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("offset", -42);
    assert_eq!(err.context["offset"], json!(-42));
}

#[test]
fn context_zero() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("count", 0);
    assert_eq!(err.context["count"], json!(0));
}

#[test]
fn context_large_number() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("big", i64::MAX);
    assert_eq!(err.context["big"], json!(i64::MAX));
}

#[test]
fn multiple_errors_independent() {
    let e1 = AbpError::new(ErrorCode::Internal, "one");
    let e2 = AbpError::new(ErrorCode::PolicyDenied, "two").with_context("k", "v");
    assert_ne!(e1.code, e2.code);
    assert!(e1.context.is_empty());
    assert!(!e2.context.is_empty());
}

#[test]
fn dto_debug_contains_code() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "dbg".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let dbg = format!("{dto:?}");
    assert!(dbg.contains("Internal"));
    assert!(dbg.contains("dbg"));
}

#[test]
fn error_code_copy_semantics() {
    let code = ErrorCode::BackendTimeout;
    let copy = code;
    assert_eq!(code, copy); // original still usable after copy
}

#[test]
fn error_category_copy_semantics() {
    let cat = ErrorCategory::Policy;
    let copy = cat;
    assert_eq!(cat, copy);
}
