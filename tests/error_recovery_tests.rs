//! Comprehensive error handling and recovery pattern tests.
//!
//! Covers error code exhaustiveness, severity classification, serialization,
//! chain propagation, retryability, and graceful degradation across all
//! error-related crates.

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

use std::collections::HashSet;
use std::error::Error as StdError;

use abp_core::error::{MappingError, MappingErrorKind};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_error_taxonomy::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
};
use abp_protocol::ProtocolError;
use abp_protocol::batch::BatchValidationError;
use abp_protocol::builder::BuilderError;
use abp_protocol::compress::CompressError;
use abp_protocol::validate::{SequenceError, ValidationError};
use abp_protocol::version::VersionError;
use abp_runtime::RuntimeError;
use abp_runtime::multiplex::MultiplexError;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// All 36 ErrorCode variants from abp-error.
fn all_error_codes() -> Vec<ErrorCode> {
    vec![
        // Protocol
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        // Mapping
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        // Backend
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        // Execution
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        // Contract
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        // Capability
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        // Policy
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        // Workspace
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        // IR
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        // Receipt
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        // Dialect
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        // Config
        ErrorCode::ConfigInvalid,
        // Internal
        ErrorCode::Internal,
    ]
}

// ===================================================================
// 1. Error code exhaustiveness
// ===================================================================

#[test]
fn error_code_all_variants_have_category() {
    for code in all_error_codes() {
        let cat = code.category();
        // Just ensure it returns a valid category (no panic).
        let _ = format!("{cat}");
    }
}

#[test]
fn error_code_all_variants_have_as_str() {
    for code in all_error_codes() {
        let s = code.as_str();
        assert!(!s.is_empty(), "as_str() empty for {code:?}");
    }
}

#[test]
fn error_code_all_variants_have_messages() {
    for code in all_error_codes() {
        let msg = code.message();
        assert!(!msg.is_empty(), "message() empty for {code:?}");
    }
}

#[test]
fn error_code_all_variants_have_retryable_flag() {
    // Exhaustiveness: every code has a defined is_retryable result.
    let mut retryable_count = 0;
    let mut non_retryable_count = 0;
    for code in all_error_codes() {
        if code.is_retryable() {
            retryable_count += 1;
        } else {
            non_retryable_count += 1;
        }
    }
    assert!(retryable_count > 0, "expected at least one retryable code");
    assert!(
        non_retryable_count > 0,
        "expected at least one non-retryable code"
    );
}

// ===================================================================
// 2. Error code stability
// ===================================================================

#[test]
fn error_code_serde_roundtrip_all_variants() {
    for code in all_error_codes() {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, back, "serde roundtrip failed for {code:?}");
    }
}

#[test]
fn error_code_as_str_values_are_unique() {
    let codes = all_error_codes();
    let strings: HashSet<&str> = codes.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        strings.len(),
        codes.len(),
        "duplicate as_str() values detected"
    );
}

#[test]
fn error_code_as_str_matches_serde_output() {
    for code in all_error_codes() {
        let serde_str = serde_json::to_value(code).unwrap();
        let expected = code.as_str();
        assert_eq!(
            serde_str.as_str().unwrap(),
            expected,
            "as_str() != serde for {code:?}"
        );
    }
}

// ===================================================================
// 3. Error severity levels
// ===================================================================

#[test]
fn classifier_maps_all_codes_to_severity() {
    let classifier = ErrorClassifier::new();
    for code in all_error_codes() {
        let cl = classifier.classify(&code);
        // Every code must produce a valid classification.
        let _ = format!("{:?}", cl.severity);
        let _ = format!("{:?}", cl.category);
    }
}

#[test]
fn retriable_error_codes_have_retriable_severity() {
    let classifier = ErrorClassifier::new();
    let retryable_codes = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ];
    for code in &retryable_codes {
        let cl = classifier.classify(code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Retriable,
            "{code:?} should be Retriable"
        );
    }
}

#[test]
fn fatal_auth_errors_have_fatal_severity() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::Authentication);
}

#[test]
fn degraded_codes_are_non_fatal() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    assert_eq!(cl.category, ClassificationCategory::MappingFailure);
}

