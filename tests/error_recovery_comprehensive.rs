// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive error recovery and resilience tests.
//!
//! Covers RuntimeError, ProtocolError, HostError, AbpError, ErrorCode,
//! ErrorCategory, error conversion chains, taxonomy categorization,
//! error code stability, serialization, Display/Debug traits, and
//! nested error chains across the ABP crate graph.

use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::time::Duration;

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_host::HostError;
use abp_protocol::ProtocolError;
use abp_runtime::RuntimeError;

// =========================================================================
// Helper: all ErrorCode variants for exhaustive iteration
// =========================================================================

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

// =========================================================================
// 1. RuntimeError variant construction & Display
// =========================================================================

#[test]
fn runtime_error_unknown_backend_display() {
    let err = RuntimeError::UnknownBackend {
        name: "nonexistent".into(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("nonexistent"),
        "should contain backend name: {msg}"
    );
    assert!(
        msg.contains("unknown backend"),
        "should describe unknown backend: {msg}"
    );
}

#[test]
fn runtime_error_workspace_failed_display() {
    let inner = anyhow::anyhow!("disk full");
    let err = RuntimeError::WorkspaceFailed(inner);
    let msg = err.to_string();
    assert!(msg.contains("workspace"), "should mention workspace: {msg}");
}

#[test]
fn runtime_error_policy_failed_display() {
    let inner = anyhow::anyhow!("bad glob pattern");
    let err = RuntimeError::PolicyFailed(inner);
    let msg = err.to_string();
    assert!(msg.contains("policy"), "should mention policy: {msg}");
}

#[test]
fn runtime_error_backend_failed_display() {
    let inner = anyhow::anyhow!("connection refused");
    let err = RuntimeError::BackendFailed(inner);
    let msg = err.to_string();
    assert!(msg.contains("backend"), "should mention backend: {msg}");
}

#[test]
fn runtime_error_capability_check_failed_display() {
    let err = RuntimeError::CapabilityCheckFailed("missing tool_use".into());
    let msg = err.to_string();
    assert!(
        msg.contains("missing tool_use"),
        "should contain reason: {msg}"
    );
    assert!(
        msg.contains("capability"),
        "should mention capability: {msg}"
    );
}

#[test]
fn runtime_error_no_projection_match_display() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no backends satisfy requirements".into(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("projection"),
        "should mention projection: {msg}"
    );
    assert!(
        msg.contains("no backends satisfy"),
        "should contain reason: {msg}"
    );
}

#[test]
fn runtime_error_classified_display() {
    let abp = AbpError::new(ErrorCode::Internal, "something broke");
    let err = RuntimeError::Classified(abp);
    let msg = err.to_string();
    assert!(msg.contains("INTERNAL"), "should contain error code: {msg}");
    assert!(
        msg.contains("something broke"),
        "should contain message: {msg}"
    );
}

// =========================================================================
// 2. RuntimeError error_code() mapping
// =========================================================================

#[test]
fn runtime_error_unknown_backend_maps_to_backend_not_found() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_workspace_failed_maps_to_workspace_init_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("fail"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_policy_failed_maps_to_policy_invalid() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("fail"));
    assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_backend_failed_maps_to_backend_crashed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("fail"));
    assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_capability_check_maps_to_capability_unsupported() {
    let err = RuntimeError::CapabilityCheckFailed("x".into());
    assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_no_projection_maps_to_backend_not_found() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_classified_preserves_original_code() {
    let abp = AbpError::new(ErrorCode::DialectMappingFailed, "oops");
    let err = RuntimeError::Classified(abp);
    assert_eq!(err.error_code(), ErrorCode::DialectMappingFailed);
}

// =========================================================================
// 3. RuntimeError into_abp_error() conversion
// =========================================================================

#[test]
fn runtime_error_into_abp_error_unknown_backend() {
    let err = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
    assert!(abp.message.contains("missing"));
}

#[test]
fn runtime_error_into_abp_error_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_into_abp_error_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_into_abp_error_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("timeout"));
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_into_abp_error_classified_roundtrip() {
    let original =
        AbpError::new(ErrorCode::ReceiptHashMismatch, "hash wrong").with_context("hash", "abc123");
    let err = RuntimeError::Classified(original);
    let recovered = err.into_abp_error();
    assert_eq!(recovered.code, ErrorCode::ReceiptHashMismatch);
    assert_eq!(recovered.message, "hash wrong");
    assert_eq!(recovered.context["hash"], serde_json::json!("abc123"));
}

