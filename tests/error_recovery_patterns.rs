#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Comprehensive error recovery pattern tests exercising the full ABP error
//! taxonomy through realistic failure scenarios.
//!
//! Covers: backend failures, protocol errors, mapping errors, workspace errors,
//! policy violations, error chaining, error recovery, serialization roundtrips,
//! HTTP status mapping, and cross-SDK error mapping.

use std::collections::HashSet;
use std::error::Error as StdError;

use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use abp_error_taxonomy::{
    ClassificationCategory, ErrorClassifier, ErrorSeverity, RecoveryAction, RecoverySuggestion,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// All 36 ErrorCode variants.
fn all_error_codes() -> Vec<ErrorCode> {
    vec![
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
    ]
}

/// Map an ErrorCode to the HTTP status code that a gateway would return.
fn suggested_http_status(code: &ErrorCode) -> u16 {
    match code {
        // 400 Bad Request — client sent something malformed / invalid
        ErrorCode::ProtocolInvalidEnvelope
        | ErrorCode::ProtocolMissingRefId
        | ErrorCode::ContractSchemaViolation
        | ErrorCode::PolicyInvalid
        | ErrorCode::IrInvalid
        | ErrorCode::ConfigInvalid => 400,

        // 401 Unauthorized
        ErrorCode::BackendAuthFailed => 401,

        // 403 Forbidden — policy denied
        ErrorCode::PolicyDenied | ErrorCode::ExecutionPermissionDenied => 403,

        // 404 Not Found
        ErrorCode::BackendNotFound | ErrorCode::BackendModelNotFound => 404,

        // 409 Conflict — version / contract mismatch
        ErrorCode::ProtocolVersionMismatch
        | ErrorCode::ContractVersionMismatch
        | ErrorCode::MappingDialectMismatch => 409,

        // 422 Unprocessable — structurally valid but semantically wrong
        ErrorCode::MappingUnsupportedCapability
        | ErrorCode::MappingUnmappableTool
        | ErrorCode::CapabilityUnsupported
        | ErrorCode::ContractInvalidReceipt
        | ErrorCode::ReceiptHashMismatch
        | ErrorCode::ReceiptChainBroken
        | ErrorCode::DialectUnknown
        | ErrorCode::IrLoweringFailed
        | ErrorCode::DialectMappingFailed => 422,

        // 424 Failed Dependency — upstream / workspace issue
        ErrorCode::ProtocolHandshakeFailed
        | ErrorCode::ProtocolUnexpectedMessage
        | ErrorCode::WorkspaceInitFailed
        | ErrorCode::WorkspaceStagingFailed
        | ErrorCode::ExecutionWorkspaceError
        | ErrorCode::ExecutionToolFailed
        | ErrorCode::CapabilityEmulationFailed => 424,

        // 429 Too Many Requests
        ErrorCode::BackendRateLimited | ErrorCode::RateLimitExceeded => 429,

        // 502 Bad Gateway — upstream backend error
        ErrorCode::BackendCrashed | ErrorCode::BackendUnavailable => 502,

        // 503 Service Unavailable — circuit breaker
        ErrorCode::CircuitBreakerOpen | ErrorCode::StreamClosed => 503,

        // 504 Gateway Timeout
        ErrorCode::BackendTimeout => 504,

        // 206 Partial Content — lossy but succeeded
        ErrorCode::MappingLossyConversion => 206,

        // 500 Internal
        ErrorCode::Internal
        | ErrorCode::ReceiptStoreFailed
        | ErrorCode::ValidationFailed
        | ErrorCode::SidecarSpawnFailed
        | ErrorCode::BackendContentFiltered
        | ErrorCode::BackendContextLength => 500,
    }
}

/// Simulate a cross-SDK error originating from e.g. OpenAI, Anthropic, etc.
fn map_sdk_error_to_abp(sdk: &str, sdk_error_type: &str) -> ErrorCode {
    match (sdk, sdk_error_type) {
        // OpenAI-style errors
        (_, "invalid_api_key") => ErrorCode::BackendAuthFailed,
        (_, "rate_limit_exceeded") => ErrorCode::BackendRateLimited,
        (_, "model_not_found") => ErrorCode::BackendModelNotFound,
        (_, "server_error") => ErrorCode::BackendUnavailable,
        (_, "timeout") => ErrorCode::BackendTimeout,
        (_, "context_length_exceeded") => ErrorCode::MappingUnsupportedCapability,
        (_, "invalid_request_error") => ErrorCode::ContractSchemaViolation,
        // Anthropic-style errors
        (_, "overloaded_error") => ErrorCode::BackendUnavailable,
        (_, "permission_error") => ErrorCode::ExecutionPermissionDenied,
        (_, "not_found_error") => ErrorCode::BackendNotFound,
        // Gemini-style
        (_, "RESOURCE_EXHAUSTED") => ErrorCode::BackendRateLimited,
        (_, "UNAUTHENTICATED") => ErrorCode::BackendAuthFailed,
        (_, "NOT_FOUND") => ErrorCode::BackendModelNotFound,
        (_, "INTERNAL") => ErrorCode::Internal,
        _ => ErrorCode::Internal,
    }
}

// =========================================================================
// 1. Backend failure scenarios
// =========================================================================

#[test]
fn backend_timeout_produces_correct_code() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_context("timeout_ms", 30_000)
        .with_context("backend", "openai");
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.code.as_str(), "backend_timeout");
    assert!(err.is_retryable());
}

