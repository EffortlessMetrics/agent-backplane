#![allow(clippy::all, dead_code)]
//! Wave-43 error taxonomy tests.
//!
//! Covers: new ErrorCode variants, new ErrorCategory variants, ErrorContext
//! builder, vendor SDK error mapping for all 6 vendors, serde roundtrips,
//! categorisation, diagnostics, recovery plans, and serialisation stability.

use abp_error_taxonomy::classification::{ClassificationCategory, ErrorClassifier, ErrorSeverity};
use abp_error_taxonomy::context::ErrorContextBuilder;
use abp_error_taxonomy::docs::error_code_doc;
use abp_error_taxonomy::mapping::{VendorError, VendorErrorMapper, VendorKind};
use abp_error_taxonomy::recovery::RecoveryPlan;
use abp_error_taxonomy::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode, ErrorInfo};
use std::collections::HashSet;

// =========================================================================
// Helpers
// =========================================================================

const NEW_CODES: &[ErrorCode] = &[
    ErrorCode::RateLimitExceeded,
    ErrorCode::CircuitBreakerOpen,
    ErrorCode::StreamClosed,
    ErrorCode::ReceiptStoreFailed,
    ErrorCode::ValidationFailed,
    ErrorCode::SidecarSpawnFailed,
    ErrorCode::BackendContentFiltered,
    ErrorCode::BackendContextLength,
];

const NEW_CATEGORIES: &[ErrorCategory] = &[
    ErrorCategory::RateLimit,
    ErrorCategory::Stream,
    ErrorCategory::Validation,
    ErrorCategory::Sidecar,
];

// =========================================================================
// 1. New ErrorCode serde roundtrips (8 tests)
// =========================================================================

#[test]
fn serde_roundtrip_rate_limit_exceeded() {
    let code = ErrorCode::RateLimitExceeded;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"rate_limit_exceeded\"");
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn serde_roundtrip_circuit_breaker_open() {
    let code = ErrorCode::CircuitBreakerOpen;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"circuit_breaker_open\"");
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn serde_roundtrip_stream_closed() {
    let code = ErrorCode::StreamClosed;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"stream_closed\"");
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn serde_roundtrip_receipt_store_failed() {
    let code = ErrorCode::ReceiptStoreFailed;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"receipt_store_failed\"");
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn serde_roundtrip_validation_failed() {
    let code = ErrorCode::ValidationFailed;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"validation_failed\"");
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn serde_roundtrip_sidecar_spawn_failed() {
    let code = ErrorCode::SidecarSpawnFailed;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"sidecar_spawn_failed\"");
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn serde_roundtrip_backend_content_filtered() {
    let code = ErrorCode::BackendContentFiltered;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"backend_content_filtered\"");
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

#[test]
fn serde_roundtrip_backend_context_length() {
    let code = ErrorCode::BackendContextLength;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"backend_context_length\"");
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, code);
}

// =========================================================================
// 2. New ErrorCategory serde roundtrips (4 tests)
// =========================================================================

#[test]
fn serde_roundtrip_category_rate_limit() {
    let cat = ErrorCategory::RateLimit;
    let json = serde_json::to_string(&cat).unwrap();
    assert_eq!(json, "\"rate_limit\"");
    let back: ErrorCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cat);
}

#[test]
fn serde_roundtrip_category_stream() {
    let cat = ErrorCategory::Stream;
    let json = serde_json::to_string(&cat).unwrap();
    assert_eq!(json, "\"stream\"");
    let back: ErrorCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cat);
}

#[test]
fn serde_roundtrip_category_validation() {
    let cat = ErrorCategory::Validation;
    let json = serde_json::to_string(&cat).unwrap();
    assert_eq!(json, "\"validation\"");
    let back: ErrorCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cat);
}

#[test]
fn serde_roundtrip_category_sidecar() {
    let cat = ErrorCategory::Sidecar;
    let json = serde_json::to_string(&cat).unwrap();
    assert_eq!(json, "\"sidecar\"");
    let back: ErrorCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cat);
}

// =========================================================================
// 3. New code categorisation (8 tests)
// =========================================================================

