#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive error-path coverage tests for every error type in the
//! Agent Backplane codebase.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::io;

// ── abp-error (unified taxonomy) ──────────────────────────────────────
use abp_error::{
    abp_err, AbpError, AbpErrorDto, ErrorCategory, ErrorChain, ErrorCode, ErrorInfo, ErrorLocation,
    ErrorStats,
};

// ── abp-error-taxonomy ────────────────────────────────────────────────
use abp_error_taxonomy::classification::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
    RecoverySuggestion,
};
use abp_error_taxonomy::context::{EnrichError, ErrorContextBuilder};
use abp_error_taxonomy::docs::error_code_doc;
use abp_error_taxonomy::mapping::{VendorError, VendorErrorMapper, VendorKind};
use abp_error_taxonomy::recovery::{RecoveryPlan, RecoveryStep, RetryPolicy};

// ── abp-core (ErrorCode / MappingError / ErrorCatalog) ────────────────
use abp_core::error::{
    ErrorCatalog, ErrorCode as CoreErrorCode, ErrorInfo as CoreErrorInfo,
    MappingError as CoreMappingError, MappingErrorKind, MappingResult,
};

// ── abp-protocol ─────────────────────────────────────────────────────
use abp_protocol::ProtocolError;

// ── abp-projection ────────────────────────────────────────────────────
use abp_projection::ProjectionError;

// ── abp-runtime ──────────────────────────────────────────────────────
use abp_runtime::RuntimeError;

// ── abp-config ───────────────────────────────────────────────────────
use abp_config::ConfigError;

// ── abp-validate ─────────────────────────────────────────────────────
use abp_validate::{ValidationError, ValidationErrorKind, ValidationErrors};

// ═══════════════════════════════════════════════════════════════════════
// SECTION 1 — abp-error ErrorCode (36 variants)
// ═══════════════════════════════════════════════════════════════════════

const ALL_ABP_ERROR_CODES: &[ErrorCode] = &[
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

// -- 1.1 Every code has a non-empty as_str --
#[test]
fn error_code_as_str_non_empty() {
    for code in ALL_ABP_ERROR_CODES {
        assert!(!code.as_str().is_empty(), "{:?} has empty as_str", code);
    }
}

// -- 1.2 Every code has a non-empty message --
#[test]
fn error_code_message_non_empty() {
    for code in ALL_ABP_ERROR_CODES {
        assert!(!code.message().is_empty(), "{:?} has empty message", code);
    }
}

// -- 1.3 Display produces the message text --
#[test]
fn error_code_display_matches_message() {
    for code in ALL_ABP_ERROR_CODES {
        assert_eq!(code.to_string(), code.message());
    }
}

// -- 1.4 Category assignment coverage --
#[test]
fn error_code_protocol_category() {
    let protos = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in &protos {
        assert_eq!(code.category(), ErrorCategory::Protocol, "{:?}", code);
    }
}

#[test]
fn error_code_mapping_category() {
    let mappings = [
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
    ];
    for code in &mappings {
        assert_eq!(code.category(), ErrorCategory::Mapping, "{:?}", code);
    }
}

#[test]
fn error_code_backend_category() {
    let backends = [
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
    ];
    for code in &backends {
        assert_eq!(code.category(), ErrorCategory::Backend, "{:?}", code);
    }
}

#[test]
fn error_code_execution_category() {
    let execs = [
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
    ];
    for code in &execs {
        assert_eq!(code.category(), ErrorCategory::Execution, "{:?}", code);
    }
}

#[test]
fn error_code_contract_category() {
    let contracts = [
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ];
    for code in &contracts {
        assert_eq!(code.category(), ErrorCategory::Contract, "{:?}", code);
    }
}

#[test]
fn error_code_capability_category() {
    let caps = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in &caps {
        assert_eq!(code.category(), ErrorCategory::Capability, "{:?}", code);
    }
}

#[test]
fn error_code_policy_category() {
    let pols = [ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid];
    for code in &pols {
        assert_eq!(code.category(), ErrorCategory::Policy, "{:?}", code);
    }
}

#[test]
fn error_code_workspace_category() {
    let ws = [
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
    ];
    for code in &ws {
        assert_eq!(code.category(), ErrorCategory::Workspace, "{:?}", code);
    }
}

#[test]
fn error_code_ir_category() {
    let ir = [ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid];
    for code in &ir {
        assert_eq!(code.category(), ErrorCategory::Ir, "{:?}", code);
    }
}

#[test]
fn error_code_receipt_category() {
    let receipts = [
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ];
    for code in &receipts {
        assert_eq!(code.category(), ErrorCategory::Receipt, "{:?}", code);
    }
}

#[test]
fn error_code_dialect_category() {
    let dialects = [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed];
    for code in &dialects {
        assert_eq!(code.category(), ErrorCategory::Dialect, "{:?}", code);
    }
}

#[test]
fn error_code_config_category() {
    assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
}

#[test]
fn error_code_internal_category() {
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

// -- 1.5 Retryability --
#[test]
fn retryable_codes_are_correct() {
    let retryable = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ];
    for code in &retryable {
        assert!(code.is_retryable(), "{:?} should be retryable", code);
    }
}

#[test]
fn non_retryable_codes_are_correct() {
    let non_retryable = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendAuthFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::MappingDialectMismatch,
    ];
    for code in &non_retryable {
        assert!(!code.is_retryable(), "{:?} should not be retryable", code);
    }
}

