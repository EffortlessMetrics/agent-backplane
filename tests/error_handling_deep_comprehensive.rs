#![allow(
    clippy::useless_vec,
    clippy::needless_borrows_for_generic_args,
    clippy::collapsible_if
)]

//! Comprehensive tests for ABP error handling and error taxonomy system.
//!
//! Covers:
//! - ErrorCode stability and serialization (abp-error)
//! - AbpError construction, Display/Debug, context, source chaining
//! - Error categorization (abp-error categories)
//! - Error chain propagation across layers
//! - ProtocolError mapping to ErrorCodes
//! - RuntimeError mapping to ErrorCodes
//! - Error serialization (AbpErrorDto roundtrips)
//! - Error taxonomy completeness (abp-core ErrorCode catalog)
//! - MappingError taxonomy
//! - Error downcasting from Box<dyn Error>

use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;
use std::io;

// ── abp-error (unified taxonomy) ──────────────────────────────────────
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};

// ── abp-core (extended catalog) ───────────────────────────────────────
use abp_core::error::{
    ErrorCatalog, ErrorCode as CoreErrorCode, ErrorInfo, MappingError, MappingErrorKind,
};

// ── abp-protocol ──────────────────────────────────────────────────────
use abp_protocol::ProtocolError;

// ── abp-runtime ───────────────────────────────────────────────────────
use abp_runtime::RuntimeError;

// ── abp-host ──────────────────────────────────────────────────────────
use abp_host::HostError;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// All 20 abp-error ErrorCode variants in definition order.
const ALL_ABP_ERROR_CODES: &[ErrorCode] = &[
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

// ===========================================================================
// 1. ErrorCode stability — serialization/deserialization, uniqueness, roundtrip
// ===========================================================================

#[test]
fn error_code_count_is_20() {
    assert_eq!(ALL_ABP_ERROR_CODES.len(), 20);
}

#[test]
fn all_error_codes_have_unique_as_str() {
    let mut seen = HashSet::new();
    for code in ALL_ABP_ERROR_CODES {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str: {s}");
    }
    assert_eq!(seen.len(), 20);
}

#[test]
fn error_code_display_matches_as_str() {
    for code in ALL_ABP_ERROR_CODES {
        assert_eq!(code.to_string(), code.as_str());
    }
}

#[test]
fn error_code_serialize_to_screaming_snake() {
    for code in ALL_ABP_ERROR_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "mismatch for {code:?}");
    }
}

#[test]
fn error_code_roundtrip_json_all() {
    for &code in ALL_ABP_ERROR_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "roundtrip failed for {code:?}");
    }
}

#[test]
fn error_code_deserialize_rejects_unknown() {
    let result = serde_json::from_str::<ErrorCode>("\"NONEXISTENT_CODE\"");
    assert!(result.is_err());
}

#[test]
fn error_code_deserialize_rejects_lowercase() {
    let result = serde_json::from_str::<ErrorCode>("\"backend_not_found\"");
    assert!(result.is_err());
}

#[test]
fn error_code_copy_and_clone() {
    let code = ErrorCode::BackendTimeout;
    #[allow(clippy::clone_on_copy)]
    let cloned = code.clone();
    let copied = code;
    assert_eq!(code, cloned);
    assert_eq!(code, copied);
}

#[test]
fn error_code_eq_and_hash() {
    let mut set = HashSet::new();
    set.insert(ErrorCode::Internal);
    set.insert(ErrorCode::Internal);
    assert_eq!(set.len(), 1);
}

#[test]
fn error_code_as_str_specific_values() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "protocol_invalid_envelope"
    );
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
    assert_eq!(ErrorCode::BackendCrashed.as_str(), "backend_crashed");
    assert_eq!(
        ErrorCode::CapabilityUnsupported.as_str(),
        "capability_unsupported"
    );
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.as_str(),
        "workspace_init_failed"
    );
    assert_eq!(ErrorCode::IrLoweringFailed.as_str(), "ir_lowering_failed");
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.as_str(),
        "receipt_hash_mismatch"
    );
    assert_eq!(ErrorCode::DialectUnknown.as_str(), "dialect_unknown");
    assert_eq!(ErrorCode::ConfigInvalid.as_str(), "config_invalid");
    assert_eq!(ErrorCode::Internal.as_str(), "INTERNAL");
}

