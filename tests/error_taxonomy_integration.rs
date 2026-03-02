// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the unified error taxonomy system.
//!
//! Validates that ErrorCode, AbpError, ErrorCategory, and their interactions
//! across abp-error, abp-protocol, abp-runtime, and abp-core are consistent.

use std::collections::{BTreeMap, HashSet};

use abp_core::{AgentEvent, AgentEventKind, Outcome};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_runtime::RuntimeError;
use chrono::Utc;

// ---------------------------------------------------------------------------
// Exhaustive list of all ErrorCode variants (keep in sync with abp-error)
// ---------------------------------------------------------------------------

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
// 1. ErrorCode serialization / deserialization
// ===========================================================================

#[test]
fn all_error_codes_serde_roundtrip() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back, "roundtrip failed for {code:?}");
    }
}

#[test]
fn error_code_serializes_to_screaming_snake_case() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let s = json.trim_matches('"');
        // Must be uppercase with underscores only.
        assert!(
            s.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
            "code {code:?} serializes to {s}, which is not SCREAMING_SNAKE_CASE"
        );
    }
}

#[test]
fn error_code_serde_matches_as_str() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "serde vs as_str mismatch for {code:?}");
    }
}

// ===========================================================================
// 2. ErrorCode -> category mapping is exhaustive
// ===========================================================================

#[test]
fn category_mapping_is_exhaustive() {
    // Every code maps to a category without panicking.
    for code in ALL_CODES {
        let _ = code.category();
    }
}

#[test]
fn all_categories_covered_by_at_least_one_code() {
    let covered: HashSet<ErrorCategory> = ALL_CODES.iter().map(|c| c.category()).collect();
    for cat in ALL_CATEGORIES {
        assert!(
            covered.contains(cat),
            "category {cat:?} has no ErrorCode mapping to it"
        );
    }
}

#[test]
fn category_serde_roundtrip() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back, "category roundtrip failed for {cat:?}");
    }
}

// ===========================================================================
// 3. Error codes are stable (specific string values don't change)
// ===========================================================================