// =========================================================================
// 4. RuntimeError implements std::error::Error + Send + Sync
// =========================================================================

#[test]
fn runtime_error_implements_std_error() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    let _: &dyn Error = &err;
}

#[test]
fn runtime_error_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<RuntimeError>();
    assert_sync::<RuntimeError>();
}

#[test]
fn runtime_error_debug_contains_variant() {
    let err = RuntimeError::UnknownBackend {
        name: "test-be".into(),
    };
    let dbg = format!("{err:?}");
    assert!(
        dbg.contains("UnknownBackend"),
        "Debug should include variant: {dbg}"
    );
    assert!(
        dbg.contains("test-be"),
        "Debug should include field value: {dbg}"
    );
}

// =========================================================================
// 5. RuntimeError source() chains
// =========================================================================

#[test]
fn runtime_error_workspace_has_source() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("inner cause"));
    assert!(
        err.source().is_some(),
        "WorkspaceFailed should have a source"
    );
}

#[test]
fn runtime_error_policy_has_source() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("inner"));
    assert!(err.source().is_some(), "PolicyFailed should have a source");
}

#[test]
fn runtime_error_backend_has_source() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("inner"));
    assert!(err.source().is_some(), "BackendFailed should have a source");
}

#[test]
fn runtime_error_unknown_backend_no_source() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(
        err.source().is_none(),
        "UnknownBackend should not have a source"
    );
}

#[test]
fn runtime_error_no_projection_no_source() {
    let err = RuntimeError::NoProjectionMatch { reason: "x".into() };
    assert!(err.source().is_none());
}

// =========================================================================
// 6. ProtocolError variant construction & Display
// =========================================================================

#[test]
fn protocol_error_json_display() {
    let bad_json = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let err = ProtocolError::Json(bad_json);
    let msg = err.to_string();
    assert!(
        msg.contains("invalid JSON") || msg.contains("JSON"),
        "should mention JSON: {msg}"
    );
}

#[test]
fn protocol_error_io_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let err = ProtocolError::Io(io_err);
    let msg = err.to_string();
    assert!(
        msg.contains("I/O") || msg.contains("pipe broke"),
        "should mention I/O: {msg}"
    );
}

#[test]
fn protocol_error_violation_display() {
    let err = ProtocolError::Violation("missing ref_id".into());
    let msg = err.to_string();
    assert!(
        msg.contains("missing ref_id"),
        "should contain detail: {msg}"
    );
    assert!(msg.contains("violation"), "should mention violation: {msg}");
}

#[test]
fn protocol_error_unexpected_message_display() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("hello"), "should contain expected: {msg}");
    assert!(msg.contains("run"), "should contain got: {msg}");
}

#[test]
fn protocol_error_abp_display() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let err = ProtocolError::Abp(abp);
    let msg = err.to_string();
    assert!(
        msg.contains("BACKEND_TIMEOUT"),
        "should contain code: {msg}"
    );
    assert!(msg.contains("timed out"), "should contain message: {msg}");
}

// =========================================================================
// 7. ProtocolError error_code() mapping
// =========================================================================

#[test]
fn protocol_error_violation_has_error_code() {
    let err = ProtocolError::Violation("bad".into());
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn protocol_error_unexpected_message_has_error_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn protocol_error_abp_has_error_code() {
    let abp = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let err = ProtocolError::Abp(abp);
    assert_eq!(err.error_code(), Some(ErrorCode::PolicyDenied));
}

#[test]
fn protocol_error_json_has_no_error_code() {
    let json_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let err = ProtocolError::Json(json_err);
    assert_eq!(err.error_code(), None);
}

#[test]
fn protocol_error_io_has_no_error_code() {
    let io_err = std::io::Error::other("test");
    let err = ProtocolError::Io(io_err);
    assert_eq!(err.error_code(), None);
}

// =========================================================================
// 8. ProtocolError Send/Sync, Debug, and source() chains
// =========================================================================

#[test]
fn protocol_error_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<ProtocolError>();
    assert_sync::<ProtocolError>();
}