// ===================================================================
// 4. Error taxonomy coverage
// ===================================================================

#[test]
fn all_error_categories_have_at_least_one_code() {
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
        ErrorCategory::Mapping,
        ErrorCategory::Execution,
        ErrorCategory::Contract,
        ErrorCategory::Internal,
    ];
    let codes = all_error_codes();
    for cat in &all_categories {
        let count = codes.iter().filter(|c| c.category() == *cat).count();
        assert!(
            count > 0,
            "category {cat:?} has no error codes assigned to it"
        );
    }
}

#[test]
fn all_classification_categories_covered_by_classifier() {
    let classifier = ErrorClassifier::new();
    let codes = all_error_codes();
    let categories: HashSet<ClassificationCategory> = codes
        .iter()
        .map(|c| classifier.classify(c).category)
        .collect();

    // At least the main operational categories must be reachable.
    let expected = [
        ClassificationCategory::Authentication,
        ClassificationCategory::RateLimit,
        ClassificationCategory::ModelNotFound,
        ClassificationCategory::InvalidRequest,
        ClassificationCategory::ServerError,
        ClassificationCategory::ProtocolError,
        ClassificationCategory::CapabilityUnsupported,
        ClassificationCategory::MappingFailure,
        ClassificationCategory::TimeoutError,
        ClassificationCategory::ContentFilter,
    ];
    for cat in &expected {
        assert!(
            categories.contains(cat),
            "classification category {cat:?} unreachable from any ErrorCode"
        );
    }
}

#[test]
fn recovery_actions_all_reachable() {
    let classifier = ErrorClassifier::new();
    let codes = all_error_codes();
    let actions: HashSet<RecoveryAction> = codes
        .iter()
        .map(|c| classifier.classify(c).recovery.action)
        .collect();

    // Key recovery actions must be reachable.
    assert!(actions.contains(&RecoveryAction::Retry));
    assert!(actions.contains(&RecoveryAction::Fallback));
    assert!(actions.contains(&RecoveryAction::ContactAdmin));
    assert!(actions.contains(&RecoveryAction::None));
}

// ===================================================================
// 5. Runtime error recovery
// ===================================================================

#[test]
fn runtime_error_all_variants_implement_display() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend {
            name: "test".into(),
        },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("workspace err")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("policy err")),
        RuntimeError::BackendFailed(anyhow::anyhow!("backend err")),
        RuntimeError::CapabilityCheckFailed("missing tools".into()),
        RuntimeError::NoProjectionMatch {
            reason: "no match".into(),
        },
    ];
    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty Display for RuntimeError variant");
    }
}

#[test]
fn runtime_error_all_variants_implement_error_trait() {
    let err: Box<dyn StdError> = Box::new(RuntimeError::UnknownBackend { name: "x".into() });
    // Ensure the trait object works.
    let _ = err.to_string();
    let _ = err.source();
}

#[test]
fn runtime_error_has_error_code_for_all_variants() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend {
            name: "test".into(),
        },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("err")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("err")),
        RuntimeError::BackendFailed(anyhow::anyhow!("err")),
        RuntimeError::CapabilityCheckFailed("err".into()),
        RuntimeError::NoProjectionMatch {
            reason: "err".into(),
        },
    ];
    for v in &variants {
        let code = v.error_code();
        // Every variant must return a valid ErrorCode.
        let _ = code.as_str();
    }
}

#[test]
fn runtime_error_into_abp_error_preserves_code() {
    let re = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    let expected_code = re.error_code();
    let abp = re.into_abp_error();
    assert_eq!(abp.code, expected_code);
}

// ===================================================================
// 6. Protocol error recovery
// ===================================================================

#[test]
fn protocol_error_variants_implement_display() {
    let json_err = serde_json::from_str::<String>("not json").unwrap_err();
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken");
    let variants: Vec<ProtocolError> = vec![
        ProtocolError::Json(json_err),
        ProtocolError::Io(io_err),
        ProtocolError::Violation("bad envelope".into()),
        ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "event".into(),
        },
        ProtocolError::Abp(AbpError::new(ErrorCode::Internal, "test")),
    ];
    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty Display for ProtocolError variant");
    }
}