#[test]
fn backend_timeout_display_is_human_readable() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
    let display = err.to_string();
    assert!(display.contains("backend_timeout"));
    assert!(display.contains("timed out after 30s"));
}

#[test]
fn backend_rate_limit_is_retryable() {
    let err = AbpError::new(ErrorCode::BackendRateLimited, "rate limited by openai")
        .with_context("retry_after_ms", 2000);
    assert!(err.is_retryable());
    assert_eq!(err.code.category(), ErrorCategory::Backend);
    assert_eq!(err.code.as_str(), "backend_rate_limited");
}

#[test]
fn backend_auth_failure_is_not_retryable() {
    let err = AbpError::new(ErrorCode::BackendAuthFailed, "invalid API key");
    assert!(!err.is_retryable());
    assert_eq!(err.code.as_str(), "backend_auth_failed");
}

#[test]
fn backend_server_error_via_unavailable() {
    let err = AbpError::new(ErrorCode::BackendUnavailable, "HTTP 503 from upstream");
    assert!(err.is_retryable());
    assert_eq!(err.code.category(), ErrorCategory::Backend);
}

#[test]
fn backend_connection_refused_maps_to_unavailable() {
    let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
    let err = AbpError::new(
        ErrorCode::BackendUnavailable,
        "connection refused to openai",
    )
    .with_source(io_err)
    .with_context("host", "api.openai.com");
    assert!(err.is_retryable());
    assert!(StdError::source(&err).is_some());
}

#[test]
fn backend_crashed_is_retryable() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "sidecar exited with code 137");
    assert!(err.is_retryable());
    assert_eq!(err.code.as_str(), "backend_crashed");
}

#[test]
fn backend_not_found_is_not_retryable() {
    let err = AbpError::new(ErrorCode::BackendNotFound, "no backend named 'foo'");
    assert!(!err.is_retryable());
    assert_eq!(err.code.as_str(), "backend_not_found");
}

#[test]
fn backend_model_not_found_is_not_retryable() {
    let err = AbpError::new(
        ErrorCode::BackendModelNotFound,
        "model gpt-99 not available",
    );
    assert!(!err.is_retryable());
    assert_eq!(err.code.as_str(), "backend_model_not_found");
}

// =========================================================================
// 2. Protocol error scenarios
// =========================================================================

#[test]
fn protocol_malformed_jsonl_from_serde() {
    let json_err = serde_json::from_str::<serde_json::Value>("{{bad json").unwrap_err();
    let err: AbpError = json_err.into();
    assert_eq!(err.code, ErrorCode::ProtocolInvalidEnvelope);
    assert_eq!(err.code.as_str(), "protocol_invalid_envelope");
}

#[test]
fn protocol_missing_hello_produces_handshake_failed() {
    let err = AbpError::new(
        ErrorCode::ProtocolHandshakeFailed,
        "sidecar did not send hello within 5s",
    );
    assert_eq!(err.code.category(), ErrorCategory::Protocol);
    assert!(!err.is_retryable());
    assert_eq!(err.code.as_str(), "protocol_handshake_failed");
}