#[test]
fn category_rate_limit_exceeded() {
    assert_eq!(
        ErrorCode::RateLimitExceeded.category(),
        ErrorCategory::RateLimit
    );
}

#[test]
fn category_circuit_breaker_open() {
    assert_eq!(
        ErrorCode::CircuitBreakerOpen.category(),
        ErrorCategory::RateLimit
    );
}

#[test]
fn category_stream_closed() {
    assert_eq!(ErrorCode::StreamClosed.category(), ErrorCategory::Stream);
}

#[test]
fn category_receipt_store_failed() {
    assert_eq!(
        ErrorCode::ReceiptStoreFailed.category(),
        ErrorCategory::Receipt
    );
}

#[test]
fn category_validation_failed() {
    assert_eq!(
        ErrorCode::ValidationFailed.category(),
        ErrorCategory::Validation
    );
}

#[test]
fn category_sidecar_spawn_failed() {
    assert_eq!(
        ErrorCode::SidecarSpawnFailed.category(),
        ErrorCategory::Sidecar
    );
}

#[test]
fn category_backend_content_filtered() {
    assert_eq!(
        ErrorCode::BackendContentFiltered.category(),
        ErrorCategory::Backend
    );
}

#[test]
fn category_backend_context_length() {
    assert_eq!(
        ErrorCode::BackendContextLength.category(),
        ErrorCategory::Backend
    );
}

// =========================================================================
// 4. Retryability of new codes (3 tests)
// =========================================================================

#[test]
fn retryable_new_codes() {
    assert!(ErrorCode::RateLimitExceeded.is_retryable());
    assert!(ErrorCode::CircuitBreakerOpen.is_retryable());
    assert!(ErrorCode::StreamClosed.is_retryable());
}

#[test]
fn non_retryable_new_codes() {
    assert!(!ErrorCode::ReceiptStoreFailed.is_retryable());
    assert!(!ErrorCode::ValidationFailed.is_retryable());
    assert!(!ErrorCode::SidecarSpawnFailed.is_retryable());
    assert!(!ErrorCode::BackendContentFiltered.is_retryable());
    assert!(!ErrorCode::BackendContextLength.is_retryable());
}

#[test]
fn new_code_error_info_retryable_matches() {
    for code in NEW_CODES {
        let info = ErrorInfo::new(*code, "test");
        assert_eq!(info.is_retryable, code.is_retryable(), "{:?}", code);
    }
}

// =========================================================================
// 5. ErrorContext builder (3 tests)
// =========================================================================

#[test]
fn error_context_builder_basic() {
    let err =
        ErrorContextBuilder::from_error(AbpError::new(ErrorCode::RateLimitExceeded, "limit hit"))
            .custom("operation", "send_request")
            .build();
    assert_eq!(err.code, ErrorCode::RateLimitExceeded);
    assert_eq!(err.message, "limit hit");
    assert_eq!(
        err.context.get("operation").map(|v| v.as_str()),
        Some(Some("send_request"))
    );
}

#[test]
fn error_context_builder_full() {
    let err =
        ErrorContextBuilder::from_error(AbpError::new(ErrorCode::SidecarSpawnFailed, "no node"))
            .custom("operation", "spawn_sidecar")
            .custom("path", "/usr/bin/node")
            .custom("reason", "ENOENT")
            .build();
    assert_eq!(err.code, ErrorCode::SidecarSpawnFailed);
    assert!(err.context.contains_key("operation"));
    assert_eq!(
        err.context.get("path").and_then(|v| v.as_str()),
        Some("/usr/bin/node")
    );
    assert_eq!(
        err.context.get("reason").and_then(|v| v.as_str()),
        Some("ENOENT")
    );
}

#[test]
fn error_context_builder_serde_roundtrip() {
    let err =
        ErrorContextBuilder::from_error(AbpError::new(ErrorCode::ValidationFailed, "bad input"))
            .custom("operation", "validate")
            .custom("field", "temperature")
            .build();
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, ErrorCode::ValidationFailed);
    assert_eq!(back.message, "bad input");
}