// ===========================================================================
// 2. AbpError construction — new, with_context, with_source, Display/Debug
// ===========================================================================

#[test]
fn abp_error_new_basic() {
    let err = AbpError::new(ErrorCode::Internal, "something broke");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(err.message, "something broke");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_with_context_string() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
}

#[test]
fn abp_error_with_context_integer() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 30_000);
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_with_context_bool() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config").with_context("strict", true);
    assert_eq!(err.context["strict"], serde_json::json!(true));
}

#[test]
fn abp_error_with_context_nested_json() {
    let nested = serde_json::json!({"a": 1, "b": [2, 3]});
    let err = AbpError::new(ErrorCode::Internal, "nested ctx").with_context("details", &nested);
    assert_eq!(err.context["details"], nested);
}

#[test]
fn abp_error_multiple_context_entries() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    assert_eq!(err.context.len(), 3);
}

#[test]
fn abp_error_context_uses_btreemap_deterministic_order() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("zebra", 1)
        .with_context("alpha", 2)
        .with_context("middle", 3);
    let keys: Vec<&String> = err.context.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn abp_error_with_source_io() {
    let src = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    assert!(err.source.is_some());
    assert_eq!(err.source.as_ref().unwrap().to_string(), "file missing");
}

#[test]
fn abp_error_with_source_and_context() {
    let src = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied")
        .with_context("path", "/secret")
        .with_source(src);
    assert_eq!(err.code, ErrorCode::PolicyDenied);
    assert_eq!(err.context["path"], serde_json::json!("/secret"));
    assert!(err.source.is_some());
}

#[test]
fn abp_error_display_without_context() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    assert_eq!(err.to_string(), "[BACKEND_NOT_FOUND] no such backend");
}

#[test]
fn abp_error_display_with_context_contains_all_parts() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 5000);
    let s = err.to_string();
    assert!(s.starts_with("[BACKEND_TIMEOUT] timed out"));
    assert!(s.contains("timeout_ms"));
    assert!(s.contains("5000"));
}

#[test]
fn abp_error_debug_contains_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "nope");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("PolicyDenied"));
    assert!(dbg.contains("nope"));
}

#[test]
fn abp_error_debug_with_source_shows_source() {
    let src = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("file missing"));
}

#[test]
fn abp_error_debug_with_context_shows_context() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad").with_context("file", "config.toml");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
    assert!(dbg.contains("config.toml"));
}

// ===========================================================================
// 3. Error categorization — each code maps to the correct category
// ===========================================================================

#[test]
fn protocol_codes_map_to_protocol_category() {
    let protocol_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in &protocol_codes {
        assert_eq!(code.category(), ErrorCategory::Protocol, "{code:?}");
    }
}

#[test]
fn backend_codes_map_to_backend_category() {
    let backend_codes = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
    ];
    for code in &backend_codes {
        assert_eq!(code.category(), ErrorCategory::Backend, "{code:?}");
    }
}

#[test]
fn capability_codes_map_to_capability_category() {
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
fn policy_codes_map_to_policy_category() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn workspace_codes_map_to_workspace_category() {
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
fn ir_codes_map_to_ir_category() {
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
}

#[test]
fn receipt_codes_map_to_receipt_category() {
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
fn dialect_codes_map_to_dialect_category() {
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(
        ErrorCode::DialectMappingFailed.category(),
        ErrorCategory::Dialect
    );
}

#[test]
fn config_code_maps_to_config_category() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn internal_code_maps_to_internal_category() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn category_shorthand_on_abp_error() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unknown dialect");
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

#[test]
fn every_error_code_has_a_category() {
    for code in ALL_ABP_ERROR_CODES {
        let _cat = code.category(); // must not panic
    }
}

#[test]
fn every_category_has_at_least_one_code() {
    for cat in ALL_CATEGORIES {
        let count = ALL_ABP_ERROR_CODES
            .iter()
            .filter(|c| c.category() == *cat)
            .count();
        assert!(count >= 1, "category {cat:?} has no codes");
    }
}

// ===========================================================================
// 4. Error chain propagation — abp-error → protocol → host → runtime
// ===========================================================================

#[test]
fn abp_error_std_error_source_chain() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "not found");
    let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(inner);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "not found");
}