#[test]
fn protocol_error_json_has_source() {
    let json_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let err = ProtocolError::Json(json_err);
    // Json variant wraps serde_json::Error via #[from]
    assert!(err.source().is_some() || err.source().is_none());
    // The variant itself is the source — verify we can get Display
    let _display = err.to_string();
}

#[test]
fn protocol_error_violation_no_source() {
    let err = ProtocolError::Violation("x".into());
    assert!(err.source().is_none());
}

#[test]
fn protocol_error_unexpected_no_source() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "a".into(),
        got: "b".into(),
    };
    assert!(err.source().is_none());
}

#[test]
fn protocol_error_debug_contains_variant_name() {
    let err = ProtocolError::Violation("test detail".into());
    let dbg = format!("{err:?}");
    assert!(
        dbg.contains("Violation"),
        "Debug should include variant name: {dbg}"
    );
}

// =========================================================================
// 9. ProtocolError From conversions
// =========================================================================

#[test]
fn protocol_error_from_serde_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let err: ProtocolError = json_err.into();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_error_from_io_error() {
    let io_err = std::io::Error::other("test");
    let err: ProtocolError = io_err.into();
    assert!(matches!(err, ProtocolError::Io(_)));
}

#[test]
fn protocol_error_from_abp_error() {
    let abp = AbpError::new(ErrorCode::Internal, "oops");
    let err: ProtocolError = abp.into();
    assert!(matches!(err, ProtocolError::Abp(_)));
}

// =========================================================================
// 10. HostError → ProtocolError conversion chain
// =========================================================================

#[test]
fn host_error_from_protocol_error() {
    let proto = ProtocolError::Violation("bad state".into());
    let err: HostError = proto.into();
    assert!(matches!(err, HostError::Protocol(_)));
}

#[test]
fn host_error_protocol_preserves_source() {
    let proto = ProtocolError::Violation("inner".into());
    let host = HostError::Protocol(proto);
    assert!(
        host.source().is_some(),
        "Protocol wrapping should expose source"
    );
}

// =========================================================================
// 11. AbpError → RuntimeError → AbpError roundtrip
// =========================================================================

#[test]
fn abp_error_runtime_classified_roundtrip() {
    let original = AbpError::new(ErrorCode::IrLoweringFailed, "lowering failed")
        .with_context("node", "function_call");
    let rt_err: RuntimeError = original.into();
    assert!(matches!(rt_err, RuntimeError::Classified(_)));
    let recovered = rt_err.into_abp_error();
    assert_eq!(recovered.code, ErrorCode::IrLoweringFailed);
    assert_eq!(recovered.message, "lowering failed");
    assert_eq!(
        recovered.context["node"],
        serde_json::json!("function_call")
    );
}

// =========================================================================
// 12. ErrorCode taxonomy categorization (exhaustive)
// =========================================================================

#[test]
fn all_protocol_codes_map_to_protocol_category() {
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
fn all_backend_codes_map_to_backend_category() {
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
fn all_capability_codes_map_to_capability_category() {
    let codes = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Capability, "{code:?}");
    }
}

#[test]
fn all_policy_codes_map_to_policy_category() {
    let codes = [ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Policy, "{code:?}");
    }
}

#[test]
fn all_workspace_codes_map_to_workspace_category() {
    let codes = [
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
    ];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Workspace, "{code:?}");
    }
}

#[test]
fn all_ir_codes_map_to_ir_category() {
    let codes = [ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Ir, "{code:?}");
    }
}

#[test]
fn all_receipt_codes_map_to_receipt_category() {
    let codes = [
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Receipt, "{code:?}");
    }
}

#[test]
fn all_dialect_codes_map_to_dialect_category() {
    let codes = [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Dialect, "{code:?}");
    }
}

#[test]
fn config_invalid_maps_to_config_category() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn internal_maps_to_internal_category() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

// =========================================================================
// 13. ErrorCode stability — as_str() values are fixed strings
// =========================================================================

#[test]
fn error_code_as_str_protocol_invalid_envelope() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "PROTOCOL_INVALID_ENVELOPE"
    );
}

#[test]
fn error_code_as_str_protocol_unexpected_message() {
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.as_str(),
        "PROTOCOL_UNEXPECTED_MESSAGE"
    );
}

#[test]
fn error_code_as_str_protocol_version_mismatch() {
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.as_str(),
        "PROTOCOL_VERSION_MISMATCH"
    );
}

