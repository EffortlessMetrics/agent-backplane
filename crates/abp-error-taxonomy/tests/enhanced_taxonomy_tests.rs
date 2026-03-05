#![allow(clippy::all)]
//! Integration tests for the enhanced error taxonomy modules.

use abp_error_taxonomy::classification::{ErrorClassifier, RecoveryAction};
use abp_error_taxonomy::context::{EnrichError, ErrorContextBuilder};
use abp_error_taxonomy::docs::error_code_doc;
use abp_error_taxonomy::mapping::{VendorError, VendorErrorMapper, VendorKind};
use abp_error_taxonomy::recovery::{RecoveryPlan, RetryPolicy};
use abp_error_taxonomy::{AbpError, ErrorCode};

// =========================================================================
// Vendor mapping integration tests
// =========================================================================

#[test]
fn openai_errors_map_to_correct_abp_codes() {
    let m = VendorErrorMapper::new();
    let cases: Vec<(&str, u16, ErrorCode)> = vec![
        ("rate_limit_exceeded", 429, ErrorCode::BackendRateLimited),
        ("invalid_api_key", 401, ErrorCode::BackendAuthFailed),
        ("authentication_error", 401, ErrorCode::BackendAuthFailed),
        ("model_not_found", 404, ErrorCode::BackendModelNotFound),
        (
            "invalid_request_error",
            400,
            ErrorCode::ContractSchemaViolation,
        ),
        ("server_error", 500, ErrorCode::BackendUnavailable),
        ("timeout", 408, ErrorCode::BackendTimeout),
    ];
    for (error_type, status, expected) in cases {
        let e = VendorError::new(VendorKind::OpenAi, status, error_type);
        assert_eq!(
            m.map_to_abp(&e),
            expected,
            "OpenAI error_type={error_type} should map to {expected:?}"
        );
    }
}

#[test]
fn anthropic_errors_map_to_correct_abp_codes() {
    let m = VendorErrorMapper::new();
    let cases: Vec<(&str, u16, ErrorCode)> = vec![
        ("rate_limit_error", 429, ErrorCode::BackendRateLimited),
        ("authentication_error", 401, ErrorCode::BackendAuthFailed),
        ("not_found_error", 404, ErrorCode::BackendModelNotFound),
        (
            "invalid_request_error",
            400,
            ErrorCode::ContractSchemaViolation,
        ),
        ("overloaded_error", 529, ErrorCode::BackendUnavailable),
        ("api_error", 500, ErrorCode::Internal),
    ];
    for (error_type, status, expected) in cases {
        let e = VendorError::new(VendorKind::Anthropic, status, error_type);
        assert_eq!(
            m.map_to_abp(&e),
            expected,
            "Anthropic error_type={error_type} should map to {expected:?}"
        );
    }
}

#[test]
fn gemini_errors_map_to_correct_abp_codes() {
    let m = VendorErrorMapper::new();
    let cases: Vec<(&str, u16, ErrorCode)> = vec![
        ("RESOURCE_EXHAUSTED", 429, ErrorCode::BackendRateLimited),
        ("UNAUTHENTICATED", 401, ErrorCode::BackendAuthFailed),
        ("NOT_FOUND", 404, ErrorCode::BackendModelNotFound),
        ("INVALID_ARGUMENT", 400, ErrorCode::ContractSchemaViolation),
        ("UNAVAILABLE", 503, ErrorCode::BackendUnavailable),
        ("DEADLINE_EXCEEDED", 504, ErrorCode::BackendTimeout),
        (
            "PERMISSION_DENIED",
            403,
            ErrorCode::ExecutionPermissionDenied,
        ),
    ];
    for (error_type, status, expected) in cases {
        let e = VendorError::new(VendorKind::Gemini, status, error_type);
        assert_eq!(
            m.map_to_abp(&e),
            expected,
            "Gemini error_type={error_type} should map to {expected:?}"
        );
    }
}

#[test]
fn http_status_fallback_covers_common_codes() {
    let m = VendorErrorMapper::new();
    let cases: Vec<(u16, ErrorCode)> = vec![
        (401, ErrorCode::BackendAuthFailed),
        (403, ErrorCode::ExecutionPermissionDenied),
        (404, ErrorCode::BackendModelNotFound),
        (408, ErrorCode::BackendTimeout),
        (422, ErrorCode::ContractSchemaViolation),
        (429, ErrorCode::BackendRateLimited),
        (500, ErrorCode::Internal),
        (502, ErrorCode::BackendUnavailable),
        (503, ErrorCode::BackendUnavailable),
        (504, ErrorCode::BackendTimeout),
    ];
    for (status, expected) in cases {
        let e = VendorError::new(VendorKind::Custom, status, "unknown");
        assert_eq!(
            m.map_to_abp(&e),
            expected,
            "HTTP {status} should map to {expected:?}"
        );
    }
}

