// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for error handling across the workspace.
//!
//! Covers RuntimeError, ProtocolError, abp-error taxonomy (AbpError, ErrorCode,
//! ErrorCategory, AbpErrorDto), core ErrorInfo/ErrorCatalog/MappingError,
//! ContractError, conversions, serde roundtrips, source chains, edge cases.

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use abp_core::error::{ErrorCatalog, ErrorCode as CatalogCode, ErrorInfo, MappingError};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_protocol::ProtocolError;
use abp_runtime::RuntimeError;

// ═══════════════════════════════════════════════════════════════════════════
// Section 1 — RuntimeError variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_unknown_backend_display() {
    let err = RuntimeError::UnknownBackend {
        name: "sidecar:node".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("unknown backend"));
    assert!(msg.contains("sidecar:node"));
}

#[test]
fn runtime_workspace_failed_display() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("tmp dir full"));
    assert!(err.to_string().contains("workspace preparation failed"));
}

#[test]
fn runtime_policy_failed_display() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("invalid glob"));
    assert!(err.to_string().contains("policy compilation failed"));
}

#[test]
fn runtime_backend_failed_display() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("OOM"));
    assert!(err.to_string().contains("backend execution failed"));
}

#[test]
fn runtime_capability_check_failed_display() {
    let err = RuntimeError::CapabilityCheckFailed("missing streaming".into());
    let msg = err.to_string();
    assert!(msg.contains("capability check failed"));
    assert!(msg.contains("missing streaming"));
}

#[test]
fn runtime_classified_display() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "30 s exceeded");
    let err = RuntimeError::Classified(abp);
    let msg = err.to_string();
    assert!(msg.contains("BACKEND_TIMEOUT"));
    assert!(msg.contains("30 s exceeded"));
}

#[test]
fn runtime_no_projection_match_display() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no backend scored above threshold".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("projection failed"));
    assert!(msg.contains("threshold"));
}

// ── RuntimeError Debug ────────────────────────────────────────────────────

#[test]
fn runtime_unknown_backend_debug() {
    let err = RuntimeError::UnknownBackend {
        name: "test-be".into(),
    };
    let dbg = format!("{err:?}");
    assert!(dbg.contains("UnknownBackend"));
    assert!(dbg.contains("test-be"));
}

#[test]
fn runtime_workspace_failed_debug() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    let dbg = format!("{err:?}");
    assert!(dbg.contains("WorkspaceFailed"));
}

#[test]
fn runtime_backend_failed_debug() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    let dbg = format!("{err:?}");
    assert!(dbg.contains("BackendFailed"));
}

#[test]
fn runtime_classified_debug() {
    let abp = AbpError::new(ErrorCode::Internal, "oops");
    let err = RuntimeError::Classified(abp);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("Classified"));
}

#[test]
fn runtime_no_projection_debug() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    let dbg = format!("{err:?}");
    assert!(dbg.contains("NoProjectionMatch"));
}

// ── RuntimeError source chains ────────────────────────────────────────────

#[test]
fn runtime_unknown_backend_no_source() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(err.source().is_none());
}

#[test]
fn runtime_workspace_failed_has_source() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("inner ws"));
    assert!(err.source().is_some());
}

#[test]
fn runtime_policy_failed_has_source() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("inner pol"));
    assert!(err.source().is_some());
}

#[test]
fn runtime_backend_failed_has_source() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("inner be"));
    assert!(err.source().is_some());
}

#[test]
fn runtime_capability_check_no_source() {
    let err = RuntimeError::CapabilityCheckFailed("cap".into());
    assert!(err.source().is_none());
}

#[test]
fn runtime_no_projection_no_source() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "n".into(),
    };
    assert!(err.source().is_none());
}

// ── RuntimeError error_code() ─────────────────────────────────────────────

#[test]
fn runtime_error_code_unknown_backend() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_code_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("w"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_code_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("p"));
    assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_code_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("b"));
    assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_code_capability() {
    let err = RuntimeError::CapabilityCheckFailed("c".into());
    assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
}

#[test]
fn runtime_error_code_classified() {
    let abp = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let err = RuntimeError::Classified(abp);
    assert_eq!(err.error_code(), ErrorCode::PolicyDenied);
}