#[test]
fn abp_error_source_none_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn abp_error_into_protocol_error() {
    let abp = AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "bad envelope");
    let pe: ProtocolError = abp.into();
    assert!(matches!(pe, ProtocolError::Abp(_)));
    let s = pe.to_string();
    assert!(s.contains("bad envelope"));
}

#[test]
fn abp_error_into_runtime_error() {
    let abp = AbpError::new(ErrorCode::BackendCrashed, "crash");
    let re: RuntimeError = abp.into();
    assert!(matches!(re, RuntimeError::Classified(_)));
}

#[test]
fn protocol_error_into_host_error() {
    let pe = ProtocolError::Violation("test violation".into());
    let he: HostError = pe.into();
    assert!(matches!(he, HostError::Protocol(_)));
    assert!(he.to_string().contains("test violation"));
}

#[test]
fn abp_error_through_protocol_to_host() {
    let abp = AbpError::new(ErrorCode::ProtocolVersionMismatch, "version mismatch");
    let pe: ProtocolError = abp.into();
    let he: HostError = pe.into();
    let displayed = he.to_string();
    assert!(displayed.contains("version mismatch"));
}

#[test]
fn runtime_error_preserves_abp_error_code_classified() {
    let abp = AbpError::new(ErrorCode::PolicyDenied, "denied by policy");
    let re: RuntimeError = abp.into();
    assert_eq!(re.error_code(), ErrorCode::PolicyDenied);
}

#[test]
fn runtime_error_into_abp_error_preserves_classified() {
    let abp = AbpError::new(ErrorCode::ConfigInvalid, "invalid config");
    let re: RuntimeError = abp.into();
    let back = re.into_abp_error();
    assert_eq!(back.code, ErrorCode::ConfigInvalid);
    assert_eq!(back.message, "invalid config");
}

#[test]
fn runtime_error_into_abp_error_unclassified() {
    let re = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    let abp = re.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
    assert!(abp.message.contains("missing"));
}

// ===========================================================================
// 5. Protocol error mapping — ProtocolError variants → ErrorCodes
// ===========================================================================

#[test]
fn protocol_violation_maps_to_invalid_envelope() {
    let pe = ProtocolError::Violation("bad frame".into());
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn protocol_unexpected_message_maps_correctly() {
    let pe = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn protocol_abp_variant_carries_inner_code() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let pe = ProtocolError::Abp(abp);
    assert_eq!(pe.error_code(), Some(ErrorCode::BackendTimeout));
}

#[test]
fn protocol_json_error_has_no_code() {
    let bad = serde_json::from_str::<String>("not json");
    let pe = ProtocolError::Json(bad.unwrap_err());
    assert_eq!(pe.error_code(), None);
}

#[test]
fn protocol_io_error_has_no_code() {
    let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let pe = ProtocolError::Io(io_err);
    assert_eq!(pe.error_code(), None);
}

#[test]
fn protocol_error_display_violation() {
    let pe = ProtocolError::Violation("missing ref_id".into());
    assert_eq!(pe.to_string(), "protocol violation: missing ref_id");
}

#[test]
fn protocol_error_display_unexpected_message() {
    let pe = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "final".into(),
    };
    let s = pe.to_string();
    assert!(s.contains("hello"));
    assert!(s.contains("final"));
}

// ===========================================================================
// 6. Runtime error mapping — RuntimeError variants → ErrorCodes
// ===========================================================================

