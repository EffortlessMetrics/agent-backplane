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
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Deep tests for error taxonomy and classification.
//!
//! Covers: ErrorCode coverage (all 36 variants), ErrorClassifier (severity,
//! category, recovery for each code), ErrorSeverity levels, ClassificationCategory
//! groups, RecoverySuggestion, error context chains, Display impls, serde
//! roundtrips, error conversions, and error response construction.

use abp_error_taxonomy::classification::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
    RecoverySuggestion,
};
use abp_error_taxonomy::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use std::collections::HashSet;
use std::error::Error as StdError;
use std::io;

// =========================================================================
// Helpers
// =========================================================================

const ALL_CODES: &[ErrorCode] = &[
    // Protocol (5)
    ErrorCode::ProtocolInvalidEnvelope,
    ErrorCode::ProtocolHandshakeFailed,
    ErrorCode::ProtocolMissingRefId,
    ErrorCode::ProtocolUnexpectedMessage,
    ErrorCode::ProtocolVersionMismatch,
    // Mapping (4)
    ErrorCode::MappingUnsupportedCapability,
    ErrorCode::MappingDialectMismatch,
    ErrorCode::MappingLossyConversion,
    ErrorCode::MappingUnmappableTool,
    // Backend (7)
    ErrorCode::BackendNotFound,
    ErrorCode::BackendUnavailable,
    ErrorCode::BackendTimeout,
    ErrorCode::BackendRateLimited,
    ErrorCode::BackendAuthFailed,
    ErrorCode::BackendModelNotFound,
    ErrorCode::BackendCrashed,
    // Execution (3)
    ErrorCode::ExecutionToolFailed,
    ErrorCode::ExecutionWorkspaceError,
    ErrorCode::ExecutionPermissionDenied,
    // Contract (3)
    ErrorCode::ContractVersionMismatch,
    ErrorCode::ContractSchemaViolation,
    ErrorCode::ContractInvalidReceipt,
    // Capability (2)
    ErrorCode::CapabilityUnsupported,
    ErrorCode::CapabilityEmulationFailed,
    // Policy (2)
    ErrorCode::PolicyDenied,
    ErrorCode::PolicyInvalid,
    // Workspace (2)
    ErrorCode::WorkspaceInitFailed,
    ErrorCode::WorkspaceStagingFailed,
    // IR (2)
    ErrorCode::IrLoweringFailed,
    ErrorCode::IrInvalid,
    // Receipt (2)
    ErrorCode::ReceiptHashMismatch,
    ErrorCode::ReceiptChainBroken,
    // Dialect (2)
    ErrorCode::DialectUnknown,
    ErrorCode::DialectMappingFailed,
    // Config (1)
    ErrorCode::ConfigInvalid,
    // Internal (1)
    ErrorCode::Internal,
];

fn classifier() -> ErrorClassifier {
    ErrorClassifier::new()
}

// =========================================================================
// 1. ErrorCode coverage — all 36 variants exist and serialize correctly
// =========================================================================

#[test]
fn exactly_36_error_codes() {
    assert_eq!(ALL_CODES.len(), 36);
}

#[test]
fn all_codes_have_unique_as_str() {
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
fn all_codes_serialize_to_snake_case_string() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let inner = json.trim_matches('"');
        assert!(
            inner.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "code {:?} serialized to non-snake_case: {}",
            code,
            inner
        );
    }
}

#[test]
fn all_codes_json_matches_as_str() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "JSON mismatch for {:?}", code);
    }
}

#[test]
fn all_codes_have_non_empty_message() {
    for code in ALL_CODES {
        assert!(!code.message().is_empty(), "{:?} has empty message", code);
    }
}

#[test]
fn all_codes_have_a_category() {
    let valid_categories: HashSet<ErrorCategory> = [
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
    ]
    .into_iter()
    .collect();

    for code in ALL_CODES {
        assert!(
            valid_categories.contains(&code.category()),
            "{:?} has unexpected category {:?}",
            code,
            code.category()
        );
    }
}

// =========================================================================
// 2. ErrorClassifier — classify() returns correct severity, category,
//    recovery for each code
// =========================================================================

#[test]
fn classify_all_codes_produces_valid_classification() {
    let c = classifier();
    for code in ALL_CODES {
        let cl = c.classify(code);
        assert_eq!(&cl.code, code);
        assert!(!cl.recovery.description.is_empty());
    }
}