#[test]
fn runtime_error_code_no_projection() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "x".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

// ── RuntimeError into_abp_error ───────────────────────────────────────────

#[test]
fn runtime_into_abp_error_preserves_code() {
    let err = RuntimeError::UnknownBackend {
        name: "sidecar:py".into(),
    };
    let abp = err.into_abp_error();
    assert_eq!(abp.code, ErrorCode::BackendNotFound);
    assert!(abp.message.contains("unknown backend"));
}

#[test]
fn runtime_classified_into_abp_error_roundtrip() {
    let original = AbpError::new(ErrorCode::ConfigInvalid, "bad config");
    let err = RuntimeError::Classified(original);
    let recovered = err.into_abp_error();
    assert_eq!(recovered.code, ErrorCode::ConfigInvalid);
    assert_eq!(recovered.message, "bad config");
}

// ── RuntimeError pattern matching ─────────────────────────────────────────

#[test]
fn runtime_pattern_match_all_variants() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "a".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("b")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("c")),
        RuntimeError::BackendFailed(anyhow::anyhow!("d")),
        RuntimeError::CapabilityCheckFailed("e".into()),
        RuntimeError::Classified(AbpError::new(ErrorCode::Internal, "f")),
        RuntimeError::NoProjectionMatch {
            reason: "g".into(),
        },
    ];
    let mut count = 0;
    for v in &variants {
        match v {
            RuntimeError::UnknownBackend { .. } => count += 1,
            RuntimeError::WorkspaceFailed(_) => count += 1,
            RuntimeError::PolicyFailed(_) => count += 1,
            RuntimeError::BackendFailed(_) => count += 1,
            RuntimeError::CapabilityCheckFailed(_) => count += 1,
            RuntimeError::Classified(_) => count += 1,
            RuntimeError::NoProjectionMatch { .. } => count += 1,
        }
    }
    assert_eq!(count, 7);
}

// ── RuntimeError downcast & trait objects ──────────────────────────────────

#[test]
fn runtime_error_downcast_from_dyn() {
    let err: Box<dyn Error + Send + Sync> = Box::new(RuntimeError::UnknownBackend {
        name: "test".into(),
    });
    assert!(err.downcast_ref::<RuntimeError>().is_some());
}

#[test]
fn runtime_error_in_result() {
    fn might_fail(name: &str) -> Result<(), RuntimeError> {
        Err(RuntimeError::UnknownBackend {
            name: name.into(),
        })
    }
    let res = might_fail("oops");
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { name } if name == "oops"));
}

#[test]
fn runtime_error_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<RuntimeError>();
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 2 — ProtocolError variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_json_error_display() {
    let e = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
    let err = ProtocolError::Json(e);
    assert!(err.to_string().contains("invalid JSON"));
}

#[test]
fn protocol_io_error_display() {
    let err = ProtocolError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke"));
    let msg = err.to_string();
    assert!(msg.contains("I/O error"));
    assert!(msg.contains("pipe broke"));
}

#[test]
fn protocol_violation_display() {
    let err = ProtocolError::Violation("hello must come first".into());
    let msg = err.to_string();
    assert!(msg.contains("protocol violation"));
    assert!(msg.contains("hello must come first"));
}

#[test]
fn protocol_unexpected_message_display() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("unexpected message"));
    assert!(msg.contains("hello"));
    assert!(msg.contains("run"));
}

#[test]
fn protocol_abp_variant_display() {
    let abp = AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "bad envelope");
    let err = ProtocolError::Abp(abp);
    let msg = err.to_string();
    assert!(msg.contains("PROTOCOL_INVALID_ENVELOPE"));
    assert!(msg.contains("bad envelope"));
}

// ── ProtocolError source chains ───────────────────────────────────────────

#[test]
fn protocol_json_has_source() {
    let je = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let err = ProtocolError::Json(je);
    assert!(err.source().is_some());
}

#[test]
fn protocol_io_has_source() {
    let err = ProtocolError::Io(io::Error::other("x"));
    assert!(err.source().is_some());
}

#[test]
fn protocol_violation_no_source() {
    let err = ProtocolError::Violation("v".into());
    assert!(err.source().is_none());
}

#[test]
fn protocol_unexpected_no_source() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "a".into(),
        got: "b".into(),
    };
    assert!(err.source().is_none());
}