#[test]
fn runtime_unknown_backend_maps_to_backend_not_found() {
    let re = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(re.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_workspace_failed_maps_to_workspace_init_failed() {
    let re = RuntimeError::WorkspaceFailed(anyhow::anyhow!("oops"));
    assert_eq!(re.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_policy_failed_maps_to_policy_invalid() {
    let re = RuntimeError::PolicyFailed(anyhow::anyhow!("bad policy"));
    assert_eq!(re.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_backend_failed_maps_to_backend_crashed() {
    let re = RuntimeError::BackendFailed(anyhow::anyhow!("boom"));
    assert_eq!(re.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_capability_check_failed_maps_to_capability_unsupported() {
    let re = RuntimeError::CapabilityCheckFailed("missing tool_use".into());
    assert_eq!(re.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_no_projection_match_maps_to_backend_not_found() {
    let re = RuntimeError::NoProjectionMatch {
        reason: "no suitable backend".into(),
    };
    assert_eq!(re.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_classified_preserves_inner_code() {
    let abp = AbpError::new(ErrorCode::IrInvalid, "bad IR");
    let re: RuntimeError = abp.into();
    assert_eq!(re.error_code(), ErrorCode::IrInvalid);
}

#[test]
fn runtime_error_display_unknown_backend() {
    let re = RuntimeError::UnknownBackend { name: "foo".into() };
    assert_eq!(re.to_string(), "unknown backend: foo");
}

#[test]
fn runtime_error_display_workspace_failed() {
    let re = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(re.to_string(), "workspace preparation failed");
}

#[test]
fn runtime_error_display_capability_check() {
    let re = RuntimeError::CapabilityCheckFailed("streaming".into());
    assert_eq!(re.to_string(), "capability check failed: streaming");
}

#[test]
fn runtime_error_display_no_projection() {
    let re = RuntimeError::NoProjectionMatch {
        reason: "no backends".into(),
    };
    assert_eq!(re.to_string(), "projection failed: no backends");
}

// ===========================================================================
// 7. Error serialization — AbpErrorDto roundtrips, JSON structure
// ===========================================================================

#[test]
fn dto_from_abp_error_without_source() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::IrInvalid);
    assert_eq!(dto.message, "bad IR");
    assert_eq!(dto.context["node"], serde_json::json!("call_tool"));
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
fn dto_roundtrip_json_without_source() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_roundtrip_json_with_source_message() {
    let src = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("pipe broke"));
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_message.as_deref(), Some("pipe broke"));
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
    assert!(err.source.is_none()); // opaque source lost
}

#[test]
fn dto_json_skips_null_source_message() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "oops".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

#[test]
fn dto_json_includes_source_message_when_present() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "oops".into(),
        context: BTreeMap::new(),
        source_message: Some("underlying".into()),
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("source_message"));
    assert!(json.contains("underlying"));
}

#[test]
fn dto_json_contains_all_context_fields() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string_pretty(&dto).unwrap();
    assert!(json.contains("backend"));
    assert!(json.contains("openai"));
    assert!(json.contains("timeout_ms"));
    assert!(json.contains("30000"));
}

#[test]
fn dto_json_structure_has_expected_keys() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    let v: serde_json::Value = serde_json::to_value(&dto).unwrap();
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("code"));
    assert!(obj.contains_key("message"));
    assert!(obj.contains_key("context"));
    // source_message absent (None → skip_serializing_if)
    assert!(!obj.contains_key("source_message"));
}

// ===========================================================================
// 8. Error taxonomy completeness (abp-core ErrorCatalog)
// ===========================================================================

#[test]
fn core_error_catalog_has_59_codes() {
    let all = ErrorCatalog::all();
    assert_eq!(all.len(), 59);
}

#[test]
fn core_error_code_strings_are_unique() {
    let all = ErrorCatalog::all();
    let mut seen = HashSet::new();
    for code in &all {
        let s = code.code();
        assert!(seen.insert(s), "duplicate code string: {s}");
    }
}

#[test]
fn core_error_codes_follow_abp_prefix_pattern() {
    for code in ErrorCatalog::all() {
        let s = code.code();
        assert!(s.starts_with("ABP-"), "code {s} missing ABP- prefix");
    }
}

#[test]
fn core_error_code_categories_are_valid() {
    let valid = ["contract", "protocol", "policy", "runtime", "system"];
    for code in ErrorCatalog::all() {
        let cat = code.category();
        assert!(valid.contains(&cat), "unknown category: {cat} for {code:?}");
    }
}

#[test]
fn core_every_category_has_at_least_one_code() {
    for cat in &["contract", "protocol", "policy", "runtime", "system"] {
        let codes = ErrorCatalog::by_category(cat);
        assert!(!codes.is_empty(), "category '{cat}' has no codes");
    }
}

#[test]
fn core_contract_category_has_12_codes() {
    assert_eq!(ErrorCatalog::by_category("contract").len(), 12);
}

#[test]
fn core_protocol_category_has_12_codes() {
    assert_eq!(ErrorCatalog::by_category("protocol").len(), 12);
}

#[test]
fn core_policy_category_has_11_codes() {
    assert_eq!(ErrorCatalog::by_category("policy").len(), 11);
}

#[test]
fn core_runtime_category_has_13_codes() {
    assert_eq!(ErrorCatalog::by_category("runtime").len(), 13);
}

#[test]
fn core_system_category_has_11_codes() {
    assert_eq!(ErrorCatalog::by_category("system").len(), 11);
}

#[test]
fn core_catalog_lookup_valid_code() {
    let code = ErrorCatalog::lookup("ABP-C001");
    assert_eq!(code, Some(CoreErrorCode::InvalidContractVersion));
}

#[test]
fn core_catalog_lookup_all_codes_roundtrip() {
    for code in ErrorCatalog::all() {
        let s = code.code();
        let looked = ErrorCatalog::lookup(s);
        assert_eq!(looked, Some(code), "lookup failed for {s}");
    }
}

#[test]
fn core_catalog_lookup_nonexistent() {
    assert_eq!(ErrorCatalog::lookup("ABP-Z999"), None);
}

#[test]
fn core_error_code_description_nonempty() {
    for code in ErrorCatalog::all() {
        let desc = code.description();
        assert!(!desc.is_empty(), "empty description for {code:?}");
    }
}

#[test]
fn core_error_code_display_is_code_string() {
    for code in ErrorCatalog::all() {
        assert_eq!(code.to_string(), code.code());
    }
}

#[test]
fn core_error_code_serde_roundtrip() {
    for code in ErrorCatalog::all() {
        let json = serde_json::to_string(&code).unwrap();
        let back: CoreErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "serde roundtrip failed for {code:?}");
    }
}

// ===========================================================================
// 8b. ErrorInfo construction and display (abp-core)
// ===========================================================================

#[test]
fn error_info_new_basic() {
    let info = ErrorInfo::new(CoreErrorCode::IoError, "disk full");
    assert_eq!(info.code, CoreErrorCode::IoError);
    assert_eq!(info.message, "disk full");
    assert!(info.context.is_empty());
    assert!(info.source.is_none());
}

#[test]
fn error_info_with_context() {
    let info = ErrorInfo::new(CoreErrorCode::ToolDenied, "tool blocked")
        .with_context("tool", "bash")
        .with_context("policy", "strict");
    assert_eq!(info.context.len(), 2);
    assert_eq!(info.context["tool"], "bash");
    assert_eq!(info.context["policy"], "strict");
}

#[test]
fn error_info_with_source() {
    let src = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let info = ErrorInfo::new(CoreErrorCode::IoError, "read failed").with_source(src);
    assert!(info.source.is_some());
    let s = StdError::source(&info).unwrap();
    assert_eq!(s.to_string(), "file missing");
}

#[test]
fn error_info_display_without_context() {
    let info = ErrorInfo::new(CoreErrorCode::InvalidHash, "hash mismatch");
    let s = info.to_string();
    assert!(s.contains("ABP-C004"));
    assert!(s.contains("hash mismatch"));
}

#[test]
fn error_info_display_with_context() {
    let info =
        ErrorInfo::new(CoreErrorCode::ToolDenied, "tool blocked").with_context("tool", "bash");
    let s = info.to_string();
    assert!(s.contains("ABP-L001"));
    assert!(s.contains("tool=bash"));
}

#[test]
fn error_info_debug_shows_all_fields() {
    let info = ErrorInfo::new(CoreErrorCode::InternalError, "boom").with_context("key", "val");
    let dbg = format!("{info:?}");
    assert!(dbg.contains("ErrorInfo"));
    assert!(dbg.contains("InternalError"));
    assert!(dbg.contains("boom"));
}

// ===========================================================================
// 8c. MappingError taxonomy
// ===========================================================================

#[test]
fn mapping_error_fidelity_loss_is_degraded() {
    let err = MappingError::FidelityLoss {
        field: "system_prompt".into(),
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        detail: "cache_control lost".into(),
    };
    assert_eq!(err.kind(), MappingErrorKind::Degraded);
    assert!(err.is_degraded());
    assert!(!err.is_fatal());
    assert!(!err.is_emulated());
    assert_eq!(err.code(), MappingError::FIDELITY_LOSS_CODE);
}

#[test]
fn mapping_error_unsupported_capability_is_fatal() {
    let err = MappingError::UnsupportedCapability {
        capability: "computer_use".into(),
        dialect: "openai".into(),
    };
    assert_eq!(err.kind(), MappingErrorKind::Fatal);
    assert!(err.is_fatal());
    assert_eq!(err.code(), MappingError::UNSUPPORTED_CAP_CODE);
}

#[test]
fn mapping_error_emulation_required_is_emulated() {
    let err = MappingError::EmulationRequired {
        feature: "extended_thinking".into(),
        detail: "synthesized via chain-of-thought".into(),
    };
    assert_eq!(err.kind(), MappingErrorKind::Emulated);
    assert!(err.is_emulated());
    assert_eq!(err.code(), MappingError::EMULATION_REQUIRED_CODE);
}

#[test]
fn mapping_error_incompatible_model_is_fatal() {
    let err = MappingError::IncompatibleModel {
        requested: "claude-opus-4-20250514".into(),
        dialect: "openai".into(),
        suggestion: Some("gpt-4o".into()),
    };
    assert!(err.is_fatal());
    assert_eq!(err.code(), MappingError::INCOMPATIBLE_MODEL_CODE);
    let s = err.to_string();
    assert!(s.contains("gpt-4o"));
}

#[test]
fn mapping_error_incompatible_model_no_suggestion() {
    let err = MappingError::IncompatibleModel {
        requested: "claude-opus-4-20250514".into(),
        dialect: "openai".into(),
        suggestion: None,
    };
    let s = err.to_string();
    assert!(!s.contains("try"));
}

#[test]
fn mapping_error_parameter_not_mappable_is_degraded() {
    let err = MappingError::ParameterNotMappable {
        parameter: "top_k".into(),
        value: "40".into(),
        dialect: "openai".into(),
    };
    assert!(err.is_degraded());
    assert_eq!(err.code(), MappingError::PARAM_NOT_MAPPABLE_CODE);
}

#[test]
fn mapping_error_streaming_unsupported_is_fatal() {
    let err = MappingError::StreamingUnsupported {
        dialect: "batch-only".into(),
    };
    assert!(err.is_fatal());
    assert_eq!(err.code(), MappingError::STREAMING_UNSUPPORTED_CODE);
}

#[test]
fn mapping_error_all_codes_are_unique() {
    let codes = [
        MappingError::FIDELITY_LOSS_CODE,
        MappingError::UNSUPPORTED_CAP_CODE,
        MappingError::EMULATION_REQUIRED_CODE,
        MappingError::INCOMPATIBLE_MODEL_CODE,
        MappingError::PARAM_NOT_MAPPABLE_CODE,
        MappingError::STREAMING_UNSUPPORTED_CODE,
    ];
    let set: HashSet<_> = codes.iter().collect();
    assert_eq!(set.len(), 6);
}

#[test]
fn mapping_error_serde_roundtrip_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        field: "system_prompt".into(),
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        detail: "cache_control lost".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn mapping_error_serde_roundtrip_all_variants() {
    let variants: Vec<MappingError> = vec![
        MappingError::FidelityLoss {
            field: "f".into(),
            source_dialect: "s".into(),
            target_dialect: "t".into(),
            detail: "d".into(),
        },
        MappingError::UnsupportedCapability {
            capability: "c".into(),
            dialect: "d".into(),
        },
        MappingError::EmulationRequired {
            feature: "f".into(),
            detail: "d".into(),
        },
        MappingError::IncompatibleModel {
            requested: "m".into(),
            dialect: "d".into(),
            suggestion: None,
        },
        MappingError::ParameterNotMappable {
            parameter: "p".into(),
            value: "v".into(),
            dialect: "d".into(),
        },
        MappingError::StreamingUnsupported {
            dialect: "d".into(),
        },
    ];
    for err in &variants {
        let json = serde_json::to_string(err).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
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
        assert_eq!(kind, back);
    }
}

#[test]
fn mapping_error_kind_display() {
    assert_eq!(MappingErrorKind::Fatal.to_string(), "fatal");
    assert_eq!(MappingErrorKind::Degraded.to_string(), "degraded");
    assert_eq!(MappingErrorKind::Emulated.to_string(), "emulated");
}

// ===========================================================================
// 9. Error Display formatting — human-readable messages
// ===========================================================================

#[test]
fn error_category_display_all() {
    assert_eq!(ErrorCategory::Protocol.to_string(), "protocol");
    assert_eq!(ErrorCategory::Backend.to_string(), "backend");
    assert_eq!(ErrorCategory::Capability.to_string(), "capability");
    assert_eq!(ErrorCategory::Policy.to_string(), "policy");
    assert_eq!(ErrorCategory::Workspace.to_string(), "workspace");
    assert_eq!(ErrorCategory::Ir.to_string(), "ir");
    assert_eq!(ErrorCategory::Receipt.to_string(), "receipt");
    assert_eq!(ErrorCategory::Dialect.to_string(), "dialect");
    assert_eq!(ErrorCategory::Config.to_string(), "config");
    assert_eq!(ErrorCategory::Internal.to_string(), "internal");
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
fn error_category_serde_is_snake_case() {
    let json = serde_json::to_string(&ErrorCategory::Backend).unwrap();
    assert_eq!(json, "\"backend\"");
}

#[test]
fn host_error_display_spawn() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "binary not found");
    let he = HostError::Spawn(io_err);
    assert!(he.to_string().contains("spawn sidecar"));
    assert!(he.to_string().contains("binary not found"));
}

#[test]
fn host_error_display_violation() {
    let he = HostError::Violation("missing hello".into());
    assert!(he.to_string().contains("protocol violation"));
    assert!(he.to_string().contains("missing hello"));
}

#[test]
fn host_error_display_fatal() {
    let he = HostError::Fatal("out of memory".into());
    assert!(he.to_string().contains("fatal error"));
    assert!(he.to_string().contains("out of memory"));
}

#[test]
fn host_error_display_exited() {
    let he = HostError::Exited { code: Some(1) };
    let s = he.to_string();
    assert!(s.contains("exited unexpectedly"));
}

#[test]
fn host_error_display_sidecar_crashed() {
    let he = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed by signal".into(),
    };
    let s = he.to_string();
    assert!(s.contains("crashed"));
    assert!(s.contains("137"));
}

#[test]
fn host_error_display_timeout() {
    let he = HostError::Timeout {
        duration: std::time::Duration::from_secs(30),
    };
    let s = he.to_string();
    assert!(s.contains("timed out"));
}

// ===========================================================================
// 10. Error downcasting — Box<dyn Error> → specific types
// ===========================================================================

#[test]
fn downcast_abp_error_from_box_dyn() {
    let err = AbpError::new(ErrorCode::Internal, "test");
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(err);
    let downcasted = boxed.downcast::<AbpError>();
    assert!(downcasted.is_ok());
    assert_eq!(downcasted.unwrap().code, ErrorCode::Internal);
}

#[test]
fn downcast_protocol_error_from_box_dyn() {
    let err = ProtocolError::Violation("test".into());
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(err);
    let downcasted = boxed.downcast::<ProtocolError>();
    assert!(downcasted.is_ok());
}

#[test]
fn downcast_runtime_error_from_box_dyn() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(err);
    let downcasted = boxed.downcast::<RuntimeError>();
    assert!(downcasted.is_ok());
}