#[test]
fn classify_protocol_codes_severity_and_category() {
    let c = classifier();
    let protocol_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in &protocol_codes {
        let cl = c.classify(code);
        assert_eq!(cl.severity, ErrorSeverity::Fatal);
        assert_eq!(cl.category, ClassificationCategory::ProtocolError);
    }
}

#[test]
fn classify_protocol_missing_ref_id() {
    let cl = classifier().classify(&ErrorCode::ProtocolMissingRefId);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

#[test]
fn classify_retriable_backend_codes() {
    let c = classifier();
    let retriable = [
        (
            ErrorCode::BackendUnavailable,
            ClassificationCategory::ServerError,
        ),
        (
            ErrorCode::BackendTimeout,
            ClassificationCategory::TimeoutError,
        ),
        (
            ErrorCode::BackendRateLimited,
            ClassificationCategory::RateLimit,
        ),
        (
            ErrorCode::BackendCrashed,
            ClassificationCategory::ServerError,
        ),
    ];
    for (code, expected_cat) in &retriable {
        let cl = c.classify(code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Retriable,
            "wrong severity for {:?}",
            code
        );
        assert_eq!(cl.category, *expected_cat, "wrong category for {:?}", code);
    }
}

#[test]
fn classify_fatal_backend_codes() {
    let c = classifier();
    let fatal = [
        (
            ErrorCode::BackendNotFound,
            ClassificationCategory::ServerError,
        ),
        (
            ErrorCode::BackendAuthFailed,
            ClassificationCategory::Authentication,
        ),
        (
            ErrorCode::BackendModelNotFound,
            ClassificationCategory::ModelNotFound,
        ),
    ];
    for (code, expected_cat) in &fatal {
        let cl = c.classify(code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Fatal,
            "wrong severity for {:?}",
            code
        );
        assert_eq!(cl.category, *expected_cat, "wrong category for {:?}", code);
    }
}

#[test]
fn classify_mapping_codes() {
    let c = classifier();
    assert_eq!(
        c.classify(&ErrorCode::MappingUnsupportedCapability)
            .category,
        ClassificationCategory::CapabilityUnsupported
    );
    assert_eq!(
        c.classify(&ErrorCode::MappingDialectMismatch).category,
        ClassificationCategory::MappingFailure
    );
    assert_eq!(
        c.classify(&ErrorCode::MappingLossyConversion).severity,
        ErrorSeverity::Degraded
    );
    assert_eq!(
        c.classify(&ErrorCode::MappingUnmappableTool).category,
        ClassificationCategory::MappingFailure
    );
}

#[test]
fn classify_execution_codes_are_all_fatal() {
    let c = classifier();
    for code in &[
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
    ] {
        let cl = c.classify(code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Fatal,
            "{:?} should be Fatal",
            code
        );
    }
}

#[test]
fn classify_contract_codes_are_all_fatal() {
    let c = classifier();
    for code in &[
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ] {
        let cl = c.classify(code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Fatal,
            "{:?} should be Fatal",
            code
        );
    }
}

#[test]
fn classify_policy_denied_is_content_filter() {
    let cl = classifier().classify(&ErrorCode::PolicyDenied);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ContentFilter);
}

#[test]
fn classify_policy_invalid_is_invalid_request() {
    let cl = classifier().classify(&ErrorCode::PolicyInvalid);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

#[test]
fn classify_workspace_codes_are_server_error() {
    let c = classifier();
    for code in &[
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
    ] {
        let cl = c.classify(code);
        assert_eq!(cl.category, ClassificationCategory::ServerError);
    }
}

#[test]
fn classify_ir_codes() {
    let c = classifier();
    assert_eq!(
        c.classify(&ErrorCode::IrLoweringFailed).category,
        ClassificationCategory::MappingFailure
    );
    assert_eq!(
        c.classify(&ErrorCode::IrInvalid).category,
        ClassificationCategory::InvalidRequest
    );
}

#[test]
fn classify_receipt_codes_are_invalid_request() {
    let c = classifier();
    for code in &[
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
    ] {
        let cl = c.classify(code);
        assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
    }
}

#[test]
fn classify_dialect_codes_are_mapping_failure() {
    let c = classifier();
    for code in &[ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed] {
        let cl = c.classify(code);
        assert_eq!(cl.category, ClassificationCategory::MappingFailure);
    }
}

#[test]
fn classify_config_invalid() {
    let cl = classifier().classify(&ErrorCode::ConfigInvalid);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

#[test]
fn classify_internal_error() {
    let cl = classifier().classify(&ErrorCode::Internal);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
}

// =========================================================================
// 3. ErrorSeverity levels — test all variants and serde
// =========================================================================

#[test]
fn severity_all_variants_serde_roundtrip() {
    let all = [
        ErrorSeverity::Fatal,
        ErrorSeverity::Retriable,
        ErrorSeverity::Degraded,
        ErrorSeverity::Informational,
    ];
    for sev in &all {
        let json = serde_json::to_string(sev).unwrap();
        let back: ErrorSeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(*sev, back);
    }
}

#[test]
fn severity_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&ErrorSeverity::Fatal).unwrap(),
        "\"fatal\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorSeverity::Retriable).unwrap(),
        "\"retriable\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorSeverity::Degraded).unwrap(),
        "\"degraded\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorSeverity::Informational).unwrap(),
        "\"informational\""
    );
}

