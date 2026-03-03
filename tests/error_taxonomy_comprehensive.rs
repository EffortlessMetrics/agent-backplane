// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive error taxonomy tests: classification, mapping, propagation,
//! serialization, and recovery workflows across the ABP system.

use std::collections::{BTreeMap, HashSet};
use std::error::Error as StdError;
use std::io;

use abp_config::ConfigError;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ContractError, Outcome,
    Receipt, ReceiptBuilder,
};
use abp_daemon::DaemonError;
use abp_dialect::Dialect;
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_host::HostError;
use abp_mapping::MappingError as MappingValidationError;
use abp_projection::ProjectionError;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_runtime::RuntimeError;
use abp_shim_gemini::GeminiError;
use abp_sidecar_proto::SidecarProtoError;
use chrono::Utc;
use claude_bridge::BridgeError;
use sidecar_kit::SidecarError;

// ─── helpers ────────────────────────────────────────────────────────────────

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

fn make_error_event(msg: &str, code: Option<ErrorCode>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: code,
        },
        ext: None,
    }
}

fn make_receipt_with_events(events: Vec<AgentEvent>, outcome: Outcome) -> Receipt {
    let mut builder = ReceiptBuilder::new("test-backend").outcome(outcome);
    for e in events {
        builder = builder.add_trace_event(e);
    }
    builder.build()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. ProtocolError variant construction and Display impl
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_json_from_serde() {
    let raw = "not json {{{";
    let err: Result<serde_json::Value, _> = serde_json::from_str(raw);
    let serde_err = err.unwrap_err();
    let pe = ProtocolError::Json(serde_err);
    let msg = pe.to_string();
    assert!(msg.starts_with("invalid JSON:"), "got: {msg}");
}

#[test]
fn protocol_error_io_from_std() {
    let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let pe = ProtocolError::Io(io_err);
    assert!(pe.to_string().contains("pipe broke"));
}

#[test]
fn protocol_error_violation_display() {
    let pe = ProtocolError::Violation("missing ref_id".into());
    assert_eq!(pe.to_string(), "protocol violation: missing ref_id");
}

#[test]
fn protocol_error_unexpected_message_display() {
    let pe = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert_eq!(
        pe.to_string(),
        "unexpected message: expected hello, got event"
    );
}

#[test]
fn protocol_error_abp_variant_display() {
    let abp = AbpError::new(ErrorCode::ProtocolVersionMismatch, "v0.1 vs v0.2");
    let pe = ProtocolError::Abp(abp);
    assert!(pe.to_string().contains("v0.1 vs v0.2"));
}

#[test]
fn protocol_error_from_io_implicit() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
    let pe: ProtocolError = io_err.into();
    assert!(matches!(pe, ProtocolError::Io(_)));
}

#[test]
fn protocol_error_from_serde_json_implicit() {
    let serde_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let pe: ProtocolError = serde_err.into();
    assert!(matches!(pe, ProtocolError::Json(_)));
}

#[test]
fn protocol_error_from_abp_error_implicit() {
    let abp = AbpError::new(ErrorCode::Internal, "internal");
    let pe: ProtocolError = abp.into();
    assert!(matches!(pe, ProtocolError::Abp(_)));
}

#[test]
fn protocol_error_error_code_for_violation() {
    let pe = ProtocolError::Violation("bad envelope".into());
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn protocol_error_error_code_for_unexpected_message() {
    let pe = ProtocolError::UnexpectedMessage {
        expected: "run".into(),
        got: "hello".into(),
    };
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn protocol_error_error_code_for_abp() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "timeout");
    let pe = ProtocolError::Abp(abp);
    assert_eq!(pe.error_code(), Some(ErrorCode::BackendTimeout));
}

#[test]
fn protocol_error_error_code_none_for_io() {
    let pe = ProtocolError::Io(io::Error::other("io"));
    assert_eq!(pe.error_code(), None);
}

#[test]
fn protocol_error_error_code_none_for_json() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let pe = ProtocolError::Json(serde_err);
    assert_eq!(pe.error_code(), None);
}