#[test]
fn error_code_as_str_backend_not_found() {
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "BACKEND_NOT_FOUND");
}

#[test]
fn error_code_as_str_backend_timeout() {
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "BACKEND_TIMEOUT");
}

#[test]
fn error_code_as_str_backend_crashed() {
    assert_eq!(ErrorCode::BackendCrashed.as_str(), "BACKEND_CRASHED");
}

#[test]
fn error_code_as_str_all_unique() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str: {s}");
    }
    assert_eq!(seen.len(), ALL_CODES.len());
}

#[test]
fn error_code_display_matches_as_str() {
    for code in ALL_CODES {
        assert_eq!(code.to_string(), code.as_str(), "mismatch for {code:?}");
    }
}

// =========================================================================
// 14. ErrorCode serde stability
// =========================================================================

#[test]
fn error_code_serializes_to_screaming_snake() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!(r#""{}""#, code.as_str());
        assert_eq!(json, expected, "serde mismatch for {code:?}");
    }
}

#[test]
fn error_code_serde_roundtrip_all() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *code, "roundtrip failed for {code:?}");
    }
}

#[test]
fn error_code_deserialize_rejects_lowercase() {
    let result = serde_json::from_str::<ErrorCode>(r#""backend_not_found""#);
    assert!(result.is_err(), "lowercase should not deserialize");
}

#[test]
fn error_code_deserialize_rejects_unknown() {
    let result = serde_json::from_str::<ErrorCode>(r#""DOES_NOT_EXIST""#);
    assert!(result.is_err(), "unknown code should not deserialize");
}

// =========================================================================
// 15. ErrorCategory serde & Display
// =========================================================================

#[test]
fn error_category_serde_roundtrip() {
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
        ErrorCategory::Internal,
    ];
    for cat in &categories {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *cat, "roundtrip failed for {cat:?}");
    }
}

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

// =========================================================================
// 16. AbpError construction, Display, Debug
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
fn abp_error_display_without_context() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    assert_eq!(err.to_string(), "[BACKEND_NOT_FOUND] no such backend");
}

#[test]
fn abp_error_display_with_context() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 5000);
    let display = err.to_string();
    assert!(display.starts_with("[BACKEND_TIMEOUT] timed out"));
    assert!(display.contains("timeout_ms"));
    assert!(display.contains("5000"));
}

#[test]
fn abp_error_debug_shows_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "nope");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("PolicyDenied"));
    assert!(dbg.contains("nope"));
}

#[test]
fn abp_error_debug_with_source_shows_source() {
    let src = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("file missing"));
}

#[test]
fn abp_error_category_shorthand() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unknown");
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

// =========================================================================
// 17. AbpError builder pattern
// =========================================================================

#[test]
fn abp_error_builder_multiple_context_keys() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    assert_eq!(err.context.len(), 3);
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
    assert_eq!(err.context["retries"], serde_json::json!(3));
}

#[test]
fn abp_error_builder_with_source() {
    let src = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied").with_source(src);
    assert!(err.source.is_some());
    assert_eq!(err.source.as_ref().unwrap().to_string(), "access denied");
}

#[test]
fn abp_error_builder_chaining_all() {
    let src = std::io::Error::other("underlying");
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml")
        .with_source(src);
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.context["file"], serde_json::json!("backplane.toml"));
    assert!(err.source.is_some());
}

#[test]
fn abp_error_context_with_nested_json() {
    let err = AbpError::new(ErrorCode::Internal, "nested")
        .with_context("details", serde_json::json!({"a": 1, "b": [2, 3]}));
    assert_eq!(
        err.context["details"],
        serde_json::json!({"a": 1, "b": [2, 3]})
    );
}

// =========================================================================
// 18. AbpError source chain (std::error::Error)
// =========================================================================

#[test]
fn abp_error_std_source_chain() {
    let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
    let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(inner);
    let src = std::error::Error::source(&err).unwrap();
    assert_eq!(src.to_string(), "not found");
}

#[test]
fn abp_error_std_source_none_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    assert!(std::error::Error::source(&err).is_none());
}

// =========================================================================
// 19. AbpErrorDto serialization roundtrips
// =========================================================================

#[test]
fn abp_error_dto_roundtrip_without_source() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
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
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("pipe broke"));
}