// -- 1.6 ErrorCategory Display --
#[test]
fn error_category_display() {
    assert_eq!(ErrorCategory::Protocol.to_string(), "protocol");
    assert_eq!(ErrorCategory::Backend.to_string(), "backend");
    assert_eq!(ErrorCategory::Capability.to_string(), "capability");
    assert_eq!(ErrorCategory::Policy.to_string(), "policy");
    assert_eq!(ErrorCategory::Workspace.to_string(), "workspace");
    assert_eq!(ErrorCategory::Ir.to_string(), "ir");
    assert_eq!(ErrorCategory::Receipt.to_string(), "receipt");
    assert_eq!(ErrorCategory::Dialect.to_string(), "dialect");
    assert_eq!(ErrorCategory::Config.to_string(), "config");
    assert_eq!(ErrorCategory::Mapping.to_string(), "mapping");
    assert_eq!(ErrorCategory::Execution.to_string(), "execution");
    assert_eq!(ErrorCategory::Contract.to_string(), "contract");
    assert_eq!(ErrorCategory::Internal.to_string(), "internal");
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 2 — AbpError construction, builder, Display, Debug
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_basic_construction() {
    let err = AbpError::new(ErrorCode::Internal, "boom");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(err.message, "boom");
    assert!(err.source.is_none());
    assert!(err.context.is_empty());
    assert!(err.location.is_none());
}

#[test]
fn abp_error_with_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    assert_eq!(err.context.len(), 2);
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_with_source() {
    let src = io::Error::new(io::ErrorKind::NotFound, "missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(src);
    assert!(err.source.is_some());
    assert!(err.source().is_some());
}

#[test]
fn abp_error_with_location() {
    let loc = ErrorLocation::new("src/main.rs", 42, 5);
    let err = AbpError::new(ErrorCode::Internal, "oops").with_location(loc);
    let location = err.location.as_ref().unwrap();
    assert_eq!(location.file, "src/main.rs");
    assert_eq!(location.line, 42);
    assert_eq!(location.column, 5);
}

#[test]
fn abp_error_display_no_context() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no backend");
    assert_eq!(err.to_string(), "[backend_not_found] no backend");
}

#[test]
fn abp_error_display_with_context() {
    let err =
        AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("backend", "openai");
    let s = err.to_string();
    assert!(s.starts_with("[backend_timeout] timed out"));
    assert!(s.contains("openai"));
}

#[test]
fn abp_error_debug_impl() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let d = format!("{:?}", err);
    assert!(d.contains("PolicyDenied"));
    assert!(d.contains("denied"));
}

#[test]
fn abp_error_debug_with_source() {
    let src = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let err = AbpError::new(ErrorCode::ExecutionPermissionDenied, "perm").with_source(src);
    let d = format!("{:?}", err);
    assert!(d.contains("source"));
    assert!(d.contains("access denied"));
}

#[test]
fn abp_error_category_shorthand() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "t");
    assert_eq!(err.category(), ErrorCategory::Backend);
}

#[test]
fn abp_error_is_retryable_shorthand() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "t");
    assert!(err.is_retryable());
    let err2 = AbpError::new(ErrorCode::PolicyDenied, "d");
    assert!(!err2.is_retryable());
}

#[test]
fn abp_error_matches_code() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash");
    assert!(err.matches_code(ErrorCode::BackendCrashed));
    assert!(!err.matches_code(ErrorCode::Internal));
}

#[test]
fn abp_error_matches_category() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert!(err.matches_category(ErrorCategory::Policy));
    assert!(!err.matches_category(ErrorCategory::Backend));
}