#[test]
fn protocol_unexpected_envelope_type() {
    let err = AbpError::new(
        ErrorCode::ProtocolUnexpectedMessage,
        "expected 'hello', got 'event'",
    );
    assert_eq!(err.code.as_str(), "protocol_unexpected_message");
    assert!(!err.is_retryable());
}

#[test]
fn protocol_version_mismatch_scenario() {
    let err = AbpError::new(
        ErrorCode::ProtocolVersionMismatch,
        "host speaks abp/v0.1, sidecar speaks abp/v0.2",
    )
    .with_context("host_version", "abp/v0.1")
    .with_context("sidecar_version", "abp/v0.2");
    assert_eq!(err.code.as_str(), "protocol_version_mismatch");
    assert!(!err.is_retryable());
    assert_eq!(err.context.len(), 2);
}

#[test]
fn protocol_missing_ref_id() {
    let err = AbpError::new(
        ErrorCode::ProtocolMissingRefId,
        "envelope has no ref_id field",
    );
    assert_eq!(err.code.as_str(), "protocol_missing_ref_id");
    assert_eq!(err.code.category(), ErrorCategory::Protocol);
}

#[test]
fn protocol_invalid_envelope_display() {
    let msg = ErrorCode::ProtocolInvalidEnvelope.to_string();
    assert!(msg.contains("parse") || msg.contains("invalid"));
    assert!(!msg.contains('_'));
}

// =========================================================================
// 3. Mapping error scenarios
// =========================================================================

#[test]
fn mapping_unmappable_capability() {
    let err = AbpError::new(
        ErrorCode::MappingUnsupportedCapability,
        "target dialect does not support computer_use",
    )
    .with_context("capability", "computer_use")
    .with_context("target", "openai");
    assert_eq!(err.code.as_str(), "mapping_unsupported_capability");
    assert!(!err.is_retryable());
}

#[test]
fn mapping_lossy_conversion_is_degraded_not_fatal() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    assert_eq!(cl.code.as_str(), "mapping_lossy_conversion");
}

#[test]
fn mapping_dialect_mismatch_is_fatal() {
    let err = AbpError::new(
        ErrorCode::MappingDialectMismatch,
        "cannot map claude → codex: incompatible tool models",
    );
    assert!(!err.is_retryable());
    assert_eq!(err.code.category(), ErrorCategory::Mapping);
}

#[test]
fn mapping_unmappable_tool_has_correct_category() {
    let err = AbpError::new(
        ErrorCode::MappingUnmappableTool,
        "tool bash_20241022 has no codex equivalent",
    );
    assert_eq!(err.code.category(), ErrorCategory::Mapping);
    assert_eq!(err.code.as_str(), "mapping_unmappable_tool");
}

#[test]
fn mapping_lossy_conversion_display() {
    let display = ErrorCode::MappingLossyConversion.to_string();
    assert!(
        display.contains("lost") || display.contains("lossy") || display.contains("information")
    );
}

// =========================================================================
// 4. Workspace error scenarios
// =========================================================================

#[test]
fn workspace_init_failed_from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let err = AbpError::new(ErrorCode::WorkspaceInitFailed, "cannot create temp dir")
        .with_source(io_err)
        .with_context("path", "/tmp/abp-workspace-xyz");
    assert_eq!(err.code.as_str(), "workspace_init_failed");
    assert!(StdError::source(&err).is_some());
}

#[test]
fn workspace_staging_failed_disk_full() {
    let err = AbpError::new(
        ErrorCode::WorkspaceStagingFailed,
        "disk full while copying files to staged workspace",
    )
    .with_context("errno", "ENOSPC");
    assert_eq!(err.code.category(), ErrorCategory::Workspace);
    assert!(!err.is_retryable());
}

#[test]
fn workspace_init_failed_permission_denied() {
    let err = AbpError::new(
        ErrorCode::WorkspaceInitFailed,
        "permission denied creating workspace directory",
    )
    .with_context("dir", "/restricted/workspace");
    assert_eq!(err.code.as_str(), "workspace_init_failed");
    let display = err.to_string();
    assert!(display.contains("permission denied"));
}