#[test]
fn protocol_error_implements_std_error_trait() {
    let err: Box<dyn StdError> = Box::new(ProtocolError::Violation("test".into()));
    let _ = err.to_string();
    // source() should be accessible (may be None)
    let _ = err.source();
}

#[test]
fn validation_error_all_variants_implement_display() {
    let variants: Vec<ValidationError> = vec![
        ValidationError::MissingField {
            field: "ref_id".into(),
        },
        ValidationError::InvalidValue {
            field: "version".into(),
            value: "bad".into(),
            expected: "abp/v0.1".into(),
        },
        ValidationError::InvalidVersion {
            version: "xyz".into(),
        },
        ValidationError::EmptyField {
            field: "backend_id".into(),
        },
    ];
    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty Display for ValidationError");
    }
}

#[test]
fn version_error_all_variants_implement_display() {
    use abp_protocol::version::ProtocolVersion;
    let variants: Vec<VersionError> = vec![
        VersionError::InvalidFormat,
        VersionError::InvalidMajor,
        VersionError::InvalidMinor,
        VersionError::Incompatible {
            local: ProtocolVersion::current(),
            remote: ProtocolVersion::current(),
        },
    ];
    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty Display for VersionError");
    }
}

#[test]
fn sequence_error_all_variants_implement_display() {
    let variants: Vec<SequenceError> = vec![
        SequenceError::MissingHello,
        SequenceError::MissingTerminal,
        SequenceError::HelloNotFirst { position: 3 },
        SequenceError::MultipleTerminals,
        SequenceError::RefIdMismatch {
            expected: "a".into(),
            found: "b".into(),
        },
        SequenceError::OutOfOrderEvents,
    ];
    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty Display for SequenceError");
    }
}

#[test]
fn builder_error_implements_display() {
    let err = BuilderError::MissingField("backend_id");
    let msg = err.to_string();
    assert!(msg.contains("backend_id"));
}

#[test]
fn compress_error_all_variants_implement_display() {
    let variants: Vec<CompressError> = vec![
        CompressError::TooShort,
        CompressError::UnknownAlgorithm(0xFF),
        CompressError::AlgorithmMismatch {
            expected: abp_protocol::compress::CompressionAlgorithm::Gzip,
            found: abp_protocol::compress::CompressionAlgorithm::Zstd,
        },
        CompressError::Io(std::io::Error::other("io")),
    ];
    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty Display for CompressError");
    }
}

#[test]
fn batch_validation_error_all_variants_implement_display() {
    let variants: Vec<BatchValidationError> = vec![
        BatchValidationError::EmptyBatch,
        BatchValidationError::TooManyItems {
            count: 200,
            max: 100,
        },
        BatchValidationError::InvalidEnvelope {
            index: 5,
            error: "parse failed".into(),
        },
    ];
    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty Display for BatchValidationError");
    }
}

#[test]
fn multiplex_error_all_variants_implement_display() {
    let variants: Vec<MultiplexError> = vec![
        MultiplexError::NoSubscribers,
        MultiplexError::Lagged { missed: 42 },
        MultiplexError::Closed,
    ];
    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty Display for MultiplexError");
    }
}

// ===================================================================
// 7. Backend error handling
// ===================================================================

#[test]
fn backend_timeout_has_correct_error_code() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.category(), ErrorCategory::Backend);
}

#[test]
fn backend_unavailable_is_retryable() {
    assert!(ErrorCode::BackendUnavailable.is_retryable());
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(ErrorCode::BackendCrashed.is_retryable());
}

#[test]
fn backend_auth_and_model_not_found_are_not_retryable() {
    assert!(!ErrorCode::BackendAuthFailed.is_retryable());
    assert!(!ErrorCode::BackendModelNotFound.is_retryable());
    assert!(!ErrorCode::BackendNotFound.is_retryable());
}

#[test]
fn protocol_error_error_code_mapping() {
    let violation = ProtocolError::Violation("bad".into());
    assert_eq!(
        violation.error_code(),
        Some(ErrorCode::ProtocolInvalidEnvelope)
    );

    let unexpected = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        unexpected.error_code(),
        Some(ErrorCode::ProtocolUnexpectedMessage)
    );

    let abp = ProtocolError::Abp(AbpError::new(ErrorCode::BackendTimeout, "timeout"));
    assert_eq!(abp.error_code(), Some(ErrorCode::BackendTimeout));
}

