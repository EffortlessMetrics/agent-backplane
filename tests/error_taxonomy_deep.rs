//! Comprehensive error taxonomy tests covering serialization, conversions,
//! display formatting, metadata preservation, and cross-crate interoperability.

use std::collections::{BTreeMap, HashSet};

use abp_core::{AgentEvent, AgentEventKind, ContractError, Outcome, ReceiptBuilder};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_runtime::RuntimeError;

// ─── helpers ────────────────────────────────────────────────────────────────

/// Exhaustive list of all 20 error codes.
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

/// Exhaustive list of all 10 error categories.
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

// ═══════════════════════════════════════════════════════════════════════════
// 1. ErrorCategory serialization / deserialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn category_serde_roundtrip_all() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back, "roundtrip failed for {cat:?}");
    }
}

#[test]
fn category_serializes_to_snake_case() {
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Protocol).unwrap(),
        r#""protocol""#
    );
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Internal).unwrap(),
        r#""internal""#
    );
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Ir).unwrap(),
        r#""ir""#
    );
}

#[test]
fn category_deserializes_from_snake_case() {
    let cat: ErrorCategory = serde_json::from_str(r#""backend""#).unwrap();
    assert_eq!(cat, ErrorCategory::Backend);
}

#[test]
fn category_rejects_unknown_variant() {
    let result = serde_json::from_str::<ErrorCategory>(r#""unknown_variant""#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. ErrorCode serde roundtrip stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_serde_roundtrip_all() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back, "roundtrip failed for {code:?}");
    }
}

#[test]
fn error_code_serializes_to_screaming_snake_case() {
    let json = serde_json::to_string(&ErrorCode::BackendTimeout).unwrap();
    assert_eq!(json, r#""BACKEND_TIMEOUT""#);
}

#[test]
fn error_code_as_str_matches_serde() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!(r#""{}""#, code.as_str());
        assert_eq!(json, expected, "as_str/serde mismatch for {code:?}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Error Display messages are human-readable
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_display_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "backend 'foo' not found");
    let s = err.to_string();
    assert!(s.contains("BACKEND_NOT_FOUND"), "missing code in: {s}");
    assert!(
        s.contains("backend 'foo' not found"),
        "missing message in: {s}"
    );
}

#[test]
fn abp_error_display_includes_context_when_present() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("timeout_ms", 5000)
        .with_context("backend", "openai");
    let s = err.to_string();
    assert!(s.contains("timeout_ms"), "missing context key in: {s}");
    assert!(s.contains("5000"), "missing context value in: {s}");
}

#[test]
fn abp_error_display_omits_context_when_empty() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let s = err.to_string();
    // Should be exactly "[INTERNAL] oops" with no trailing JSON
    assert_eq!(s, "[INTERNAL] oops");
}

#[test]
fn error_code_display_matches_as_str() {
    for code in ALL_CODES {
        assert_eq!(code.to_string(), code.as_str());
    }
}