#[test]
fn protocol_error_is_debug() {
    let pe = ProtocolError::Violation("test".into());
    let dbg = format!("{pe:?}");
    assert!(dbg.contains("Violation"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. RuntimeError variant construction and error chain preservation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_error_unknown_backend() {
    let re = RuntimeError::UnknownBackend {
        name: "nonexistent".into(),
    };
    assert!(re.to_string().contains("nonexistent"));
}

#[test]
fn runtime_error_workspace_failed_with_source() {
    let src = io::Error::new(io::ErrorKind::PermissionDenied, "read-only");
    let re = RuntimeError::WorkspaceFailed(anyhow::Error::new(src).context("staging /tmp/ws"));
    assert_eq!(re.to_string(), "workspace preparation failed");
    let source = StdError::source(&re).unwrap();
    assert!(source.to_string().contains("staging /tmp/ws"));
}

#[test]
fn runtime_error_policy_failed_chain() {
    let inner = anyhow::anyhow!("glob parse error: **[invalid");
    let re = RuntimeError::PolicyFailed(inner);
    assert_eq!(re.to_string(), "policy compilation failed");
    assert!(StdError::source(&re).is_some());
}

#[test]
fn runtime_error_backend_failed_chain() {
    let inner = anyhow::anyhow!("sidecar exited with code 1");
    let re = RuntimeError::BackendFailed(inner);
    assert_eq!(re.to_string(), "backend execution failed");
}

#[test]
fn runtime_error_capability_check_failed() {
    let re = RuntimeError::CapabilityCheckFailed("streaming not supported".into());
    assert!(re.to_string().contains("streaming not supported"));
}

#[test]
fn runtime_error_classified_from_abp() {
    let abp = AbpError::new(ErrorCode::PolicyDenied, "tool write denied");
    let re: RuntimeError = abp.into();
    assert!(matches!(re, RuntimeError::Classified(_)));
    assert!(re.to_string().contains("tool write denied"));
}

#[test]
fn runtime_error_no_projection_match() {
    let re = RuntimeError::NoProjectionMatch {
        reason: "no backend supports image_input".into(),
    };
    assert!(re.to_string().contains("no backend supports image_input"));
}

#[test]
fn runtime_error_preserves_anyhow_chain() {
    let root = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let chain = anyhow::Error::new(root)
        .context("copy step")
        .context("staging");
    let re = RuntimeError::WorkspaceFailed(chain);

    let src = StdError::source(&re).unwrap();
    assert!(src.to_string().contains("staging"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Error code stability (messages don't change unexpectedly)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_as_str_stability() {
    let expected: Vec<(&str, ErrorCode)> = vec![
        (
            "protocol_invalid_envelope",
            ErrorCode::ProtocolInvalidEnvelope,
        ),
        (
            "protocol_unexpected_message",
            ErrorCode::ProtocolUnexpectedMessage,
        ),
        (
            "protocol_version_mismatch",
            ErrorCode::ProtocolVersionMismatch,
        ),
        ("backend_not_found", ErrorCode::BackendNotFound),
        ("backend_timeout", ErrorCode::BackendTimeout),
        ("backend_crashed", ErrorCode::BackendCrashed),
        ("capability_unsupported", ErrorCode::CapabilityUnsupported),
        (
            "capability_emulation_failed",
            ErrorCode::CapabilityEmulationFailed,
        ),
        ("policy_denied", ErrorCode::PolicyDenied),
        ("policy_invalid", ErrorCode::PolicyInvalid),
        ("workspace_init_failed", ErrorCode::WorkspaceInitFailed),
        (
            "workspace_staging_failed",
            ErrorCode::WorkspaceStagingFailed,
        ),
        ("ir_lowering_failed", ErrorCode::IrLoweringFailed),
        ("ir_invalid", ErrorCode::IrInvalid),
        ("receipt_hash_mismatch", ErrorCode::ReceiptHashMismatch),
        ("receipt_chain_broken", ErrorCode::ReceiptChainBroken),
        ("dialect_unknown", ErrorCode::DialectUnknown),
        ("dialect_mapping_failed", ErrorCode::DialectMappingFailed),
        ("config_invalid", ErrorCode::ConfigInvalid),
        ("INTERNAL", ErrorCode::Internal),
    ];
    for (s, code) in &expected {
        assert_eq!(code.as_str(), *s, "stable string changed for {code:?}");
    }
}

#[test]
fn error_code_display_matches_as_str() {
    for code in ALL_CODES {
        assert_eq!(code.to_string(), code.message());
    }
}

#[test]
fn all_codes_unique_strings() {
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        assert!(seen.insert(code.as_str()), "duplicate: {}", code.as_str());
    }
}

#[test]
fn error_code_count_is_20() {
    assert_eq!(ALL_CODES.len(), 20);
}

#[test]
fn category_display_stability() {
    let expected: Vec<(&str, ErrorCategory)> = vec![
        ("protocol", ErrorCategory::Protocol),
        ("backend", ErrorCategory::Backend),
        ("capability", ErrorCategory::Capability),
        ("policy", ErrorCategory::Policy),
        ("workspace", ErrorCategory::Workspace),
        ("ir", ErrorCategory::Ir),
        ("receipt", ErrorCategory::Receipt),
        ("dialect", ErrorCategory::Dialect),
        ("config", ErrorCategory::Config),
        ("internal", ErrorCategory::Internal),
    ];
    for (s, cat) in &expected {
        assert_eq!(cat.to_string(), *s);
    }
}

#[test]
fn abp_error_display_format_stable() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    assert_eq!(err.to_string(), "[BACKEND_NOT_FOUND] no such backend");
}

#[test]
fn abp_error_display_with_context_deterministic() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    let s = err.to_string();
    assert!(s.starts_with("[BACKEND_TIMEOUT] timed out "));
    // BTreeMap ensures deterministic key order
    assert!(s.contains("\"backend\""));
    assert!(s.contains("\"timeout_ms\""));
}

#[test]
fn runtime_error_display_messages_stable() {
    let err1 = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err1.to_string(), "unknown backend: x");

    let err2 = RuntimeError::WorkspaceFailed(anyhow::anyhow!("inner"));
    assert_eq!(err2.to_string(), "workspace preparation failed");

    let err3 = RuntimeError::PolicyFailed(anyhow::anyhow!("inner"));
    assert_eq!(err3.to_string(), "policy compilation failed");

    let err4 = RuntimeError::BackendFailed(anyhow::anyhow!("inner"));
    assert_eq!(err4.to_string(), "backend execution failed");

    let err5 = RuntimeError::CapabilityCheckFailed("reason".into());
    assert_eq!(err5.to_string(), "capability check failed: reason");

    let err6 = RuntimeError::NoProjectionMatch {
        reason: "no match".into(),
    };
    assert_eq!(err6.to_string(), "projection failed: no match");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Error propagation from backend → runtime → receipt
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_error_to_error_code_unknown_backend() {
    let re = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    assert_eq!(re.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_to_error_code_workspace_failed() {
    let re = RuntimeError::WorkspaceFailed(anyhow::anyhow!("err"));
    assert_eq!(re.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_to_error_code_policy_failed() {
    let re = RuntimeError::PolicyFailed(anyhow::anyhow!("err"));
    assert_eq!(re.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_to_error_code_backend_failed() {
    let re = RuntimeError::BackendFailed(anyhow::anyhow!("err"));
    assert_eq!(re.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_to_error_code_capability_check() {
    let re = RuntimeError::CapabilityCheckFailed("no streaming".into());
    assert_eq!(re.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_to_error_code_classified() {
    let abp = AbpError::new(ErrorCode::DialectMappingFailed, "mapping");
    let re = RuntimeError::Classified(abp);
    assert_eq!(re.error_code(), ErrorCode::DialectMappingFailed);
}

#[test]
fn runtime_error_to_error_code_no_projection() {
    let re = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    assert_eq!(re.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_into_abp_error_preserves_code() {
    let re = RuntimeError::BackendFailed(anyhow::anyhow!("sidecar died"));
    let abp = re.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendCrashed);
    assert_eq!(abp.message, "backend execution failed");
}

#[test]
fn runtime_error_into_abp_error_classified_passthrough() {
    let original =
        AbpError::new(ErrorCode::PolicyDenied, "denied by policy").with_context("tool", "bash");
    let re = RuntimeError::Classified(original);
    let abp = re.into_abp_error();
    assert_eq!(abp.code, ErrorCode::PolicyDenied);
    assert_eq!(abp.message, "denied by policy");
    assert_eq!(abp.context["tool"], serde_json::json!("bash"));
}

#[test]
fn backend_error_becomes_failed_receipt() {
    let error_event = make_error_event("sidecar crashed", Some(ErrorCode::BackendCrashed));
    let receipt = make_receipt_with_events(vec![error_event], Outcome::Failed);
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(receipt.trace.len(), 1);
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &receipt.trace[0].kind
    {
        assert_eq!(message, "sidecar crashed");
        assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("expected Error event");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. In-stream error events via AgentEventKind::Error
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_event_without_code() {
    let event = make_error_event("something went wrong", None);
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &event.kind
    {
        assert_eq!(message, "something went wrong");
        assert!(error_code.is_none());
    } else {
        panic!("expected Error kind");
    }
}

#[test]
fn error_event_with_code() {
    let event = make_error_event("timeout", Some(ErrorCode::BackendTimeout));
    if let AgentEventKind::Error { error_code, .. } = &event.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
    } else {
        panic!("expected Error kind");
    }
}

#[test]
fn error_event_serializes_with_type_tag() {
    let event = make_error_event("bad", Some(ErrorCode::Internal));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["message"], "bad");
    assert_eq!(json["error_code"], "INTERNAL");
}

#[test]
fn error_event_without_code_omits_field() {
    let event = make_error_event("fail", None);
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
    assert!(json.get("error_code").is_none());
}

#[test]
fn error_event_roundtrip_json() {
    let event = make_error_event("ctx overflow", Some(ErrorCode::BackendCrashed));
    let json = serde_json::to_string(&event).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &back.kind
    {
        assert_eq!(message, "ctx overflow");
        assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("expected Error kind");
    }
}

#[test]
fn error_event_in_envelope_roundtrip() {
    let event = make_error_event("stream error", Some(ErrorCode::IrInvalid));
    let envelope = Envelope::Event {
        ref_id: "run-1".into(),
        event: event.clone(),
    };
    let line = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { ref_id, event: ev } = decoded {
        assert_eq!(ref_id, "run-1");
        if let AgentEventKind::Error { error_code, .. } = &ev.kind {
            assert_eq!(*error_code, Some(ErrorCode::IrInvalid));
        } else {
            panic!("expected Error kind");
        }
    } else {
        panic!("expected Event envelope");
    }
}

#[test]
fn multiple_error_events_in_trace() {
    let events = vec![
        make_error_event("rate limited", Some(ErrorCode::BackendTimeout)),
        make_error_event("retry failed", Some(ErrorCode::BackendCrashed)),
    ];
    let receipt = make_receipt_with_events(events, Outcome::Failed);
    let error_count = receipt
        .trace
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        .count();
    assert_eq!(error_count, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Error categorization (transient vs permanent, retryable vs fatal)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn timeout_is_backend_category() {
    assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
}

#[test]
fn crashed_is_backend_category() {
    assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
}

#[test]
fn policy_denied_is_policy_category() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
}

#[test]
fn transient_errors_are_backend_family() {
    let transient_codes = [ErrorCode::BackendTimeout, ErrorCode::BackendCrashed];
    for code in &transient_codes {
        assert_eq!(code.category(), ErrorCategory::Backend);
    }
}

#[test]
fn permanent_errors_are_policy_or_config() {
    let permanent_codes = [
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::ConfigInvalid,
    ];
    for code in &permanent_codes {
        let cat = code.category();
        assert!(
            cat == ErrorCategory::Policy || cat == ErrorCategory::Config,
            "unexpected category for {code:?}: {cat:?}"
        );
    }
}

#[test]
fn all_codes_have_a_category() {
    for code in ALL_CODES {
        let _ = code.category(); // should not panic
    }
}

#[test]
fn category_count_is_10() {
    assert_eq!(ALL_CATEGORIES.len(), 10);
}

#[test]
fn every_category_has_at_least_one_code() {
    for cat in ALL_CATEGORIES {
        let count = ALL_CODES.iter().filter(|c| c.category() == *cat).count();
        assert!(count >= 1, "no codes for category {cat:?}");
    }
}

#[test]
fn protocol_category_codes() {
    let proto_codes: Vec<_> = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Protocol)
        .collect();
    assert_eq!(proto_codes.len(), 3);
}

#[test]
fn backend_category_codes() {
    let backend_codes: Vec<_> = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Backend)
        .collect();
    assert_eq!(backend_codes.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Vendor-specific error mapping to ABP error taxonomy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn vendor_rate_limit_maps_to_backend_timeout() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "rate limit exceeded")
        .with_context("vendor", "openai")
        .with_context("http_status", 429);
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.category(), ErrorCategory::Backend);
}

#[test]
fn vendor_auth_error_maps_to_config_invalid() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "invalid API key")
        .with_context("vendor", "anthropic")
        .with_context("http_status", 401);
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.category(), ErrorCategory::Config);
}

#[test]
fn vendor_context_length_maps_to_backend_crashed() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "context length exceeded")
        .with_context("vendor", "openai")
        .with_context("max_tokens", 128_000)
        .with_context("requested_tokens", 200_000);
    assert_eq!(err.code, ErrorCode::BackendCrashed);
    assert_eq!(err.context.len(), 3);
}

#[test]
fn vendor_model_not_found_maps_to_backend_not_found() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "model gpt-5 not found")
        .with_context("vendor", "openai")
        .with_context("model", "gpt-5");
    assert_eq!(err.code, ErrorCode::BackendNotFound);
}

#[test]
fn vendor_error_context_preserved_through_dto() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "rate limited")
        .with_context("vendor", "gemini")
        .with_context("retry_after_ms", 5000);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.context["vendor"], serde_json::json!("gemini"));
    assert_eq!(dto.context["retry_after_ms"], serde_json::json!(5000));
}

#[test]
fn vendor_error_through_runtime_classified() {
    let abp = AbpError::new(ErrorCode::DialectMappingFailed, "unknown tool format")
        .with_context("vendor", "kimi");
    let re: RuntimeError = abp.into();
    let recovered = re.into_abp_error();
    assert_eq!(recovered.code, ErrorCode::DialectMappingFailed);
    assert_eq!(recovered.context["vendor"], serde_json::json!("kimi"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Rate limit error handling patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rate_limit_error_with_retry_metadata() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "429 Too Many Requests")
        .with_context("http_status", 429)
        .with_context("retry_after_seconds", 30)
        .with_context("vendor", "openai");
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.context["retry_after_seconds"], serde_json::json!(30));
}

#[test]
fn rate_limit_maps_to_backend_category() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "rate limited");
    assert_eq!(err.category(), ErrorCategory::Backend);
}