#[test]
fn severity_eq_and_clone() {
    let a = ErrorSeverity::Fatal;
    #[allow(clippy::clone_on_copy)]
    let b = a.clone();
    assert_eq!(a, b);
    assert_ne!(ErrorSeverity::Fatal, ErrorSeverity::Retriable);
}

// =========================================================================
// 4. ClassificationCategory groups — all 12 variants
// =========================================================================

#[test]
fn classification_category_all_variants_serde_roundtrip() {
    let all = [
        ClassificationCategory::Authentication,
        ClassificationCategory::RateLimit,
        ClassificationCategory::ModelNotFound,
        ClassificationCategory::InvalidRequest,
        ClassificationCategory::ContentFilter,
        ClassificationCategory::ContextLength,
        ClassificationCategory::ServerError,
        ClassificationCategory::NetworkError,
        ClassificationCategory::ProtocolError,
        ClassificationCategory::CapabilityUnsupported,
        ClassificationCategory::MappingFailure,
        ClassificationCategory::TimeoutError,
    ];
    for cat in &all {
        let json = serde_json::to_string(cat).unwrap();
        let back: ClassificationCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back);
    }
    assert_eq!(all.len(), 12);
}

#[test]
fn classification_category_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&ClassificationCategory::RateLimit).unwrap(),
        "\"rate_limit\""
    );
    assert_eq!(
        serde_json::to_string(&ClassificationCategory::ModelNotFound).unwrap(),
        "\"model_not_found\""
    );
    assert_eq!(
        serde_json::to_string(&ClassificationCategory::CapabilityUnsupported).unwrap(),
        "\"capability_unsupported\""
    );
}

// =========================================================================
// 5. RecoverySuggestion — action types and delay semantics
// =========================================================================

#[test]
fn recovery_action_all_variants_serde_roundtrip() {
    let all = [
        RecoveryAction::Retry,
        RecoveryAction::Fallback,
        RecoveryAction::ReduceContext,
        RecoveryAction::ChangeModel,
        RecoveryAction::ContactAdmin,
        RecoveryAction::None,
    ];
    for action in &all {
        let json = serde_json::to_string(action).unwrap();
        let back: RecoveryAction = serde_json::from_str(&json).unwrap();
        assert_eq!(*action, back);
    }
    assert_eq!(all.len(), 6);
}

#[test]
fn retriable_codes_get_retry_with_delay() {
    let c = classifier();
    for code in &[
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendCrashed,
    ] {
        let cl = c.classify(code);
        assert_eq!(cl.recovery.action, RecoveryAction::Retry, "{:?}", code);
        assert!(
            cl.recovery.delay_ms.is_some(),
            "{:?} should have delay",
            code
        );
    }
}

#[test]
fn auth_failed_recovery_is_contact_admin_no_delay() {
    let cl = classifier().classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    assert!(cl.recovery.delay_ms.is_none());
}

#[test]
fn model_not_found_recovery_is_change_model() {
    let cl = classifier().classify(&ErrorCode::BackendModelNotFound);
    assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
    assert!(cl.recovery.delay_ms.is_none());
}