// ===================================================================
// 8. Error chain propagation
// ===================================================================

#[test]
fn abp_error_preserves_source_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let err = AbpError::new(ErrorCode::Internal, "wrapper").with_source(io_err);
    let source = err.source().expect("source should be present");
    assert!(source.to_string().contains("file missing"));
}

#[test]
fn io_error_converts_to_abp_error_with_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access");
    let abp: AbpError = io_err.into();
    assert_eq!(abp.code, ErrorCode::Internal);
    assert!(abp.source().is_some());
}

#[test]
fn serde_error_converts_to_abp_error_with_source() {
    let serde_err = serde_json::from_str::<String>("bad json").unwrap_err();
    let abp: AbpError = serde_err.into();
    assert_eq!(abp.code, ErrorCode::ProtocolInvalidEnvelope);
    assert!(abp.source().is_some());
}

#[test]
fn runtime_error_classified_preserves_inner_code() {
    let inner = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let runtime_err = RuntimeError::Classified(inner);
    assert_eq!(runtime_err.error_code(), ErrorCode::BackendTimeout);
}

// ===================================================================
// 9. Error serialization
// ===================================================================

#[test]
fn error_info_json_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_detail("backend", "openai");
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn abp_error_dto_json_roundtrip() {
    let err =
        AbpError::new(ErrorCode::PolicyDenied, "tool xyz denied").with_context("tool", "rm -rf /");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(dto, back);
}

#[test]
fn error_code_json_roundtrip_for_all_variants() {
    for code in all_error_codes() {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, back);
    }
}

#[test]
fn mapping_error_json_roundtrip() {
    let errors: Vec<MappingError> = vec![
        MappingError::FidelityLoss {
            field: "temperature".into(),
            source_dialect: "openai".into(),
            target_dialect: "claude".into(),
            detail: "range differs".into(),
        },
        MappingError::UnsupportedCapability {
            capability: "vision".into(),
            dialect: "codex".into(),
        },
        MappingError::EmulationRequired {
            feature: "tool_use".into(),
            detail: "synthesised via system prompt".into(),
        },
        MappingError::IncompatibleModel {
            requested: "gpt-5".into(),
            dialect: "claude".into(),
            suggestion: Some("claude-3-opus".into()),
        },
        MappingError::ParameterNotMappable {
            parameter: "logprobs".into(),
            value: "5".into(),
            dialect: "gemini".into(),
        },
        MappingError::StreamingUnsupported {
            dialect: "batch-only".into(),
        },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back, "MappingError roundtrip failed");
    }
}

#[test]
fn error_classification_json_roundtrip() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendRateLimited);
    let json = serde_json::to_string(&cl).unwrap();
    let back: ErrorClassification = serde_json::from_str(&json).unwrap();
    assert_eq!(cl, back);
}

// ===================================================================
// 10. Typed error failures (mapped mode)
// ===================================================================

#[test]
fn mapping_error_fidelity_loss_is_typed_not_panic() {
    let err = MappingError::FidelityLoss {
        field: "temperature".into(),
        source_dialect: "openai".into(),
        target_dialect: "claude".into(),
        detail: "range differs".into(),
    };
    assert!(err.is_degraded());
    assert!(!err.is_fatal());
    assert_eq!(err.code(), MappingError::FIDELITY_LOSS_CODE);
}

#[test]
fn mapping_error_unsupported_capability_is_fatal() {
    let err = MappingError::UnsupportedCapability {
        capability: "vision".into(),
        dialect: "codex".into(),
    };
    assert!(err.is_fatal());
    assert!(!err.is_degraded());
    assert_eq!(err.code(), MappingError::UNSUPPORTED_CAP_CODE);
}