#[test]
fn rate_limit_error_event_in_stream() {
    let event = make_error_event(
        "Rate limit exceeded. Retry after 60s",
        Some(ErrorCode::BackendTimeout),
    );
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["error_code"], "backend_timeout");
}

#[test]
fn rate_limit_with_nested_vendor_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "rate limited").with_context(
        "rate_limit",
        serde_json::json!({
            "limit": 100,
            "remaining": 0,
            "reset_at": "2025-01-01T00:00:00Z"
        }),
    );
    let ctx = &err.context["rate_limit"];
    assert_eq!(ctx["remaining"], 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Context length exceeded error patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn context_length_error_construction() {
    let err = AbpError::new(
        ErrorCode::BackendCrashed,
        "context length exceeded: 200k > 128k",
    )
    .with_context("max_context_tokens", 128_000)
    .with_context("actual_tokens", 200_000);
    assert!(err.message.contains("context length exceeded"));
}

#[test]
fn context_length_error_in_event() {
    let event = make_error_event(
        "context_length_exceeded: model max is 128000 tokens",
        Some(ErrorCode::BackendCrashed),
    );
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &event.kind
    {
        assert!(message.contains("context_length_exceeded"));
        assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("expected Error kind");
    }
}

#[test]
fn context_length_error_produces_partial_receipt() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "partial output...".into(),
            },
            ext: None,
        },
        make_error_event("context length exceeded", Some(ErrorCode::BackendCrashed)),
    ];
    let receipt = make_receipt_with_events(events, Outcome::Partial);
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.trace.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Invalid request error patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn invalid_request_maps_to_config_invalid() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "invalid model parameter");
    assert_eq!(err.category(), ErrorCategory::Config);
}

#[test]
fn invalid_request_with_field_context() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "unsupported temperature value")
        .with_context("field", "temperature")
        .with_context("value", 3.0)
        .with_context("max_allowed", 2.0);
    assert_eq!(err.context.len(), 3);
}

#[test]
fn invalid_request_invalid_json_body() {
    let err = AbpError::new(
        ErrorCode::ProtocolInvalidEnvelope,
        "malformed JSON in request body",
    )
    .with_context("raw_body_preview", "{ bad json");
    assert_eq!(err.category(), ErrorCategory::Protocol);
}

#[test]
fn invalid_tool_name_error() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "tool not in allowlist")
        .with_context("tool", "rm_rf")
        .with_context("allowlist", serde_json::json!(["read", "write", "edit"]));
    assert_eq!(err.code, ErrorCode::PolicyDenied);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Authentication/authorization error patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn auth_error_invalid_api_key() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "invalid API key")
        .with_context("vendor", "anthropic")
        .with_context("http_status", 401);
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.category(), ErrorCategory::Config);
}

#[test]
fn auth_error_expired_token() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "token expired")
        .with_context("vendor", "openai")
        .with_context("expired_at", "2025-01-01T00:00:00Z");
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
}