#[test]
fn workspace_staging_failed_git_conflict() {
    let err = AbpError::new(
        ErrorCode::WorkspaceStagingFailed,
        "git baseline commit failed: index.lock exists",
    )
    .with_context("reason", "lock_file_conflict");
    assert_eq!(err.code.as_str(), "workspace_staging_failed");
    assert_eq!(err.code.category(), ErrorCategory::Workspace);
}

// =========================================================================
// 5. Policy violation scenarios
// =========================================================================

#[test]
fn policy_denied_tool_use() {
    let err = AbpError::new(
        ErrorCode::PolicyDenied,
        "tool 'bash' is not in the allow list",
    )
    .with_context("tool", "bash")
    .with_context("policy", "restrict_tools");
    assert_eq!(err.code.as_str(), "policy_denied");
    assert!(!err.is_retryable());
}

#[test]
fn policy_denied_file_access() {
    let err = AbpError::new(
        ErrorCode::PolicyDenied,
        "read access denied for /etc/shadow",
    )
    .with_context("path", "/etc/shadow")
    .with_context("action", "read");
    assert_eq!(err.code.category(), ErrorCategory::Policy);
    assert_eq!(err.code.as_str(), "policy_denied");
}

#[test]
fn policy_denied_write_operation() {
    let err = AbpError::new(
        ErrorCode::PolicyDenied,
        "write to /usr/bin denied by policy",
    )
    .with_context("path", "/usr/bin/evil")
    .with_context("action", "write");
    assert_eq!(err.code.as_str(), "policy_denied");
    let display = err.to_string();
    assert!(display.contains("denied"));
}

#[test]
fn policy_invalid_definition() {
    let err = AbpError::new(
        ErrorCode::PolicyInvalid,
        "malformed glob pattern in deny list: '[invalid'",
    );
    assert_eq!(err.code.as_str(), "policy_invalid");
    assert!(!err.is_retryable());
}

#[test]
fn policy_denied_is_classified_as_content_filter() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::PolicyDenied);
    assert_eq!(cl.category, ClassificationCategory::ContentFilter);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
}

// =========================================================================
// 6. Chain of errors — original → wrapped → user-facing
// =========================================================================

#[test]
fn error_chain_io_to_workspace_to_dto() {
    // Original: IO error
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file: /src/main.rs");
    // Wrapped: workspace error with IO source
    let abp_err = AbpError::new(ErrorCode::WorkspaceStagingFailed, "file copy failed")
        .with_source(io_err)
        .with_context("file", "/src/main.rs");
    // User-facing: DTO
    let dto: AbpErrorDto = (&abp_err).into();
    assert_eq!(dto.code, ErrorCode::WorkspaceStagingFailed);
    assert_eq!(
        dto.source_message.as_deref(),
        Some("no such file: /src/main.rs")
    );
    assert_eq!(dto.context["file"], serde_json::json!("/src/main.rs"));
}

#[test]
fn error_chain_json_to_protocol() {
    let json_err = serde_json::from_str::<serde_json::Value>("not valid json").unwrap_err();
    let abp_err: AbpError = json_err.into();
    assert_eq!(abp_err.code, ErrorCode::ProtocolInvalidEnvelope);
    let dto: AbpErrorDto = (&abp_err).into();
    assert!(dto.source_message.is_some());
}

#[test]
fn error_chain_nested_source_display() {
    let inner = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
    let err = AbpError::new(ErrorCode::BackendCrashed, "sidecar pipe broke").with_source(inner);
    let src = StdError::source(&err).unwrap();
    assert_eq!(src.to_string(), "pipe broken");
}

#[test]
fn error_chain_dto_preserves_context() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "anthropic")
        .with_context("model", "claude-3")
        .with_context("timeout_ms", 60_000);
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.context.len(), 3);
    assert_eq!(dto.context["backend"], serde_json::json!("anthropic"));
}

#[test]
fn error_chain_dto_to_abp_error_roundtrip() {
    let original = AbpError::new(ErrorCode::ConfigInvalid, "missing field 'backend'")
        .with_context("file", "backplane.toml");
    let dto: AbpErrorDto = (&original).into();
    let reconstructed: AbpError = dto.into();
    assert_eq!(reconstructed.code, original.code);
    assert_eq!(reconstructed.message, original.message);
    assert_eq!(reconstructed.context, original.context);
    // Source is lost in DTO round-trip
    assert!(reconstructed.source.is_none());
}

