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
//! Exhaustive error taxonomy and error propagation tests.
//!
//! Verifies ErrorCode exhaustiveness, AbpError propagation, error context
//! preservation, taxonomy completeness, and ShimError consistency.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error as StdError;

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_error_taxonomy::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
    RecoverySuggestion,
};

// ---------------------------------------------------------------------------
// Canonical list — must stay in sync with `ErrorCode` variants.
// ---------------------------------------------------------------------------

const ALL_CODES: &[ErrorCode] = &[
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
    ErrorCategory::Mapping,
    ErrorCategory::Execution,
    ErrorCategory::Contract,
    ErrorCategory::Internal,
];

// ═══════════════════════════════════════════════════════════════════════════
// 1. ErrorCode exhaustiveness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_count_is_36() {
    assert_eq!(ALL_CODES.len(), 36, "expected 36 ErrorCode variants");
}

#[test]
fn every_code_has_unique_as_str() {
    let mut seen = HashSet::new();
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str: {s}");
    }
    assert_eq!(seen.len(), ALL_CODES.len());
}

#[test]
fn every_code_has_nonempty_message() {
    for &code in ALL_CODES {
        let msg = code.message();
        assert!(!msg.is_empty(), "{code:?} has empty message");
    }
}

#[test]
fn every_code_has_unique_message() {
    let mut seen = HashSet::new();
    for &code in ALL_CODES {
        let msg = code.message();
        assert!(seen.insert(msg), "duplicate message for {code:?}: {msg}");
    }
}

#[test]
fn every_code_has_valid_category() {
    let valid: HashSet<ErrorCategory> = ALL_CATEGORIES.iter().copied().collect();
    for &code in ALL_CODES {
        let cat = code.category();
        assert!(
            valid.contains(&cat),
            "{code:?} has unknown category {cat:?}"
        );
    }
}

#[test]
fn every_code_serializes_deserializes_roundtrip() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "serde roundtrip failed for {code:?}");
    }
}

#[test]
fn every_code_serializes_to_snake_case_matching_as_str() {
    for &code in ALL_CODES {
        let json = serde_json::to_string(&code).unwrap();
        let expected = format!("\"{}\"", code.as_str());
        assert_eq!(json, expected, "serde value != as_str for {code:?}");
    }
}

#[test]
fn display_format_is_human_readable_not_snake_case() {
    for &code in ALL_CODES {
        let display = code.to_string();
        // Display should be the message (human-readable), not the code string.
        assert_eq!(
            display,
            code.message(),
            "{code:?}: Display should equal message()"
        );
        assert_ne!(
            display,
            code.as_str(),
            "{code:?}: Display must not be the snake_case code"
        );
    }
}

#[test]
fn as_str_is_lowercase_snake_case() {
    for &code in ALL_CODES {
        let s = code.as_str();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{code:?}: as_str() contains non-snake_case chars: {s}"
        );
    }
}

#[test]
fn retryable_codes_are_exactly_four() {
    let retryable: Vec<ErrorCode> = ALL_CODES
        .iter()
        .copied()
        .filter(|c| c.is_retryable())
        .collect();
    assert_eq!(
        retryable,
        vec![
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendCrashed,
        ]
    );
}

#[test]
fn non_retryable_codes_count() {
    let non_retryable = ALL_CODES.iter().filter(|c| !c.is_retryable()).count();
    assert_eq!(non_retryable, 32);
}

#[test]
fn every_category_has_at_least_one_code() {
    let mut category_codes: HashMap<ErrorCategory, Vec<ErrorCode>> = HashMap::new();
    for &code in ALL_CODES {
        category_codes
            .entry(code.category())
            .or_default()
            .push(code);
    }
    for &cat in ALL_CATEGORIES {
        assert!(
            category_codes.contains_key(&cat),
            "category {cat:?} has no associated codes"
        );
    }
}