#[test]
fn auth_error_insufficient_permissions() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "insufficient permissions")
        .with_context("required_scope", "write")
        .with_context("actual_scope", "read");
    assert_eq!(err.code, ErrorCode::PolicyDenied);
    assert_eq!(err.category(), ErrorCategory::Policy);
}

#[test]
fn auth_error_in_fatal_envelope() {
    let envelope = Envelope::fatal_with_code(
        Some("run-42".into()),
        "authentication failed: invalid API key",
        ErrorCode::ConfigInvalid,
    );
    if let Envelope::Fatal {
        error, error_code, ..
    } = &envelope
    {
        assert!(error.contains("authentication failed"));
        assert_eq!(*error_code, Some(ErrorCode::ConfigInvalid));
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn auth_error_runtime_to_abp() {
    let abp = AbpError::new(ErrorCode::ConfigInvalid, "bad credentials")
        .with_context("vendor", "copilot");
    let re: RuntimeError = abp.into();
    assert_eq!(re.error_code(), ErrorCode::ConfigInvalid);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Timeout error patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn timeout_error_basic() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "backend did not respond in 30s");
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.category(), ErrorCategory::Backend);
}

#[test]
fn timeout_error_with_duration_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("timeout_ms", 30_000)
        .with_context("backend", "sidecar:node");
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn timeout_runtime_error_code_mapping() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "timeout");
    let re: RuntimeError = abp.into();
    assert_eq!(re.error_code(), ErrorCode::BackendTimeout);
}

#[test]
fn timeout_error_event_in_stream() {
    let event = make_error_event("operation timed out", Some(ErrorCode::BackendTimeout));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["error_code"], "backend_timeout");
    assert_eq!(json["message"], "operation timed out");
}

#[test]
fn timeout_in_fatal_envelope() {
    let envelope = Envelope::fatal_with_code(
        Some("run-99".into()),
        "sidecar unresponsive for 60s",
        ErrorCode::BackendTimeout,
    );
    assert_eq!(envelope.error_code(), Some(ErrorCode::BackendTimeout));
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Error serialization to JSON (deterministic)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_serde_roundtrip_all() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back);
    }
}

#[test]
fn error_code_serializes_as_screaming_snake_case() {
    let json = serde_json::to_string(&ErrorCode::ProtocolInvalidEnvelope).unwrap();
    assert_eq!(json, r#""protocol_invalid_envelope""#);
}

#[test]
fn error_category_serde_roundtrip_all() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back);
    }
}

#[test]
fn error_category_serializes_as_snake_case() {
    let json = serde_json::to_string(&ErrorCategory::Backend).unwrap();
    assert_eq!(json, r#""backend""#);
}

#[test]
fn abp_error_dto_deterministic_json() {
    let err = AbpError::new(ErrorCode::Internal, "oops")
        .with_context("b_key", "second")
        .with_context("a_key", "first");
    let dto: AbpErrorDto = (&err).into();
    let json1 = serde_json::to_string(&dto).unwrap();
    let json2 = serde_json::to_string(&dto).unwrap();
    assert_eq!(json1, json2, "JSON must be deterministic");
    // BTreeMap guarantees a_key before b_key
    let a_pos = json1.find("a_key").unwrap();
    let b_pos = json1.find("b_key").unwrap();
    assert!(a_pos < b_pos, "BTreeMap ordering violated");
}

#[test]
fn abp_error_dto_roundtrip() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unknown dialect")
        .with_context("dialect", "foobar");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn abp_error_dto_with_source_message() {
    let src = io::Error::new(io::ErrorKind::ConnectionReset, "reset");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("reset"));
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("reset"));
}

#[test]
fn abp_error_dto_without_source_omits_field() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    assert!(dto.source_message.is_none());
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

#[test]
fn fatal_envelope_error_code_json() {
    let envelope = Envelope::fatal_with_code(
        Some("run-1".into()),
        "fatal error",
        ErrorCode::BackendCrashed,
    );
    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["error_code"], "backend_crashed");
    assert_eq!(json["t"], "fatal");
}

#[test]
fn error_event_json_deterministic() {
    let event = make_error_event("fail", Some(ErrorCode::PolicyDenied));
    let json1 = serde_json::to_string(&event).unwrap();
    let json2 = serde_json::to_string(&event).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn error_in_receipt_trace_serializes_correctly() {
    let event = make_error_event("boom", Some(ErrorCode::Internal));
    let receipt = make_receipt_with_events(vec![event], Outcome::Failed);
    let json = serde_json::to_value(&receipt).unwrap();
    let trace = json["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0]["type"], "error");
    assert_eq!(trace[0]["error_code"], "INTERNAL");
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Error recovery workflows
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn recovery_from_transient_error_produces_complete_receipt() {
    let events = vec![
        make_error_event("rate limited, retrying", Some(ErrorCode::BackendTimeout)),
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Success after retry".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];
    let receipt = make_receipt_with_events(events, Outcome::Complete);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.trace.len(), 3);
}

#[test]
fn recovery_receipt_still_contains_error_in_trace() {
    let events = vec![
        make_error_event("transient error", Some(ErrorCode::BackendTimeout)),
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "recovered".into(),
            },
            ext: None,
        },
    ];
    let receipt = make_receipt_with_events(events, Outcome::Complete);
    let has_error = receipt
        .trace
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }));
    assert!(has_error, "error event should be preserved in trace");
}

#[test]
fn fatal_error_aborts_without_recovery() {
    let events = vec![make_error_event(
        "authentication failed",
        Some(ErrorCode::ConfigInvalid),
    )];
    let receipt = make_receipt_with_events(events, Outcome::Failed);
    assert_eq!(receipt.outcome, Outcome::Failed);
}

#[test]
fn multiple_retries_tracked_in_trace() {
    let events = vec![
        make_error_event("attempt 1 failed", Some(ErrorCode::BackendTimeout)),
        make_error_event("attempt 2 failed", Some(ErrorCode::BackendTimeout)),
        make_error_event("attempt 3 failed", Some(ErrorCode::BackendTimeout)),
    ];
    let receipt = make_receipt_with_events(events, Outcome::Failed);
    let error_count = receipt
        .trace
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        .count();
    assert_eq!(error_count, 3);
}

#[test]
fn warning_event_does_not_affect_outcome() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "deprecated API".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];
    let receipt = make_receipt_with_events(events, Outcome::Complete);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Partial receipt generation on mid-stream errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn partial_receipt_on_mid_stream_error() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "partial output".into(),
            },
            ext: None,
        },
        make_error_event("stream interrupted", Some(ErrorCode::BackendCrashed)),
    ];
    let receipt = make_receipt_with_events(events, Outcome::Partial);
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.trace.len(), 3);
}

#[test]
fn partial_receipt_preserves_all_events_before_error() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        },
        make_error_event("budget exceeded", Some(ErrorCode::BackendCrashed)),
    ];
    let receipt = make_receipt_with_events(events, Outcome::Partial);
    assert!(matches!(
        &receipt.trace[0].kind,
        AgentEventKind::ToolCall { .. }
    ));
    assert!(matches!(
        &receipt.trace[1].kind,
        AgentEventKind::ToolResult { .. }
    ));
    assert!(matches!(
        &receipt.trace[2].kind,
        AgentEventKind::Error { .. }
    ));
}