#[test]
fn abp_error_has_context_key() {
    let err = AbpError::new(ErrorCode::Internal, "x").with_context("foo", "bar");
    assert!(err.has_context_key("foo"));
    assert!(!err.has_context_key("baz"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 3 — ErrorChain traversal
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_chain_no_source() {
    let err = AbpError::new(ErrorCode::Internal, "leaf");
    assert_eq!(err.chain_depth(), 0);
    assert_eq!(err.error_chain().count(), 0);
}

#[test]
fn error_chain_single_source() {
    let inner = io::Error::new(io::ErrorKind::Other, "inner");
    let err = AbpError::new(ErrorCode::Internal, "outer").with_source(inner);
    assert_eq!(err.chain_depth(), 1);
    let mut chain = err.error_chain();
    let first = chain.next().unwrap();
    assert!(first.to_string().contains("inner"));
    assert!(chain.next().is_none());
}

#[test]
fn error_chain_nested_source() {
    let level2 = io::Error::new(io::ErrorKind::Other, "level2");
    let level1 = AbpError::new(ErrorCode::Internal, "level1").with_source(level2);
    let root = AbpError::new(ErrorCode::BackendCrashed, "root").with_source(level1);
    assert_eq!(root.chain_depth(), 2);
}

#[test]
fn display_chain_format() {
    let inner = io::Error::new(io::ErrorKind::Other, "disk full");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init").with_source(inner);
    let chain_str = err.display_chain();
    assert!(chain_str.contains("[workspace_init_failed] init"));
    assert!(chain_str.contains("caused by 0: disk full"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 4 — From conversions on AbpError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_from_io_error() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "missing");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert!(abp.source.is_some());
}

#[test]
fn abp_error_from_serde_json_error() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{{bad}}").unwrap_err();
    let abp: AbpError = serde_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
}

#[test]
fn abp_error_from_string() {
    let abp: AbpError = "something went wrong".to_string().into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert_eq!(abp.message, "something went wrong");
}

#[test]
fn abp_error_from_str() {
    let abp: AbpError = "bad".into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert_eq!(abp.message, "bad");
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 5 — ErrorInfo (abp-error)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_info_construction() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert!(info.is_retryable);
}

#[test]
fn error_info_with_detail() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "t").with_detail("backend", "openai");
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
}

#[test]
fn error_info_display() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
    let s = info.to_string();
    assert!(s.contains("policy_denied"));
    assert!(s.contains("denied"));
}

#[test]
fn abp_error_to_info() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow").with_context("x", "y");
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "slow");
    assert!(info.is_retryable);
    assert!(info.details.contains_key("x"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 6 — ErrorLocation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_location_display() {
    let loc = ErrorLocation::new("src/lib.rs", 10, 3);
    assert_eq!(loc.to_string(), "src/lib.rs:10:3");
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 7 — AbpErrorDto serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_dto_from_error() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::Internal);
    assert_eq!(dto.message, "oops");
    assert!(dto.source_message.is_none());
    assert!(dto.cause_chain.is_empty());
}

#[test]
fn abp_error_dto_with_source_message() {
    let inner = io::Error::new(io::ErrorKind::Other, "disk");
    let err = AbpError::new(ErrorCode::Internal, "fail").with_source(inner);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message, Some("disk".into()));
    assert_eq!(dto.cause_chain.len(), 1);
}

#[test]
fn abp_error_dto_roundtrip() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timeout")
        .with_context("ms", 1000)
        .with_location(ErrorLocation::new("test.rs", 1, 1));
    let dto: AbpErrorDto = (&err).into();
    let back: AbpError = dto.into();
    assert_eq!(back.code, ErrorCode::BackendTimeout);
    assert_eq!(back.message, "timeout");
    assert_eq!(back.context["ms"], serde_json::json!(1000));
}

#[test]
fn abp_error_to_json() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let json = err.to_json().unwrap();
    assert!(json.contains("internal"));
    assert!(json.contains("oops"));
}

#[test]
fn abp_error_to_json_pretty() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let json = err.to_json_pretty().unwrap();
    assert!(json.contains('\n'));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 8 — abp_err! macro
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn abp_err_macro_captures_location() {
    let err = abp_err!(ErrorCode::Internal, "macro test");
    assert!(err.location.is_some());
    let loc = err.location.as_ref().unwrap();
    assert!(loc.file.contains("error_path_exhaustive"));
    assert!(loc.line > 0);
}