#[test]
fn error_chain_to_info_preserves_retryability() {
    let err = AbpError::new(ErrorCode::BackendRateLimited, "429 Too Many Requests")
        .with_context("retry_after", "2s");
    let info = err.to_info();
    assert!(info.is_retryable);
    assert_eq!(info.code, ErrorCode::BackendRateLimited);
    assert_eq!(info.details["retry_after"], serde_json::json!("2s"));
}

// =========================================================================
// 7. Error recovery — retry transient, fallback permanent
// =========================================================================

#[test]
fn retry_after_transient_timeout() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendTimeout);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    assert!(cl.recovery.delay_ms.is_some());
}

#[test]
fn retry_after_rate_limit_with_delay() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendRateLimited);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
    let delay = cl.recovery.delay_ms.unwrap();
    assert!(delay > 0);
}

#[test]
fn retry_after_backend_crash() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendCrashed);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.recovery.action, RecoveryAction::Retry);
}

#[test]
fn retry_after_backend_unavailable() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendUnavailable);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::ServerError);
}

#[test]
fn fallback_after_permanent_auth_failure() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendAuthFailed);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn fallback_after_model_not_found() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendModelNotFound);
    assert_eq!(cl.recovery.action, RecoveryAction::ChangeModel);
}

#[test]
fn fallback_after_capability_unsupported() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::CapabilityUnsupported);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn fallback_after_mapping_failure() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::MappingDialectMismatch);
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn no_recovery_for_invalid_request() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::ContractSchemaViolation);
    assert_eq!(cl.recovery.action, RecoveryAction::None);
}

#[test]
fn contact_admin_for_protocol_error() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::ProtocolInvalidEnvelope);
    assert_eq!(cl.recovery.action, RecoveryAction::ContactAdmin);
}

#[test]
fn degraded_lossy_conversion_suggests_fallback() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::MappingLossyConversion);
    assert_eq!(cl.severity, ErrorSeverity::Degraded);
    // MappingFailure category → Fallback recovery
    assert_eq!(cl.recovery.action, RecoveryAction::Fallback);
}

#[test]
fn recovery_suggestion_has_human_readable_description() {
    let classifier = ErrorClassifier::new();
    for code in all_error_codes() {
        let cl = classifier.classify(&code);
        assert!(
            !cl.recovery.description.is_empty(),
            "recovery description is empty for {code:?}"
        );
    }
}

#[test]
fn simulated_retry_loop_transient_errors() {
    let transient_codes = [
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendCrashed,
    ];
    let classifier = ErrorClassifier::new();
    for code in transient_codes {
        let cl = classifier.classify(&code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Retriable,
            "expected retriable for {code:?}"
        );
        assert_eq!(
            cl.recovery.action,
            RecoveryAction::Retry,
            "expected Retry for {code:?}"
        );
    }
}

#[test]
fn simulated_fallback_permanent_errors() {
    let permanent_fallback = [
        ErrorCode::CapabilityUnsupported,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
    ];
    let classifier = ErrorClassifier::new();
    for code in permanent_fallback {
        let cl = classifier.classify(&code);
        assert_eq!(
            cl.severity,
            ErrorSeverity::Fatal,
            "expected fatal for {code:?}"
        );
        assert_eq!(
            cl.recovery.action,
            RecoveryAction::Fallback,
            "expected Fallback for {code:?}"
        );
    }
}

// =========================================================================
// 8. Error serialization — JSON roundtrip, stable codes
// =========================================================================

#[test]
fn all_error_codes_json_roundtrip() {
    for code in all_error_codes() {
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code, "roundtrip failed for {code:?}");
    }
}

