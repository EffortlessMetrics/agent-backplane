//! Classification tests for the ABP error taxonomy.
//!
//! Validates severity mapping, category assignment, recovery suggestions,
//! serialisation round-trips, and exhaustive coverage of all error codes.

use abp_error_taxonomy::ErrorCode;
use abp_error_taxonomy::classification::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
    RecoverySuggestion,
};

// =========================================================================
// Helpers
// =========================================================================

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

fn classifier() -> ErrorClassifier {
    ErrorClassifier::new()
}

// =========================================================================
// 1. Exhaustive coverage
// =========================================================================

#[test]
fn all_error_codes_are_classifiable() {
    let c = classifier();
    for code in ALL_CODES {
        let cl = c.classify(code);
        assert_eq!(&cl.code, code, "classification should echo back the code");
    }
}

#[test]
fn every_classification_has_non_empty_recovery_description() {
    let c = classifier();
    for code in ALL_CODES {
        let cl = c.classify(code);
        assert!(
            !cl.recovery.description.is_empty(),
            "{:?} has empty recovery description",
            code
        );
    }
}

// =========================================================================
// 2. Severity tests
// =========================================================================

#[test]
fn backend_rate_limited_is_retriable() {
    let cl = classifier().classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
}

#[test]
fn backend_timeout_is_retriable() {
    let cl = classifier().classify(&ErrorCode::BackendTimeout);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
}

#[test]
fn backend_unavailable_is_retriable() {
    let cl = classifier().classify(&ErrorCode::BackendUnavailable);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
}

#[test]
fn backend_crashed_is_retriable() {
    let cl = classifier().classify(&ErrorCode::BackendCrashed);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
}

#[test]
fn backend_auth_failed_is_fatal() {
    let cl = classifier().classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
}

#[test]
fn protocol_invalid_envelope_is_fatal() {
    let cl = classifier().classify(&ErrorCode::ProtocolInvalidEnvelope);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
}

#[test]
fn mapping_lossy_conversion_is_degraded() {
    let cl = classifier().classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
}

#[test]
fn capability_emulation_failed_is_degraded() {
    let cl = classifier().classify(&ErrorCode::CapabilityEmulationFailed);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
}

#[test]
fn internal_error_is_fatal() {
    let cl = classifier().classify(&ErrorCode::Internal);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
}

// =========================================================================
// 3. Category tests
// =========================================================================

#[test]
fn backend_rate_limited_is_rate_limit_category() {
    let cl = classifier().classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.category, ClassificationCategory::RateLimit);
}

#[test]
fn backend_timeout_is_timeout_category() {
    let cl = classifier().classify(&ErrorCode::BackendTimeout);
    assert_eq!(cl.category, ClassificationCategory::TimeoutError);
}

#[test]
fn backend_auth_failed_is_authentication_category() {
    let cl = classifier().classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
}

#[test]
fn backend_model_not_found_is_model_not_found_category() {
    let cl = classifier().classify(&ErrorCode::BackendModelNotFound);
    assert_eq!(cl.category, ClassificationCategory::ModelNotFound);
}

#[test]
fn protocol_handshake_failed_is_protocol_error_category() {
    let cl = classifier().classify(&ErrorCode::ProtocolHandshakeFailed);
    assert_eq!(cl.category, ClassificationCategory::ProtocolError);
}

#[test]
fn policy_denied_is_content_filter_category() {
    let cl = classifier().classify(&ErrorCode::PolicyDenied);
    assert_eq!(cl.category, ClassificationCategory::ContentFilter);
}

#[test]
fn mapping_dialect_mismatch_is_mapping_failure_category() {
    let cl = classifier().classify(&ErrorCode::MappingDialectMismatch);
    assert_eq!(cl.category, ClassificationCategory::MappingFailure);
}

#[test]
fn capability_unsupported_is_capability_unsupported_category() {
    let cl = classifier().classify(&ErrorCode::CapabilityUnsupported);
    assert_eq!(cl.category, ClassificationCategory::CapabilityUnsupported);
}

#[test]
fn config_invalid_is_invalid_request_category() {
    let cl = classifier().classify(&ErrorCode::ConfigInvalid);
    assert_eq!(cl.category, ClassificationCategory::InvalidRequest);
}

// =========================================================================
// 4. Recovery suggestion tests
// =========================================================================