#[test]
fn error_codes_stable_strings() {
    let expected: &[(&str, ErrorCode)] = &[
        (
            "PROTOCOL_INVALID_ENVELOPE",
            ErrorCode::ProtocolInvalidEnvelope,
        ),
        (
            "PROTOCOL_UNEXPECTED_MESSAGE",
            ErrorCode::ProtocolUnexpectedMessage,
        ),
        (
            "PROTOCOL_VERSION_MISMATCH",
            ErrorCode::ProtocolVersionMismatch,
        ),
        ("BACKEND_NOT_FOUND", ErrorCode::BackendNotFound),
        ("BACKEND_TIMEOUT", ErrorCode::BackendTimeout),
        ("BACKEND_CRASHED", ErrorCode::BackendCrashed),
        ("CAPABILITY_UNSUPPORTED", ErrorCode::CapabilityUnsupported),
        (
            "CAPABILITY_EMULATION_FAILED",
            ErrorCode::CapabilityEmulationFailed,
        ),
        ("POLICY_DENIED", ErrorCode::PolicyDenied),
        ("POLICY_INVALID", ErrorCode::PolicyInvalid),
        ("WORKSPACE_INIT_FAILED", ErrorCode::WorkspaceInitFailed),
        (
            "WORKSPACE_STAGING_FAILED",
            ErrorCode::WorkspaceStagingFailed,
        ),
        ("IR_LOWERING_FAILED", ErrorCode::IrLoweringFailed),
        ("IR_INVALID", ErrorCode::IrInvalid),
        ("RECEIPT_HASH_MISMATCH", ErrorCode::ReceiptHashMismatch),
        ("RECEIPT_CHAIN_BROKEN", ErrorCode::ReceiptChainBroken),
        ("DIALECT_UNKNOWN", ErrorCode::DialectUnknown),
        ("DIALECT_MAPPING_FAILED", ErrorCode::DialectMappingFailed),
        ("CONFIG_INVALID", ErrorCode::ConfigInvalid),
        ("INTERNAL", ErrorCode::Internal),
    ];

    for (stable_str, code) in expected {
        assert_eq!(
            code.as_str(),
            *stable_str,
            "stable string for {code:?} changed!"
        );
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

// ===========================================================================
// 4. AbpError builder pattern works for all fields
// ===========================================================================

#[test]
fn abp_error_builder_minimal() {
    let err = AbpError::new(ErrorCode::Internal, "boom");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(err.message, "boom");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_builder_with_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
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
    let src = std::io::Error::new(std::io::ErrorKind::TimedOut, "connection timeout");
    let err = AbpError::new(ErrorCode::BackendTimeout, "upstream failed").with_source(src);
    assert!(err.source.is_some());
    let src_msg = std::error::Error::source(&err).unwrap().to_string();
    assert_eq!(src_msg, "connection timeout");
}

#[test]
fn abp_error_builder_full_chain() {
    let src = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "stage failed")
        .with_context("path", "/tmp/workspace")
        .with_context("file_count", 42)
        .with_source(src);

    assert_eq!(err.code, ErrorCode::WorkspaceStagingFailed);
    assert_eq!(err.context.len(), 2);
    assert!(err.source.is_some());
    assert_eq!(err.category(), ErrorCategory::Workspace);
}

#[test]
fn abp_error_builder_context_nested_json() {
    let nested = serde_json::json!({"a": 1, "b": [2, 3]});
    let err = AbpError::new(ErrorCode::Internal, "nested").with_context("details", nested.clone());
    assert_eq!(err.context["details"], nested);
}

// ===========================================================================
// 5. AbpErrorDto conversion roundtrip
// ===========================================================================

#[test]
fn dto_from_abp_error_preserves_fields() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml");
    let dto: AbpErrorDto = (&err).into();

    assert_eq!(dto.code, ErrorCode::ConfigInvalid);
    assert_eq!(dto.message, "bad config");
    assert_eq!(
        dto.context.get("file"),
        Some(&serde_json::json!("backplane.toml"))
    );
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_from_abp_error_captures_source_message() {
    let src = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crashed").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn dto_serde_roundtrip() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad IR")
        .with_context("node", "call_tool")
        .with_context("depth", 5);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_to_abp_error_conversion() {
    let dto = AbpErrorDto {
        code: ErrorCode::PolicyDenied,
        message: "access denied".into(),
        context: {
            let mut ctx = BTreeMap::new();
            ctx.insert("tool".into(), serde_json::json!("write_file"));
            ctx
        },
        source_message: Some("policy rule #3".into()),
    };

    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::PolicyDenied);
    assert_eq!(err.message, "access denied");
    assert_eq!(
        err.context.get("tool"),
        Some(&serde_json::json!("write_file"))
    );
    // Source is lost in DTO → AbpError (opaque type can't be reconstructed).
    assert!(err.source.is_none());
}

#[test]
fn dto_roundtrip_preserves_all_error_codes() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, format!("test {code:?}"));
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, *code);
        let reconverted: AbpError = back.into();
        assert_eq!(reconverted.code, *code);
    }
}

// ===========================================================================
// 6. Error display format consistency
// ===========================================================================

#[test]
fn display_format_bracket_code_message() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    assert_eq!(err.to_string(), "[BACKEND_NOT_FOUND] no such backend");
}

#[test]
fn display_format_with_context_includes_json() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 5000);
    let s = err.to_string();
    assert!(s.starts_with("[BACKEND_TIMEOUT] timed out"));
    assert!(s.contains("timeout_ms"));
    assert!(s.contains("5000"));
}

#[test]
fn display_format_all_codes() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, "test message");
        let display = err.to_string();
        assert!(
            display.starts_with(&format!("[{}]", code.as_str())),
            "display for {code:?} = {display}"
        );
    }
}

#[test]
fn error_code_display_matches_as_str() {
    for code in ALL_CODES {
        assert_eq!(code.to_string(), code.as_str());
    }
}

// ===========================================================================
// 7. Error propagation: backend error → AgentEvent with error_code → Receipt
// ===========================================================================