#[test]
fn vendor_error_serde_round_trip() {
    let e =
        VendorError::new(VendorKind::OpenAi, 429, "rate_limit_exceeded").with_message("slow down");
    let json = serde_json::to_string(&e).unwrap();
    let back: VendorError = serde_json::from_str(&json).unwrap();
    assert_eq!(e, back);
}

#[test]
fn vendor_kind_serde_round_trip() {
    for kind in [
        VendorKind::OpenAi,
        VendorKind::Anthropic,
        VendorKind::Gemini,
        VendorKind::Custom,
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        let back: VendorKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }
}

// =========================================================================
// Context enrichment integration tests
// =========================================================================

#[test]
fn context_builder_preserves_original_error_fields() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
    let enriched = ErrorContextBuilder::from_error(err)
        .backend("openai")
        .build();
    assert_eq!(enriched.code, ErrorCode::BackendTimeout);
    assert_eq!(enriched.message, "timed out");
}

#[test]
fn enrich_extension_chain() {
    let enriched = AbpError::new(ErrorCode::BackendRateLimited, "429")
        .enrich()
        .backend("anthropic")
        .model("claude-3")
        .request_id("req-42")
        .retry_count(2)
        .elapsed_ms(1500)
        .http_status(429)
        .build();

    assert_eq!(enriched.context.len(), 6);
    assert_eq!(enriched.context["backend"], serde_json::json!("anthropic"));
    assert_eq!(enriched.context["model"], serde_json::json!("claude-3"));
}

#[test]
fn vendor_to_abp_error_enrichment_pipeline() {
    let mapper = VendorErrorMapper::new();
    let vendor_err = VendorError::new(VendorKind::OpenAi, 429, "rate_limit_exceeded")
        .with_message("Rate limit reached");

    let abp_err = mapper.to_abp_error(&vendor_err);
    let enriched = abp_err
        .enrich()
        .request_id("req-001")
        .retry_count(0)
        .build();

    assert_eq!(enriched.code, ErrorCode::BackendRateLimited);
    assert!(enriched.context.contains_key("vendor"));
    assert!(enriched.context.contains_key("request_id"));
    assert!(enriched.context.contains_key("retry_count"));
}

// =========================================================================
// Recovery plan integration tests
// =========================================================================

#[test]
fn recovery_plan_serde_round_trip() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendRateLimited);
    let plan = RecoveryPlan::from_classification(&cl);
    let json = serde_json::to_string(&plan).unwrap();
    let back: RecoveryPlan = serde_json::from_str(&json).unwrap();
    assert_eq!(plan, back);
}

#[test]
fn retry_policy_serde_round_trip() {
    let policy = RetryPolicy::default();
    let json = serde_json::to_string(&policy).unwrap();
    let back: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(policy, back);
}

#[test]
fn retriable_codes_have_retry_in_plan() {
    let c = ErrorClassifier::new();
    for code in [
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendCrashed,
    ] {
        let cl = c.classify(&code);
        let plan = RecoveryPlan::from_classification(&cl);
        assert!(
            plan.has_retry(),
            "{:?} should have a retry step in its plan",
            code
        );
    }
}

#[test]
fn fatal_auth_has_no_retry() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendAuthFailed);
    let plan = RecoveryPlan::from_classification(&cl);
    assert!(!plan.has_retry());
    assert!(plan.is_terminal());
}

#[test]
fn model_not_found_plan_has_change_model_and_fallback() {
    let c = ErrorClassifier::new();
    let cl = c.classify(&ErrorCode::BackendModelNotFound);
    let plan = RecoveryPlan::from_classification(&cl);
    assert!(
        plan.steps
            .iter()
            .any(|s| s.action == RecoveryAction::ChangeModel)
    );
    assert!(
        plan.steps
            .iter()
            .any(|s| s.action == RecoveryAction::Fallback)
    );
}

// =========================================================================
// Docs integration tests
// =========================================================================

#[test]
fn all_error_code_docs_are_non_trivial() {
    let all_codes = [
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
    for code in &all_codes {
        let doc = error_code_doc(code);
        assert!(
            doc.description.len() > doc.code.message().len(),
            "{:?} doc description should be longer than the short message",
            code
        );
    }
}

#[test]
fn end_to_end_vendor_classify_recover_document() {
    // Full pipeline: vendor error → ABP error → classify → recovery plan → docs
    let mapper = VendorErrorMapper::new();
    let vendor_err = VendorError::new(VendorKind::OpenAi, 429, "rate_limit_exceeded")
        .with_message("Rate limit hit");

    let abp_err = mapper.to_abp_error(&vendor_err);
    let enriched = abp_err.enrich().request_id("req-e2e").build();

    let classifier = ErrorClassifier::new();
    let classification = classifier.classify(&enriched.code);
    let plan = RecoveryPlan::from_classification(&classification);
    let doc = error_code_doc(&enriched.code);

    assert_eq!(enriched.code, ErrorCode::BackendRateLimited);
    assert!(plan.has_retry());
    assert!(!doc.description.is_empty());
    assert!(enriched.context.contains_key("request_id"));
}