#[test]
fn error_category_count_is_13() {
    assert_eq!(ALL_CATEGORIES.len(), 13);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. AbpError propagation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_error_propagation_to_error_info() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "sidecar exited")
        .with_context("backend", "openai");
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendCrashed);
    assert!(info.is_retryable);
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
}

#[test]
fn policy_violation_propagation() {
    let err = AbpError::new(ErrorCode::PolicyDenied, "write to /etc forbidden")
        .with_context("path", "/etc/passwd");
    assert_eq!(err.code, ErrorCode::PolicyDenied);
    assert_eq!(err.category(), ErrorCategory::Policy);
    assert!(!err.is_retryable());
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::PolicyDenied);
}

#[test]
fn protocol_error_propagation() {
    let src = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad envelope json");
    let err = AbpError::new(ErrorCode::ProtocolInvalidEnvelope, "parse failed")
        .with_source(src)
        .with_context("line", 42);
    assert_eq!(err.category(), ErrorCategory::Protocol);
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(!info.is_retryable);
}

#[test]
fn config_error_propagation() {
    let err = AbpError::new(ErrorCode::ConfigInvalid, "missing backend field")
        .with_context("file", "backplane.toml");
    assert_eq!(err.category(), ErrorCategory::Config);
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::ConfigInvalid);
}

#[test]
fn workspace_error_propagation() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "cannot write");
    let err =
        AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging failed").with_source(io_err);
    assert_eq!(err.category(), ErrorCategory::Workspace);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "cannot write");
}

#[test]
fn io_error_converts_to_abp_internal() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert!(StdError::source(&abp).is_some());
}

#[test]
fn serde_error_converts_to_protocol_invalid_envelope() {
    let bad_json = "not json";
    let serde_err = serde_json::from_str::<serde_json::Value>(bad_json).unwrap_err();
    let abp: AbpError = serde_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(StdError::source(&abp).is_some());
}

#[test]
fn error_info_retryable_mirrors_error_code() {
    for &code in ALL_CODES {
        let info = ErrorInfo::new(code, "test");
        assert_eq!(
            info.is_retryable,
            code.is_retryable(),
            "ErrorInfo.is_retryable mismatch for {code:?}"
        );
    }
}

#[test]
fn dto_roundtrip_preserves_code_and_message() {
    for &code in ALL_CODES {
        let err = AbpError::new(code, format!("msg for {}", code.as_str()));
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, code);
        assert_eq!(back.message, format!("msg for {}", code.as_str()));
    }
}

#[test]
fn dto_preserves_source_message() {
    let src = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let err = AbpError::new(ErrorCode::BackendCrashed, "crash").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.source_message.as_deref(), Some("pipe broke"));
}

#[test]
fn dto_to_abp_error_loses_source() {
    let dto = AbpErrorDto {
        code: ErrorCode::Internal,
        message: "oops".into(),
        context: BTreeMap::new(),
        source_message: Some("inner".into()),
    };
    let err: AbpError = dto.into();
    assert!(err.source.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Error context preservation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn source_error_available_via_std_error_source() {
    let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "init").with_source(inner);
    assert!(StdError::source(&err).is_some());
    assert_eq!(StdError::source(&err).unwrap().to_string(), "not found");
}

#[test]
fn source_none_when_not_set() {
    let err = AbpError::new(ErrorCode::Internal, "no source");
    assert!(StdError::source(&err).is_none());
}

#[test]
fn display_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no such backend");
    let display = err.to_string();
    assert!(display.contains("backend_not_found"));
    assert!(display.contains("no such backend"));
}

#[test]
fn display_includes_context_keys() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("timeout_ms", 5000)
        .with_context("backend", "openai");
    let display = err.to_string();
    assert!(display.contains("timeout_ms"));
    assert!(display.contains("backend"));
    assert!(display.contains("5000"));
}