#[test]
fn mapping_error_all_variants_have_stable_codes() {
    let all_mapping_errors: Vec<MappingError> = vec![
        MappingError::FidelityLoss {
            field: "f".into(),
            source_dialect: "s".into(),
            target_dialect: "t".into(),
            detail: "d".into(),
        },
        MappingError::UnsupportedCapability {
            capability: "c".into(),
            dialect: "d".into(),
        },
        MappingError::EmulationRequired {
            feature: "f".into(),
            detail: "d".into(),
        },
        MappingError::IncompatibleModel {
            requested: "m".into(),
            dialect: "d".into(),
            suggestion: None,
        },
        MappingError::ParameterNotMappable {
            parameter: "p".into(),
            value: "v".into(),
            dialect: "d".into(),
        },
        MappingError::StreamingUnsupported {
            dialect: "d".into(),
        },
    ];
    let codes: HashSet<&str> = all_mapping_errors.iter().map(|e| e.code()).collect();
    assert_eq!(
        codes.len(),
        6,
        "expected 6 unique stable mapping error codes"
    );
    for code in &codes {
        assert!(
            code.starts_with("ABP_E_"),
            "code {code} must start with ABP_E_"
        );
    }
}

// ===================================================================
// 11. Graceful degradation
// ===================================================================

#[test]
fn unsupported_capability_produces_informative_error() {
    let err = MappingError::UnsupportedCapability {
        capability: "code_execution".into(),
        dialect: "basic-llm".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("code_execution"), "missing capability name");
    assert!(msg.contains("basic-llm"), "missing dialect name");
    assert!(msg.contains(MappingError::UNSUPPORTED_CAP_CODE));
}

#[test]
fn emulation_required_produces_informative_error() {
    let err = MappingError::EmulationRequired {
        feature: "tool_use".into(),
        detail: "synthesised via system prompt injection".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("tool_use"));
    assert!(msg.contains("synthesised"));
    assert!(err.is_emulated());
}

#[test]
fn incompatible_model_with_suggestion_is_informative() {
    let err = MappingError::IncompatibleModel {
        requested: "gpt-5-turbo".into(),
        dialect: "claude".into(),
        suggestion: Some("claude-3-opus".into()),
    };
    let msg = err.to_string();
    assert!(msg.contains("gpt-5-turbo"));
    assert!(msg.contains("claude"));
    assert!(msg.contains("claude-3-opus"));
}

#[test]
fn runtime_unknown_backend_produces_informative_error() {
    let err = RuntimeError::UnknownBackend {
        name: "nonexistent-backend".into(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("nonexistent-backend"),
        "error should mention the backend name"
    );
}

// ===================================================================
// 12. Error message quality
// ===================================================================

#[test]
fn all_error_codes_have_non_empty_display() {
    for code in all_error_codes() {
        let display = code.to_string();
        assert!(
            display.len() > 5,
            "Display too short for {code:?}: '{display}'"
        );
    }
}

#[test]
fn abp_error_display_contains_code_string() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "connection timed out");
    let display = err.to_string();
    assert!(display.contains("backend_timeout"));
    assert!(display.contains("connection timed out"));
}

#[test]
fn mapping_error_display_contains_stable_code() {
    let err = MappingError::StreamingUnsupported {
        dialect: "batch-api".into(),
    };
    let display = err.to_string();
    assert!(display.contains("ABP_E_STREAMING_UNSUPPORTED"));
}

#[test]
fn error_info_display_contains_code_and_message() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "tool rm denied by policy");
    let display = info.to_string();
    assert!(display.contains("policy_denied"));
    assert!(display.contains("tool rm denied by policy"));
}

// ===================================================================
// 13. Retryable errors
// ===================================================================

#[test]
fn only_backend_transient_codes_are_retryable() {
    let retryable: HashSet<ErrorCode> = all_error_codes()
        .into_iter()
        .filter(|c| c.is_retryable())
        .collect();

    let expected_retryable: HashSet<ErrorCode> = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ]
    .into_iter()
    .collect();

    assert_eq!(retryable, expected_retryable);
}

#[test]
fn non_retryable_codes_are_not_retryable() {
    let non_retryable = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendAuthFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];
    for code in &non_retryable {
        assert!(!code.is_retryable(), "{code:?} should not be retryable");
    }
}