#[test]
fn capability_unsupported_recovery_is_fallback() {
    let cl = classifier().classify(&ErrorCode::CapabilityUnsupported);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn mapping_failure_recovery_is_fallback() {
    let cl = classifier().classify(&ErrorCode::MappingDialectMismatch);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn degraded_lossy_conversion_recovery_has_no_action() {
    let cl = classifier().classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    // Degraded mapping failure → Fallback
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn suggest_recovery_consistency() {
    let c = classifier();
    for code in ALL_CODES {
        let cl = c.classify(code);
        let suggestion = c.suggest_recovery(&cl);
        assert_eq!(
            cl.recovery, suggestion,
            "suggest_recovery differs from classify for {:?}",
            code
        );
    }
}

#[test]
fn recovery_suggestion_serde_roundtrip() {
    let suggestion = RecoverySuggestion {
        action: RecoveryAction::Retry,
        description: "test recovery".into(),
        delay_ms: Some(1500),
    };
    let json = serde_json::to_string(&suggestion).unwrap();
    let back: RecoverySuggestion = serde_json::from_str(&json).unwrap();
    assert_eq!(suggestion, back);
}

#[test]
fn recovery_suggestion_without_delay_roundtrip() {
    let suggestion = RecoverySuggestion {
        action: RecoveryAction::ContactAdmin,
        description: "contact support".into(),
        delay_ms: None,
    };
    let json = serde_json::to_string(&suggestion).unwrap();
    let back: RecoverySuggestion = serde_json::from_str(&json).unwrap();
    assert_eq!(suggestion, back);
}

// =========================================================================
// 6. Error context — test AbpError with nested context chains
// =========================================================================

#[test]
fn abp_error_source_chain_single_level() {
    let inner = io::Error::new(io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init failed").with_source(inner);
    let src = err.source().expect("should have source");
    assert_eq!(src.to_string(), "file missing");
}

#[test]
fn abp_error_source_chain_two_levels() {
    let root = io::Error::new(io::ErrorKind::PermissionDenied, "no perms");
    let mid = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(root);
    let outer = AbpError::new(ErrorCode::Internal, "wrapper").with_source(mid);

    let src1 = outer.source().unwrap();
    assert!(src1.to_string().contains("workspace_staging_failed"));
    let src2 = src1.source().unwrap();
    assert_eq!(src2.to_string(), "no perms");
}

#[test]
fn abp_error_context_multiple_keys() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000)
        .with_context("retries", 3);
    assert_eq!(err.context.len(), 3);
    assert_eq!(err.context["backend"], serde_json::json!("openai"));
    assert_eq!(err.context["timeout_ms"], serde_json::json!(30_000));
    assert_eq!(err.context["retries"], serde_json::json!(3));
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

#[test]
fn abp_error_context_overwrite_same_key() {
    let err = AbpError::new(ErrorCode::Internal, "overwrite")
        .with_context("key", "first")
        .with_context("key", "second");
    assert_eq!(err.context.len(), 1);
    assert_eq!(err.context["key"], serde_json::json!("second"));
}

#[test]
fn abp_error_no_source_by_default() {
    let err = AbpError::new(ErrorCode::Internal, "bare");
    assert!(err.source().is_none());
    assert!(err.context.is_empty());
}

// =========================================================================
// 7. Error display — all error types implement Display correctly
// =========================================================================

#[test]
fn error_code_display_matches_message() {
    for code in ALL_CODES {
        assert_eq!(code.to_string(), code.message());
    }
}

#[test]
fn error_category_display_is_lowercase() {
    let all_cats = [
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
    for cat in &all_cats {
        let s = cat.to_string();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase()),
            "{:?} display is not lowercase: {}",
            cat,
            s
        );
    }
}

#[test]
fn abp_error_display_contains_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "not found");
    let display = err.to_string();
    assert!(display.contains("backend_not_found"));
    assert!(display.contains("not found"));
}

#[test]
fn abp_error_display_includes_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow").with_context("ms", 5000);
    let display = err.to_string();
    assert!(display.contains("5000"));
    assert!(display.contains("ms"));
}

#[test]
fn abp_error_display_no_context_when_empty() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert_eq!(err.to_string(), "[policy_denied] denied");
}

#[test]
fn error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out after 30 s");
    let display = info.to_string();
    assert!(display.contains("backend_timeout"));
    assert!(display.contains("timed out after 30 s"));
}

#[test]
fn abp_error_debug_shows_code_and_message() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("PolicyDenied"));
    assert!(dbg.contains("denied"));
}

#[test]
fn abp_error_debug_omits_source_and_context_when_absent() {
    let err = AbpError::new(ErrorCode::Internal, "bare");
    let dbg = format!("{err:?}");
    assert!(!dbg.contains("source"));
    assert!(!dbg.contains("context"));
}

// =========================================================================
// 8. Serde roundtrip — ErrorCode, AbpError (via DTO), ErrorClassification
// =========================================================================