#[test]
fn rate_limited_recovery_has_retry_with_delay() {
    let cl = classifier().classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.unwrap() > 0);
}

#[test]
fn auth_failed_recovery_is_contact_admin() {
    let cl = classifier().classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
    assert!(cl.recovery.delay_ms.is_none());
}

#[test]
fn model_not_found_recovery_is_change_model() {
    let cl = classifier().classify(&ErrorCode::BackendModelNotFound);
    assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
}

#[test]
fn capability_unsupported_recovery_is_fallback() {
    let cl = classifier().classify(&ErrorCode::CapabilityUnsupported);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn suggest_recovery_matches_classify_recovery() {
    let c = classifier();
    for code in ALL_CODES {
        let cl = c.classify(code);
        let suggestion = c.suggest_recovery(&cl);
        assert_eq!(
            cl.recovery, suggestion,
            "{:?} suggest_recovery differs from classify",
            code
        );
    }
}

// =========================================================================
// 5. Serde round-trip tests
// =========================================================================

#[test]
fn error_severity_serde_round_trip() {
    for severity in &[
        ErrorSeverity::Fatal,
        ErrorSeverity::Retriable,
        ErrorSeverity::Degraded,
        ErrorSeverity::Informational,
    ] {
        let json = serde_json::to_string(severity).unwrap();
        let back: ErrorSeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(*severity, back);
    }
}

#[test]
fn classification_category_serde_round_trip() {
    let categories = [
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
    for cat in &categories {
        let json = serde_json::to_string(cat).unwrap();
        let back: ClassificationCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(*cat, back);
    }
}

#[test]
fn recovery_action_serde_round_trip() {
    for action in &[
        RecoveryAction::Retry,
        RecoveryAction::Fallback,
        RecoveryAction::ReduceContext,
        RecoveryAction::ChangeModel,
        RecoveryAction::ContactAdmin,
        RecoveryAction::None,
    ] {
        let json = serde_json::to_string(action).unwrap();
        let back: RecoveryAction = serde_json::from_str(&json).unwrap();
        assert_eq!(*action, back);
    }
}

#[test]
fn error_classification_serde_round_trip() {
    let c = classifier();
    for code in ALL_CODES {
        let cl = c.classify(code);
        let json = serde_json::to_string(&cl).unwrap();
        let back: ErrorClassification = serde_json::from_str(&json).unwrap();
        assert_eq!(cl, back, "round-trip failed for {:?}", code);
    }
}

#[test]
fn recovery_suggestion_serde_round_trip() {
    let suggestion = RecoverySuggestion {
        action: RecoveryAction::Retry,
        description: "test suggestion".into(),
        delay_ms: Some(500),
    };
    let json = serde_json::to_string(&suggestion).unwrap();
    let back: RecoverySuggestion = serde_json::from_str(&json).unwrap();
    assert_eq!(suggestion, back);
}

// =========================================================================
// 6. Serde snake_case naming
// =========================================================================

#[test]
fn severity_serialises_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&ErrorSeverity::Retriable).unwrap(),
        "\"retriable\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorSeverity::Fatal).unwrap(),
        "\"fatal\""
    );
}

#[test]
fn classification_category_serialises_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&ClassificationCategory::RateLimit).unwrap(),
        "\"rate_limit\""
    );
    assert_eq!(
        serde_json::to_string(&ClassificationCategory::ModelNotFound).unwrap(),
        "\"model_not_found\""
    );
}

#[test]
fn recovery_action_serialises_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&RecoveryAction::ReduceContext).unwrap(),
        "\"reduce_context\""
    );
    assert_eq!(
        serde_json::to_string(&RecoveryAction::ChangeModel).unwrap(),
        "\"change_model\""
    );
    assert_eq!(
        serde_json::to_string(&RecoveryAction::ContactAdmin).unwrap(),
        "\"contact_admin\""
    );
}

// =========================================================================
// 7. Clone / Debug / Default
// =========================================================================

#[test]
fn classifier_implements_debug_clone_default() {
    let c1 = ErrorClassifier;
    let c2 = c1.clone();
    let _ = format!("{:?}", c2);
}

#[test]
fn classification_implements_debug_clone() {
    let cl = classifier().classify(&ErrorCode::Internal);
    let cl2 = cl.clone();
    let _ = format!("{:?}", cl2);
}