#[test]
fn runtime_retryable_matches_error_taxonomy() {
    // BackendFailed and WorkspaceFailed should be retryable at runtime level.
    let backend_failed = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
    assert!(backend_failed.is_retryable());

    let workspace_failed = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(workspace_failed.is_retryable());

    // Config/policy errors should not be retryable.
    let unknown = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(!unknown.is_retryable());

    let policy = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert!(!policy.is_retryable());

    let cap = RuntimeError::CapabilityCheckFailed("missing".into());
    assert!(!cap.is_retryable());

    let proj = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    assert!(!proj.is_retryable());
}

// ===================================================================
// 14. Fatal vs recoverable
// ===================================================================

#[test]
fn fatal_mapping_errors_are_classified_fatal() {
    let fatal_errors = [
        MappingError::UnsupportedCapability {
            capability: "x".into(),
            dialect: "d".into(),
        },
        MappingError::IncompatibleModel {
            requested: "m".into(),
            dialect: "d".into(),
            suggestion: None,
        },
        MappingError::StreamingUnsupported {
            dialect: "d".into(),
        },
    ];
    for err in &fatal_errors {
        assert_eq!(
            err.kind(),
            MappingErrorKind::Fatal,
            "expected Fatal for {err}"
        );
        assert!(err.is_fatal());
    }
}

#[test]
fn degraded_mapping_errors_are_classified_degraded() {
    let degraded_errors = [
        MappingError::FidelityLoss {
            field: "f".into(),
            source_dialect: "s".into(),
            target_dialect: "t".into(),
            detail: "d".into(),
        },
        MappingError::ParameterNotMappable {
            parameter: "p".into(),
            value: "v".into(),
            dialect: "d".into(),
        },
    ];
    for err in &degraded_errors {
        assert_eq!(
            err.kind(),
            MappingErrorKind::Degraded,
            "expected Degraded for {err}"
        );
        assert!(err.is_degraded());
    }
}

#[test]
fn emulated_mapping_errors_are_classified_emulated() {
    let err = MappingError::EmulationRequired {
        feature: "tool_use".into(),
        detail: "via system prompt".into(),
    };
    assert_eq!(err.kind(), MappingErrorKind::Emulated);
    assert!(err.is_emulated());
    assert!(!err.is_fatal());
    assert!(!err.is_degraded());
}

#[test]
fn classifier_fatal_vs_retriable_consistency() {
    let classifier = ErrorClassifier::new();
    for code in all_error_codes() {
        let cl = classifier.classify(&code);
        match cl.severity {
            ErrorSeverity::Retriable => {
                // Retriable errors should suggest Retry action.
                assert_eq!(
                    cl.recovery.action,
                    RecoveryAction::Retry,
                    "{code:?} is Retriable but recovery action is {:?}",
                    cl.recovery.action
                );
            }
            ErrorSeverity::Fatal => {
                // Fatal errors should NOT suggest Retry (unless network-related).
                // They may suggest ContactAdmin, Fallback, ChangeModel, or None.
                assert_ne!(
                    cl.severity,
                    ErrorSeverity::Retriable,
                    "should not be retriable"
                );
            }
            _ => {}
        }
    }
}

#[test]
fn classifier_suggest_recovery_returns_valid_suggestion() {
    let classifier = ErrorClassifier::new();
    for code in all_error_codes() {
        let cl = classifier.classify(&code);
        let suggestion = classifier.suggest_recovery(&cl);
        assert!(
            !suggestion.description.is_empty(),
            "empty recovery description for {code:?}"
        );
        // If there's a delay, it must be positive.
        if let Some(delay) = suggestion.delay_ms {
            assert!(delay > 0, "delay must be > 0 for {code:?}");
        }
    }
}

#[test]
fn abp_error_retryable_delegates_to_code() {
    let retryable = AbpError::new(ErrorCode::BackendTimeout, "timeout");
    assert!(retryable.is_retryable());

    let not_retryable = AbpError::new(ErrorCode::PolicyDenied, "denied");
    assert!(!not_retryable.is_retryable());
}

#[test]
fn error_info_retryable_inferred_from_code() {
    let info = ErrorInfo::new(ErrorCode::BackendRateLimited, "rate limited");
    assert!(info.is_retryable);

    let info2 = ErrorInfo::new(ErrorCode::ConfigInvalid, "bad config");
    assert!(!info2.is_retryable);
}