#[test]
fn error_code_json_roundtrip_all() {
    for code in ALL_CODES {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *code, "roundtrip mismatch for {:?}", code);
    }
}

#[test]
fn error_classification_json_roundtrip_all() {
    let c = classifier();
    for code in ALL_CODES {
        let cl = c.classify(code);
        let json = serde_json::to_string(&cl).unwrap();
        let back: ErrorClassification = serde_json::from_str(&json).unwrap();
        assert_eq!(cl, back, "classification roundtrip failed for {:?}", code);
    }
}

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
    let inner = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(inner);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn error_info_serde_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timeout")
        .with_detail("backend", "openai")
        .with_detail("retries", 3);
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn error_category_json_roundtrip() {
    let all_cats = [
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
    for cat in &all_cats {
        let json = serde_json::to_string(cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back);
    }
}

// =========================================================================
// 9. Error conversion — From<io::Error>, From<serde_json::Error>
// =========================================================================

#[test]
fn from_io_error_produces_internal_code() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "gone");
    let err: AbpError = io_err.into();
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.source().is_some());
}

#[test]
fn from_serde_json_error_produces_protocol_invalid_envelope() {
    let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let err: AbpError = serde_err.into();
    assert_eq!(err.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(err.source().is_some());
}

#[test]
fn from_io_error_preserves_source_message() {
    let io_err = io::Error::new(io::ErrorKind::TimedOut, "deadline exceeded");
    let err: AbpError = io_err.into();
    assert!(err.message.contains("deadline exceeded"));
}

#[test]
fn from_serde_json_error_source_is_downcastable() {
    let serde_err = serde_json::from_str::<i32>("\"nope\"").unwrap_err();
    let err: AbpError = serde_err.into();
    let boxed = err.source.as_ref().unwrap();
    assert!(boxed.downcast_ref::<serde_json::Error>().is_some());
}

// =========================================================================
// 10. ErrorResponse construction — building error responses with codes
// =========================================================================

#[test]
fn error_info_construction_with_retryable_code() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
    assert!(info.is_retryable);
    assert_eq!(info.code, ErrorCode::BackendTimeout);
}

#[test]
fn error_info_construction_with_non_retryable_code() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "denied");
    assert!(!info.is_retryable);
}

#[test]
fn error_info_with_details() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timeout")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 30_000);
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
    assert_eq!(info.details["timeout_ms"], serde_json::json!(30_000));
}

#[test]
fn abp_error_to_info_preserves_code_and_message() {
    let err =
        AbpError::new(ErrorCode::BackendNotFound, "not found").with_context("backend", "openai");
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendNotFound);
    assert_eq!(info.message, "not found");
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
}

#[test]
fn abp_error_to_info_infers_retryable() {
    let err = AbpError::new(ErrorCode::BackendRateLimited, "rate limited");
    let info = err.to_info();
    assert!(info.is_retryable);

    let err2 = AbpError::new(ErrorCode::Internal, "oops");
    let info2 = err2.to_info();
    assert!(!info2.is_retryable);
}

#[test]
fn dto_from_abp_error_and_back() {
    let err =
        AbpError::new(ErrorCode::ConfigInvalid, "bad config").with_context("file", "config.toml");
    let dto: AbpErrorDto = (&err).into();
    let back: AbpError = dto.into();
    assert_eq!(back.code, ErrorCode::ConfigInvalid);
    assert_eq!(back.message, "bad config");
    assert_eq!(back.context["file"], serde_json::json!("config.toml"));
    // Source is lost in DTO round-trip
    assert!(back.source.is_none());
}

#[test]
fn dto_serialized_omits_null_source() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

#[test]
fn error_response_with_classification_context() {
    let c = classifier();
    let code = ErrorCode::BackendRateLimited;
    let cl = c.classify(&code);
    let err = AbpError::new(code, "rate limited")
        .with_context("severity", format!("{:?}", cl.severity))
        .with_context("recovery_action", format!("{:?}", cl.recovery.action));
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("Retriable"));
    assert!(json.contains("Retry"));
}

#[test]
fn abp_error_is_retryable_matches_error_code() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, "test");
        assert_eq!(err.is_retryable(), code.is_retryable());
    }
}

#[test]
fn abp_error_category_matches_error_code() {
    for code in ALL_CODES {
        let err = AbpError::new(*code, "test");
        assert_eq!(err.category(), code.category());
    }
}