#[test]
fn abp_err_macro_with_context() {
    let err = abp_err!(ErrorCode::BackendTimeout, "timeout", "backend" => "openai", "ms" => 5000);
    assert!(err.location.is_some());
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
    assert_eq!(err.context["ms"], serde_json::json!(5000));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 9 — ErrorStats
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_stats_empty() {
    let stats = ErrorStats::new();
    assert_eq!(stats.total(), 0);
    assert_eq!(stats.count_by_code(ErrorCode::Internal), 0);
}

#[test]
fn error_stats_record_and_count() {
    let mut stats = ErrorStats::new();
    stats.record(&AbpError::new(ErrorCode::BackendTimeout, "t1"));
    stats.record(&AbpError::new(ErrorCode::BackendTimeout, "t2"));
    stats.record(&AbpError::new(ErrorCode::PolicyDenied, "d1"));
    assert_eq!(stats.total(), 3);
    assert_eq!(stats.count_by_code(ErrorCode::BackendTimeout), 2);
    assert_eq!(stats.count_by_code(ErrorCode::PolicyDenied), 1);
    assert_eq!(stats.count_by_category(ErrorCategory::Backend), 2);
    assert_eq!(stats.count_by_category(ErrorCategory::Policy), 1);
}

#[test]
fn error_stats_record_code() {
    let mut stats = ErrorStats::new();
    stats.record_code(ErrorCode::Internal);
    assert_eq!(stats.total(), 1);
    assert_eq!(stats.count_by_code(ErrorCode::Internal), 1);
}

#[test]
fn error_stats_reset() {
    let mut stats = ErrorStats::new();
    stats.record_code(ErrorCode::Internal);
    stats.reset();
    assert_eq!(stats.total(), 0);
    assert!(stats.codes().is_empty());
    assert!(stats.categories().is_empty());
}

#[test]
fn error_stats_codes_and_categories_maps() {
    let mut stats = ErrorStats::new();
    stats.record_code(ErrorCode::BackendTimeout);
    assert!(stats.codes().contains_key(&ErrorCode::BackendTimeout));
    assert!(stats.categories().contains_key(&ErrorCategory::Backend));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 10 — abp-core ErrorCode (55 variants)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_error_code_all_have_code_string() {
    for code in ErrorCatalog::all() {
        let c = code.code();
        assert!(c.starts_with("ABP-"), "{:?} code={}", code, c);
    }
}

#[test]
fn core_error_code_all_have_category() {
    let valid_cats = ["contract", "protocol", "policy", "runtime", "system"];
    for code in ErrorCatalog::all() {
        assert!(
            valid_cats.contains(&code.category()),
            "{:?} has unexpected category {}",
            code,
            code.category()
        );
    }
}

#[test]
fn core_error_code_all_have_description() {
    for code in ErrorCatalog::all() {
        assert!(!code.description().is_empty(), "{:?}", code);
    }
}

#[test]
fn core_error_code_display_is_code_string() {
    let code = CoreErrorCode::InvalidContractVersion;
    assert_eq!(code.to_string(), "ABP-C001");
}

#[test]
fn core_error_code_implements_std_error() {
    let code = CoreErrorCode::IoError;
    let _: &dyn StdError = &code;
}

#[test]
fn core_error_catalog_lookup() {
    assert_eq!(
        ErrorCatalog::lookup("ABP-C001"),
        Some(CoreErrorCode::InvalidContractVersion)
    );
    assert_eq!(
        ErrorCatalog::lookup("ABP-P001"),
        Some(CoreErrorCode::InvalidEnvelope)
    );
    assert_eq!(ErrorCatalog::lookup("ABP-ZZZZ"), None);
}

#[test]
fn core_error_catalog_by_category() {
    let contracts = ErrorCatalog::by_category("contract");
    assert_eq!(contracts.len(), 12);
    let protos = ErrorCatalog::by_category("protocol");
    assert_eq!(protos.len(), 12);
    let policies = ErrorCatalog::by_category("policy");
    assert_eq!(policies.len(), 11);
    let runtimes = ErrorCatalog::by_category("runtime");
    assert_eq!(runtimes.len(), 13);
    let systems = ErrorCatalog::by_category("system");
    assert_eq!(systems.len(), 11);
}

#[test]
fn core_error_catalog_all_count() {
    assert_eq!(ErrorCatalog::all().len(), 59);
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 11 — abp-core ErrorInfo
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_error_info_construction() {
    let info = CoreErrorInfo::new(CoreErrorCode::BackendTimeout, "timed out");
    assert_eq!(info.code, CoreErrorCode::BackendTimeout);
    assert_eq!(info.message, "timed out");
    assert!(info.context.is_empty());
    assert!(info.source.is_none());
}

#[test]
fn core_error_info_with_context() {
    let info =
        CoreErrorInfo::new(CoreErrorCode::BackendTimeout, "t").with_context("backend", "openai");
    assert_eq!(info.context["backend"], "openai");
}

#[test]
fn core_error_info_with_source() {
    let src = io::Error::new(io::ErrorKind::Other, "disk");
    let info = CoreErrorInfo::new(CoreErrorCode::IoError, "io").with_source(src);
    assert!(info.source.is_some());
    assert!(info.source().is_some());
}

#[test]
fn core_error_info_display_no_context() {
    let info = CoreErrorInfo::new(CoreErrorCode::InternalError, "oops");
    assert_eq!(info.to_string(), "[ABP-S003] oops");
}

#[test]
fn core_error_info_display_with_context() {
    let info =
        CoreErrorInfo::new(CoreErrorCode::BackendTimeout, "slow").with_context("backend", "openai");
    let s = info.to_string();
    assert!(s.contains("ABP-R002"));
    assert!(s.contains("backend=openai"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 12 — abp-core MappingError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_fidelity_loss() {
    let err = CoreMappingError::FidelityLoss {
        field: "system_prompt".into(),
        source_dialect: "openai".into(),
        target_dialect: "anthropic".into(),
        detail: "metadata dropped".into(),
    };
    assert_eq!(err.code(), CoreMappingError::FIDELITY_LOSS_CODE);
    assert_eq!(err.kind(), MappingErrorKind::Degraded);
    assert!(err.is_degraded());
    assert!(!err.is_fatal());
    assert!(!err.is_emulated());
}

#[test]
fn mapping_error_unsupported_capability() {
    let err = CoreMappingError::UnsupportedCapability {
        capability: "tool_use".into(),
        dialect: "gemini".into(),
    };
    assert_eq!(err.code(), CoreMappingError::UNSUPPORTED_CAP_CODE);
    assert_eq!(err.kind(), MappingErrorKind::Fatal);
    assert!(err.is_fatal());
}

#[test]
fn mapping_error_emulation_required() {
    let err = CoreMappingError::EmulationRequired {
        feature: "streaming".into(),
        detail: "via polling".into(),
    };
    assert_eq!(err.code(), CoreMappingError::EMULATION_REQUIRED_CODE);
    assert_eq!(err.kind(), MappingErrorKind::Emulated);
    assert!(err.is_emulated());
}

#[test]
fn mapping_error_incompatible_model_with_suggestion() {
    let err = CoreMappingError::IncompatibleModel {
        requested: "gpt-5".into(),
        dialect: "anthropic".into(),
        suggestion: Some("claude-3".into()),
    };
    assert_eq!(err.code(), CoreMappingError::INCOMPATIBLE_MODEL_CODE);
    assert!(err.is_fatal());
    let s = err.to_string();
    assert!(s.contains("gpt-5"));
    assert!(s.contains("try claude-3"));
}

#[test]
fn mapping_error_incompatible_model_without_suggestion() {
    let err = CoreMappingError::IncompatibleModel {
        requested: "gpt-5".into(),
        dialect: "anthropic".into(),
        suggestion: None,
    };
    let s = err.to_string();
    assert!(!s.contains("try"));
}

#[test]
fn mapping_error_parameter_not_mappable() {
    let err = CoreMappingError::ParameterNotMappable {
        parameter: "top_k".into(),
        value: "40".into(),
        dialect: "openai".into(),
    };
    assert_eq!(err.code(), CoreMappingError::PARAM_NOT_MAPPABLE_CODE);
    assert!(err.is_degraded());
}

#[test]
fn mapping_error_streaming_unsupported() {
    let err = CoreMappingError::StreamingUnsupported {
        dialect: "custom".into(),
    };
    assert_eq!(err.code(), CoreMappingError::STREAMING_UNSUPPORTED_CODE);
    assert!(err.is_fatal());
}

#[test]
fn mapping_error_kind_display() {
    assert_eq!(MappingErrorKind::Fatal.to_string(), "fatal");
    assert_eq!(MappingErrorKind::Degraded.to_string(), "degraded");
    assert_eq!(MappingErrorKind::Emulated.to_string(), "emulated");
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 13 — ProtocolError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_json_variant() {
    let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let proto: ProtocolError = serde_err.into();
    let s = proto.to_string();
    assert!(s.contains("invalid JSON"));
}

#[test]
fn protocol_error_io_variant() {
    let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe");
    let proto: ProtocolError = io_err.into();
    assert!(proto.to_string().contains("I/O error"));
}

#[test]
fn protocol_error_violation_variant() {
    let proto = ProtocolError::Violation("missing discriminator".into());
    assert!(proto.to_string().contains("protocol violation"));
    assert_eq!(
        proto.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_variant() {
    let proto = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert!(proto.to_string().contains("hello"));
    assert!(proto.to_string().contains("event"));
    assert_eq!(
        proto.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn protocol_error_abp_variant() {
    let abp = abp_error::AbpError::new(abp_error::ErrorCode::ProtocolHandshakeFailed, "no hello");
    let proto: ProtocolError = abp.into();
    assert_eq!(
        proto.error_code(),
        Some(abp_error::ErrorCode::ProtocolHandshakeFailed)
    );
}

#[test]
fn protocol_error_json_has_no_error_code() {
    let serde_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
    let proto: ProtocolError = serde_err.into();
    assert_eq!(proto.error_code(), None);
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 14 — ProjectionError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn projection_error_no_suitable_backend() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "no match".into(),
    };
    assert!(err.to_string().contains("no match"));
}

#[test]
fn projection_error_empty_matrix() {
    let err = ProjectionError::EmptyMatrix;
    assert!(err.to_string().contains("empty"));
}

#[test]
fn projection_error_unsupported_dialect_pair() {
    use abp_dialect::Dialect;
    let err = ProjectionError::UnsupportedDialectPair {
        src_dialect: Dialect::OpenAi,
        tgt_dialect: Dialect::Gemini,
    };
    let s = err.to_string();
    assert!(s.contains("unsupported dialect pair"));
}

#[test]
fn projection_error_mapping_failed() {
    let err = ProjectionError::MappingFailed {
        reason: "incompatible".into(),
    };
    assert!(err.to_string().contains("mapping failed"));
}

#[test]
fn projection_error_configuration_error() {
    let err = ProjectionError::ConfigurationError {
        reason: "bad weights".into(),
    };
    assert!(err.to_string().contains("configuration error"));
}

#[test]
fn projection_error_serde_roundtrip() {
    let errors = vec![
        ProjectionError::EmptyMatrix,
        ProjectionError::NoSuitableBackend { reason: "x".into() },
        ProjectionError::MappingFailed { reason: "y".into() },
        ProjectionError::ConfigurationError { reason: "z".into() },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, err);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 15 — RuntimeError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn runtime_error_unknown_backend() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert!(err.to_string().contains("foo"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    assert!(!err.is_retryable());
}

#[test]
fn runtime_error_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(err.to_string().contains("workspace"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
    assert!(err.is_retryable());
}

#[test]
fn runtime_error_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert!(err.to_string().contains("policy"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
    assert!(!err.is_retryable());
}

#[test]
fn runtime_error_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert!(err.to_string().contains("backend"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
    assert!(err.is_retryable());
}

#[test]
fn runtime_error_capability_check_failed() {
    let err = RuntimeError::CapabilityCheckFailed("missing mcp".into());
    assert!(err.to_string().contains("missing mcp"));
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
    assert!(!err.is_retryable());
}

#[test]
fn runtime_error_classified() {
    let abp = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "slow");
    let err: RuntimeError = abp.into();
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendTimeout);
    assert!(err.is_retryable());
}

#[test]
fn runtime_error_no_projection_match() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no backend".into(),
    };
    assert!(err.to_string().contains("no backend"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    assert!(!err.is_retryable());
}

#[test]
fn runtime_error_into_abp_error() {
    let rt_err = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    let abp = rt_err.into_abp_error();
    assert_eq!(abp.code, abp_error::ErrorCode::BackendNotFound);
    assert!(abp.message.contains("missing"));
}

#[test]
fn runtime_error_classified_roundtrip() {
    let abp = abp_error::AbpError::new(abp_error::ErrorCode::ConfigInvalid, "bad")
        .with_context("file", "cfg.toml");
    let rt_err: RuntimeError = abp.into();
    let back = rt_err.into_abp_error();
    assert_eq!(back.code, abp_error::ErrorCode::ConfigInvalid);
    assert_eq!(
        back.context.get("file"),
        Some(&serde_json::json!("cfg.toml"))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 16 — ConfigError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_error_file_not_found() {
    let err = ConfigError::FileNotFound {
        path: "/tmp/missing.toml".into(),
    };
    assert!(err.to_string().contains("/tmp/missing.toml"));
}

#[test]
fn config_error_parse_error() {
    let err = ConfigError::ParseError {
        reason: "unexpected EOF".into(),
    };
    assert!(err.to_string().contains("unexpected EOF"));
}

#[test]
fn config_error_validation_error() {
    let err = ConfigError::ValidationError {
        reasons: vec!["invalid port".into(), "missing host".into()],
    };
    let s = err.to_string();
    assert!(s.contains("invalid port"));
}

#[test]
fn config_error_merge_conflict() {
    let err = ConfigError::MergeConflict {
        reason: "conflicting backends".into(),
    };
    assert!(err.to_string().contains("merge conflict"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 17 — ValidationError / ValidationErrors
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_error_display() {
    let ve = ValidationError {
        path: "config.timeout".into(),
        kind: ValidationErrorKind::OutOfRange,
        message: "must be positive".into(),
    };
    assert_eq!(ve.to_string(), "config.timeout: must be positive");
}

#[test]
fn validation_error_kind_display() {
    assert_eq!(ValidationErrorKind::Required.to_string(), "required");
    assert_eq!(
        ValidationErrorKind::InvalidFormat.to_string(),
        "invalid_format"
    );
    assert_eq!(ValidationErrorKind::OutOfRange.to_string(), "out_of_range");
    assert_eq!(
        ValidationErrorKind::InvalidReference.to_string(),
        "invalid_reference"
    );
    assert_eq!(ValidationErrorKind::Custom.to_string(), "custom");
}

#[test]
fn validation_errors_empty() {
    let errs = ValidationErrors::new();
    assert!(errs.is_empty());
    assert_eq!(errs.len(), 0);
}

#[test]
fn validation_errors_push_and_len() {
    let mut errs = ValidationErrors::new();
    errs.push(ValidationError {
        path: "a".into(),
        kind: ValidationErrorKind::Required,
        message: "missing".into(),
    });
    assert_eq!(errs.len(), 1);
    assert!(!errs.is_empty());
}

#[test]
fn validation_errors_add() {
    let mut errs = ValidationErrors::new();
    errs.add("task", ValidationErrorKind::Required, "task is required");
    assert_eq!(errs.len(), 1);
}

#[test]
fn validation_errors_into_result_ok() {
    let errs = ValidationErrors::new();
    assert!(errs.into_result().is_ok());
}

#[test]
fn validation_errors_into_result_err() {
    let mut errs = ValidationErrors::new();
    errs.add("x", ValidationErrorKind::Custom, "bad");
    assert!(errs.into_result().is_err());
}

#[test]
fn validation_errors_merge() {
    let mut a = ValidationErrors::new();
    a.add("a", ValidationErrorKind::Required, "r");
    let mut b = ValidationErrors::new();
    b.add("b", ValidationErrorKind::Custom, "c");
    a.merge(b);
    assert_eq!(a.len(), 2);
}

#[test]
fn validation_errors_filter_by_kind() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "r");
    errs.add("b", ValidationErrorKind::Custom, "c");
    errs.add("c", ValidationErrorKind::Required, "r2");
    let req = errs.filter_by_kind(&ValidationErrorKind::Required);
    assert_eq!(req.len(), 2);
}

#[test]
fn validation_errors_filter_by_path_prefix() {
    let mut errs = ValidationErrors::new();
    errs.add("config.timeout", ValidationErrorKind::OutOfRange, "low");
    errs.add("config.host", ValidationErrorKind::Required, "missing");
    errs.add("task.name", ValidationErrorKind::Required, "missing");
    let config_errs = errs.filter_by_path_prefix("config.");
    assert_eq!(config_errs.len(), 2);
}

#[test]
fn validation_errors_format_report_empty() {
    let errs = ValidationErrors::new();
    assert_eq!(errs.format_report(), "No validation errors.");
}

#[test]
fn validation_errors_format_report_non_empty() {
    let mut errs = ValidationErrors::new();
    errs.add("x", ValidationErrorKind::Required, "required");
    let report = errs.format_report();
    assert!(report.contains("1 validation error(s)"));
    assert!(report.contains("[x]"));
}

#[test]
fn validation_errors_into_inner() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "r");
    let v = errs.into_inner();
    assert_eq!(v.len(), 1);
}

#[test]
fn validation_errors_iter() {
    let mut errs = ValidationErrors::new();
    errs.add("a", ValidationErrorKind::Required, "r");
    errs.add("b", ValidationErrorKind::Custom, "c");
    let paths: Vec<_> = errs.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, &["a", "b"]);
}

#[test]
fn validation_errors_display() {
    let mut errs = ValidationErrors::new();
    errs.add("x", ValidationErrorKind::Required, "missing");
    let s = errs.to_string();
    assert!(s.contains("1 error"));
    assert!(s.contains("[x] missing"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 18 — ErrorClassifier + ErrorClassification
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn classifier_rate_limited() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::RateLimit);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.is_some());
}

#[test]
fn classifier_auth_failed() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn classifier_timeout() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendTimeout);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::TimeoutError);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
}

#[test]
fn classifier_model_not_found() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendModelNotFound);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ModelNotFound);
    assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
}

#[test]
fn classifier_mapping_lossy() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    assert_eq!(cl.category, ClassificationCategory::MappingFailure);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn classifier_policy_denied() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::PolicyDenied);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ContentFilter);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn classifier_capability_unsupported() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::CapabilityUnsupported);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn classifier_internal_error() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::Internal);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
}

#[test]
fn classifier_every_code_gets_classified() {
    let c = ErrorClassifier::new();
    for code in ALL_ABP_ERROR_CODES {
        let cl = c.classify(code);
        assert_eq!(cl.code, *code, "{:?}", code);
        // Ensure recovery suggestion is present
        assert!(!cl.recovery.description.is_empty(), "{:?}", code);
    }
}

#[test]
fn suggest_recovery_from_classification() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendTimeout);
    let suggestion = c.suggest_recovery(&cl);
    assert_eq!(suggestion.action, RecoveryAction::Retry);
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 19 — RecoveryPlan
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn recovery_plan_rate_limited_has_retry() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendRateLimited);
    let plan = RecoveryPlan::from_classification(&cl);
    assert!(plan.has_retry());
    assert!(!plan.steps.is_empty());
    assert_eq!(plan.steps[0].action, RecoveryAction::Retry);
    assert!(plan.steps[0].retry_policy.is_some());
}