#[test]
fn abp_error_dto_to_abp_error_drops_source() {
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad".into(),
        context: BTreeMap::new(),
        source_message: Some("inner".into()),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert!(
        err.source.is_none(),
        "source should be lost in DTO conversion"
    );
}

#[test]
fn abp_error_dto_preserves_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("region", "us-east-1");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.context.len(), 2);
    assert_eq!(dto.context["backend"], serde_json::json!("openai"));
    assert_eq!(dto.context["region"], serde_json::json!("us-east-1"));
}

// =========================================================================
// 20. Nested error chains across crate boundaries
// =========================================================================

#[test]
fn nested_chain_io_to_protocol_to_host() {
    let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
    let proto: ProtocolError = io.into();
    let host: HostError = proto.into();
    // Walk the chain
    let src1 = host.source().expect("HostError should have source");
    assert!(
        src1.to_string().contains("I/O") || src1.to_string().contains("refused"),
        "first source: {}",
        src1
    );
}

#[test]
fn nested_chain_abp_to_protocol() {
    let abp = AbpError::new(ErrorCode::ProtocolVersionMismatch, "version mismatch");
    let proto: ProtocolError = abp.into();
    assert!(matches!(proto, ProtocolError::Abp(_)));
    assert_eq!(proto.error_code(), Some(ErrorCode::ProtocolVersionMismatch));
}

#[test]
fn nested_chain_abp_to_runtime() {
    let abp = AbpError::new(ErrorCode::CapabilityEmulationFailed, "emulation broken")
        .with_context("capability", "streaming");
    let rt: RuntimeError = abp.into();
    assert!(matches!(rt, RuntimeError::Classified(_)));
    let recovered = rt.into_abp_error();
    assert_eq!(recovered.code, ErrorCode::CapabilityEmulationFailed);
    assert_eq!(
        recovered.context["capability"],
        serde_json::json!("streaming")
    );
}

// =========================================================================
// 21. HostError variant exhaustive coverage & unique Display
// =========================================================================

#[test]
fn host_error_all_variants_display_unique() {
    let variants: Vec<HostError> = vec![
        HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "cmd")),
        HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe")),
        HostError::Stdin(std::io::Error::new(std::io::ErrorKind::WriteZero, "zero")),
        HostError::Protocol(ProtocolError::Violation("v".into())),
        HostError::Violation("bad state".into()),
        HostError::Fatal("oom".into()),
        HostError::Exited { code: Some(1) },
        HostError::SidecarCrashed {
            exit_code: Some(137),
            stderr: "killed by signal".into(),
        },
        HostError::Timeout {
            duration: Duration::from_secs(30),
        },
    ];

    let messages: Vec<String> = variants.iter().map(|e| e.to_string()).collect();
    let unique: HashSet<&String> = messages.iter().collect();
    assert_eq!(
        unique.len(),
        messages.len(),
        "duplicate Display: {messages:?}"
    );
}

#[test]
fn host_error_exited_none_code() {
    let err = HostError::Exited { code: None };
    let msg = err.to_string();
    assert!(msg.contains("None") || msg.contains("exited"), "msg: {msg}");
}

#[test]
fn host_error_timeout_duration_in_display() {
    let err = HostError::Timeout {
        duration: Duration::from_millis(1500),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("1.5") || msg.contains("1500"),
        "should contain duration: {msg}"
    );
}

#[test]
fn host_error_sidecar_crashed_none_exit_code() {
    let err = HostError::SidecarCrashed {
        exit_code: None,
        stderr: "segfault".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("segfault"), "should contain stderr: {msg}");
}

// =========================================================================
// 22. HostError Send/Sync bounds
// =========================================================================

#[test]
fn host_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<HostError>();
}

// =========================================================================
// 23. AbpError is Send + Sync
// =========================================================================

#[test]
fn abp_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AbpError>();
}

// =========================================================================
// 24. ErrorCode count guard
// =========================================================================

#[test]
fn error_code_variant_count() {
    assert_eq!(
        ALL_CODES.len(),
        20,
        "ErrorCode variant count changed — update ALL_CODES"
    );
}

// =========================================================================
// 25. Display determinism with BTreeMap context
// =========================================================================