#[test]
fn debug_includes_source_when_present() {
    let src = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "staging").with_source(src);
    let dbg = format!("{err:?}");
    assert!(dbg.contains("source"));
    assert!(dbg.contains("file missing"));
}

#[test]
fn context_preserves_nested_json() {
    let nested = serde_json::json!({"a": 1, "b": [2, 3], "c": {"d": true}});
    let err = AbpError::new(ErrorCode::Internal, "nested").with_context("details", nested.clone());
    assert_eq!(err.context["details"], nested);
}

#[test]
fn error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "access denied");
    let s = info.to_string();
    assert_eq!(s, "[policy_denied] access denied");
}

#[test]
fn error_info_with_detail_preserves_values() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited")
        .with_detail("retry_after_ms", 2000)
        .with_detail("backend", "claude");
    assert_eq!(info.details["retry_after_ms"], serde_json::json!(2000));
    assert_eq!(info.details["backend"], serde_json::json!("claude"));
}

#[test]
fn error_info_serde_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timeout")
        .with_detail("ms", 30_000)
        .with_detail("backend", "openai");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back, info);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Error taxonomy completeness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn classifier_covers_every_error_code() {
    let classifier = ErrorClassifier::new();
    for &code in ALL_CODES {
        let cl = classifier.classify(&code);
        assert_eq!(cl.code, code, "classification code mismatch for {code:?}");
        // Ensure the fields are populated.
        assert!(
            !cl.recovery.description.is_empty(),
            "{code:?}: empty recovery description"
        );
    }
}

#[test]
fn retriable_codes_have_retriable_severity() {
    let classifier = ErrorClassifier::new();
    let retryable = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ];
    for code in retryable {
        let cl = classifier.classify(&code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Retriable,
            "{code:?}: expected Retriable severity"
        );
    }
}

#[test]
fn retriable_severity_implies_retry_action() {
    let classifier = ErrorClassifier::new();
    for &code in ALL_CODES {
        let cl = classifier.classify(&code);
        if cl.severity == ErrorSeverity::Retriable {
            assert_eq!(
                cl.recovery.action,
                RecoveryAction::Retry,
                "{code:?}: Retriable severity should have Retry action"
            );
            assert!(
                cl.recovery.delay_ms.is_some(),
                "{code:?}: Retriable codes should suggest a delay"
            );
        }
    }
}

#[test]
fn fatal_non_retryable_codes_have_fatal_severity() {
    let classifier = ErrorClassifier::new();
    let expect_fatal = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendAuthFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::Internal,
    ];
    for code in expect_fatal {
        let cl = classifier.classify(&code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Fatal,
            "{code:?}: expected Fatal severity"
        );
    }
}

#[test]
fn degraded_codes_have_degraded_severity() {
    let classifier = ErrorClassifier::new();
    let degraded = [
        ErrorCode::MappingLossyConversion,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in degraded {
        let cl = classifier.classify(&code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Degraded,
            "{code:?}: expected Degraded severity"
        );
    }
}

#[test]
fn classification_category_maps_auth_failed() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn classification_category_maps_rate_limited() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.category, ClassificationCategory::RateLimit);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.unwrap() >= 1000);
}

#[test]
fn classification_category_maps_model_not_found() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendModelNotFound);
    assert_eq!(cl.category, ClassificationCategory::ModelNotFound);
    assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
}

#[test]
fn classification_category_maps_timeout() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendTimeout);
    assert_eq!(cl.category, ClassificationCategory::TimeoutError);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
}

#[test]
fn classification_category_maps_policy_denied_to_content_filter() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::PolicyDenied);
    assert_eq!(cl.category, ClassificationCategory::ContentFilter);
}

#[test]
fn classification_category_maps_capability_unsupported() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::CapabilityUnsupported);
    assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn classification_category_maps_mapping_failures() {
    let classifier = ErrorClassifier::new();
    for code in [
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::IrLoweringFailed,
    ] {
        let cl = classifier.classify(&code);
        assert_eq!(
            cl.category,
            ClassificationCategory::MappingFailure,
            "{code:?} should map to MappingFailure"
        );
    }
}