#[test]
fn recovery_plan_auth_is_terminal() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendAuthFailed);
    let plan = RecoveryPlan::from_classification(&cl);
    assert!(plan.is_terminal());
    assert!(!plan.has_retry());
}

#[test]
fn recovery_plan_model_not_found_suggests_change() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendModelNotFound);
    let plan = RecoveryPlan::from_classification(&cl);
    assert!(!plan.steps.is_empty());
    assert_eq!(plan.steps[0].action, RecoveryAction::ChangeModel);
}

#[test]
fn recovery_plan_every_code_non_empty() {
    let c = ErrorClassifier::new();
    for code in ALL_ABP_ERROR_CODES {
        let cl = c.classify(code);
        let plan = RecoveryPlan::from_classification(&cl);
        assert!(!plan.steps.is_empty(), "{:?} has empty plan", code);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 20 — RetryPolicy
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retry_policy_delay_calculation() {
    let p = RetryPolicy {
        max_retries: 3,
        initial_delay_ms: 1000,
        backoff_factor: 2,
        max_delay_ms: 10_000,
    };
    assert_eq!(p.delay_for_attempt(0), Some(1000));
    assert_eq!(p.delay_for_attempt(1), Some(2000));
    assert_eq!(p.delay_for_attempt(2), Some(4000));
    assert_eq!(p.delay_for_attempt(3), None);
}

#[test]
fn retry_policy_respects_max_delay() {
    let p = RetryPolicy {
        max_retries: 10,
        initial_delay_ms: 1000,
        backoff_factor: 10,
        max_delay_ms: 5000,
    };
    assert_eq!(p.delay_for_attempt(3), Some(5000));
}

#[test]
fn retry_policy_default() {
    let p = RetryPolicy::default();
    assert_eq!(p.max_retries, 3);
    assert_eq!(p.initial_delay_ms, 1000);
    assert_eq!(p.backoff_factor, 2);
    assert_eq!(p.max_delay_ms, 30_000);
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 21 — ErrorContextBuilder + EnrichError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn context_builder_all_fields() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "t");
    let enriched = ErrorContextBuilder::from_error(err)
        .backend("openai")
        .request_id("req-1")
        .model("gpt-4")
        .elapsed_ms(5000)
        .retry_count(3)
        .http_status(504)
        .sidecar_pid(1234)
        .work_order_id("wo-99")
        .custom("extra", "val")
        .build();
    assert_eq!(enriched.context["backend"], serde_json::json!("openai"));
    assert_eq!(enriched.context["request_id"], serde_json::json!("req-1"));
    assert_eq!(enriched.context["model"], serde_json::json!("gpt-4"));
    assert_eq!(enriched.context["elapsed_ms"], serde_json::json!(5000));
    assert_eq!(enriched.context["retry_count"], serde_json::json!(3));
    assert_eq!(enriched.context["http_status"], serde_json::json!(504));
    assert_eq!(enriched.context["sidecar_pid"], serde_json::json!(1234));
    assert_eq!(
        enriched.context["work_order_id"],
        serde_json::json!("wo-99")
    );
    assert_eq!(enriched.context["extra"], serde_json::json!("val"));
}

#[test]
fn enrich_extension_trait() {
    let err = AbpError::new(ErrorCode::Internal, "x")
        .enrich()
        .backend("mock")
        .build();
    assert_eq!(err.context["backend"], serde_json::json!("mock"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 22 — VendorErrorMapper
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vendor_mapper_openai_rate_limit() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::OpenAi, 429, "rate_limit_exceeded");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendRateLimited);
}

#[test]
fn vendor_mapper_openai_auth() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::OpenAi, 401, "invalid_api_key");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendAuthFailed);
}