// ── ProtocolError error_code() ────────────────────────────────────────────

#[test]
fn protocol_violation_error_code() {
    let err = ProtocolError::Violation("x".into());
    assert_eq!(
        err.error_code(),
        Some(ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_unexpected_error_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn protocol_abp_variant_error_code() {
    let abp = AbpError::new(ErrorCode::BackendCrashed, "crash");
    let err = ProtocolError::Abp(abp);
    assert_eq!(err.error_code(), Some(ErrorCode::BackendCrashed));
}

#[test]
fn protocol_json_no_error_code() {
    let je = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let err = ProtocolError::Json(je);
    assert!(err.error_code().is_none());
}

#[test]
fn protocol_io_no_error_code() {
    let err = ProtocolError::Io(io::Error::other("x"));
    assert!(err.error_code().is_none());
}

// ── ProtocolError From impls ──────────────────────────────────────────────

#[test]
fn protocol_from_serde_json_error() {
    let je = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let pe: ProtocolError = je.into();
    assert!(matches!(pe, ProtocolError::Json(_)));
}

#[test]
fn protocol_from_io_error() {
    let ie = io::Error::other("io fail");
    let pe: ProtocolError = ie.into();
    assert!(matches!(pe, ProtocolError::Io(_)));
}

#[test]
fn protocol_from_abp_error() {
    let abp = AbpError::new(ErrorCode::Internal, "oops");
    let pe: ProtocolError = abp.into();
    assert!(matches!(pe, ProtocolError::Abp(_)));
}

#[test]
fn protocol_error_downcast() {
    let err: Box<dyn Error + Send + Sync> =
        Box::new(ProtocolError::Violation("test".into()));
    assert!(err.downcast_ref::<ProtocolError>().is_some());
}

#[test]
fn protocol_error_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<ProtocolError>();
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 3 — AbpError (abp-error crate)
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
fn abp_error_display_without_context() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no backend");
    assert_eq!(err.to_string(), "[BACKEND_NOT_FOUND] no backend");
}

#[test]
fn abp_error_display_with_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("timeout_ms", 5000);
    let s = err.to_string();
    assert!(s.starts_with("[BACKEND_TIMEOUT] timed out"));
    assert!(s.contains("timeout_ms"));
}

#[test]
fn abp_error_debug_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "nope");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("PolicyDenied"));
    assert!(dbg.contains("nope"));
}

#[test]
fn abp_error_debug_includes_source() {
    let src = io::Error::new(io::ErrorKind::NotFound, "missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("missing"));
}

#[test]
fn abp_error_with_context_chaining() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    assert_eq!(err.context.len(), 3);
}

#[test]
fn abp_error_with_source_preserves() {
    let src = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
    let err = AbpError::new(ErrorCode::PolicyDenied, "no").with_source(src);
    let s = Error::source(&err).unwrap();
    assert_eq!(s.to_string(), "denied");
}

#[test]
fn abp_error_source_none_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "x");
    assert!(Error::source(&err).is_none());
}

#[test]
fn abp_error_category_shorthand() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unknown");
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

// ── AbpError serde roundtrip via DTO ──────────────────────────────────────

#[test]
fn abp_error_dto_roundtrip_no_source() {
    let err = AbpError::new(ErrorCode::IrInvalid, "bad IR")
        .with_context("node", "call_tool");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
    assert!(back.source_message.is_none());
}