#[test]
fn all_error_codes_serialize_to_as_str() {
    for code in all_error_codes() {
        let json = serde_json::to_string(&code).unwrap();
        let expected = format!(r#""{}""#, code.as_str());
        assert_eq!(json, expected, "serde mismatch for {code:?}");
    }
}

#[test]
fn all_as_str_values_are_snake_case() {
    let snake_case_re = |s: &str| -> bool {
        s.chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
            && !s.starts_with('_')
            && !s.ends_with('_')
            && !s.contains("__")
    };
    for code in all_error_codes() {
        assert!(
            snake_case_re(code.as_str()),
            "as_str() is not snake_case for {code:?}: {}",
            code.as_str()
        );
    }
}

#[test]
fn all_as_str_values_are_unique() {
    let mut seen = HashSet::new();
    for code in all_error_codes() {
        assert!(
            seen.insert(code.as_str()),
            "duplicate as_str: {}",
            code.as_str()
        );
    }
}

#[test]
fn error_info_json_roundtrip() {
    let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out")
        .with_detail("backend", "openai")
        .with_detail("timeout_ms", 5000);
    let json = serde_json::to_string(&info).unwrap();
    let back: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn error_info_display_format() {
    let info = ErrorInfo::new(ErrorCode::PolicyDenied, "tool denied");
    let display = info.to_string();
    assert_eq!(display, "[policy_denied] tool denied");
}

#[test]
fn abp_error_dto_json_roundtrip_all_codes() {
    for code in all_error_codes() {
        let err = AbpError::new(code, format!("test message for {code:?}"))
            .with_context("test_key", "test_value");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back, "DTO roundtrip failed for {code:?}");
    }
}

#[test]
fn abp_error_dto_source_message_absent_when_no_source() {
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("source_message"));
}

#[test]
fn abp_error_dto_source_message_present_with_source() {
    let src = std::io::Error::new(std::io::ErrorKind::Other, "underlying cause");
    let err = AbpError::new(ErrorCode::Internal, "wrapper").with_source(src);
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("underlying cause"));
}