#[test]
fn agent_event_error_with_error_code_serde_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "backend crashed".into(),
            error_code: Some(ErrorCode::BackendCrashed),
        },
        ext: None,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("BACKEND_CRASHED"));

    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    match &back.kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "backend crashed");
            assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
        }
        other => panic!("expected Error event, got {other:?}"),
    }
}

#[test]
fn agent_event_error_without_code_serde_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "generic failure".into(),
            error_code: None,
        },
        ext: None,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("error_code"));

    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    match &back.kind {
        AgentEventKind::Error {
            error_code,
            message,
        } => {
            assert_eq!(message, "generic failure");
            assert_eq!(*error_code, None);
        }
        other => panic!("expected Error event, got {other:?}"),
    }
}

#[test]
fn error_event_in_receipt_trace() {
    let error_event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "policy denied write".into(),
            error_code: Some(ErrorCode::PolicyDenied),
        },
        ext: None,
    };

    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .add_trace_event(error_event)
        .build();

    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(receipt.trace.len(), 1);
    match &receipt.trace[0].kind {
        AgentEventKind::Error { error_code, .. } => {
            assert_eq!(*error_code, Some(ErrorCode::PolicyDenied));
        }
        other => panic!("expected Error event, got {other:?}"),
    }
}

// ===========================================================================
// 8. Protocol errors have correct codes
// ===========================================================================

#[test]
fn protocol_error_violation_has_code() {
    let err = ProtocolError::Violation("missing ref_id".into());
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn protocol_error_unexpected_message_has_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn protocol_error_from_abp_error_preserves_code() {
    let abp_err = AbpError::new(ErrorCode::ProtocolVersionMismatch, "version mismatch");
    let proto_err: ProtocolError = abp_err.into();
    assert_eq!(
        proto_err.error_code(),
        Some(ErrorCode::ProtocolVersionMismatch)
    );
}

#[test]
fn protocol_error_json_has_no_code() {
    let json_err: serde_json::Error = serde_json::from_str::<String>("not json").unwrap_err();
    let proto_err = ProtocolError::Json(json_err);
    assert_eq!(proto_err.error_code(), None);
}

#[test]
fn protocol_error_io_has_no_code() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let proto_err = ProtocolError::Io(io_err);
    assert_eq!(proto_err.error_code(), None);
}

// ===========================================================================
// 9. Protocol envelope Fatal carries error_code
// ===========================================================================

#[test]
fn fatal_envelope_with_code_roundtrip() {
    let envelope = Envelope::fatal_with_code(
        Some("run-123".into()),
        "out of memory",
        ErrorCode::BackendCrashed,
    );

    let json = JsonlCodec::encode(&envelope).unwrap();
    assert!(json.contains("BACKEND_CRASHED"));

    let back = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(back.error_code(), Some(ErrorCode::BackendCrashed));
}

#[test]
fn fatal_envelope_from_abp_error() {
    let abp_err = AbpError::new(ErrorCode::PolicyDenied, "denied by policy");
    let envelope = Envelope::fatal_from_abp_error(Some("run-456".into()), &abp_err);

    let json = JsonlCodec::encode(&envelope).unwrap();
    let back = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(back.error_code(), Some(ErrorCode::PolicyDenied));
    if let Envelope::Fatal { error, .. } = back {
        assert_eq!(error, "denied by policy");
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn fatal_envelope_without_code() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "generic error".into(),
        error_code: None,
    };
    assert_eq!(envelope.error_code(), None);

    let json = JsonlCodec::encode(&envelope).unwrap();
    assert!(!json.contains("error_code"));
}

#[test]
fn non_fatal_envelopes_have_no_error_code() {
    let hello = Envelope::hello(
        abp_core::BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        abp_core::CapabilityManifest::new(),
    );
    assert_eq!(hello.error_code(), None);
}

// ===========================================================================
// 10. Runtime errors map to correct categories
// ===========================================================================

#[test]
fn runtime_error_unknown_backend_maps_to_backend_category() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    let code = err.error_code();
    assert_eq!(code, ErrorCode::BackendNotFound);
    assert_eq!(code.category(), ErrorCategory::Backend);
}