#[test]
fn classification_category_maps_protocol_errors() {
    let classifier = ErrorClassifier::new();
    for code in [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::ContractVersionMismatch,
    ] {
        let cl = classifier.classify(&code);
        assert_eq!(
            cl.category,
            ClassificationCategory::ProtocolError,
            "{code:?} should map to ProtocolError"
        );
    }
}

#[test]
fn classification_category_maps_invalid_request() {
    let classifier = ErrorClassifier::new();
    for code in [
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        ErrorCode::PolicyInvalid,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::ConfigInvalid,
    ] {
        let cl = classifier.classify(&code);
        assert_eq!(
            cl.category,
            ClassificationCategory::InvalidRequest,
            "{code:?} should map to InvalidRequest"
        );
    }
}

#[test]
fn severity_consistency_with_retryability() {
    let classifier = ErrorClassifier::new();
    for &code in ALL_CODES {
        let cl = classifier.classify(&code);
        if code.is_retryable() {
            assert_eq!(
                cl.severity,
                ErrorSeverity::Retriable,
                "{code:?}: retryable code must have Retriable severity"
            );
        }
    }
}

#[test]
fn recovery_suggestion_serde_roundtrip() {
    let suggestion = RecoverySuggestion {
        action: RecoveryAction::Retry,
        description: "retry after delay".into(),
        delay_ms: Some(2000),
    };
    let json = serde_json::to_string(&suggestion).unwrap();
    let back: RecoverySuggestion = serde_json::from_str(&json).unwrap();
    assert_eq!(back, suggestion);
}

#[test]
fn error_classification_serde_roundtrip() {
    let classifier = ErrorClassifier::new();
    for &code in ALL_CODES {
        let cl = classifier.classify(&code);
        let json = serde_json::to_string(&cl).unwrap();
        let back: ErrorClassification = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back, cl,
            "ErrorClassification roundtrip failed for {code:?}"
        );
    }
}

#[test]
fn error_severity_serde_roundtrip() {
    let severities = [
        ErrorSeverity::Fatal,
        ErrorSeverity::Retriable,
        ErrorSeverity::Degraded,
        ErrorSeverity::Informational,
    ];
    for sev in severities {
        let json = serde_json::to_string(&sev).unwrap();
        let back: ErrorSeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sev);
    }
}

#[test]
fn classification_category_serde_roundtrip() {
    let cats = [
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
    for cat in cats {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ClassificationCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat);
    }
}

#[test]
fn recovery_action_serde_roundtrip() {
    let actions = [
        RecoveryAction::Retry,
        RecoveryAction::Fallback,
        RecoveryAction::ReduceContext,
        RecoveryAction::ChangeModel,
        RecoveryAction::ContactAdmin,
        RecoveryAction::None,
    ];
    for action in actions {
        let json = serde_json::to_string(&action).unwrap();
        let back: RecoveryAction = serde_json::from_str(&json).unwrap();
        assert_eq!(back, action);
    }
}

/// HTTP status code mapping for the daemon API — categories → expected status.
#[test]
fn classification_category_maps_to_expected_http_status() {
    fn expected_http_status(cat: ClassificationCategory) -> u16 {
        match cat {
            ClassificationCategory::Authentication => 401,
            ClassificationCategory::RateLimit => 429,
            ClassificationCategory::ModelNotFound => 404,
            ClassificationCategory::InvalidRequest => 400,
            ClassificationCategory::ContentFilter => 403,
            ClassificationCategory::ContextLength => 413,
            ClassificationCategory::ServerError => 500,
            ClassificationCategory::NetworkError => 502,
            ClassificationCategory::ProtocolError => 502,
            ClassificationCategory::CapabilityUnsupported => 501,
            ClassificationCategory::MappingFailure => 422,
            ClassificationCategory::TimeoutError => 504,
        }
    }

    let classifier = ErrorClassifier::new();
    for &code in ALL_CODES {
        let cl = classifier.classify(&code);
        let status = expected_http_status(cl.category);
        // Just verify the mapping doesn't panic and produces valid HTTP range.
        assert!(
            (400..=599).contains(&status),
            "{code:?} → category {:?} → status {status} outside 4xx/5xx",
            cl.category
        );
    }
}