// =========================================================================
// 6. Vendor mapping — Codex (2 tests)
// =========================================================================

#[test]
fn codex_context_length_maps_correctly() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Codex, 400, "context_length_exceeded");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendContextLength);
}

#[test]
fn codex_content_filter_maps_correctly() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Codex, 400, "content_filter");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendContentFiltered);
}

// =========================================================================
// 7. Vendor mapping — Copilot (2 tests)
// =========================================================================

#[test]
fn copilot_auth_maps_to_backend_auth_failed() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Copilot, 403, "forbidden");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendAuthFailed);
}

#[test]
fn copilot_content_filter_maps_correctly() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Copilot, 400, "content_filter");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendContentFiltered);
}

// =========================================================================
// 8. Vendor mapping — Kimi (2 tests)
// =========================================================================

#[test]
fn kimi_rate_limit_maps_correctly() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Kimi, 429, "rate_limit_exceeded");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendRateLimited);
}

#[test]
fn kimi_context_length_maps_correctly() {
    let m = VendorErrorMapper::new();
    let e = VendorError::new(VendorKind::Kimi, 400, "context_length_exceeded");
    assert_eq!(m.map_to_abp(&e), ErrorCode::BackendContextLength);
}

// =========================================================================
// 9. All 6 vendors produce AbpError with context (1 test)
// =========================================================================

#[test]
fn all_vendor_kinds_produce_contextualised_abp_error() {
    let m = VendorErrorMapper::new();
    let vendors = [
        VendorKind::OpenAi,
        VendorKind::Anthropic,
        VendorKind::Gemini,
        VendorKind::Codex,
        VendorKind::Copilot,
        VendorKind::Kimi,
    ];
    for vendor in &vendors {
        let e = VendorError::new(*vendor, 500, "server_error").with_message("oops");
        let abp = m.to_abp_error(&e);
        assert!(
            abp.context.contains_key("vendor"),
            "{:?} missing vendor context",
            vendor
        );
        assert!(
            abp.context.contains_key("http_status"),
            "{:?} missing http_status context",
            vendor
        );
    }
}

// =========================================================================
// 10. Classification of new codes (4 tests)
// =========================================================================

#[test]
fn classify_rate_limit_exceeded_is_retriable() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::RateLimitExceeded);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
    assert_eq!(cl.category, ClassificationCategory::RateLimit);
}

#[test]
fn classify_circuit_breaker_open_is_retriable() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::CircuitBreakerOpen);
    assert_eq!(cl.severity, ErrorSeverity::Retriable);
}

#[test]
fn classify_backend_content_filtered_is_fatal() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendContentFiltered);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ContentFilter);
}

#[test]
fn classify_backend_context_length_is_fatal() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendContextLength);
    assert_eq!(cl.severity, ErrorSeverity::Fatal);
    assert_eq!(cl.category, ClassificationCategory::ContextLength);
}

// =========================================================================
// 11. Documentation for new codes (2 tests)
// =========================================================================

#[test]
fn all_new_codes_have_documentation() {
    for code in NEW_CODES {
        let doc = error_code_doc(code);
        assert_eq!(doc.code, *code);
        assert!(!doc.description.is_empty(), "{:?} has empty doc", code);
        assert!(!doc.example.is_empty(), "{:?} has empty example", code);
    }
}

#[test]
fn new_code_docs_mention_variant_name() {
    for code in NEW_CODES {
        let doc = error_code_doc(code);
        let variant = format!("{:?}", code);
        assert!(
            doc.example.contains(&variant),
            "{:?} example doesn't mention variant",
            code
        );
    }
}

// =========================================================================
// 12. Recovery plans for new codes (2 tests)
// =========================================================================

#[test]
fn recovery_plan_for_retriable_new_codes_has_retry() {
    let classifier = ErrorClassifier::new();
    for code in &[
        ErrorCode::RateLimitExceeded,
        ErrorCode::CircuitBreakerOpen,
        ErrorCode::StreamClosed,
    ] {
        let cl = classifier.classify(code);
        let plan = RecoveryPlan::from_classification(&cl);
        assert!(
            plan.has_retry(),
            "recovery plan for {:?} should have retry step",
            code
        );
    }
}