#[test]
fn runtime_error_workspace_failed_maps_to_workspace_category() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    let code = err.error_code();
    assert_eq!(code, ErrorCode::WorkspaceInitFailed);
    assert_eq!(code.category(), ErrorCategory::Workspace);
}

#[test]
fn runtime_error_policy_failed_maps_to_policy_category() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    let code = err.error_code();
    assert_eq!(code, ErrorCode::PolicyInvalid);
    assert_eq!(code.category(), ErrorCategory::Policy);
}

#[test]
fn runtime_error_backend_failed_maps_to_backend_category() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    let code = err.error_code();
    assert_eq!(code, ErrorCode::BackendCrashed);
    assert_eq!(code.category(), ErrorCategory::Backend);
}

#[test]
fn runtime_error_capability_check_maps_to_capability_category() {
    let err = RuntimeError::CapabilityCheckFailed("missing mcp".into());
    let code = err.error_code();
    assert_eq!(code, ErrorCode::CapabilityUnsupported);
    assert_eq!(code.category(), ErrorCategory::Capability);
}

#[test]
fn runtime_error_classified_preserves_original_code() {
    let abp_err = AbpError::new(ErrorCode::DialectMappingFailed, "mapping failed");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(rt_err.error_code(), ErrorCode::DialectMappingFailed);
    assert_eq!(rt_err.error_code().category(), ErrorCategory::Dialect);
}

// ===========================================================================
// 11. Error chaining: context preservation across layers
// ===========================================================================

#[test]
fn abp_error_to_runtime_classified_preserves_context() {
    let abp_err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml")
        .with_context("line", 42);

    let rt_err: RuntimeError = abp_err.into();

    // Convert back through into_abp_error.
    let recovered = rt_err.into_abp_error();
    assert_eq!(recovered.code, ErrorCode::ConfigInvalid);
    assert_eq!(
        recovered.context.get("file"),
        Some(&serde_json::json!("backplane.toml"))
    );
    assert_eq!(recovered.context.get("line"), Some(&serde_json::json!(42)));
}

#[test]
fn non_classified_runtime_error_into_abp_error_has_message() {
    let rt_err = RuntimeError::UnknownBackend {
        name: "missing-backend".into(),
    };
    let abp = rt_err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
    assert!(abp.message.contains("missing-backend"));
}

#[test]
fn error_source_chain_preserved_through_abp_error() {
    let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let abp_err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(inner);

    // Source chain accessible via std::error::Error.
    let source = std::error::Error::source(&abp_err).unwrap();
    assert_eq!(source.to_string(), "access denied");

    // DTO captures source as string.
    let dto: AbpErrorDto = (&abp_err).into();
    assert_eq!(dto.source_message.as_deref(), Some("access denied"));
}

#[test]
fn custom_error_context_preserved_through_dto_conversion() {
    let err = AbpError::new(ErrorCode::DialectMappingFailed, "mapping failed")
        .with_context("source_dialect", "openai")
        .with_context("target_dialect", "anthropic")
        .with_context("unsupported_feature", "tool_streaming");

    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    let recovered: AbpError = back.into();

    assert_eq!(recovered.code, ErrorCode::DialectMappingFailed);
    assert_eq!(
        recovered.context.get("source_dialect"),
        Some(&serde_json::json!("openai"))
    );
    assert_eq!(
        recovered.context.get("target_dialect"),
        Some(&serde_json::json!("anthropic"))
    );
    assert_eq!(
        recovered.context.get("unsupported_feature"),
        Some(&serde_json::json!("tool_streaming"))
    );
}

// ===========================================================================
// 12. Cross-layer error propagation scenario
// ===========================================================================

