// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive error taxonomy tests: construction, Display/Debug,
//! categorisation, serde roundtrips, DTO conversion, error chains, cross-crate
//! propagation, envelope integration, retry/fatal classification, context
//! enrichment, batch handling, and taxonomy tree traversal.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error as StdError;
use std::io;

use abp_core::{AgentEvent, AgentEventKind, ContractError, Outcome, ReceiptBuilder};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_runtime::RuntimeError;
use chrono::Utc;

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

fn make_error(code: ErrorCode, msg: &str) -> AbpError {
    AbpError::new(code, msg)
}

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

fn make_receipt_with_events(events: Vec<AgentEvent>, outcome: Outcome) -> abp_core::Receipt {
    let mut builder = ReceiptBuilder::new("test-backend").outcome(outcome);
    for e in events {
        builder = builder.add_trace_event(e);
    }
    builder.build()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. ErrorCode construction and Display/Debug impls
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_display_equals_as_str_for_all() {
    for code in ALL_CODES {
        assert_eq!(
            code.to_string(),
            code.as_str(),
            "Display != as_str for {code:?}"
        );
    }
}

#[test]
fn error_code_debug_contains_variant_name() {
    let dbg = format!("{:?}", ErrorCode::BackendTimeout);
    assert!(dbg.contains("BackendTimeout"));
}

#[test]
fn error_code_debug_all_variants_non_empty() {
    for code in ALL_CODES {
        let dbg = format!("{code:?}");
        assert!(!dbg.is_empty());
    }
}

#[test]
fn error_code_as_str_is_screaming_snake_case() {
    for code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
            "as_str for {code:?} is not SCREAMING_SNAKE: {s}"
        );
    }
}

#[test]
fn error_code_count_is_20() {
    assert_eq!(ALL_CODES.len(), 20);
}

#[test]
fn error_code_as_str_unique() {
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
fn error_code_clone_eq() {
    for code in ALL_CODES {
        let cloned = *code;
        assert_eq!(*code, cloned);
    }
}

#[test]
fn error_code_hash_consistency() {
    use std::hash::{Hash, Hasher};
    for code in ALL_CODES {
        let mut h1 = std::collections::hash_map::DefaultHasher::new();
        let mut h2 = std::collections::hash_map::DefaultHasher::new();
        code.hash(&mut h1);
        code.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. ErrorCategory enum variants and conversions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn category_count_is_10() {
    assert_eq!(ALL_CATEGORIES.len(), 10);
}

#[test]
fn category_display_all() {
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
    for (cat, s) in &expected {
        assert_eq!(cat.to_string(), *s);
    }
}

#[test]
fn category_debug_non_empty() {
    for cat in ALL_CATEGORIES {
        assert!(!format!("{cat:?}").is_empty());
    }
}

#[test]
fn category_clone_eq() {
    for cat in ALL_CATEGORIES {
        let cloned = *cat;
        assert_eq!(*cat, cloned);
    }
}

#[test]
fn every_category_has_at_least_one_code() {
    for cat in ALL_CATEGORIES {
        let count = ALL_CODES.iter().filter(|c| c.category() == *cat).count();
        assert!(count >= 1, "no codes for category {cat:?}");
    }
}

#[test]
fn protocol_category_has_3_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Protocol)
        .count();
    assert_eq!(count, 3);
}

#[test]
fn backend_category_has_3_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Backend)
        .count();
    assert_eq!(count, 3);
}

#[test]
fn capability_category_has_2_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Capability)
        .count();
    assert_eq!(count, 2);
}

#[test]
fn policy_category_has_2_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Policy)
        .count();
    assert_eq!(count, 2);
}

#[test]
fn workspace_category_has_2_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Workspace)
        .count();
    assert_eq!(count, 2);
}

#[test]
fn ir_category_has_2_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Ir)
        .count();
    assert_eq!(count, 2);
}

#[test]
fn receipt_category_has_2_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Receipt)
        .count();
    assert_eq!(count, 2);
}

#[test]
fn dialect_category_has_2_codes() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Dialect)
        .count();
    assert_eq!(count, 2);
}

#[test]
fn config_category_has_1_code() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Config)
        .count();
    assert_eq!(count, 1);
}

#[test]
fn internal_category_has_1_code() {
    let count = ALL_CODES
        .iter()
        .filter(|c| c.category() == ErrorCategory::Internal)
        .count();
    assert_eq!(count, 1);
}