#[test]
fn recovery_plan_for_fatal_new_codes_is_terminal() {
    let classifier = ErrorClassifier::new();
    for code in &[
        ErrorCode::ValidationFailed,
        ErrorCode::SidecarSpawnFailed,
        ErrorCode::BackendContentFiltered,
    ] {
        let cl = classifier.classify(code);
        let plan = RecoveryPlan::from_classification(&cl);
        assert!(
            plan.is_terminal(),
            "recovery plan for {:?} should be terminal",
            code
        );
    }
    // BackendContextLength has ReduceContext/ChangeModel steps — not terminal
    // but it should NOT have retry steps.
    let cl = classifier.classify(&ErrorCode::BackendContextLength);
    let plan = RecoveryPlan::from_classification(&cl);
    assert!(
        !plan.has_retry(),
        "BackendContextLength should not have retry"
    );
}

// =========================================================================
// 13. AbpError construction and DTO roundtrip for new codes (2 tests)
// =========================================================================

#[test]
fn abp_error_new_codes_construct_correctly() {
    for code in NEW_CODES {
        let err = AbpError::new(*code, format!("test {}", code.as_str()));
        assert_eq!(err.code, *code);
        assert!(err.message.contains(code.as_str()));
    }
}

#[test]
fn abp_error_dto_roundtrip_for_new_codes() {
    for code in NEW_CODES {
        let err = AbpError::new(*code, "roundtrip test").with_context("key", "value");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string_pretty(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, *code);
        assert_eq!(back.message, "roundtrip test");
    }
}

// =========================================================================
// 14. VendorKind serde (3 tests)
// =========================================================================

#[test]
fn vendor_kind_codex_serde_roundtrip() {
    let json = serde_json::to_string(&VendorKind::Codex).unwrap();
    assert_eq!(json, "\"codex\"");
    let back: VendorKind = serde_json::from_str(&json).unwrap();
    assert_eq!(back, VendorKind::Codex);
}

#[test]
fn vendor_kind_copilot_serde_roundtrip() {
    let json = serde_json::to_string(&VendorKind::Copilot).unwrap();
    assert_eq!(json, "\"copilot\"");
    let back: VendorKind = serde_json::from_str(&json).unwrap();
    assert_eq!(back, VendorKind::Copilot);
}

#[test]
fn vendor_kind_kimi_serde_roundtrip() {
    let json = serde_json::to_string(&VendorKind::Kimi).unwrap();
    assert_eq!(json, "\"kimi\"");
    let back: VendorKind = serde_json::from_str(&json).unwrap();
    assert_eq!(back, VendorKind::Kimi);
}

// =========================================================================
// 15. Vendor HTTP fallback for new vendors (1 test)
// =========================================================================

#[test]
fn new_vendor_unknown_type_falls_back_to_http() {
    let m = VendorErrorMapper::new();
    for vendor in &[VendorKind::Codex, VendorKind::Copilot, VendorKind::Kimi] {
        let e = VendorError::new(*vendor, 503, "unknown_new_error");
        assert_eq!(
            m.map_to_abp(&e),
            ErrorCode::BackendUnavailable,
            "{:?} should fall back to HTTP 503 mapping",
            vendor
        );
    }
}

// =========================================================================
// 16. New code as_str values are unique and snake_case (1 test)
// =========================================================================

#[test]
fn new_code_as_str_values_unique_and_snake_case() {
    let mut seen = HashSet::new();
    for code in NEW_CODES {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str: {s}");
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{s} is not snake_case"
        );
    }
}

// =========================================================================
// 17. New code messages are non-empty and unique (1 test)
// =========================================================================

#[test]
fn new_code_messages_non_empty_and_unique() {
    let mut seen = HashSet::new();
    for code in NEW_CODES {
        let msg = code.message();
        assert!(!msg.is_empty(), "{:?} has empty message", code);
        assert!(seen.insert(msg), "{:?} has duplicate message: {msg}", code);
    }
}