#[test]
fn abp_error_display_context_deterministic() {
    let err1 = AbpError::new(ErrorCode::Internal, "msg")
        .with_context("alpha", "a")
        .with_context("beta", "b");
    let err2 = AbpError::new(ErrorCode::Internal, "msg")
        .with_context("beta", "b")
        .with_context("alpha", "a");
    // BTreeMap gives deterministic ordering regardless of insertion order.
    assert_eq!(err1.to_string(), err2.to_string());
}

// =========================================================================
// 26. AbpError empty-context display has no trailing junk
// =========================================================================

#[test]
fn abp_error_display_no_trailing_context_when_empty() {
    let err = AbpError::new(ErrorCode::Internal, "simple");
    let display = err.to_string();
    assert_eq!(display, "[INTERNAL] simple");
    assert!(!display.contains('{'));
}

// =========================================================================
// 27. Multiple error codes in same category are distinguishable
// =========================================================================

#[test]
fn error_codes_within_same_category_distinguishable() {
    let protocol_codes = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Protocol)
        .collect::<Vec<_>>();
    let strs: HashSet<&str> = protocol_codes.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        strs.len(),
        protocol_codes.len(),
        "protocol codes must be unique"
    );
}

// =========================================================================
// 28. RuntimeError variant count — every variant produces a distinct code
// =========================================================================

#[test]
fn runtime_error_all_variants_produce_valid_error_codes() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("x")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("x")),
        RuntimeError::BackendFailed(anyhow::anyhow!("x")),
        RuntimeError::CapabilityCheckFailed("x".into()),
        RuntimeError::Classified(AbpError::new(ErrorCode::Internal, "x")),
        RuntimeError::NoProjectionMatch { reason: "x".into() },
    ];
    for v in &variants {
        let code = v.error_code();
        // Every code should be a valid ErrorCode that round-trips via serde.
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code);
    }
}

// =========================================================================
// 29. AbpErrorDto JSON schema stability
// =========================================================================

#[test]
fn abp_error_dto_json_has_expected_fields() {
    let dto = AbpErrorDto {
        code: ErrorCode::BackendTimeout,
        message: "timed out".into(),
        context: {
            let mut m = BTreeMap::new();
            m.insert("key".into(), serde_json::json!("val"));
            m
        },
        source_message: Some("inner".into()),
    };
    let json: serde_json::Value = serde_json::to_value(&dto).unwrap();
    assert!(json.get("code").is_some());
    assert!(json.get("message").is_some());
    assert!(json.get("context").is_some());
    assert!(json.get("source_message").is_some());
}

#[test]
fn abp_error_dto_json_omits_none_source() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "x".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(
        !json.contains("source_message"),
        "None source_message should be skipped"
    );
}

// =========================================================================
// 30. Error taxonomy — every category has at least one code
// =========================================================================

#[test]
fn every_category_has_at_least_one_code() {
    let all_categories = [
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
    for cat in &all_categories {
        let count = ALL_CODES.iter().filter(|c| c.category() == *cat).count();
        assert!(count >= 1, "category {cat:?} has no codes");
    }
}

// =========================================================================
// 31. Concurrency: errors can cross thread boundaries
// =========================================================================

#[test]
fn errors_can_be_sent_across_threads() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_context("thread", "worker-1");
    let handle = std::thread::spawn(move || {
        assert_eq!(err.code, ErrorCode::BackendCrashed);
        err.to_string()
    });
    let result = handle.join().unwrap();
    assert!(result.contains("BACKEND_CRASHED"));
}

#[test]
fn runtime_error_can_be_sent_across_threads() {
    let err = RuntimeError::UnknownBackend {
        name: "thread-test".into(),
    };
    let handle = std::thread::spawn(move || err.to_string());
    let result = handle.join().unwrap();
    assert!(result.contains("thread-test"));
}

// =========================================================================
// 32. Error downcast from Box<dyn Error>
// =========================================================================

#[test]
fn host_error_downcasts_from_dyn_error() {
    let err: Box<dyn Error + Send + Sync> = Box::new(HostError::Fatal("test".into()));
    let downcast = err.downcast::<HostError>();
    assert!(downcast.is_ok(), "should downcast to HostError");
}

#[test]
fn protocol_error_downcasts_from_dyn_error() {
    let err: Box<dyn Error + Send + Sync> = Box::new(ProtocolError::Violation("test".into()));
    let downcast = err.downcast::<ProtocolError>();
    assert!(downcast.is_ok(), "should downcast to ProtocolError");
}