#[test]
fn vendor_mapper_anthropic_overloaded() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Anthropic, 529, "overloaded_error");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendUnavailable);
}

#[test]
fn vendor_mapper_gemini_deadline_exceeded() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Gemini, 504, "DEADLINE_EXCEEDED");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendTimeout);
}

#[test]
fn vendor_mapper_custom_falls_back_to_http() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Custom, 429, "anything");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendRateLimited);
}

#[test]
fn vendor_mapper_unknown_type_falls_back_to_http() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::OpenAi, 503, "brand_new_error");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendUnavailable);
}

#[test]
fn vendor_mapper_to_abp_error_includes_context() {
    let m = VendorErrorMapper::new();
    let e =
        VendorError::new(VendorKind::OpenAi, 429, "rate_limit_exceeded").with_message("slow down");
    let abp = m.to_abp_error(&e);
    assert_eq!(abp.code, ErrorCode::BackendRateLimited);
    assert_eq!(abp.message, "slow down");
    assert!(abp.context.contains_key("vendor"));
    assert!(abp.context.contains_key("http_status"));
    assert!(abp.context.contains_key("vendor_error_type"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 23 — ErrorCodeDoc
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn every_code_has_documentation() {
    for code in ALL_ABP_ERROR_CODES {
        let doc = error_code_doc(code);
        assert_eq!(doc.code, *code);
        assert!(!doc.description.is_empty(), "{:?}", code);
        assert!(!doc.example.is_empty(), "{:?}", code);
    }
}

#[test]
fn doc_descriptions_longer_than_short_message() {
    for code in ALL_ABP_ERROR_CODES {
        let doc = error_code_doc(code);
        assert_ne!(doc.description, code.message(), "{:?}", code);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 24 — Cross-cutting: std::error::Error trait compliance
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AbpError>();
}

#[test]
fn protocol_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProtocolError>();
}

#[test]
fn runtime_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeError>();
}

#[test]
fn config_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ConfigError>();
}

#[test]
fn projection_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProjectionError>();
}