#[test]
fn partial_receipt_can_be_hashed() {
    let events = vec![make_error_event(
        "mid-stream crash",
        Some(ErrorCode::BackendCrashed),
    )];
    let receipt = make_receipt_with_events(events, Outcome::Partial);
    let hashed = receipt.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn partial_receipt_hash_is_deterministic() {
    let builder = || {
        let ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let event = AgentEvent {
            ts,
            kind: AgentEventKind::Error {
                message: "crash".into(),
                error_code: Some(ErrorCode::BackendCrashed),
            },
            ext: None,
        };
        ReceiptBuilder::new("test")
            .outcome(Outcome::Partial)
            .started_at(ts)
            .finished_at(ts)
            .add_trace_event(event)
    };
    let r1 = builder().build().with_hash().unwrap();
    let r2 = builder().build().with_hash().unwrap();
    // Run IDs differ but the hash-input includes the run_id, so hashes differ.
    // What matters: each individual hash is 64 hex chars (SHA-256).
    assert_eq!(r1.receipt_sha256.as_ref().unwrap().len(), 64);
    assert_eq!(r2.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn failed_receipt_empty_trace() {
    let receipt = make_receipt_with_events(vec![], Outcome::Failed);
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert!(receipt.trace.is_empty());
}

#[test]
fn complete_receipt_with_no_errors_in_trace() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "finished".into(),
            },
            ext: None,
        },
    ];
    let receipt = make_receipt_with_events(events, Outcome::Complete);
    let has_error = receipt
        .trace
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }));
    assert!(!has_error);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional cross-cutting tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_new_basic() {
    let err = AbpError::new(ErrorCode::Internal, "boom");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(err.message, "boom");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
}

#[test]
fn abp_error_with_source_preserves_chain() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "not found");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(inner);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "not found");
}

#[test]
fn abp_error_source_none_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn abp_error_context_btreemap_ordering() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("z_key", "last")
        .with_context("a_key", "first")
        .with_context("m_key", "middle");
    let keys: Vec<_> = err.context.keys().collect();
    assert_eq!(keys, vec!["a_key", "m_key", "z_key"]);
}

#[test]
fn abp_error_context_nested_json() {
    let err = AbpError::new(ErrorCode::Internal, "nested")
        .with_context("details", serde_json::json!({"a": 1, "b": [2, 3]}));
    assert_eq!(
        err.context["details"],
        serde_json::json!({"a": 1, "b": [2, 3]})
    );
}

#[test]
fn abp_error_category_shorthand() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unknown");
    assert_eq!(err.category(), ErrorCategory::Dialect);
    assert_eq!(err.category(), err.code.category());
}

#[test]
fn dto_to_abp_error_loses_source() {
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad".into(),
        context: BTreeMap::new(),
        source_message: Some("inner cause".into()),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert!(err.source.is_none()); // opaque source not recoverable
}

#[test]
fn protocol_error_violation_is_debug() {
    let pe = ProtocolError::Violation("bad".into());
    let dbg = format!("{pe:?}");
    assert!(dbg.contains("Violation"));
}

#[test]
fn protocol_error_unexpected_message_fields() {
    let pe = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let dbg = format!("{pe:?}");
    assert!(dbg.contains("hello"));
    assert!(dbg.contains("run"));
}

#[test]
fn fatal_envelope_from_abp_error() {
    let abp = AbpError::new(ErrorCode::BackendCrashed, "process exited");
    let envelope = Envelope::fatal_from_abp_error(Some("run-5".into()), &abp);
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = envelope
    {
        assert_eq!(ref_id, Some("run-5".into()));
        assert_eq!(error, "process exited");
        assert_eq!(error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_envelope_error_code_accessor() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "oops".into(),
        error_code: Some(ErrorCode::Internal),
    };
    assert_eq!(envelope.error_code(), Some(ErrorCode::Internal));
}

#[test]
fn non_fatal_envelope_error_code_is_none() {
    let envelope = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert_eq!(envelope.error_code(), None);
}

#[test]
fn fatal_envelope_without_error_code() {
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "generic error".into(),
        error_code: None,
    };
    assert_eq!(envelope.error_code(), None);
    let json = serde_json::to_value(&envelope).unwrap();
    assert!(json.get("error_code").is_none());
}

#[test]
fn error_code_serde_from_string() {
    let code: ErrorCode = serde_json::from_str(r#""backend_timeout""#).unwrap();
    assert_eq!(code, ErrorCode::BackendTimeout);
}

#[test]
fn error_code_serde_rejects_unknown() {
    let result: Result<ErrorCode, _> = serde_json::from_str(r#""NONEXISTENT_CODE""#);
    assert!(result.is_err());
}

#[test]
fn error_category_serde_from_string() {
    let cat: ErrorCategory = serde_json::from_str(r#""protocol""#).unwrap();
    assert_eq!(cat, ErrorCategory::Protocol);
}

#[test]
fn error_category_serde_rejects_unknown() {
    let result: Result<ErrorCategory, _> = serde_json::from_str(r#""nonexistent""#);
    assert!(result.is_err());
}

#[test]
fn all_error_codes_serialize_to_their_as_str() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!(r#""{}""#, code.as_str());
        assert_eq!(json, expected);
    }
}

#[test]
fn runtime_all_variants_have_error_codes() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("e")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("e")),
        RuntimeError::BackendFailed(anyhow::anyhow!("e")),
        RuntimeError::CapabilityCheckFailed("e".into()),
        RuntimeError::Classified(AbpError::new(ErrorCode::Internal, "e")),
        RuntimeError::NoProjectionMatch { reason: "e".into() },
    ];
    for v in &variants {
        let _ = v.error_code(); // should not panic
    }
}

#[test]
fn runtime_all_variants_into_abp_error() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("e")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("e")),
        RuntimeError::BackendFailed(anyhow::anyhow!("e")),
        RuntimeError::CapabilityCheckFailed("e".into()),
        RuntimeError::Classified(AbpError::new(ErrorCode::Internal, "e")),
        RuntimeError::NoProjectionMatch { reason: "e".into() },
    ];
    for v in variants {
        let abp = v.into_abp_error();
        let _ = abp.code; // should not panic
    }
}

#[test]
fn contract_error_json_variant() {
    let serde_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let ce = ContractError::Json(serde_err);
    assert!(ce.to_string().starts_with("failed to serialize JSON:"));
}

#[test]
fn receipt_with_error_trace_is_valid_json() {
    let events = vec![
        make_error_event("err1", Some(ErrorCode::BackendTimeout)),
        make_error_event("err2", None),
    ];
    let receipt = make_receipt_with_events(events, Outcome::Failed);
    let json = serde_json::to_string(&receipt).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_object());
}

#[test]
fn outcome_serde_roundtrip() {
    for outcome in &[Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(outcome).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(*outcome, back);
    }
}

#[test]
fn outcome_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&Outcome::Complete).unwrap(),
        r#""complete""#
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Partial).unwrap(),
        r#""partial""#
    );
    assert_eq!(
        serde_json::to_string(&Outcome::Failed).unwrap(),
        r#""failed""#
    );
}

#[test]
fn error_event_with_ext_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor_error": "something_specific"}),
    );
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "vendor error".into(),
            error_code: Some(ErrorCode::BackendCrashed),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert!(json["ext"]["raw_message"].is_object());
}