#[test]
fn error_code_deserialization_rejects_unknown_variant() {
    let result = serde_json::from_str::<ErrorCode>(r#""totally_unknown_code""#);
    assert!(result.is_err());
}

#[test]
fn error_category_json_roundtrip() {
    let categories = [
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
    for cat in categories {
        let json = serde_json::to_string(&cat).unwrap();
        let back: ErrorCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat, "category roundtrip failed for {cat:?}");
    }
}

#[test]
fn classification_json_roundtrip() {
    let classifier = ErrorClassifier::new();
    let cl = classifier.classify(&ErrorCode::BackendTimeout);
    let json = serde_json::to_string(&cl).unwrap();
    let back: abp_error_taxonomy::ErrorClassification = serde_json::from_str(&json).unwrap();
    assert_eq!(cl.code, back.code);
    assert_eq!(cl.severity, back.severity);
    assert_eq!(cl.category, back.category);
}

#[test]
fn recovery_suggestion_json_roundtrip() {
    let suggestion = RecoverySuggestion {
        action: RecoveryAction::Retry,
        description: "retry after delay".into(),
        delay_ms: Some(1000),
    };
    let json = serde_json::to_string(&suggestion).unwrap();
    let back: RecoverySuggestion = serde_json::from_str(&json).unwrap();
    assert_eq!(suggestion, back);
}

// =========================================================================
// 9. HTTP status mapping
// =========================================================================

#[test]
fn http_status_backend_timeout_is_504() {
    assert_eq!(suggested_http_status(&ErrorCode::BackendTimeout), 504);
}

#[test]
fn http_status_rate_limited_is_429() {
    assert_eq!(suggested_http_status(&ErrorCode::BackendRateLimited), 429);
}

#[test]
fn http_status_auth_failed_is_401() {
    assert_eq!(suggested_http_status(&ErrorCode::BackendAuthFailed), 401);
}

#[test]
fn http_status_policy_denied_is_403() {
    assert_eq!(suggested_http_status(&ErrorCode::PolicyDenied), 403);
}

#[test]
fn http_status_not_found_is_404() {
    assert_eq!(suggested_http_status(&ErrorCode::BackendNotFound), 404);
    assert_eq!(suggested_http_status(&ErrorCode::BackendModelNotFound), 404);
}

#[test]
fn http_status_invalid_envelope_is_400() {
    assert_eq!(
        suggested_http_status(&ErrorCode::ProtocolInvalidEnvelope),
        400
    );
}

#[test]
fn http_status_internal_is_500() {
    assert_eq!(suggested_http_status(&ErrorCode::Internal), 500);
}

#[test]
fn http_status_version_mismatch_is_409() {
    assert_eq!(
        suggested_http_status(&ErrorCode::ProtocolVersionMismatch),
        409
    );
    assert_eq!(
        suggested_http_status(&ErrorCode::ContractVersionMismatch),
        409
    );
}

#[test]
fn http_status_lossy_conversion_is_206() {
    assert_eq!(
        suggested_http_status(&ErrorCode::MappingLossyConversion),
        206
    );
}

#[test]
fn http_status_backend_crashed_is_502() {
    assert_eq!(suggested_http_status(&ErrorCode::BackendCrashed), 502);
    assert_eq!(suggested_http_status(&ErrorCode::BackendUnavailable), 502);
}

#[test]
fn http_status_all_codes_have_mapping() {
    for code in all_error_codes() {
        let status = suggested_http_status(&code);
        assert!(
            (200..=599).contains(&status),
            "invalid HTTP status {status} for {code:?}"
        );
    }
}

// =========================================================================
// 10. Cross-SDK error mapping
// =========================================================================

#[test]
fn sdk_openai_invalid_api_key() {
    let code = map_sdk_error_to_abp("openai", "invalid_api_key");
    assert_eq!(code, ErrorCode::BackendAuthFailed);
    assert_eq!(code.as_str(), "backend_auth_failed");
}

#[test]
fn sdk_openai_rate_limit() {
    let code = map_sdk_error_to_abp("openai", "rate_limit_exceeded");
    assert_eq!(code, ErrorCode::BackendRateLimited);
    assert!(code.is_retryable());
}

#[test]
fn sdk_openai_model_not_found() {
    let code = map_sdk_error_to_abp("openai", "model_not_found");
    assert_eq!(code, ErrorCode::BackendModelNotFound);
}

#[test]
fn sdk_openai_server_error() {
    let code = map_sdk_error_to_abp("openai", "server_error");
    assert_eq!(code, ErrorCode::BackendUnavailable);
    assert!(code.is_retryable());
}

#[test]
fn sdk_openai_timeout() {
    let code = map_sdk_error_to_abp("openai", "timeout");
    assert_eq!(code, ErrorCode::BackendTimeout);
    assert!(code.is_retryable());
}

#[test]
fn sdk_anthropic_overloaded() {
    let code = map_sdk_error_to_abp("anthropic", "overloaded_error");
    assert_eq!(code, ErrorCode::BackendUnavailable);
    assert!(code.is_retryable());
}

#[test]
fn sdk_anthropic_permission_error() {
    let code = map_sdk_error_to_abp("anthropic", "permission_error");
    assert_eq!(code, ErrorCode::ExecutionPermissionDenied);
    assert!(!code.is_retryable());
}

#[test]
fn sdk_anthropic_not_found() {
    let code = map_sdk_error_to_abp("anthropic", "not_found_error");
    assert_eq!(code, ErrorCode::BackendNotFound);
}

#[test]
fn sdk_gemini_resource_exhausted() {
    let code = map_sdk_error_to_abp("gemini", "RESOURCE_EXHAUSTED");
    assert_eq!(code, ErrorCode::BackendRateLimited);
}

#[test]
fn sdk_gemini_unauthenticated() {
    let code = map_sdk_error_to_abp("gemini", "UNAUTHENTICATED");
    assert_eq!(code, ErrorCode::BackendAuthFailed);
}

#[test]
fn sdk_gemini_not_found() {
    let code = map_sdk_error_to_abp("gemini", "NOT_FOUND");
    assert_eq!(code, ErrorCode::BackendModelNotFound);
}

#[test]
fn sdk_gemini_internal() {
    let code = map_sdk_error_to_abp("gemini", "INTERNAL");
    assert_eq!(code, ErrorCode::Internal);
}

#[test]
fn sdk_unknown_error_falls_back_to_internal() {
    let code = map_sdk_error_to_abp("unknown_sdk", "weird_error");
    assert_eq!(code, ErrorCode::Internal);
}

#[test]
fn sdk_context_length_maps_to_unsupported_capability() {
    let code = map_sdk_error_to_abp("openai", "context_length_exceeded");
    assert_eq!(code, ErrorCode::MappingUnsupportedCapability);
}

// =========================================================================
// Additional coverage: Display, Debug, misc
// =========================================================================

#[test]
fn all_error_codes_have_non_empty_message() {
    for code in all_error_codes() {
        let msg = code.message();
        assert!(!msg.is_empty(), "empty message for {code:?}");
    }
}

#[test]
fn all_error_codes_display_differs_from_as_str() {
    for code in all_error_codes() {
        let display = code.to_string();
        let code_str = code.as_str();
        assert_ne!(
            display, code_str,
            "Display should differ from as_str for {code:?}"
        );
    }
}

#[test]
fn all_error_codes_have_non_empty_category_display() {
    for code in all_error_codes() {
        let cat_display = code.category().to_string();
        assert!(!cat_display.is_empty());
    }
}

#[test]
fn abp_error_debug_includes_code_and_message() {
    let err = AbpError::new(ErrorCode::IrLoweringFailed, "lowering failed");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("IrLoweringFailed"));
    assert!(dbg.contains("lowering failed"));
}