#[test]
fn validation_errors_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ValidationErrors>();
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 25 — HTTP status fallback exhaustive
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vendor_mapper_http_401() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Custom, 401, "x");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendAuthFailed);
}

#[test]
fn vendor_mapper_http_403() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Custom, 403, "x");
    assert_eq!(m.map_to_abp(&e), ErrorCode::ExecutionPermissionDenied);
}

#[test]
fn vendor_mapper_http_404() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Custom, 404, "x");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendModelNotFound);
}

#[test]
fn vendor_mapper_http_408() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Custom, 408, "x");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendTimeout);
}

#[test]
fn vendor_mapper_http_500() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Custom, 500, "x");
    assert_eq!(m.map_to_abp(&e), ErrorCode::Internal);
}

#[test]
fn vendor_mapper_http_502() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Custom, 502, "x");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendUnavailable);
}

#[test]
fn vendor_mapper_http_504() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Custom, 504, "x");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendTimeout);
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 26 — Serde roundtrip for key error types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_code_serde_roundtrip() {
    for code in ALL_ABP_ERROR_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back, "roundtrip failed for {:?}", code);
    }
}

#[test]
fn error_info_serde_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "t").with_detail("k", "v");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn error_location_serde_roundtrip() {
    let loc = ErrorLocation::new("f.rs", 1, 2);
    let json = serde_json::to_string(&loc).unwrap();
    let back: ErrorLocation = serde_json::from_str(&json).unwrap();
    assert_eq!(loc, back);
}

#[test]
fn core_mapping_error_serde_roundtrip() {
    let err = CoreMappingError::FidelityLoss {
        field: "x".into(),
        source_dialect: "a".into(),
        target_dialect: "b".into(),
        detail: "d".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: CoreMappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}