#[test]
fn full_error_propagation_scenario() {
    // Step 1: A backend produces an AbpError.
    let backend_err = AbpError::new(ErrorCode::BackendTimeout, "upstream timed out after 30 s")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);

    // Step 2: The error flows into an AgentEvent.
    let error_event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: backend_err.message.clone(),
            error_code: Some(backend_err.code),
        },
        ext: None,
    };

    // Step 3: The event is serialized through JSONL protocol.
    let envelope = Envelope::Event {
        ref_id: "run-789".into(),
        event: error_event.clone(),
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::Error {
            error_code,
            message,
        } = &event.kind
        {
            assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
            assert!(message.contains("timed out"));
        } else {
            panic!("expected Error event kind");
        }
    } else {
        panic!("expected Event envelope");
    }

    // Step 4: The event ends up in the receipt trace.
    let receipt = abp_core::ReceiptBuilder::new("openai")
        .outcome(Outcome::Failed)
        .add_trace_event(error_event)
        .build();

    assert_eq!(receipt.outcome, Outcome::Failed);
    match &receipt.trace[0].kind {
        AgentEventKind::Error { error_code, .. } => {
            assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
        }
        other => panic!("expected Error event, got {other:?}"),
    }

    // Step 5: The original AbpError can also be serialized as a DTO.
    let dto: AbpErrorDto = (&backend_err).into();
    let dto_json = serde_json::to_string(&dto).unwrap();
    assert!(dto_json.contains("BACKEND_TIMEOUT"));
    assert!(dto_json.contains("openai"));
}

// ===========================================================================
// 13. Category consistency: code.category() matches prefix convention
// ===========================================================================

#[test]
fn protocol_codes_map_to_protocol_category() {
    let protocol_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in &protocol_codes {
        assert_eq!(code.category(), ErrorCategory::Protocol, "for {code:?}");
    }
}

#[test]
fn backend_codes_map_to_backend_category() {
    let codes = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
    ];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Backend, "for {code:?}");
    }
}

#[test]
fn capability_codes_map_to_capability_category() {
    let codes = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Capability, "for {code:?}");
    }
}

#[test]
fn workspace_codes_map_to_workspace_category() {
    let codes = [
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
    ];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Workspace, "for {code:?}");
    }
}

#[test]
fn ir_codes_map_to_ir_category() {
    let codes = [ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Ir, "for {code:?}");
    }
}

#[test]
fn receipt_codes_map_to_receipt_category() {
    let codes = [
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Receipt, "for {code:?}");
    }
}

#[test]
fn dialect_codes_map_to_dialect_category() {
    let codes = [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed];
    for code in &codes {
        assert_eq!(code.category(), ErrorCategory::Dialect, "for {code:?}");
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

// ===========================================================================
// 14. Error category Display consistency
// ===========================================================================

#[test]
fn error_category_display_is_lowercase() {
    for cat in ALL_CATEGORIES {
        let s = cat.to_string();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase()),
            "category {cat:?} display is not lowercase: {s}"
        );
    }
}

#[test]
fn error_category_display_matches_serde() {
    for cat in ALL_CATEGORIES {
        let display = cat.to_string();
        let serde_str = serde_json::to_string(cat).unwrap();
        let serde_val = serde_str.trim_matches('"');
        assert_eq!(display, serde_val, "display vs serde mismatch for {cat:?}");
    }
}

// ===========================================================================
// 15. Debug format consistency
// ===========================================================================

#[test]
fn abp_error_debug_contains_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "access denied");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("PolicyDenied"));
    assert!(dbg.contains("access denied"));
}

#[test]
fn abp_error_debug_includes_source_when_present() {
    let src = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("not found"));
}

#[test]
fn abp_error_debug_includes_context_when_present() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout").with_context("backend", "openai");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
    assert!(dbg.contains("openai"));
}

// ===========================================================================
// 16. std::error::Error trait implementation
// ===========================================================================

#[test]
fn abp_error_implements_std_error() {
    let err = AbpError::new(ErrorCode::Internal, "something broke");
    let _: &dyn std::error::Error = &err;
}

#[test]
fn abp_error_source_none_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    assert!(std::error::Error::source(&err).is_none());
}

#[test]
fn abp_error_source_chain() {
    let inner = std::io::Error::other("root cause");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(inner);
    let src = std::error::Error::source(&err).unwrap();
    assert_eq!(src.to_string(), "root cause");
}