#[test]
fn category_hash_in_hashmap() {
    let mut m: HashMap<ErrorCategory, usize> = HashMap::new();
    for code in ALL_CODES {
        *m.entry(code.category()).or_default() += 1;
    }
    assert_eq!(m.len(), 10);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Error code → category mapping stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_codes_map_to_protocol() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.category(),
        ErrorCategory::Protocol
    );
}

#[test]
fn backend_codes_map_to_backend() {
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
    assert_eq!(ErrorCode::BackendCrashed.category(), ErrorCategory::Backend);
}

#[test]
fn capability_codes_map_to_capability() {
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
fn policy_codes_map_to_policy() {
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::PolicyInvalid.category(), ErrorCategory::Policy);
}

#[test]
fn workspace_codes_map_to_workspace() {
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
fn ir_codes_map_to_ir() {
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(ErrorCode::IrInvalid.category(), ErrorCategory::Ir);
}

#[test]
fn receipt_codes_map_to_receipt() {
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
fn dialect_codes_map_to_dialect() {
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(
        ErrorCode::DialectMappingFailed.category(),
        ErrorCategory::Dialect
    );
}

#[test]
fn config_code_maps_to_config() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn internal_code_maps_to_internal() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. AbpError construction and builder pattern
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
fn abp_error_with_context_string() {
    let err = make_error(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
}

#[test]
fn abp_error_with_context_integer() {
    let err = make_error(ErrorCode::BackendTimeout, "timed out").with_context("timeout_ms", 30_000);
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_with_context_float() {
    let err = make_error(ErrorCode::ConfigInvalid, "bad temp").with_context("temperature", 2.5);
    assert_eq!(err.context["temperature"], serde_json::json!(2.5));
}

#[test]
fn abp_error_with_context_bool() {
    let err = make_error(ErrorCode::Internal, "debug").with_context("verbose", true);
    assert_eq!(err.context["verbose"], serde_json::json!(true));
}

#[test]
fn abp_error_with_context_null() {
    let err =
        make_error(ErrorCode::Internal, "null ctx").with_context("empty", serde_json::Value::Null);
    assert_eq!(err.context["empty"], serde_json::Value::Null);
}

#[test]
fn abp_error_with_context_nested_json() {
    let nested = serde_json::json!({"a": 1, "b": [2, 3]});
    let err = make_error(ErrorCode::Internal, "nested").with_context("details", nested.clone());
    assert_eq!(err.context["details"], nested);
}

#[test]
fn abp_error_with_context_overwrites_key() {
    let err = make_error(ErrorCode::Internal, "overwrite")
        .with_context("key", "first")
        .with_context("key", "second");
    assert_eq!(err.context["key"], serde_json::json!("second"));
    assert_eq!(err.context.len(), 1);
}

#[test]
fn abp_error_with_multiple_contexts() {
    let err = make_error(ErrorCode::BackendTimeout, "timeout")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    assert_eq!(err.context.len(), 3);
}

#[test]
fn abp_error_with_source() {
    let src = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let err = make_error(ErrorCode::PolicyDenied, "denied").with_source(src);
    assert!(err.source.is_some());
    assert_eq!(err.source.as_ref().unwrap().to_string(), "access denied");
}

#[test]
fn abp_error_builder_chaining() {
    let src = io::Error::other("underlying");
    let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml")
        .with_source(src);
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.context["file"], serde_json::json!("backplane.toml"));
    assert!(err.source.is_some());
}

#[test]
fn abp_error_category_shorthand() {
    let err = make_error(ErrorCode::DialectUnknown, "unknown dialect");
    assert_eq!(err.category(), ErrorCategory::Dialect);
    assert_eq!(err.category(), err.code.category());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. AbpError Display and Debug
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_display_format_no_context() {
    let err = make_error(ErrorCode::BackendNotFound, "no such backend");
    assert_eq!(err.to_string(), "[BACKEND_NOT_FOUND] no such backend");
}

#[test]
fn abp_error_display_format_with_context() {
    let err = make_error(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 5000);
    let s = err.to_string();
    assert!(s.starts_with("[BACKEND_TIMEOUT] timed out "));
    assert!(s.contains("\"backend\""));
    assert!(s.contains("\"timeout_ms\""));
}

#[test]
fn abp_error_display_btreemap_deterministic_ordering() {
    let err = make_error(ErrorCode::Internal, "test")
        .with_context("z_key", "last")
        .with_context("a_key", "first");
    let s = err.to_string();
    let a_pos = s.find("a_key").unwrap();
    let z_pos = s.find("z_key").unwrap();
    assert!(a_pos < z_pos, "BTreeMap ordering violated");
}

#[test]
fn abp_error_debug_contains_code() {
    let err = make_error(ErrorCode::PolicyDenied, "nope");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("PolicyDenied"));
}

#[test]
fn abp_error_debug_contains_message() {
    let err = make_error(ErrorCode::PolicyDenied, "nope");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("nope"));
}

#[test]
fn abp_error_debug_contains_source_when_present() {
    let src = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let err = make_error(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("file missing"));
}

#[test]
fn abp_error_debug_omits_source_when_absent() {
    let err = make_error(ErrorCode::Internal, "oops");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("source"));
}

#[test]
fn abp_error_debug_contains_context_when_present() {
    let err = make_error(ErrorCode::Internal, "ctx").with_context("key", "val");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
}

#[test]
fn abp_error_debug_omits_context_when_empty() {
    let err = make_error(ErrorCode::Internal, "no ctx");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("context"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Error chain/cause tracking
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn std_error_source_returns_inner() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "not found");
    let err = make_error(ErrorCode::WorkspaceStagingFailed, "staging").with_source(inner);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "not found");
}