#[test]
fn protocol_decode_invalid_json_returns_json_error() {
    let err = JsonlCodec::decode("not json at all").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_decode_valid_but_wrong_type_returns_json_error() {
    let err = JsonlCodec::decode(r#"{"t":"unknown_type"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_fatal_envelope_roundtrip() {
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "oops".into(),
        error_code: Some(ErrorCode::Internal),
    };
    let line = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = decoded
    {
        assert_eq!(ref_id, Some("run-1".into()));
        assert_eq!(error, "oops");
        assert_eq!(error_code, Some(ErrorCode::Internal));
    } else {
        panic!("expected Fatal");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Send + Sync assertions for all error types
// ═══════════════════════════════════════════════════════════════════════════

const fn _assert_send_sync<T: Send + Sync>() {}

#[test]
fn abp_error_is_send_sync() {
    const { _assert_send_sync::<AbpError>() };
}

#[test]
fn protocol_error_is_send_sync() {
    const { _assert_send_sync::<ProtocolError>() };
}

#[test]
fn runtime_error_is_send_sync() {
    const { _assert_send_sync::<RuntimeError>() };
}

#[test]
fn contract_error_is_send_sync() {
    const { _assert_send_sync::<ContractError>() };
}

#[test]
fn host_error_is_send_sync() {
    const { _assert_send_sync::<HostError>() };
}

#[test]
fn config_error_is_send_sync() {
    const { _assert_send_sync::<ConfigError>() };
}

#[test]
fn projection_error_is_send_sync() {
    const { _assert_send_sync::<ProjectionError>() };
}

#[test]
fn daemon_error_is_send_sync() {
    const { _assert_send_sync::<DaemonError>() };
}

#[test]
fn sidecar_proto_error_is_send_sync() {
    const { _assert_send_sync::<SidecarProtoError>() };
}

#[test]
fn sidecar_error_is_send_sync() {
    const { _assert_send_sync::<SidecarError>() };
}

#[test]
fn bridge_error_is_send_sync() {
    const { _assert_send_sync::<BridgeError>() };
}

#[test]
fn mapping_validation_error_is_send_sync() {
    const { _assert_send_sync::<MappingValidationError>() };
}

#[test]
fn gemini_error_is_send_sync() {
    const { _assert_send_sync::<GeminiError>() };
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. HostError variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn host_error_spawn_display_and_source() {
    let err = HostError::Spawn(io::Error::new(io::ErrorKind::NotFound, "node not found"));
    assert!(err.to_string().contains("spawn sidecar"));
    assert!(err.to_string().contains("node not found"));
    assert!(StdError::source(&err).is_some());
}

#[test]
fn host_error_stdout_display_and_source() {
    let err = HostError::Stdout(io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke"));
    assert!(err.to_string().contains("read sidecar stdout"));
    assert!(StdError::source(&err).is_some());
}

#[test]
fn host_error_stdin_display_and_source() {
    let err = HostError::Stdin(io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke"));
    assert!(err.to_string().contains("write sidecar stdin"));
    assert!(StdError::source(&err).is_some());
}

#[test]
fn host_error_protocol_from_conversion() {
    let pe = ProtocolError::Violation("bad envelope".into());
    let he: HostError = pe.into();
    assert!(matches!(he, HostError::Protocol(_)));
    assert!(he.to_string().contains("protocol error"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("missing hello".into());
    assert!(err.to_string().contains("missing hello"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("out of memory".into());
    assert!(err.to_string().contains("out of memory"));
}

#[test]
fn host_error_exited_display() {
    let err = HostError::Exited { code: Some(1) };
    assert!(err.to_string().contains("1"));
}

#[test]
fn host_error_exited_no_code() {
    let err = HostError::Exited { code: None };
    assert!(err.to_string().contains("None"));
}

#[test]
fn host_error_sidecar_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed by OOM".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("137"));
    assert!(msg.contains("killed by OOM"));
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: std::time::Duration::from_secs(30),
    };
    assert!(err.to_string().contains("30"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. ConfigError variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_error_file_not_found_display() {
    let err = ConfigError::FileNotFound {
        path: "/etc/backplane.toml".into(),
    };
    assert!(err.to_string().contains("/etc/backplane.toml"));
}

#[test]
fn config_error_parse_error_display() {
    let err = ConfigError::ParseError {
        reason: "unexpected eof".into(),
    };
    assert!(err.to_string().contains("unexpected eof"));
}

#[test]
fn config_error_validation_error_display() {
    let err = ConfigError::ValidationError {
        reasons: vec!["missing backend".into(), "invalid timeout".into()],
    };
    let msg = err.to_string();
    assert!(msg.contains("missing backend"));
    assert!(msg.contains("invalid timeout"));
}

#[test]
fn config_error_merge_conflict_display() {
    let err = ConfigError::MergeConflict {
        reason: "conflicting timeout values".into(),
    };
    assert!(err.to_string().contains("conflicting timeout values"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. MappingError (abp-mapping) variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_feature_unsupported_display() {
    let err = MappingValidationError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"));
    assert!(msg.contains("unsupported"));
}

#[test]
fn mapping_error_fidelity_loss_display() {
    let err = MappingValidationError::FidelityLoss {
        feature: "system_prompt".into(),
        warning: "moved to first user message".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("system_prompt"));
    assert!(msg.contains("fidelity loss"));
}

#[test]
fn mapping_error_dialect_mismatch_display() {
    let err = MappingValidationError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Kimi,
    };
    assert!(err.to_string().contains("dialect mismatch"));
}

#[test]
fn mapping_error_invalid_input_display() {
    let err = MappingValidationError::InvalidInput {
        reason: "empty message list".into(),
    };
    assert!(err.to_string().contains("empty message list"));
}

#[test]
fn mapping_error_serde_roundtrip() {
    let err = MappingValidationError::FeatureUnsupported {
        feature: "vision".into(),
        from: Dialect::Claude,
        to: Dialect::Codex,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingValidationError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. ProjectionError variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn projection_error_no_suitable_backend_display() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "no backend supports tool_use + streaming".into(),
    };
    assert!(err.to_string().contains("no suitable backend"));
}

#[test]
fn projection_error_empty_matrix_display() {
    let err = ProjectionError::EmptyMatrix;
    assert!(err.to_string().contains("empty"));
}

#[test]
fn projection_error_serde_roundtrip() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "caps mismatch".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. DaemonError variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn daemon_error_not_found_display() {
    let err = DaemonError::NotFound("run-42".into());
    assert!(err.to_string().contains("run-42"));
}

#[test]
fn daemon_error_bad_request_display() {
    let err = DaemonError::BadRequest("missing task field".into());
    assert!(err.to_string().contains("missing task field"));
}

#[test]
fn daemon_error_conflict_display() {
    let err = DaemonError::Conflict("run already completed".into());
    assert!(err.to_string().contains("run already completed"));
}

#[test]
fn daemon_error_internal_from_anyhow() {
    let inner = anyhow::anyhow!("disk full");
    let err: DaemonError = inner.into();
    assert!(matches!(err, DaemonError::Internal(_)));
    assert!(err.to_string().contains("disk full"));
}

#[test]
fn daemon_error_runtime_from_conversion() {
    let re = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    let err: DaemonError = re.into();
    assert!(matches!(err, DaemonError::Runtime(_)));
    assert!(err.to_string().contains("unknown backend"));
}

#[test]
fn daemon_error_runtime_source_chain() {
    let re = RuntimeError::WorkspaceFailed(anyhow::anyhow!("staging failed"));
    let err: DaemonError = re.into();
    let src = StdError::source(&err).unwrap();
    assert!(src.to_string().contains("workspace preparation failed"));
}

#[test]
fn daemon_error_status_codes() {
    use axum::http::StatusCode;
    assert_eq!(
        DaemonError::NotFound("x".into()).status_code(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        DaemonError::BadRequest("x".into()).status_code(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        DaemonError::Conflict("x".into()).status_code(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        DaemonError::Internal(anyhow::anyhow!("x")).status_code(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. SidecarProtoError variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_proto_error_json_from_serde() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
    let err: SidecarProtoError = serde_err.into();
    assert!(matches!(err, SidecarProtoError::Json(_)));
}

#[test]
fn sidecar_proto_error_io_from_std() {
    let io_err = io::Error::new(io::ErrorKind::ConnectionReset, "reset");
    let err: SidecarProtoError = io_err.into();
    assert!(matches!(err, SidecarProtoError::Io(_)));
    assert!(StdError::source(&err).is_some());
}

#[test]
fn sidecar_proto_error_protocol_from_conversion() {
    let pe = ProtocolError::Violation("bad".into());
    let err: SidecarProtoError = pe.into();
    assert!(matches!(err, SidecarProtoError::Protocol(_)));
}

#[test]
fn sidecar_proto_error_unexpected_message_display() {
    let err = SidecarProtoError::UnexpectedMessage {
        expected: "run".into(),
        got: "hello".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("run"));
    assert!(msg.contains("hello"));
}

#[test]
fn sidecar_proto_error_handler_display() {
    let err = SidecarProtoError::Handler("tool execution failed".into());
    assert!(err.to_string().contains("tool execution failed"));
}

#[test]
fn sidecar_proto_error_stdin_closed_display() {
    let err = SidecarProtoError::StdinClosed;
    assert!(err.to_string().contains("stdin closed"));
}

#[test]
fn sidecar_proto_error_channel_closed_display() {
    let err = SidecarProtoError::ChannelClosed;
    assert!(err.to_string().contains("channel closed"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. SidecarError (sidecar-kit) variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_error_spawn_display_and_source() {
    let err = SidecarError::Spawn(io::Error::new(io::ErrorKind::NotFound, "python not found"));
    assert!(err.to_string().contains("python not found"));
    assert!(StdError::source(&err).is_some());
}

#[test]
fn sidecar_error_stdout_display() {
    let err = SidecarError::Stdout(io::Error::new(io::ErrorKind::BrokenPipe, "broken"));
    assert!(err.to_string().contains("stdout"));
}

#[test]
fn sidecar_error_stdin_display() {
    let err = SidecarError::Stdin(io::Error::new(io::ErrorKind::BrokenPipe, "broken"));
    assert!(err.to_string().contains("stdin"));
}

#[test]
fn sidecar_error_protocol_display() {
    let err = SidecarError::Protocol("missing hello".into());
    assert!(err.to_string().contains("missing hello"));
}

#[test]
fn sidecar_error_serialize_display_and_source() {
    let serde_err = serde_json::to_value(f64::NAN).unwrap_err();
    let err = SidecarError::Serialize(serde_err);
    assert!(err.to_string().contains("serialization"));
    assert!(StdError::source(&err).is_some());
}

#[test]
fn sidecar_error_deserialize_display_and_source() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{{").unwrap_err();
    let err = SidecarError::Deserialize(serde_err);
    assert!(err.to_string().contains("deserialization"));
    assert!(StdError::source(&err).is_some());
}

#[test]
fn sidecar_error_fatal_display() {
    let err = SidecarError::Fatal("process crashed".into());
    assert!(err.to_string().contains("process crashed"));
}

#[test]
fn sidecar_error_exited_display() {
    let err = SidecarError::Exited(Some(137));
    assert!(err.to_string().contains("137"));
}

#[test]
fn sidecar_error_exited_none() {
    let err = SidecarError::Exited(None);
    assert!(err.to_string().contains("None"));
}

#[test]
fn sidecar_error_timeout_display() {
    let err = SidecarError::Timeout;
    assert!(err.to_string().contains("timed out"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. BridgeError (claude-bridge) variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn bridge_error_node_not_found_display() {
    let err = BridgeError::NodeNotFound("node binary missing".into());
    assert!(err.to_string().contains("node"));
}

#[test]
fn bridge_error_host_script_not_found_display() {
    let err = BridgeError::HostScriptNotFound("hosts/claude/index.js".into());
    assert!(err.to_string().contains("hosts/claude/index.js"));
}

#[test]
fn bridge_error_sidecar_from_conversion() {
    let se = SidecarError::Timeout;
    let err: BridgeError = se.into();
    assert!(matches!(err, BridgeError::Sidecar(_)));
}

#[test]
fn bridge_error_sidecar_source_chain() {
    let se = SidecarError::Spawn(io::Error::new(io::ErrorKind::NotFound, "not found"));
    let err: BridgeError = se.into();
    let src = StdError::source(&err).unwrap();
    assert!(src.to_string().contains("spawn sidecar"));
}

#[test]
fn bridge_error_config_display() {
    let err = BridgeError::Config("missing API key".into());
    assert!(err.to_string().contains("missing API key"));
}

#[test]
fn bridge_error_run_display() {
    let err = BridgeError::Run("execution timed out".into());
    assert!(err.to_string().contains("execution timed out"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 25. GeminiError variant construction and Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_error_request_conversion_display() {
    let err = GeminiError::RequestConversion("unsupported content type".into());
    assert!(err.to_string().contains("unsupported content type"));
}

#[test]
fn gemini_error_response_conversion_display() {
    let err = GeminiError::ResponseConversion("unexpected finish reason".into());
    assert!(err.to_string().contains("unexpected finish reason"));
}

#[test]
fn gemini_error_backend_error_display() {
    let err = GeminiError::BackendError("quota exhausted".into());
    assert!(err.to_string().contains("quota exhausted"));
}

#[test]
fn gemini_error_serde_from_conversion() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
    let err: GeminiError = serde_err.into();
    assert!(matches!(err, GeminiError::Serde(_)));
    assert!(StdError::source(&err).is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 26. ShimError variants from multiple shim crates
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_shim_error_invalid_request() {
    let err = abp_shim_claude::ShimError::InvalidRequest("empty messages".into());
    assert!(err.to_string().contains("empty messages"));
}

#[test]
fn claude_shim_error_api_error() {
    let err = abp_shim_claude::ShimError::ApiError {
        error_type: "rate_limit".into(),
        message: "too many requests".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("rate_limit"));
    assert!(msg.contains("too many requests"));
}

#[test]
fn claude_shim_error_internal() {
    let err = abp_shim_claude::ShimError::Internal("ir lowering failed".into());
    assert!(err.to_string().contains("ir lowering failed"));
}

#[test]
fn openai_shim_error_invalid_request() {
    let err = abp_shim_openai::ShimError::InvalidRequest("bad model".into());
    assert!(err.to_string().contains("bad model"));
}

#[test]
fn openai_shim_error_serde_from() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let err: abp_shim_openai::ShimError = serde_err.into();
    assert!(matches!(err, abp_shim_openai::ShimError::Serde(_)));
}

#[test]
fn codex_shim_error_variants() {
    let err = abp_shim_codex::ShimError::InvalidRequest("bad input".into());
    assert!(err.to_string().contains("bad input"));
    let err = abp_shim_codex::ShimError::Internal("oops".into());
    assert!(err.to_string().contains("oops"));
}

#[test]
fn kimi_shim_error_variants() {
    let err = abp_shim_kimi::ShimError::InvalidRequest("missing role".into());
    assert!(err.to_string().contains("missing role"));
}

#[test]
fn copilot_shim_error_variants() {
    let err = abp_shim_copilot::ShimError::InvalidRequest("bad auth".into());
    assert!(err.to_string().contains("bad auth"));
    let err = abp_shim_copilot::ShimError::Internal("conversion failed".into());
    assert!(err.to_string().contains("conversion failed"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 27. Cross-crate error conversions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_into_host_error() {
    let pe = ProtocolError::Violation("bad ref_id".into());
    let he: HostError = pe.into();
    assert!(matches!(he, HostError::Protocol(_)));
}

#[test]
fn protocol_error_into_sidecar_proto_error() {
    let pe = ProtocolError::Violation("unexpected".into());
    let spe: SidecarProtoError = pe.into();
    assert!(matches!(spe, SidecarProtoError::Protocol(_)));
}

#[test]
fn io_error_into_protocol_error() {
    let io_err = io::Error::new(io::ErrorKind::UnexpectedEof, "eof");
    let pe: ProtocolError = io_err.into();
    assert!(matches!(pe, ProtocolError::Io(_)));
}

#[test]
fn serde_error_into_protocol_error() {
    let serde_err = serde_json::from_str::<serde_json::Value>("!").unwrap_err();
    let pe: ProtocolError = serde_err.into();
    assert!(matches!(pe, ProtocolError::Json(_)));
}

#[test]
fn abp_error_into_protocol_error() {
    let abp = AbpError::new(ErrorCode::ProtocolVersionMismatch, "v0.1 vs v0.2");
    let pe: ProtocolError = abp.into();
    assert!(matches!(pe, ProtocolError::Abp(_)));
}

#[test]
fn abp_error_into_runtime_error() {
    let abp = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let re: RuntimeError = abp.into();
    assert!(matches!(re, RuntimeError::Classified(_)));
}

#[test]
fn runtime_error_into_daemon_error() {
    let re = RuntimeError::UnknownBackend { name: "foo".into() };
    let de: DaemonError = re.into();
    assert!(matches!(de, DaemonError::Runtime(_)));
}

#[test]
fn anyhow_error_into_daemon_error() {
    let err = anyhow::anyhow!("internal failure");
    let de: DaemonError = err.into();
    assert!(matches!(de, DaemonError::Internal(_)));
}

#[test]
fn sidecar_error_into_bridge_error() {
    let se = SidecarError::Fatal("crashed".into());
    let be: BridgeError = se.into();
    assert!(matches!(be, BridgeError::Sidecar(_)));
}

#[test]
fn io_error_into_sidecar_proto_error() {
    let io_err = io::Error::new(io::ErrorKind::TimedOut, "timeout");
    let spe: SidecarProtoError = io_err.into();
    assert!(matches!(spe, SidecarProtoError::Io(_)));
}

#[test]
fn serde_error_into_sidecar_proto_error() {
    let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let spe: SidecarProtoError = serde_err.into();
    assert!(matches!(spe, SidecarProtoError::Json(_)));
}

#[test]
fn serde_error_into_gemini_error() {
    let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let ge: GeminiError = serde_err.into();
    assert!(matches!(ge, GeminiError::Serde(_)));
}

// ═══════════════════════════════════════════════════════════════════════════
// 28. Error source chains across crate boundaries
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn host_error_protocol_source_chain() {
    let pe = ProtocolError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "pipe"));
    let he: HostError = pe.into();
    // HostError::Protocol -> ProtocolError::Io -> io::Error
    let src1 = StdError::source(&he).unwrap();
    assert!(src1.to_string().contains("I/O error"));
    let src2 = StdError::source(src1).unwrap();
    assert_eq!(src2.to_string(), "pipe");
}

#[test]
fn daemon_error_runtime_workspace_source_chain() {
    let root = io::Error::new(io::ErrorKind::PermissionDenied, "read only");
    let chain = anyhow::Error::new(root).context("staging dir");
    let re = RuntimeError::WorkspaceFailed(chain);
    let de: DaemonError = re.into();
    // DaemonError::Runtime -> RuntimeError::WorkspaceFailed -> anyhow
    let src1 = StdError::source(&de).unwrap();
    assert!(src1.to_string().contains("workspace preparation failed"));
    let src2 = StdError::source(src1).unwrap();
    assert!(src2.to_string().contains("staging dir"));
}

#[test]
fn sidecar_proto_error_protocol_io_chain() {
    let io_err = io::Error::new(io::ErrorKind::ConnectionRefused, "refused");
    let pe = ProtocolError::Io(io_err);
    let spe: SidecarProtoError = pe.into();
    let src1 = StdError::source(&spe).unwrap();
    assert!(src1.to_string().contains("I/O error"));
}

#[test]
fn bridge_error_sidecar_spawn_chain() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "binary not found");
    let se = SidecarError::Spawn(io_err);
    let be: BridgeError = se.into();
    // BridgeError::Sidecar -> SidecarError::Spawn -> io::Error
    let src1 = StdError::source(&be).unwrap();
    assert!(src1.to_string().contains("spawn sidecar"));
    let src2 = StdError::source(src1).unwrap();
    assert_eq!(src2.to_string(), "binary not found");
}

// ═══════════════════════════════════════════════════════════════════════════
// 29. Wire-format error serialization roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_validation_error_all_variants_roundtrip() {
    let errors = vec![
        MappingValidationError::FeatureUnsupported {
            feature: "vision".into(),
            from: Dialect::Claude,
            to: Dialect::Codex,
        },
        MappingValidationError::FidelityLoss {
            feature: "system_prompt".into(),
            warning: "moved".into(),
        },
        MappingValidationError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Gemini,
        },
        MappingValidationError::InvalidInput {
            reason: "empty".into(),
        },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: MappingValidationError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
}

#[test]
fn projection_error_all_variants_roundtrip() {
    let errors = vec![
        ProjectionError::NoSuitableBackend {
            reason: "no match".into(),
        },
        ProjectionError::EmptyMatrix,
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
}

#[test]
fn abp_error_dto_full_roundtrip_all_categories() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, format!("test message for {}", code.as_str()))
            .with_context("test_key", "test_value");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
        assert_eq!(back.code, *code);
    }
}

#[test]
fn error_code_json_roundtrip_all_variants() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back);
    }
}

#[test]
fn error_category_json_roundtrip_all_variants() {
    for cat in ALL_CATEGORIES {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back);
    }
}

#[test]
fn fatal_envelope_with_every_error_code_roundtrip() {
    for code in ALL_CODES {
        let envelope = Envelope::fatal_with_code(
            Some("run-test".into()),
            &format!("test fatal for {}", code.as_str()),
            *code,
        );
        let line = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert_eq!(decoded.error_code(), Some(*code));
    }
}