#[test]
fn abp_error_dto_roundtrip_with_source() {
    let src = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn abp_error_dto_to_abp_error_loses_source() {
    let dto = AbpErrorDto {
        code: ErrorCode::ConfigInvalid,
        message: "bad".into(),
        context: BTreeMap::new(),
        source_message: Some("inner".into()),
    };
    let err: AbpError = dto.into();
    assert_eq!(err.code, ErrorCode::ConfigInvalid);
    assert!(err.source.is_none());
}

// ── ErrorCode serde ───────────────────────────────────────────────────────

#[test]
fn error_code_serde_roundtrip() {
    let code = ErrorCode::ProtocolInvalidEnvelope;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""PROTOCOL_INVALID_ENVELOPE""#);
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn error_code_display_matches_as_str() {
    let code = ErrorCode::BackendTimeout;
    assert_eq!(code.to_string(), code.as_str());
}

// ── ErrorCategory serde ───────────────────────────────────────────────────

#[test]
fn error_category_serde_roundtrip() {
    let cat = ErrorCategory::Backend;
    let json = serde_json::to_string(&cat).unwrap();
    assert_eq!(json, r#""backend""#);
    let back: ErrorCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cat);
}

#[test]
fn error_category_display() {
    assert_eq!(ErrorCategory::Protocol.to_string(), "protocol");
    assert_eq!(ErrorCategory::Workspace.to_string(), "workspace");
    assert_eq!(ErrorCategory::Internal.to_string(), "internal");
}

// ── ErrorCode → ErrorCategory mapping ─────────────────────────────────────

#[test]
fn error_code_categories_correct() {
    assert_eq!(ErrorCode::ProtocolInvalidEnvelope.category(), ErrorCategory::Protocol);
    assert_eq!(ErrorCode::BackendNotFound.category(), ErrorCategory::Backend);
    assert_eq!(ErrorCode::CapabilityUnsupported.category(), ErrorCategory::Capability);
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::WorkspaceInitFailed.category(), ErrorCategory::Workspace);
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
    assert_eq!(ErrorCode::ReceiptHashMismatch.category(), ErrorCategory::Receipt);
    assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 4 — Core error catalog (ErrorInfo, ErrorCatalog, ErrorCode)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_info_basic_construction() {
    let info = ErrorInfo::new(CatalogCode::IoError, "disk full");
    assert_eq!(info.code, CatalogCode::IoError);
    assert_eq!(info.message, "disk full");
    assert!(info.context.is_empty());
    assert!(info.source.is_none());
}

#[test]
fn error_info_display_with_context() {
    let info = ErrorInfo::new(CatalogCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("ms", "5000");
    let s = info.to_string();
    assert!(s.contains("ABP-R002"));
    assert!(s.contains("slow"));
    assert!(s.contains("backend=openai"));
    assert!(s.contains("ms=5000"));
}

#[test]
fn error_info_display_without_context() {
    let info = ErrorInfo::new(CatalogCode::RunCancelled, "cancelled");
    let s = info.to_string();
    assert!(s.contains("ABP-R005"));
    assert!(s.contains("cancelled"));
    assert!(!s.contains('('));
}

#[test]
fn error_info_debug() {
    let info = ErrorInfo::new(CatalogCode::IoError, "io")
        .with_source(io::Error::other("underlying"));
    let dbg = format!("{info:?}");
    assert!(dbg.contains("ErrorInfo"));
    assert!(dbg.contains("underlying"));
}

#[test]
fn error_info_source_chain() {
    let src = io::Error::new(io::ErrorKind::NotFound, "missing file");
    let info = ErrorInfo::new(CatalogCode::IoError, "io").with_source(src);
    let s = Error::source(&info).unwrap();
    assert_eq!(s.to_string(), "missing file");
}

#[test]
fn error_info_no_source_by_default() {
    let info = ErrorInfo::new(CatalogCode::InternalError, "oops");
    assert!(Error::source(&info).is_none());
}

#[test]
fn error_catalog_lookup_contract() {
    assert_eq!(
        ErrorCatalog::lookup("ABP-C001"),
        Some(CatalogCode::InvalidContractVersion)
    );
}

#[test]
fn error_catalog_lookup_protocol() {
    assert_eq!(
        ErrorCatalog::lookup("ABP-P003"),
        Some(CatalogCode::UnexpectedMessage)
    );
}

#[test]
fn error_catalog_lookup_policy() {
    assert_eq!(
        ErrorCatalog::lookup("ABP-L001"),
        Some(CatalogCode::ToolDenied)
    );
}

#[test]
fn error_catalog_lookup_runtime() {
    assert_eq!(
        ErrorCatalog::lookup("ABP-R001"),
        Some(CatalogCode::BackendUnavailable)
    );
}

#[test]
fn error_catalog_lookup_system() {
    assert_eq!(
        ErrorCatalog::lookup("ABP-S001"),
        Some(CatalogCode::IoError)
    );
}

#[test]
fn error_catalog_lookup_missing() {
    assert!(ErrorCatalog::lookup("ABP-X999").is_none());
}

#[test]
fn error_catalog_all_count() {
    let all = ErrorCatalog::all();
    assert!(all.len() >= 50, "should have many codes, got {}", all.len());
}

#[test]
fn error_catalog_by_category_contract() {
    let codes = ErrorCatalog::by_category("contract");
    assert!(codes.len() >= 10);
    for c in &codes {
        assert_eq!(c.category(), "contract");
    }
}

#[test]
fn error_catalog_by_category_system() {
    let codes = ErrorCatalog::by_category("system");
    assert!(codes.len() >= 8);
    for c in &codes {
        assert_eq!(c.category(), "system");
    }
}

#[test]
fn catalog_code_description_non_empty() {
    let all = ErrorCatalog::all();
    for code in &all {
        assert!(!code.description().is_empty(), "empty description for {code}");
    }
}

#[test]
fn catalog_code_display_is_code_string() {
    let code = CatalogCode::InvalidContractVersion;
    assert_eq!(code.to_string(), "ABP-C001");
}

#[test]
fn catalog_code_implements_std_error() {
    let code = CatalogCode::IoError;
    let _: &dyn Error = &code;
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 5 — ContractError
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_error_from_serde() {
    use abp_core::ContractError;
    let je = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
    let ce: ContractError = je.into();
    assert!(ce.to_string().contains("serialize JSON"));
}

#[test]
fn contract_error_display() {
    use abp_core::ContractError;
    let je = serde_json::from_str::<serde_json::Value>("???").unwrap_err();
    let ce = ContractError::Json(je);
    let msg = ce.to_string();
    assert!(msg.contains("failed to serialize JSON"));
}

#[test]
fn contract_error_source_chain() {
    use abp_core::ContractError;
    let je = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    let ce = ContractError::Json(je);
    assert!(Error::source(&ce).is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 6 — Error conversion chains
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_from_abp_error_via_classified() {
    let abp = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let rt: RuntimeError = abp.into();
    assert!(matches!(rt, RuntimeError::Classified(_)));
}

#[test]
fn runtime_error_chain_anyhow_context() {
    let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "cannot write");
    let anyhow_err = anyhow::Error::new(io_err).context("staging workspace");
    let rt = RuntimeError::WorkspaceFailed(anyhow_err);
    let src = rt.source().expect("should have source");
    let chain = src.to_string();
    assert!(chain.contains("staging workspace"));
}

#[test]
fn protocol_error_to_dyn_error() {
    let err: Box<dyn Error> =
        Box::new(ProtocolError::Violation("bad".into()));
    assert!(!err.to_string().is_empty());
}

#[test]
fn abp_error_into_protocol_error() {
    let abp = AbpError::new(ErrorCode::ProtocolVersionMismatch, "v mismatch");
    let pe = ProtocolError::from(abp);
    assert!(matches!(pe, ProtocolError::Abp(_)));
    assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolVersionMismatch));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 7 — Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_unknown_backend_empty_name() {
    let err = RuntimeError::UnknownBackend {
        name: String::new(),
    };
    let msg = err.to_string();
    assert!(msg.contains("unknown backend"));
}

#[test]
fn protocol_violation_empty_message() {
    let err = ProtocolError::Violation(String::new());
    let msg = err.to_string();
    assert!(msg.contains("protocol violation"));
}

#[test]
fn abp_error_empty_message() {
    let err = AbpError::new(ErrorCode::Internal, "");
    let s = err.to_string();
    assert!(s.contains("[INTERNAL]"));
}

#[test]
fn abp_error_unicode_message() {
    let err = AbpError::new(ErrorCode::Internal, "エラー発生 🔥");
    let s = err.to_string();
    assert!(s.contains("エラー発生"));
    assert!(s.contains("🔥"));
}

#[test]
fn runtime_unknown_backend_unicode_name() {
    let err = RuntimeError::UnknownBackend {
        name: "バックエンド".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("バックエンド"));
}

#[test]
fn protocol_unexpected_message_unicode() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "こんにちは".into(),
        got: "走る".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("こんにちは"));
    assert!(msg.contains("走る"));
}

#[test]
fn runtime_unknown_backend_very_long_name() {
    let long_name = "x".repeat(10_000);
    let err = RuntimeError::UnknownBackend {
        name: long_name.clone(),
    };
    let msg = err.to_string();
    assert!(msg.contains(&long_name));
}

#[test]
fn abp_error_very_long_message() {
    let long_msg = "error ".repeat(2_000);
    let err = AbpError::new(ErrorCode::Internal, &long_msg);
    assert_eq!(err.message, long_msg);
}

#[test]
fn protocol_violation_very_long_message() {
    let long = "v".repeat(10_000);
    let err = ProtocolError::Violation(long.clone());
    assert!(err.to_string().contains(&long));
}

#[test]
fn error_info_empty_message() {
    let info = ErrorInfo::new(CatalogCode::InternalError, "");
    let s = info.to_string();
    assert!(s.contains("ABP-S003"));
}

#[test]
fn error_info_unicode_context() {
    let info = ErrorInfo::new(CatalogCode::IoError, "io")
        .with_context("パス", "/tmp/テスト");
    let s = info.to_string();
    assert!(s.contains("パス"));
    assert!(s.contains("/tmp/テスト"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 8 — MappingError serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_serde_roundtrip_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        field: "max_tokens".into(),
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        detail: "range differs".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn mapping_error_serde_roundtrip_streaming() {
    let err = MappingError::StreamingUnsupported {
        dialect: "batch-only".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn mapping_error_serde_roundtrip_incompatible_model_with_suggestion() {
    let err = MappingError::IncompatibleModel {
        requested: "claude-opus".into(),
        dialect: "openai".into(),
        suggestion: Some("gpt-4o".into()),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn mapping_error_serde_roundtrip_incompatible_model_no_suggestion() {
    let err = MappingError::IncompatibleModel {
        requested: "claude-4".into(),
        dialect: "gemini".into(),
        suggestion: None,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 9 — Cross-type downcast and boxing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_error_types_box_dyn_error() {
    let errors: Vec<Box<dyn Error + Send + Sync>> = vec![
        Box::new(RuntimeError::UnknownBackend { name: "a".into() }),
        Box::new(ProtocolError::Violation("b".into())),
        Box::new(AbpError::new(ErrorCode::Internal, "c")),
    ];
    for e in &errors {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn downcast_abp_error_from_protocol() {
    let abp = AbpError::new(ErrorCode::BackendTimeout, "slow");
    let pe = ProtocolError::Abp(abp);
    if let ProtocolError::Abp(inner) = pe {
        assert_eq!(inner.code, ErrorCode::BackendTimeout);
    } else {
        panic!("expected Abp variant");
    }
}

#[test]
fn downcast_runtime_classified_inner() {
    let abp = AbpError::new(ErrorCode::PolicyInvalid, "bad policy");
    let rt = RuntimeError::Classified(abp);
    if let RuntimeError::Classified(inner) = rt {
        assert_eq!(inner.code, ErrorCode::PolicyInvalid);
        assert_eq!(inner.message, "bad policy");
    } else {
        panic!("expected Classified variant");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 10 — Error code metadata stability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_code_as_str_stability() {
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "BACKEND_NOT_FOUND");
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "POLICY_DENIED");
    assert_eq!(ErrorCode::WorkspaceStagingFailed.as_str(), "WORKSPACE_STAGING_FAILED");
    assert_eq!(ErrorCode::Internal.as_str(), "INTERNAL");
}

#[test]
fn catalog_code_string_stability() {
    assert_eq!(CatalogCode::InvalidContractVersion.code(), "ABP-C001");
    assert_eq!(CatalogCode::HandshakeFailed.code(), "ABP-P002");
    assert_eq!(CatalogCode::ToolDenied.code(), "ABP-L001");
    assert_eq!(CatalogCode::BackendUnavailable.code(), "ABP-R001");
    assert_eq!(CatalogCode::IoError.code(), "ABP-S001");
}

#[test]
fn all_abp_error_codes_unique_as_str() {
    use std::collections::HashSet;

    let all: &[ErrorCode] = &[
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
    let mut seen = HashSet::new();
    for code in all {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str: {s}");
    }
}

#[test]
fn all_catalog_codes_unique_code_string() {
    use std::collections::HashSet;
    let all = ErrorCatalog::all();
    let mut seen = HashSet::new();
    for code in &all {
        let s = code.code();
        assert!(seen.insert(s), "duplicate code string: {s}");
    }
}