#[test]
fn suggest_recovery_matches_classify_recovery() {
    let classifier = ErrorClassifier::new();
    for &code in ALL_CODES {
        let cl = classifier.classify(&code);
        let suggestion = classifier.suggest_recovery(&cl);
        assert_eq!(
            suggestion, cl.recovery,
            "{code:?}: suggest_recovery != classify.recovery"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. ShimError consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_shim_error_invalid_request_display() {
    let err = abp_shim_openai::ShimError::InvalidRequest("bad input".into());
    assert!(err.to_string().contains("invalid request"));
    assert!(err.to_string().contains("bad input"));
}

#[test]
fn openai_shim_error_internal_display() {
    let err = abp_shim_openai::ShimError::Internal("crash".into());
    assert!(err.to_string().contains("internal"));
    assert!(err.to_string().contains("crash"));
}

#[test]
fn openai_shim_error_serde_from() {
    let bad_json = "not json at all";
    let serde_err = serde_json::from_str::<serde_json::Value>(bad_json).unwrap_err();
    let shim_err: abp_shim_openai::ShimError = serde_err.into();
    assert!(matches!(shim_err, abp_shim_openai::ShimError::Serde(_)));
}

#[test]
fn claude_shim_error_invalid_request_display() {
    let err = abp_shim_claude::ShimError::InvalidRequest("missing model".into());
    assert!(err.to_string().contains("invalid request"));
    assert!(err.to_string().contains("missing model"));
}

#[test]
fn claude_shim_error_api_error_display() {
    let err = abp_shim_claude::ShimError::ApiError {
        error_type: "overloaded".into(),
        message: "too many requests".into(),
    };
    let display = err.to_string();
    assert!(display.contains("overloaded"));
    assert!(display.contains("too many requests"));
}

#[test]
fn claude_shim_error_internal_display() {
    let err = abp_shim_claude::ShimError::Internal("unexpected".into());
    assert!(err.to_string().contains("internal"));
}

#[test]
fn codex_shim_error_variants_consistent() {
    let _ = abp_shim_codex::ShimError::InvalidRequest("x".into());
    let _ = abp_shim_codex::ShimError::Internal("y".into());
    let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let _ = abp_shim_codex::ShimError::Serde(serde_err);
}

#[test]
fn copilot_shim_error_variants_consistent() {
    let _ = abp_shim_copilot::ShimError::InvalidRequest("x".into());
    let _ = abp_shim_copilot::ShimError::Internal("y".into());
    let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let _ = abp_shim_copilot::ShimError::Serde(serde_err);
}

#[test]
fn kimi_shim_error_variants_consistent() {
    let _ = abp_shim_kimi::ShimError::InvalidRequest("x".into());
    let _ = abp_shim_kimi::ShimError::Internal("y".into());
    let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let _ = abp_shim_kimi::ShimError::Serde(serde_err);
}

#[test]
fn gemini_error_variants_display() {
    let cases: Vec<(abp_shim_gemini::GeminiError, &str)> = vec![
        (
            abp_shim_gemini::GeminiError::RequestConversion("bad req".into()),
            "request conversion",
        ),
        (
            abp_shim_gemini::GeminiError::ResponseConversion("bad resp".into()),
            "response conversion",
        ),
        (
            abp_shim_gemini::GeminiError::BackendError("down".into()),
            "backend error",
        ),
    ];
    for (err, expected_substr) in &cases {
        assert!(
            err.to_string().contains(expected_substr),
            "GeminiError display missing '{expected_substr}': {}",
            err
        );
    }
}

#[test]
fn gemini_error_serde_from() {
    let serde_err = serde_json::from_str::<serde_json::Value>("!!!").unwrap_err();
    let gemini_err: abp_shim_gemini::GeminiError = serde_err.into();
    assert!(matches!(gemini_err, abp_shim_gemini::GeminiError::Serde(_)));
}

#[test]
fn all_shim_errors_implement_std_error() {
    fn assert_std_error<E: std::error::Error>() {}
    assert_std_error::<abp_shim_openai::ShimError>();
    assert_std_error::<abp_shim_claude::ShimError>();
    assert_std_error::<abp_shim_codex::ShimError>();
    assert_std_error::<abp_shim_copilot::ShimError>();
    assert_std_error::<abp_shim_kimi::ShimError>();
    assert_std_error::<abp_shim_gemini::GeminiError>();
}

#[test]
fn all_shim_errors_are_send_sync() {
    fn assert_send_sync<E: Send + Sync>() {}
    assert_send_sync::<abp_shim_openai::ShimError>();
    assert_send_sync::<abp_shim_claude::ShimError>();
    assert_send_sync::<abp_shim_codex::ShimError>();
    assert_send_sync::<abp_shim_copilot::ShimError>();
    assert_send_sync::<abp_shim_kimi::ShimError>();
    assert_send_sync::<abp_shim_gemini::GeminiError>();
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-cutting: AbpError is Send + Sync + Error
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn abp_error_is_send_sync() {
    fn assert_send_sync<E: Send + Sync>() {}
    assert_send_sync::<AbpError>();
}

#[test]
fn abp_error_implements_std_error() {
    fn assert_error<E: std::error::Error>() {}
    assert_error::<AbpError>();
}

// ═══════════════════════════════════════════════════════════════════════════
// ErrorCategory serde + display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_category_serde_all_variants() {
    for &cat in ALL_CATEGORIES {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat, "category serde roundtrip failed for {cat:?}");
    }
}

#[test]
fn error_category_display_matches_serde() {
    for &cat in ALL_CATEGORIES {
        let display = cat.to_string();
        let serde = serde_json::to_string(&cat)
            .unwrap()
            .trim_matches('"')
            .to_owned();
        assert_eq!(
            display, serde,
            "Display and serde mismatch for {cat:?}: {display} vs {serde}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge cases and regression guards
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_info_display_format_bracket_code() {
    for &code in ALL_CODES {
        let info = ErrorInfo::new(code, "test message");
        let s = info.to_string();
        let expected = format!("[{}] test message", code.as_str());
        assert_eq!(s, expected, "ErrorInfo display mismatch for {code:?}");
    }
}

#[test]
fn abp_error_to_info_preserves_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "slow")
        .with_context("backend", "openai")
        .with_context("ms", 30000);
    let info = err.to_info();
    assert_eq!(info.code, ErrorCode::BackendTimeout);
    assert_eq!(info.message, "slow");
    assert_eq!(info.details.len(), 2);
    assert_eq!(info.details["backend"], serde_json::json!("openai"));
    assert_eq!(info.details["ms"], serde_json::json!(30000));
}

#[test]
fn empty_context_does_not_appear_in_display() {
    let err = AbpError::new(ErrorCode::Internal, "boom");
    let display = err.to_string();
    assert_eq!(display, "[internal] boom");
}

#[test]
fn multiple_contexts_deterministic_order() {
    let err = AbpError::new(ErrorCode::Internal, "test")
        .with_context("z_key", "last")
        .with_context("a_key", "first");
    let display = err.to_string();
    // BTreeMap ensures a_key comes before z_key.
    let a_pos = display.find("a_key").unwrap();
    let z_pos = display.find("z_key").unwrap();
    assert!(a_pos < z_pos, "context should be in alphabetical order");
}