#[test]
fn abp_error_debug_includes_context_when_present() {
    let err = AbpError::new(ErrorCode::Internal, "oops").with_context("detail", "something");
    let dbg = format!("{err:?}");
    assert!(dbg.contains("context"));
    assert!(dbg.contains("detail"));
}

#[test]
fn abp_error_display_with_multiple_context_keys_is_deterministic() {
    let err = AbpError::new(ErrorCode::Internal, "err")
        .with_context("z_key", "last")
        .with_context("a_key", "first")
        .with_context("m_key", "middle");
    let display = err.to_string();
    let a_pos = display.find("a_key").unwrap();
    let m_pos = display.find("m_key").unwrap();
    let z_pos = display.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn from_io_error_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::Other, "disk I/O failed");
    let abp_err: AbpError = io_err.into();
    assert_eq!(abp_err.code, ErrorCode::Internal);
    assert!(abp_err.message.contains("disk I/O failed"));
}

#[test]
fn error_info_with_detail_preserves_order() {
    let info = ErrorInfo::new(ErrorCode::Internal, "test")
        .with_detail("z", "last")
        .with_detail("a", "first");
    let json = serde_json::to_string(&info).unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn error_code_count_matches_all_codes_helper() {
    assert_eq!(all_error_codes().len(), 36);
}

#[test]
fn every_category_has_at_least_one_code() {
    let categories: HashSet<ErrorCategory> =
        all_error_codes().iter().map(|c| c.category()).collect();
    assert!(categories.contains(&ErrorCategory::Protocol));
    assert!(categories.contains(&ErrorCategory::Backend));
    assert!(categories.contains(&ErrorCategory::Mapping));
    assert!(categories.contains(&ErrorCategory::Policy));
    assert!(categories.contains(&ErrorCategory::Workspace));
    assert!(categories.contains(&ErrorCategory::Ir));
    assert!(categories.contains(&ErrorCategory::Receipt));
    assert!(categories.contains(&ErrorCategory::Dialect));
    assert!(categories.contains(&ErrorCategory::Config));
    assert!(categories.contains(&ErrorCategory::Execution));
    assert!(categories.contains(&ErrorCategory::Contract));
    assert!(categories.contains(&ErrorCategory::Capability));
    assert!(categories.contains(&ErrorCategory::Internal));
}

#[test]
fn classifier_covers_all_error_codes() {
    let classifier = ErrorClassifier::new();
    for code in all_error_codes() {
        let cl = classifier.classify(&code);
        assert_eq!(cl.code, code);
        assert!(!cl.recovery.description.is_empty());
    }
}

#[test]
fn all_retryable_codes_are_classified_retriable() {
    let classifier = ErrorClassifier::new();
    for code in all_error_codes() {
        if code.is_retryable() {
            let cl = classifier.classify(&code);
            assert_eq!(
                cl.severity,
                ErrorSeverity::Retriable,
                "retryable code {code:?} should be Retriable severity"
            );
        }
    }
}

#[test]
fn non_retryable_codes_are_not_retriable_severity() {
    let classifier = ErrorClassifier::new();
    let exempt = [
        ErrorCode::MappingLossyConversion,
        ErrorCode::CapabilityEmulationFailed,
    ];
    for code in all_error_codes() {
        if !code.is_retryable() && !exempt.contains(&code) {
            let cl = classifier.classify(&code);
            assert_ne!(
                cl.severity,
                ErrorSeverity::Retriable,
                "non-retryable code {code:?} should not be Retriable"
            );
        }
    }
}