#[test]
fn error_category_display_is_lowercase() {
    for cat in ALL_CATEGORIES {
        let s = cat.to_string();
        assert_eq!(s, s.to_lowercase(), "category display not lowercase: {s}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Error conversion chain: RuntimeError → AbpError
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_unknown_backend_converts_to_abp_error() {
    let rt = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    let abp = rt.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
    assert!(abp.message.contains("missing"));
}

#[test]
fn runtime_workspace_failed_converts_to_abp_error() {
    let rt = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    let abp = rt.into_abp_error();
    assert_eq!(abp.code, ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_policy_failed_converts_to_abp_error() {
    let rt = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    let abp = rt.into_abp_error();
    assert_eq!(abp.code, ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_backend_failed_converts_to_abp_error() {
    let rt = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    let abp = rt.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendCrashed);
}

#[test]
fn runtime_capability_check_converts_to_abp_error() {
    let rt = RuntimeError::CapabilityCheckFailed("mcp unsupported".into());
    let abp = rt.into_abp_error();
    assert_eq!(abp.code, ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_no_projection_converts_to_abp_error() {
    let rt = RuntimeError::NoProjectionMatch {
        reason: "no match".into(),
    };
    let abp = rt.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
}

#[test]
fn runtime_classified_preserves_abp_error() {
    let original = AbpError::new(ErrorCode::DialectMappingFailed, "mapping broke")
        .with_context("from", "claude")
        .with_context("to", "openai");
    let rt: RuntimeError = original.into();
    let recovered = rt.into_abp_error();
    assert_eq!(recovered.code, ErrorCode::DialectMappingFailed);
    assert_eq!(recovered.message, "mapping broke");
    assert_eq!(recovered.context.len(), 2);
}

#[test]
fn abp_error_into_runtime_error_via_from() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow");
    let rt: RuntimeError = err.into();
    assert_eq!(rt.error_code(), ErrorCode::BackendTimeout);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. ProtocolError variants cover all protocol failures
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_json_variant() {
    let bad_json = serde_json::from_str::<serde_json::Value>("not json");
    let err: ProtocolError = bad_json.unwrap_err().into();
    assert!(matches!(err, ProtocolError::Json(_)));
    assert!(err.error_code().is_none());
}

#[test]
fn protocol_error_io_variant() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let err: ProtocolError = io_err.into();
    assert!(matches!(err, ProtocolError::Io(_)));
    assert!(err.error_code().is_none());
}

#[test]
fn protocol_error_violation_variant() {
    let err = ProtocolError::Violation("missing hello".into());
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
    assert!(err.to_string().contains("missing hello"));
}

#[test]
fn protocol_error_unexpected_message_variant() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
    let s = err.to_string();
    assert!(s.contains("hello"));
    assert!(s.contains("run"));
}

#[test]
fn protocol_error_abp_variant() {
    let abp = AbpError::new(ErrorCode::ProtocolVersionMismatch, "v0.2 vs v0.1");
    let err: ProtocolError = abp.into();
    assert!(matches!(err, ProtocolError::Abp(_)));
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolVersionMismatch));
}

#[test]
fn protocol_error_display_json() {
    let result = JsonlCodec::decode("not json");
    let err = result.unwrap_err();
    let s = err.to_string();
    assert!(s.contains("invalid JSON"), "unexpected display: {s}");
}

#[test]
fn protocol_error_display_violation() {
    let err = ProtocolError::Violation("first message must be hello".into());
    let s = err.to_string();
    assert!(s.starts_with("protocol violation:"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Error metadata preservation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn context_preserved_through_dto_roundtrip() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
    assert_eq!(back.context.len(), 3);
    assert_eq!(back.context["backend"], serde_json::json!("openai"));
}

#[test]
fn source_message_preserved_in_dto() {
    let src = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("file missing"));
}

#[test]
fn dto_source_message_absent_when_no_source() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    assert!(dto.source_message.is_none());
    let json = serde_json::to_string(&dto).unwrap();
    // source_message should be skipped in JSON
    assert!(!json.contains("source_message"));
}

#[test]
fn nested_json_in_context() {
    let err = AbpError::new(ErrorCode::Internal, "nested").with_context(
        "details",
        serde_json::json!({"inner": {"deep": true}, "list": [1, 2, 3]}),
    );
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.context["details"]["inner"]["deep"], true);
    assert_eq!(back.context["details"]["list"][1], 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Nested errors (error caused by another error)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_source_chain_one_level() {
    let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied").with_source(inner);
    let src = std::error::Error::source(&err).unwrap();
    assert_eq!(src.to_string(), "access denied");
}

#[test]
fn error_source_chain_two_levels() {
    // io::Error -> AbpError -> ProtocolError::Abp
    let inner = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let abp = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(inner);
    // Verify first level
    let src = std::error::Error::source(&abp).unwrap();
    assert_eq!(src.to_string(), "pipe broke");
}

#[test]
fn protocol_error_json_has_source() {
    let result = serde_json::from_str::<serde_json::Value>("bad");
    let json_err = result.unwrap_err();
    let proto_err: ProtocolError = json_err.into();
    // thiserror provides source via #[from]
    assert!(std::error::Error::source(&proto_err).is_some());
}

#[test]
fn protocol_error_io_has_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
    let proto_err: ProtocolError = io_err.into();
    assert!(std::error::Error::source(&proto_err).is_some());
}

#[test]
fn runtime_error_source_chain() {
    let inner_err = anyhow::anyhow!("disk full");
    let rt = RuntimeError::WorkspaceFailed(inner_err);
    // thiserror provides source via #[source]
    assert!(std::error::Error::source(&rt).is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Error equality and comparison
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_equality() {
    assert_eq!(ErrorCode::BackendTimeout, ErrorCode::BackendTimeout);
    assert_ne!(ErrorCode::BackendTimeout, ErrorCode::BackendCrashed);
}

#[test]
fn error_code_hash_consistency() {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(ErrorCode::Internal, "internal");
    map.insert(ErrorCode::BackendTimeout, "timeout");
    assert_eq!(map[&ErrorCode::Internal], "internal");
    assert_eq!(map[&ErrorCode::BackendTimeout], "timeout");
}

#[test]
fn error_category_equality() {
    assert_eq!(ErrorCategory::Protocol, ErrorCategory::Protocol);
    assert_ne!(ErrorCategory::Protocol, ErrorCategory::Backend);
}

#[test]
fn error_code_clone_preserves_identity() {
    let code = ErrorCode::ReceiptHashMismatch;
    let cloned = code;
    assert_eq!(code, cloned);
}

#[test]
fn error_dto_equality() {
    let dto1 = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "oops".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let dto2 = dto1.clone();
    assert_eq!(dto1, dto2);
}

#[test]
fn error_dto_inequality_on_different_code() {
    let dto1 = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "oops".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let dto2 = AbpErrorDto {
        code: ErrorCode::BackendTimeout,
        message: "oops".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    assert_ne!(dto1, dto2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Each error variant has a corresponding code
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_error_codes_have_a_category() {
    for code in ALL_CODES {
        // category() must not panic
        let _cat = code.category();
    }
}

#[test]
fn all_error_codes_have_as_str() {
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(!s.is_empty(), "{code:?} has empty as_str");
    }
}

#[test]
fn all_runtime_variants_have_error_codes() {
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
        // error_code() must not panic
        let _code = v.error_code();
    }
}

#[test]
fn protocol_error_code_mapping_is_deterministic() {
    // Violation always maps to ProtocolInvalidEnvelope
    assert_eq!(
        ProtocolError::Violation("a".into()).error_code(),
        ProtocolError::Violation("b".into()).error_code()
    );
    // UnexpectedMessage always maps to ProtocolUnexpectedMessage
    assert_eq!(
        ProtocolError::UnexpectedMessage {
            expected: "x".into(),
            got: "y".into()
        }
        .error_code(),
        Some(ErrorCode::ProtocolUnexpectedMessage)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Error codes are unique (no duplicates)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_error_codes_have_unique_as_str() {
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
fn all_error_codes_have_unique_serde_representations() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        assert!(seen.insert(json.clone()), "duplicate serde: {json}");
    }
}

#[test]
fn error_code_count_is_twenty() {
    assert_eq!(ALL_CODES.len(), 20);
}

#[test]
fn all_categories_are_unique_display() {
    let mut seen = HashSet::new();
    for cat in ALL_CATEGORIES {
        assert!(seen.insert(cat.to_string()), "duplicate category: {}", cat);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Error documentation coverage (category mapping completeness)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn every_category_has_at_least_one_code() {
    let mut covered: HashSet<ErrorCategory> = HashSet::new();
    for code in ALL_CODES {
        covered.insert(code.category());
    }
    for cat in ALL_CATEGORIES {
        assert!(covered.contains(cat), "no code maps to category {cat:?}");
    }
}

#[test]
fn protocol_category_has_exactly_three_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Protocol)
        .count();
    assert_eq!(count, 3);
}

#[test]
fn backend_category_has_exactly_three_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Backend)
        .count();
    assert_eq!(count, 3);
}

#[test]
fn internal_category_has_exactly_one_code() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Internal)
        .count();
    assert_eq!(count, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. BTreeMap ordering of error metadata
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn context_keys_are_sorted_in_display() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("zebra", "z")
        .with_context("alpha", "a")
        .with_context("middle", "m");
    let s = err.to_string();
    let alpha_pos = s.find("alpha").unwrap();
    let middle_pos = s.find("middle").unwrap();
    let zebra_pos = s.find("zebra").unwrap();
    assert!(
        alpha_pos < middle_pos && middle_pos < zebra_pos,
        "context keys not sorted: {s}"
    );
}

#[test]
fn context_keys_are_sorted_in_dto_serialization() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("z_key", "z")
        .with_context("a_key", "a");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < z_pos, "keys not sorted in JSON: {json}");
}

#[test]
fn btreemap_insertion_order_irrelevant() {
    let err1 = AbpError::new(ErrorCode::Internal, "test")
        .with_context("b", 2)
        .with_context("a", 1);
    let err2 = AbpError::new(ErrorCode::Internal, "test")
        .with_context("a", 1)
        .with_context("b", 2);
    let dto1: AbpErrorDto = (&err1).into();
    let dto2: AbpErrorDto = (&err2).into();
    assert_eq!(
        serde_json::to_string(&dto1).unwrap(),
        serde_json::to_string(&dto2).unwrap()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Error snapshot stability (serde_json output)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_snapshot_backend_timeout() {
    let json = serde_json::to_string(&ErrorCode::BackendTimeout).unwrap();
    assert_eq!(json, r#""BACKEND_TIMEOUT""#);
}

#[test]
fn error_code_snapshot_protocol_invalid_envelope() {
    let json = serde_json::to_string(&ErrorCode::ProtocolInvalidEnvelope).unwrap();
    assert_eq!(json, r#""PROTOCOL_INVALID_ENVELOPE""#);
}

#[test]
fn error_dto_snapshot_minimal() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "oops".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert_eq!(json, r#"{"code":"INTERNAL","message":"oops","context":{}}"#);
}

#[test]
fn error_dto_snapshot_with_context() {
    let mut ctx = BTreeMap::new();
    ctx.insert("key".to_string(), serde_json::json!("value"));
    let dto = AbpErrorDto {
        code: ErrorCode::BackendNotFound,
        message: "not found".into(),
        context: ctx,
        source_message: Some("inner".into()),
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert_eq!(
        json,
        r#"{"code":"BACKEND_NOT_FOUND","message":"not found","context":{"key":"value"},"source_message":"inner"}"#
    );
}

#[test]
fn fatal_envelope_snapshot_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "something failed",
        ErrorCode::BackendCrashed,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
    assert!(json.contains(r#""error_code":"BACKEND_CRASHED""#));
    assert!(json.contains(r#""error":"something failed""#));
}

#[test]
fn fatal_envelope_snapshot_without_error_code() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""error":"boom""#));
    // error_code should be omitted when None
    assert!(!json.contains("error_code"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Cross-crate error interoperability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_to_runtime_to_abp_roundtrip() {
    let original = AbpError::new(ErrorCode::DialectUnknown, "unknown dialect")
        .with_context("dialect", "foobar");
    let code = original.code;
    let rt: RuntimeError = original.into();
    assert_eq!(rt.error_code(), code);
    let recovered = rt.into_abp_error();
    assert_eq!(recovered.code, code);
    assert_eq!(recovered.context["dialect"], serde_json::json!("foobar"));
}

#[test]
fn abp_error_to_protocol_error() {
    let abp = AbpError::new(ErrorCode::ProtocolVersionMismatch, "v mismatch");
    let proto: ProtocolError = abp.into();
    assert_eq!(proto.error_code(), Some(ErrorCode::ProtocolVersionMismatch));
}

#[test]
fn abp_error_to_fatal_envelope() {
    let abp =
        AbpError::new(ErrorCode::BackendCrashed, "process died").with_context("exit_code", 137);
    let env = Envelope::fatal_from_abp_error(Some("run-42".into()), &abp);
    assert_eq!(env.error_code(), Some(ErrorCode::BackendCrashed));
    if let Envelope::Fatal { error, ref_id, .. } = &env {
        assert_eq!(error, "process died");
        assert_eq!(ref_id.as_deref(), Some("run-42"));
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn agent_event_error_carries_error_code() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Error {
            message: "backend crashed".into(),
            error_code: Some(ErrorCode::BackendCrashed),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("BACKEND_CRASHED"));
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &back.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("expected Error event kind");
    }
}

#[test]
fn agent_event_error_without_code() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Error {
            message: "something broke".into(),
            error_code: None,
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    // error_code should be skipped when None
    assert!(!json.contains("error_code"));
}

#[test]
fn contract_error_is_std_error() {
    let bad = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let err: ContractError = bad.into();
    let s = err.to_string();
    assert!(s.contains("failed to serialize JSON"), "got: {s}");
}

#[test]
fn receipt_builder_with_hash_produces_valid_receipt() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64); // SHA-256 hex
}

#[test]
fn runtime_error_display_is_human_readable() {
    let err = RuntimeError::UnknownBackend {
        name: "sidecar:node".into(),
    };
    let s = err.to_string();
    assert!(
        s.contains("sidecar:node"),
        "display missing backend name: {s}"
    );

    let err = RuntimeError::CapabilityCheckFailed("mcp not supported".into());
    let s = err.to_string();
    assert!(
        s.contains("mcp not supported"),
        "display missing message: {s}"
    );
}

#[test]
fn dto_to_abp_error_loses_source() {
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad config".into(),
        context: BTreeMap::new(),
        source_message: Some("inner cause".into()),
    };
    let err: AbpError = dto.into();
    // Source is lost in DTO → AbpError conversion (opaque type can't be reconstructed)
    assert!(err.source.is_none());
}

#[test]
fn debug_format_includes_context_when_present() {
    let err = AbpError::new(ErrorCode::Internal, "test").with_context("key", "value");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"), "debug missing context: {dbg}");
    assert!(dbg.contains("key"), "debug missing key: {dbg}");
}

#[test]
fn debug_format_omits_context_when_empty() {
    let err = AbpError::new(ErrorCode::Internal, "test");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("context"), "debug has empty context: {dbg}");
}

#[test]
fn debug_format_omits_source_when_none() {
    let err = AbpError::new(ErrorCode::Internal, "test");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("source"), "debug has source: {dbg}");
}

#[test]
fn debug_format_includes_source_when_present() {
    let src = std::io::Error::other("inner");
    let err = AbpError::new(ErrorCode::Internal, "test").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"), "debug missing source: {dbg}");
    assert!(dbg.contains("inner"), "debug missing source msg: {dbg}");
}