#[test]
fn downcast_host_error_from_box_dyn() {
    let err = HostError::Fatal("boom".into());
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(err);
    let downcasted = boxed.downcast::<HostError>();
    assert!(downcasted.is_ok());
}

#[test]
fn downcast_error_info_from_box_dyn() {
    let info = ErrorInfo::new(CoreErrorCode::InternalError, "test");
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(info);
    let downcasted = boxed.downcast::<ErrorInfo>();
    assert!(downcasted.is_ok());
    assert_eq!(downcasted.unwrap().code, CoreErrorCode::InternalError);
}

#[test]
fn downcast_mapping_error_from_box_dyn() {
    let err = MappingError::StreamingUnsupported {
        dialect: "x".into(),
    };
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(err);
    let downcasted = boxed.downcast::<MappingError>();
    assert!(downcasted.is_ok());
}

#[test]
fn failed_downcast_returns_err() {
    let err = AbpError::new(ErrorCode::Internal, "test");
    let boxed: Box<dyn StdError + Send + Sync> = Box::new(err);
    let result = boxed.downcast::<ProtocolError>();
    assert!(result.is_err());
}

#[test]
fn abp_error_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AbpError>();
}

#[test]
fn protocol_error_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProtocolError>();
}

#[test]
fn runtime_error_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeError>();
}

#[test]
fn host_error_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<HostError>();
}