#[test]
fn std_error_source_none_by_default() {
    let err = make_error(ErrorCode::Internal, "oops");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn nested_source_chain() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let mid = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(inner);
    // Wrap the AbpError in a ProtocolError (Abp variant)
    let outer = ProtocolError::Abp(mid);
    let outer_msg = outer.to_string();
    assert!(outer_msg.contains("init failed"));
}

#[test]
fn abp_error_send_sync_static_bounds() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<AbpError>();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Error serialization/deserialization roundtrips
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
fn error_code_serializes_screaming_snake() {
    assert_eq!(
        serde_json::to_string(&ErrorCode::ProtocolInvalidEnvelope).unwrap(),
        r#""protocol_invalid_envelope""#
    );
    assert_eq!(
        serde_json::to_string(&ErrorCode::BackendTimeout).unwrap(),
        r#""backend_timeout""#
    );
}

#[test]
fn error_code_deserializes_from_string() {
    let code: ErrorCode = serde_json::from_str(r#""backend_timeout""#).unwrap();
    assert_eq!(code, ErrorCode::BackendTimeout);
}

#[test]
fn error_code_rejects_unknown_variant() {
    let result: Result<ErrorCode, _> = serde_json::from_str(r#""NONEXISTENT_CODE""#);
    assert!(result.is_err());
}

#[test]
fn error_code_rejects_lowercase() {
    let result: Result<ErrorCode, _> = serde_json::from_str(r#""backend_timeout""#);
    assert!(result.is_err());
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
fn error_category_serializes_snake_case() {
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Backend).unwrap(),
        r#""backend""#
    );
}

#[test]
fn error_category_rejects_unknown() {
    let result: Result<ErrorCategory, _> = serde_json::from_str(r#""nonexistent""#);
    assert!(result.is_err());
}

#[test]
fn all_codes_serialize_to_as_str() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!(r#""{}""#, code.as_str());
        assert_eq!(json, expected);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. AbpErrorDto conversion and roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dto_from_abp_error_without_source() {
    let err = make_error(ErrorCode::IrInvalid, "bad IR").with_context("node", "call_tool");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::IrInvalid);
    assert_eq!(dto.message, "bad IR");
    assert_eq!(dto.context["node"], serde_json::json!("call_tool"));
    assert!(dto.source_message.is_none());
}

#[test]
fn dto_from_abp_error_with_source() {
    let src = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let err = make_error(ErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn dto_serde_roundtrip() {
    let err = make_error(ErrorCode::DialectUnknown, "unknown").with_context("dialect", "foobar");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn dto_json_omits_source_when_none() {
    let err = make_error(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

#[test]
fn dto_json_includes_source_when_some() {
    let src = io::Error::new(io::ErrorKind::ConnectionReset, "reset");
    let err = make_error(ErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("source_message"));
    assert!(json.contains("reset"));
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
    assert!(err.source.is_none());
}

#[test]
fn dto_to_abp_error_preserves_context() {
    let mut ctx = BTreeMap::new();
    ctx.insert("vendor".to_string(), serde_json::json!("gemini"));
    let dto = AbpErrorDto {
        code: ErrorCode::BackendTimeout,
        message: "timeout".into(),
        context: ctx,
        source_message: None,
    };
    let err: AbpError = dto.into();
    assert_eq!(err.context["vendor"], serde_json::json!("gemini"));
}

#[test]
fn dto_deterministic_json() {
    let err = make_error(ErrorCode::Internal, "oops")
        .with_context("b_key", "second")
        .with_context("a_key", "first");
    let dto: AbpErrorDto = (&err).into();
    let json1 = serde_json::to_string(&dto).unwrap();
    let json2 = serde_json::to_string(&dto).unwrap();
    assert_eq!(json1, json2);
    let a_pos = json1.find("a_key").unwrap();
    let b_pos = json1.find("b_key").unwrap();
    assert!(a_pos < b_pos);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. ProtocolError integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_json_from_serde() {
    let err: Result<serde_json::Value, _> = serde_json::from_str("not json {{{");
    let pe = ProtocolError::Json(err.unwrap_err());
    assert!(pe.to_string().starts_with("invalid JSON:"));
}

#[test]
fn protocol_error_io() {
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
fn protocol_error_abp_variant() {
    let abp = make_error(ErrorCode::ProtocolVersionMismatch, "v0.1 vs v0.2");
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
    let abp = make_error(ErrorCode::Internal, "internal");
    let pe: ProtocolError = abp.into();
    assert!(matches!(pe, ProtocolError::Abp(_)));
}

#[test]
fn protocol_error_code_for_violation() {
    let pe = ProtocolError::Violation("bad".into());
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn protocol_error_code_for_unexpected_message() {
    let pe = ProtocolError::UnexpectedMessage {
        expected: "run".into(),
        got: "hello".into(),
    };
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn protocol_error_code_for_abp() {
    let abp = make_error(ErrorCode::BackendTimeout, "timeout");
    let pe = ProtocolError::Abp(abp);
    assert_eq!(pe.error_code(), Some(ErrorCode::BackendTimeout));
}

#[test]
fn protocol_error_code_none_for_io() {
    let pe = ProtocolError::Io(io::Error::other("io"));
    assert_eq!(pe.error_code(), None);
}

#[test]
fn protocol_error_code_none_for_json() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let pe = ProtocolError::Json(serde_err);
    assert_eq!(pe.error_code(), None);
}

#[test]
fn protocol_decode_invalid_json() {
    let err = JsonlCodec::decode("not json at all").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_decode_unknown_envelope_type() {
    let err = JsonlCodec::decode(r#"{"t":"unknown_type"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. RuntimeError integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_error_unknown_backend_display() {
    let re = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    assert!(re.to_string().contains("ghost"));
}

#[test]
fn runtime_error_workspace_failed_display() {
    let re = RuntimeError::WorkspaceFailed(anyhow::anyhow!("inner"));
    assert_eq!(re.to_string(), "workspace preparation failed");
}

#[test]
fn runtime_error_policy_failed_display() {
    let re = RuntimeError::PolicyFailed(anyhow::anyhow!("inner"));
    assert_eq!(re.to_string(), "policy compilation failed");
}

#[test]
fn runtime_error_backend_failed_display() {
    let re = RuntimeError::BackendFailed(anyhow::anyhow!("inner"));
    assert_eq!(re.to_string(), "backend execution failed");
}

#[test]
fn runtime_error_capability_check_display() {
    let re = RuntimeError::CapabilityCheckFailed("streaming not supported".into());
    assert!(re.to_string().contains("streaming not supported"));
}

#[test]
fn runtime_error_no_projection_display() {
    let re = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    assert_eq!(re.to_string(), "projection failed: none");
}

#[test]
fn runtime_error_classified_display() {
    let abp = make_error(ErrorCode::PolicyDenied, "tool write denied");
    let re: RuntimeError = abp.into();
    assert!(re.to_string().contains("tool write denied"));
}

#[test]
fn runtime_error_code_unknown_backend() {
    let re = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(re.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_code_workspace_failed() {
    let re = RuntimeError::WorkspaceFailed(anyhow::anyhow!("e"));
    assert_eq!(re.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_code_policy_failed() {
    let re = RuntimeError::PolicyFailed(anyhow::anyhow!("e"));
    assert_eq!(re.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_code_backend_failed() {
    let re = RuntimeError::BackendFailed(anyhow::anyhow!("e"));
    assert_eq!(re.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_code_capability_check() {
    let re = RuntimeError::CapabilityCheckFailed("e".into());
    assert_eq!(re.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_code_classified() {
    let abp = make_error(ErrorCode::DialectMappingFailed, "mapping");
    let re = RuntimeError::Classified(abp);
    assert_eq!(re.error_code(), ErrorCode::DialectMappingFailed);
}

#[test]
fn runtime_error_code_no_projection() {
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
        make_error(ErrorCode::PolicyDenied, "denied by policy").with_context("tool", "bash");
    let re = RuntimeError::Classified(original);
    let abp = re.into_abp_error();
    assert_eq!(abp.code, ErrorCode::PolicyDenied);
    assert_eq!(abp.message, "denied by policy");
    assert_eq!(abp.context["tool"], serde_json::json!("bash"));
}

#[test]
fn runtime_all_variants_have_error_codes() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("e")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("e")),
        RuntimeError::BackendFailed(anyhow::anyhow!("e")),
        RuntimeError::CapabilityCheckFailed("e".into()),
        RuntimeError::Classified(make_error(ErrorCode::Internal, "e")),
        RuntimeError::NoProjectionMatch { reason: "e".into() },
    ];
    for v in &variants {
        let _ = v.error_code();
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
        RuntimeError::Classified(make_error(ErrorCode::Internal, "e")),
        RuntimeError::NoProjectionMatch { reason: "e".into() },
    ];
    for v in variants {
        let abp = v.into_abp_error();
        let _ = abp.code;
    }
}

#[test]
fn runtime_workspace_failed_source_chain() {
    let src = io::Error::new(io::ErrorKind::PermissionDenied, "read-only");
    let re = RuntimeError::WorkspaceFailed(anyhow::Error::new(src).context("staging /tmp/ws"));
    let source = StdError::source(&re).unwrap();
    assert!(source.to_string().contains("staging /tmp/ws"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Retry-able vs fatal error classification
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn transient_errors_are_backend_family() {
    let transient = [ErrorCode::BackendTimeout, ErrorCode::BackendCrashed];
    for code in &transient {
        assert_eq!(code.category(), ErrorCategory::Backend);
    }
}

#[test]
fn permanent_errors_are_policy_or_config() {
    let permanent = [
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::ConfigInvalid,
    ];
    for code in &permanent {
        let cat = code.category();
        assert!(
            cat == ErrorCategory::Policy || cat == ErrorCategory::Config,
            "unexpected category for {code:?}: {cat:?}"
        );
    }
}

#[test]
fn integrity_errors_are_receipt_family() {
    let integrity = [
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ];
    for code in &integrity {
        assert_eq!(code.category(), ErrorCategory::Receipt);
    }
}

#[test]
fn translation_errors_are_ir_or_dialect() {
    let translation = [
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
    ];
    for code in &translation {
        let cat = code.category();
        assert!(
            cat == ErrorCategory::Ir || cat == ErrorCategory::Dialect,
            "unexpected category for {code:?}: {cat:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Cross-SDK error mapping patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_rate_limit_to_abp_timeout() {
    let err = make_error(ErrorCode::BackendTimeout, "rate limit exceeded")
        .with_context("vendor", "openai")
        .with_context("http_status", 429);
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.category(), ErrorCategory::Backend);
}

#[test]
fn anthropic_auth_error_to_abp_config() {
    let err = make_error(ErrorCode::ConfigInvalid, "invalid API key")
        .with_context("vendor", "anthropic")
        .with_context("http_status", 401);
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert_eq!(err.category(), ErrorCategory::Config);
}

#[test]
fn gemini_context_length_to_abp_crashed() {
    let err = make_error(ErrorCode::BackendCrashed, "context length exceeded")
        .with_context("vendor", "gemini")
        .with_context("max_tokens", 128_000);
    assert_eq!(err.code, ErrorCode::BackendCrashed);
}

#[test]
fn copilot_model_not_found_to_abp_not_found() {
    let err = make_error(ErrorCode::BackendNotFound, "model not found")
        .with_context("vendor", "copilot")
        .with_context("model", "gpt-99");
    assert_eq!(err.code, ErrorCode::BackendNotFound);
}

#[test]
fn kimi_dialect_mapping_failure() {
    let err = make_error(ErrorCode::DialectMappingFailed, "unknown tool format")
        .with_context("vendor", "kimi");
    assert_eq!(err.code, ErrorCode::DialectMappingFailed);
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

#[test]
fn vendor_error_context_preserved_through_dto_roundtrip() {
    let err = make_error(ErrorCode::BackendTimeout, "rate limited")
        .with_context("vendor", "gemini")
        .with_context("retry_after_ms", 5000);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.context["vendor"], serde_json::json!("gemini"));
    assert_eq!(back.context["retry_after_ms"], serde_json::json!(5000));
}

#[test]
fn vendor_error_through_runtime_classified_roundtrip() {
    let abp = make_error(ErrorCode::DialectMappingFailed, "unknown tool format")
        .with_context("vendor", "kimi");
    let re: RuntimeError = abp.into();
    let recovered = re.into_abp_error();
    assert_eq!(recovered.code, ErrorCode::DialectMappingFailed);
    assert_eq!(recovered.context["vendor"], serde_json::json!("kimi"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Error context enrichment
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn context_btreemap_ordering() {
    let err = make_error(ErrorCode::Internal, "test")
        .with_context("z_key", "last")
        .with_context("a_key", "first")
        .with_context("m_key", "middle");
    let keys: Vec<_> = err.context.keys().collect();
    assert_eq!(keys, vec!["a_key", "m_key", "z_key"]);
}

#[test]
fn context_with_array_value() {
    let err = make_error(ErrorCode::Internal, "arrays").with_context("items", vec!["a", "b", "c"]);
    assert_eq!(err.context["items"], serde_json::json!(["a", "b", "c"]));
}

#[test]
fn context_with_empty_object() {
    let err =
        make_error(ErrorCode::Internal, "empty obj").with_context("meta", serde_json::json!({}));
    assert_eq!(err.context["meta"], serde_json::json!({}));
}

#[test]
fn context_many_keys() {
    let mut err = make_error(ErrorCode::Internal, "many keys");
    for i in 0..50 {
        err = err.with_context(format!("key_{i:03}"), i);
    }
    assert_eq!(err.context.len(), 50);
}

#[test]
fn context_large_string_value() {
    let big = "x".repeat(10_000);
    let err = make_error(ErrorCode::Internal, "big").with_context("data", big.clone());
    assert_eq!(err.context["data"], serde_json::json!(big));
}

#[test]
fn context_unicode_key_and_value() {
    let err = make_error(ErrorCode::Internal, "unicode").with_context("日本語キー", "値テスト");
    assert_eq!(err.context["日本語キー"], serde_json::json!("値テスト"));
}

#[test]
fn context_numeric_string_vs_number() {
    let err = make_error(ErrorCode::Internal, "types")
        .with_context("str_val", "42")
        .with_context("num_val", 42);
    assert_eq!(err.context["str_val"], serde_json::json!("42"));
    assert_eq!(err.context["num_val"], serde_json::json!(42));
    assert_ne!(err.context["str_val"], err.context["num_val"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Error taxonomy tree traversal
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codes_grouped_by_category() {
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for code in ALL_CODES {
        groups
            .entry(code.category().to_string())
            .or_default()
            .push(code.as_str());
    }
    assert_eq!(groups.len(), 10);
    assert_eq!(groups["protocol"].len(), 3);
    assert_eq!(groups["backend"].len(), 3);
    assert_eq!(groups["capability"].len(), 2);
    assert_eq!(groups["policy"].len(), 2);
    assert_eq!(groups["workspace"].len(), 2);
    assert_eq!(groups["ir"].len(), 2);
    assert_eq!(groups["receipt"].len(), 2);
    assert_eq!(groups["dialect"].len(), 2);
    assert_eq!(groups["config"].len(), 1);
    assert_eq!(groups["internal"].len(), 1);
}

#[test]
fn code_prefix_matches_category() {
    for code in ALL_CODES {
        let s = code.as_str();
        let cat = code.category().to_string().to_uppercase();
        assert!(
            s.starts_with(&cat) || code.category() == ErrorCategory::Ir,
            "code {s} doesn't start with category prefix {cat}"
        );
    }
}

#[test]
fn all_categories_covered_by_all_codes() {
    let cats: HashSet<ErrorCategory> = ALL_CODES.iter().map(|c| c.category()).collect();
    for cat in ALL_CATEGORIES {
        assert!(cats.contains(cat), "category {cat:?} not covered");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Error aggregation and batch error handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn batch_errors_collected() {
    let errors: Vec<AbpError> = ALL_CODES
        .iter()
        .map(|c| make_error(*c, &format!("error for {}", c.as_str())))
        .collect();
    assert_eq!(errors.len(), 20);
}

#[test]
fn batch_errors_by_category() {
    let errors: Vec<AbpError> = ALL_CODES.iter().map(|c| make_error(*c, "err")).collect();
    let backend_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.category() == ErrorCategory::Backend)
        .collect();
    assert_eq!(backend_errors.len(), 3);
}

#[test]
fn batch_errors_to_dtos() {
    let errors: Vec<AbpError> = ALL_CODES.iter().map(|c| make_error(*c, "err")).collect();
    let dtos: Vec<AbpErrorDto> = errors.iter().map(|e| e.into()).collect();
    assert_eq!(dtos.len(), 20);
    let json = serde_json::to_string(&dtos).unwrap();
    let back: Vec<AbpErrorDto> = serde_json::from_str(&json).unwrap();
    assert_eq!(dtos, back);
}

#[test]
fn multiple_error_events_in_receipt_trace() {
    let events = vec![
        make_error_event("rate limited", Some(ErrorCode::BackendTimeout)),
        make_error_event("retry failed", Some(ErrorCode::BackendCrashed)),
        make_error_event("final failure", Some(ErrorCode::Internal)),
    ];
    let receipt = make_receipt_with_events(events, Outcome::Failed);
    let error_count = receipt
        .trace
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        .count();
    assert_eq!(error_count, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Envelope integration with errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_envelope_with_code() {
    let envelope = Envelope::fatal_with_code(
        Some("run-1".into()),
        "authentication failed",
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
fn fatal_envelope_from_abp_error() {
    let abp = make_error(ErrorCode::BackendCrashed, "process exited");
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
fn fatal_envelope_without_error_code() {
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "generic".into(),
        error_code: None,
    };
    assert_eq!(envelope.error_code(), None);
}

#[test]
fn fatal_envelope_json_roundtrip() {
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "crash".into(),
        error_code: Some(ErrorCode::BackendCrashed),
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
        assert_eq!(error, "crash");
        assert_eq!(error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_envelope_json_has_error_code_field() {
    let envelope =
        Envelope::fatal_with_code(Some("run-1".into()), "fatal", ErrorCode::BackendCrashed);
    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["error_code"], "backend_crashed");
    assert_eq!(json["t"], "fatal");
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

// ═══════════════════════════════════════════════════════════════════════════
// 17. AgentEvent error integration
// ═══════════════════════════════════════════════════════════════════════════

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
fn error_event_without_code() {
    let event = make_error_event("something went wrong", None);
    if let AgentEventKind::Error { error_code, .. } = &event.kind {
        assert!(error_code.is_none());
    } else {
        panic!("expected Error kind");
    }
}

#[test]
fn error_event_serializes_type_tag() {
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
fn error_event_serde_roundtrip() {
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
fn error_event_with_ext_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor_error": "something"}),
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

// ═══════════════════════════════════════════════════════════════════════════
// 18. Receipt integration with errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_error_trace_valid_json() {
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
fn error_in_receipt_trace_serializes() {
    let event = make_error_event("boom", Some(ErrorCode::Internal));
    let receipt = make_receipt_with_events(vec![event], Outcome::Failed);
    let json = serde_json::to_value(&receipt).unwrap();
    let trace = json["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0]["type"], "error");
    assert_eq!(trace[0]["error_code"], "INTERNAL");
}

#[test]
fn recovery_receipt_still_has_error_in_trace() {
    let events = vec![
        make_error_event("transient", Some(ErrorCode::BackendTimeout)),
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
    assert!(has_error);
}

#[test]
fn partial_receipt_on_mid_stream_error() {
    let events = vec![
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
    assert_eq!(receipt.trace.len(), 2);
}

#[test]
fn partial_receipt_can_be_hashed() {
    let events = vec![make_error_event("crash", Some(ErrorCode::BackendCrashed))];
    let receipt = make_receipt_with_events(events, Outcome::Partial);
    let hashed = receipt.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn failed_receipt_empty_trace() {
    let receipt = make_receipt_with_events(vec![], Outcome::Failed);
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert!(receipt.trace.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. User-facing vs internal error messages
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn user_facing_display_hides_internal_details() {
    let err = make_error(ErrorCode::Internal, "something broke");
    let display = err.to_string();
    assert!(display.contains("something broke"));
    assert!(!display.contains("stack trace"));
}

#[test]
fn user_facing_display_includes_code_bracket() {
    for code in ALL_CODES {
        let err = make_error(*code, "msg");
        let display = err.to_string();
        assert!(
            display.starts_with(&format!("[{}]", code.as_str())),
            "Display for {code:?} should start with bracketed code"
        );
    }
}

#[test]
fn debug_output_more_detailed_than_display() {
    let src = io::Error::other("deep cause");
    let err = make_error(ErrorCode::Internal, "msg")
        .with_context("key", "val")
        .with_source(src);
    let display = err.to_string();
    let debug = format!("{err:?}");
    assert!(debug.len() > display.len());
    assert!(debug.contains("source"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. ContractError integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_error_json_variant() {
    let serde_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let ce = ContractError::Json(serde_err);
    assert!(ce.to_string().starts_with("failed to serialize JSON:"));
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
fn outcome_serializes_snake_case() {
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

// ═══════════════════════════════════════════════════════════════════════════
// 21. Error code as_str stability snapshot
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

// ═══════════════════════════════════════════════════════════════════════════
// 22. Error construction for every code variant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn construct_abp_error_for_every_code() {
    for code in ALL_CODES {
        let err = make_error(*code, &format!("msg for {}", code.as_str()));
        assert_eq!(err.code, *code);
        assert!(err.message.contains(code.as_str()));
    }
}

#[test]
fn construct_dto_for_every_code() {
    for code in ALL_CODES {
        let err = make_error(*code, "msg");
        let dto: AbpErrorDto = (&err).into();
        assert_eq!(dto.code, *code);
    }
}

#[test]
fn construct_error_event_for_every_code() {
    for code in ALL_CODES {
        let event = make_error_event("msg", Some(*code));
        if let AgentEventKind::Error { error_code, .. } = &event.kind {
            assert_eq!(*error_code, Some(*code));
        } else {
            panic!("expected Error kind");
        }
    }
}

#[test]
fn construct_fatal_envelope_for_every_code() {
    for code in ALL_CODES {
        let envelope = Envelope::fatal_with_code(Some("run-1".into()), "fatal", *code);
        assert_eq!(envelope.error_code(), Some(*code));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. Edge cases and special scenarios
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_message_error() {
    let err = make_error(ErrorCode::Internal, "");
    assert_eq!(err.to_string(), "[INTERNAL] ");
}

#[test]
fn very_long_message_error() {
    let long_msg = "x".repeat(10_000);
    let err = make_error(ErrorCode::Internal, &long_msg);
    assert_eq!(err.message.len(), 10_000);
}

#[test]
fn unicode_message_error() {
    let err = make_error(ErrorCode::Internal, "エラーが発生しました 🚨");
    assert!(err.to_string().contains("エラーが発生しました"));
}

#[test]
fn special_chars_in_message() {
    let err = make_error(ErrorCode::Internal, r#"error: "quotes" and \backslash"#);
    let display = err.to_string();
    assert!(display.contains("quotes"));
}

#[test]
fn newline_in_message() {
    let err = make_error(ErrorCode::Internal, "line1\nline2");
    assert!(err.message.contains('\n'));
}

#[test]
fn error_code_can_be_used_as_hashmap_key() {
    let mut map: HashMap<ErrorCode, String> = HashMap::new();
    for code in ALL_CODES {
        map.insert(*code, code.as_str().to_string());
    }
    assert_eq!(map.len(), 20);
    assert_eq!(map[&ErrorCode::Internal], "INTERNAL");
}

#[test]
fn error_category_can_be_used_as_hashmap_key() {
    let mut map: HashMap<ErrorCategory, usize> = HashMap::new();
    for cat in ALL_CATEGORIES {
        map.insert(*cat, 0);
    }
    assert_eq!(map.len(), 10);
}

#[test]
fn dto_with_empty_context() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "empty ctx".into(),
        context: BTreeMap::new(),
        source_message: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert!(back.context.is_empty());
}

#[test]
fn dto_with_many_context_keys() {
    let mut ctx = BTreeMap::new();
    for i in 0..100 {
        ctx.insert(format!("key_{i}"), serde_json::json!(i));
    }
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "many".into(),
        context: ctx,
        source_message: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.context.len(), 100);
}

#[test]
fn error_display_deterministic_across_calls() {
    let err = make_error(ErrorCode::BackendTimeout, "timeout")
        .with_context("a", 1)
        .with_context("b", 2);
    let s1 = err.to_string();
    let s2 = err.to_string();
    assert_eq!(s1, s2);
}
