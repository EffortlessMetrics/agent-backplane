// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-SDK error taxonomy tests.
//!
//! Verifies that every SDK error type maps correctly into the ABP error
//! taxonomy, that retryability is consistent, and that diagnostic detail
//! is preserved through vendor-map translation.

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

use abp_claude_sdk::errors as claude;
use abp_codex_sdk::error as codex;
use abp_error::category::{RecoveryCategory, categorize, is_retryable, suggested_delay};
use abp_error::vendor_map::{
    VendorError, map_anthropic_error, map_codex_error, map_copilot_error, map_gemini_error,
    map_kimi_error, map_openai_error,
};
use abp_error::{AbpError, ErrorCode};
use abp_gemini_sdk::error as gemini;
use abp_openai_sdk::error as openai;
use std::time::Duration;

// ===================================================================
// 1. Every SDK error type maps to an ABP ErrorCode
// ===================================================================

#[test]
fn openai_all_error_types_map_to_abp_code() {
    let cases: &[(&str, u16, ErrorCode)] = &[
        (
            openai::INVALID_REQUEST_ERROR,
            400,
            ErrorCode::ContractSchemaViolation,
        ),
        (
            openai::AUTHENTICATION_ERROR,
            401,
            ErrorCode::BackendAuthFailed,
        ),
        (
            openai::NOT_FOUND_ERROR,
            404,
            ErrorCode::BackendModelNotFound,
        ),
        (openai::RATE_LIMIT_ERROR, 429, ErrorCode::BackendRateLimited),
        (openai::SERVER_ERROR, 500, ErrorCode::BackendUnavailable),
    ];
    for &(error_type, status, expected_code) in cases {
        let body = format!(
            r#"{{"error":{{"message":"test","type":"{}","param":null,"code":null}}}}"#,
            error_type
        );
        let err = map_openai_error(status, &body);
        assert_eq!(
            err.code, expected_code,
            "OpenAI error_type={error_type} status={status} expected {expected_code:?}"
        );
    }
}

#[test]
fn claude_all_error_types_map_to_abp_code() {
    let cases: &[(claude::ErrorType, u16, ErrorCode)] = &[
        (
            claude::ErrorType::InvalidRequestError,
            400,
            ErrorCode::ContractSchemaViolation,
        ),
        (
            claude::ErrorType::AuthenticationError,
            401,
            ErrorCode::BackendAuthFailed,
        ),
        (
            claude::ErrorType::NotFoundError,
            404,
            ErrorCode::BackendModelNotFound,
        ),
        (
            claude::ErrorType::RateLimitError,
            429,
            ErrorCode::BackendRateLimited,
        ),
        (
            claude::ErrorType::OverloadedError,
            529,
            ErrorCode::BackendUnavailable,
        ),
        (
            claude::ErrorType::ApiError,
            500,
            ErrorCode::BackendUnavailable,
        ),
    ];
    for &(et, status, expected_code) in cases {
        let body = format!(
            r#"{{"type":"error","error":{{"type":"{}","message":"test"}}}}"#,
            et.as_str()
        );
        let err = map_anthropic_error(status, &body);
        assert_eq!(
            err.code, expected_code,
            "Claude error_type={et:?} status={status} expected {expected_code:?}"
        );
    }
}

#[test]
fn gemini_all_error_statuses_map_to_abp_code() {
    let cases: &[(gemini::GeminiErrorStatus, u16, ErrorCode)] = &[
        (
            gemini::GeminiErrorStatus::InvalidArgument,
            400,
            ErrorCode::ContractSchemaViolation,
        ),
        (
            gemini::GeminiErrorStatus::Unauthenticated,
            401,
            ErrorCode::BackendAuthFailed,
        ),
        (
            gemini::GeminiErrorStatus::PermissionDenied,
            403,
            ErrorCode::PolicyDenied,
        ),
        (
            gemini::GeminiErrorStatus::NotFound,
            404,
            ErrorCode::BackendModelNotFound,
        ),
        (
            gemini::GeminiErrorStatus::ResourceExhausted,
            429,
            ErrorCode::BackendRateLimited,
        ),
        (
            gemini::GeminiErrorStatus::Internal,
            500,
            ErrorCode::BackendUnavailable,
        ),
        (
            gemini::GeminiErrorStatus::Unavailable,
            503,
            ErrorCode::BackendUnavailable,
        ),
    ];
    for (status_enum, http, expected_code) in cases {
        let body =
            format!(r#"{{"error":{{"code":{http},"message":"test","status":"{status_enum}"}}}}"#,);
        let err = map_gemini_error(*http, &body);
        assert_eq!(
            err.code, *expected_code,
            "Gemini status={status_enum} http={http} expected {expected_code:?}"
        );
    }
}

#[test]
fn codex_all_error_types_map_to_abp_code() {
    let cases: &[(&str, u16, ErrorCode)] = &[
        (
            codex::INVALID_REQUEST_ERROR,
            400,
            ErrorCode::ContractSchemaViolation,
        ),
        (
            codex::AUTHENTICATION_ERROR,
            401,
            ErrorCode::BackendAuthFailed,
        ),
        (codex::NOT_FOUND_ERROR, 404, ErrorCode::BackendModelNotFound),
        (codex::RATE_LIMIT_ERROR, 429, ErrorCode::BackendRateLimited),
        (codex::SERVER_ERROR, 500, ErrorCode::BackendUnavailable),
    ];
    for &(error_type, status, expected_code) in cases {
        let body = format!(
            r#"{{"error":{{"message":"test","type":"{}","param":null,"code":null}}}}"#,
            error_type
        );
        let err = map_codex_error(status, &body);
        assert_eq!(
            err.code, expected_code,
            "Codex error_type={error_type} status={status} expected {expected_code:?}"
        );
    }
}

// ===================================================================
// 2. Rate-limit errors classified consistently across all SDKs
// ===================================================================

#[test]
fn rate_limit_openai_maps_to_backend_rate_limited() {
    let body = r#"{"error":{"message":"Rate limit reached","type":"rate_limit_error","param":null,"code":"rate_limit_exceeded"}}"#;
    let err = map_openai_error(429, body);
    assert_eq!(err.code, ErrorCode::BackendRateLimited);
    assert_eq!(categorize(err.code), RecoveryCategory::RateLimit);
}

#[test]
fn rate_limit_claude_maps_to_backend_rate_limited() {
    let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limit hit"}}"#;
    let err = map_anthropic_error(429, body);
    assert_eq!(err.code, ErrorCode::BackendRateLimited);
    assert_eq!(categorize(err.code), RecoveryCategory::RateLimit);
}

#[test]
fn rate_limit_gemini_maps_to_backend_rate_limited() {
    let body = r#"{"error":{"code":429,"message":"Quota exceeded","status":"RESOURCE_EXHAUSTED"}}"#;
    let err = map_gemini_error(429, body);
    assert_eq!(err.code, ErrorCode::BackendRateLimited);
    assert_eq!(categorize(err.code), RecoveryCategory::RateLimit);
}

#[test]
fn rate_limit_codex_maps_to_backend_rate_limited() {
    let body = r#"{"error":{"message":"Rate limit reached","type":"rate_limit_error","param":null,"code":"rate_limit_exceeded"}}"#;
    let err = map_codex_error(429, body);
    assert_eq!(err.code, ErrorCode::BackendRateLimited);
    assert_eq!(categorize(err.code), RecoveryCategory::RateLimit);
}

#[test]
fn rate_limit_copilot_maps_to_backend_rate_limited() {
    let body = r#"{"message":"Too many requests"}"#;
    let err = map_copilot_error(429, body);
    assert_eq!(err.code, ErrorCode::BackendRateLimited);
    assert_eq!(categorize(err.code), RecoveryCategory::RateLimit);
}

#[test]
fn rate_limit_kimi_maps_to_backend_rate_limited() {
    let body = r#"{"error":{"message":"Rate limit","type":"rate_limit_error"}}"#;
    let err = map_kimi_error(429, body);
    assert_eq!(err.code, ErrorCode::BackendRateLimited);
    assert_eq!(categorize(err.code), RecoveryCategory::RateLimit);
}

// ===================================================================
// 3. Authentication errors map consistently
// ===================================================================

#[test]
fn auth_openai_maps_consistently() {
    let body = r#"{"error":{"message":"Invalid API key","type":"invalid_api_key"}}"#;
    let err = map_openai_error(401, body);
    assert_eq!(err.code, ErrorCode::BackendAuthFailed);
    assert_eq!(categorize(err.code), RecoveryCategory::Authentication);
}

#[test]
fn auth_claude_maps_consistently() {
    let body =
        r#"{"type":"error","error":{"type":"authentication_error","message":"invalid key"}}"#;
    let err = map_anthropic_error(401, body);
    assert_eq!(err.code, ErrorCode::BackendAuthFailed);
    assert_eq!(categorize(err.code), RecoveryCategory::Authentication);
}

#[test]
fn auth_gemini_maps_consistently() {
    let body = r#"{"error":{"code":401,"message":"API key invalid","status":"UNAUTHENTICATED"}}"#;
    let err = map_gemini_error(401, body);
    assert_eq!(err.code, ErrorCode::BackendAuthFailed);
    assert_eq!(categorize(err.code), RecoveryCategory::Authentication);
}

#[test]
fn auth_codex_maps_consistently() {
    let body = r#"{"error":{"message":"Bad key","type":"authentication_error","param":null,"code":"invalid_api_key"}}"#;
    let err = map_codex_error(401, body);
    assert_eq!(err.code, ErrorCode::BackendAuthFailed);
    assert_eq!(categorize(err.code), RecoveryCategory::Authentication);
}

#[test]
fn auth_copilot_maps_consistently() {
    let body = r#"{"message":"Unauthorized"}"#;
    let err = map_copilot_error(401, body);
    assert_eq!(err.code, ErrorCode::BackendAuthFailed);
    assert_eq!(categorize(err.code), RecoveryCategory::Authentication);
}

// ===================================================================
// 4. Server errors map consistently
// ===================================================================

#[test]
fn server_error_openai_maps_to_network_transient() {
    let body = r#"{"error":{"message":"Internal error","type":"server_error"}}"#;
    let err = map_openai_error(500, body);
    assert_eq!(err.code, ErrorCode::BackendUnavailable);
    assert_eq!(categorize(err.code), RecoveryCategory::NetworkTransient);
}

#[test]
fn server_error_claude_overloaded_maps_to_network_transient() {
    let body =
        r#"{"type":"error","error":{"type":"overloaded_error","message":"Service overloaded"}}"#;
    let err = map_anthropic_error(529, body);
    assert_eq!(err.code, ErrorCode::BackendUnavailable);
    assert_eq!(categorize(err.code), RecoveryCategory::NetworkTransient);
}

#[test]
fn server_error_gemini_unavailable_maps_to_network_transient() {
    let body = r#"{"error":{"code":503,"message":"Service unavailable","status":"UNAVAILABLE"}}"#;
    let err = map_gemini_error(503, body);
    assert_eq!(err.code, ErrorCode::BackendUnavailable);
    assert_eq!(categorize(err.code), RecoveryCategory::NetworkTransient);
}

#[test]
fn server_error_codex_maps_to_network_transient() {
    let body = r#"{"error":{"message":"Internal failure","type":"server_error"}}"#;
    let err = map_codex_error(500, body);
    assert_eq!(err.code, ErrorCode::BackendUnavailable);
    assert_eq!(categorize(err.code), RecoveryCategory::NetworkTransient);
}

// ===================================================================
// 5. Retryability consistency: retryable in SDK ↔ retryable in ABP
// ===================================================================

#[test]
fn claude_retryable_errors_map_to_retryable_abp_codes() {
    let retryable_types = [
        claude::ErrorType::RateLimitError,
        claude::ErrorType::OverloadedError,
        claude::ErrorType::ApiError,
    ];
    for et in retryable_types {
        assert!(
            et.is_retryable(),
            "{et:?} should be retryable in Claude SDK"
        );
        let body = format!(
            r#"{{"type":"error","error":{{"type":"{}","message":"test"}}}}"#,
            et.as_str()
        );
        let abp_err = map_anthropic_error(et.http_status(), &body);
        let cat = categorize(abp_err.code);
        assert!(
            is_retryable(cat),
            "Claude {et:?} -> ABP {:?} (cat={cat:?}) should be retryable",
            abp_err.code
        );
    }
}

#[test]
fn claude_non_retryable_errors_map_to_non_retryable_abp_codes() {
    let non_retryable_types = [
        claude::ErrorType::InvalidRequestError,
        claude::ErrorType::AuthenticationError,
        claude::ErrorType::PermissionError,
        claude::ErrorType::NotFoundError,
    ];
    for et in non_retryable_types {
        assert!(
            !et.is_retryable(),
            "{et:?} should NOT be retryable in Claude SDK"
        );
        let body = format!(
            r#"{{"type":"error","error":{{"type":"{}","message":"test"}}}}"#,
            et.as_str()
        );
        let abp_err = map_anthropic_error(et.http_status(), &body);
        let cat = categorize(abp_err.code);
        assert!(
            !is_retryable(cat),
            "Claude {et:?} -> ABP {:?} (cat={cat:?}) should NOT be retryable",
            abp_err.code
        );
    }
}

#[test]
fn codex_retryable_errors_map_to_retryable_abp_codes() {
    let rate_limit = codex::ApiError::rate_limit("rate limited");
    assert!(rate_limit.is_retryable());
    let abp_err = map_codex_error(
        429,
        &serde_json::to_string(&rate_limit.into_response()).unwrap(),
    );
    assert!(is_retryable(categorize(abp_err.code)));

    let server = codex::ApiError::server_error("internal");
    assert!(server.is_retryable());
    let abp_err = map_codex_error(
        500,
        &serde_json::to_string(&server.into_response()).unwrap(),
    );
    assert!(is_retryable(categorize(abp_err.code)));
}

#[test]
fn codex_non_retryable_errors_map_to_non_retryable_abp_codes() {
    let errors = [
        codex::ApiError::invalid_request("bad"),
        codex::ApiError::authentication("denied"),
        codex::ApiError::not_found("gone"),
    ];
    for sdk_err in errors {
        assert!(!sdk_err.is_retryable());
        let resp = sdk_err.clone().into_response();
        let body = serde_json::to_string(&resp).unwrap();
        // Map status codes: invalid_request->400, auth->401, not_found->404
        let status = match sdk_err.error_type.as_str() {
            codex::AUTHENTICATION_ERROR => 401,
            codex::NOT_FOUND_ERROR => 404,
            _ => 400,
        };
        let abp_err = map_codex_error(status, &body);
        assert!(
            !is_retryable(categorize(abp_err.code)),
            "Codex {:?} should NOT be retryable in ABP",
            sdk_err.error_type
        );
    }
}

#[test]
fn gemini_retryable_errors_map_to_retryable_abp_codes() {
    let retryable_statuses = [
        (gemini::GeminiErrorStatus::ResourceExhausted, 429),
        (gemini::GeminiErrorStatus::Unavailable, 503),
        (gemini::GeminiErrorStatus::Internal, 500),
    ];
    for (gs, http) in retryable_statuses {
        let resp = gemini::GeminiErrorResponse {
            error: gemini::GeminiErrorDetail {
                code: http,
                message: "test".into(),
                status: gs,
                details: None,
            },
        };
        assert!(
            gemini::is_retryable(&resp),
            "{gs:?} should be retryable in Gemini SDK"
        );
        let body = serde_json::to_string(&resp).unwrap();
        let abp_err = map_gemini_error(http, &body);
        assert!(
            is_retryable(categorize(abp_err.code)),
            "Gemini {gs:?} -> ABP {:?} should be retryable",
            abp_err.code
        );
    }
}

#[test]
fn gemini_non_retryable_errors_map_to_non_retryable_abp_codes() {
    let non_retryable = [
        (gemini::GeminiErrorStatus::InvalidArgument, 400),
        (gemini::GeminiErrorStatus::Unauthenticated, 401),
        (gemini::GeminiErrorStatus::PermissionDenied, 403),
        (gemini::GeminiErrorStatus::NotFound, 404),
    ];
    for (gs, http) in non_retryable {
        let resp = gemini::GeminiErrorResponse {
            error: gemini::GeminiErrorDetail {
                code: http,
                message: "test".into(),
                status: gs,
                details: None,
            },
        };
        assert!(
            !gemini::is_retryable(&resp),
            "{gs:?} should NOT be retryable"
        );
        let body = serde_json::to_string(&resp).unwrap();
        let abp_err = map_gemini_error(http, &body);
        assert!(
            !is_retryable(categorize(abp_err.code)),
            "Gemini {gs:?} -> ABP {:?} should NOT be retryable",
            abp_err.code
        );
    }
}

// ===================================================================
// 6. Error messages preserve diagnostic detail
// ===================================================================

#[test]
fn openai_preserves_error_message() {
    let body =
        r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota"}}"#;
    let err = map_openai_error(402, body);
    assert_eq!(err.message, "You exceeded your current quota");
}

#[test]
fn claude_preserves_error_message() {
    let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"Number of request tokens has exceeded your per-minute rate limit"}}"#;
    let err = map_anthropic_error(429, body);
    assert!(err.message.contains("request tokens"));
}

#[test]
fn gemini_preserves_error_message() {
    let body = r#"{"error":{"code":400,"message":"Invalid value at 'contents[0].parts'","status":"INVALID_ARGUMENT"}}"#;
    let err = map_gemini_error(400, body);
    assert!(err.message.contains("contents[0].parts"));
}

#[test]
fn codex_preserves_error_message() {
    let body = r#"{"error":{"message":"This model's maximum context length is 4096 tokens","type":"context_length_exceeded"}}"#;
    let err = map_codex_error(400, body);
    assert!(err.message.contains("4096 tokens"));
}

#[test]
fn vendor_context_preserved_in_abp_error() {
    let body = r#"{"error":{"message":"test","type":"server_error"}}"#;
    let err = map_openai_error(500, body);
    assert_eq!(err.context["vendor"], serde_json::json!("openai"));
    assert_eq!(err.context["vendor_status"], serde_json::json!(500));
    assert!(err.context.contains_key("vendor_body"));
}

// ===================================================================
// 7. HTTP status code mapping
// ===================================================================

#[test]
fn http_401_maps_to_auth_failed_across_all_sdks() {
    let vendors: Vec<(&str, fn(u16, &str) -> AbpError)> = vec![
        ("openai", map_openai_error),
        ("anthropic", map_anthropic_error),
        ("gemini", map_gemini_error),
        ("codex", map_codex_error),
        ("copilot", map_copilot_error),
        ("kimi", map_kimi_error),
    ];
    for (name, mapper) in vendors {
        let err = mapper(401, "{}");
        assert_eq!(
            err.code,
            ErrorCode::BackendAuthFailed,
            "{name}: 401 should map to BackendAuthFailed"
        );
    }
}

#[test]
fn http_429_maps_to_rate_limited_across_all_sdks() {
    let vendors: Vec<(&str, fn(u16, &str) -> AbpError)> = vec![
        ("openai", map_openai_error),
        ("anthropic", map_anthropic_error),
        ("gemini", map_gemini_error),
        ("codex", map_codex_error),
        ("copilot", map_copilot_error),
        ("kimi", map_kimi_error),
    ];
    for (name, mapper) in vendors {
        let err = mapper(429, "{}");
        assert_eq!(
            err.code,
            ErrorCode::BackendRateLimited,
            "{name}: 429 should map to BackendRateLimited"
        );
    }
}

#[test]
fn http_404_maps_to_model_not_found_across_all_sdks() {
    let vendors: Vec<(&str, fn(u16, &str) -> AbpError)> = vec![
        ("openai", map_openai_error),
        ("anthropic", map_anthropic_error),
        ("gemini", map_gemini_error),
        ("codex", map_codex_error),
        ("copilot", map_copilot_error),
        ("kimi", map_kimi_error),
    ];
    for (name, mapper) in vendors {
        let err = mapper(404, "{}");
        assert_eq!(
            err.code,
            ErrorCode::BackendModelNotFound,
            "{name}: 404 should map to BackendModelNotFound"
        );
    }
}

#[test]
fn http_500_maps_to_backend_unavailable_across_sdks() {
    let vendors: Vec<(&str, fn(u16, &str) -> AbpError)> = vec![
        ("openai", map_openai_error),
        ("anthropic", map_anthropic_error),
        ("gemini", map_gemini_error),
        ("codex", map_codex_error),
        ("copilot", map_copilot_error),
        ("kimi", map_kimi_error),
    ];
    for (name, mapper) in vendors {
        let err = mapper(500, "{}");
        assert_eq!(
            err.code,
            ErrorCode::BackendUnavailable,
            "{name}: 500 should map to BackendUnavailable"
        );
    }
}

#[test]
fn http_408_504_map_to_timeout() {
    for status in [408, 504] {
        let err = map_openai_error(status, "{}");
        assert_eq!(
            err.code,
            ErrorCode::BackendTimeout,
            "HTTP {status} should map to BackendTimeout"
        );
    }
}

// ===================================================================
// 8. Serde roundtrip for all SDK error responses
// ===================================================================

#[test]
fn openai_error_response_serde_roundtrip() {
    let resp = openai::ApiError::rate_limit("rate limited").into_response();
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: openai::ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn claude_error_response_serde_roundtrip() {
    let resp = claude::ErrorResponse::rate_limit("rate limited");
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: claude::ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn gemini_error_response_serde_roundtrip() {
    let resp = gemini::GeminiErrorResponse {
        error: gemini::GeminiErrorDetail {
            code: 429,
            message: "Quota exceeded".into(),
            status: gemini::GeminiErrorStatus::ResourceExhausted,
            details: None,
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: gemini::GeminiErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn codex_error_response_serde_roundtrip() {
    let resp = codex::ApiError::server_error("internal").into_response();
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: codex::ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn vendor_error_serde_roundtrip() {
    let ve = VendorError::new("openai", 429, r#"{"error":"rate limit"}"#)
        .with_vendor_code("rate_limit_error")
        .with_vendor_message("Rate limit reached");
    let json = serde_json::to_string(&ve).unwrap();
    let parsed: VendorError = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ve);
}

// ===================================================================
// 9. Codex-specific: context-length and content-filter mapping
// ===================================================================

#[test]
fn codex_context_length_exceeded_maps_correctly() {
    let body =
        r#"{"error":{"message":"Context length exceeded","type":"context_length_exceeded"}}"#;
    let err = map_codex_error(400, body);
    assert_eq!(err.code, ErrorCode::BackendContextLength);
    assert_eq!(categorize(err.code), RecoveryCategory::InputValidation);
    assert!(!is_retryable(categorize(err.code)));
}

#[test]
fn codex_content_filter_maps_correctly() {
    let body = r#"{"error":{"message":"Content filtered","type":"content_filter"}}"#;
    let err = map_codex_error(400, body);
    assert_eq!(err.code, ErrorCode::BackendContentFiltered);
    assert_eq!(categorize(err.code), RecoveryCategory::PolicyViolation);
}

// ===================================================================
// 10. Copilot-specific: auth and content-filter
// ===================================================================

#[test]
fn copilot_403_maps_to_auth_failed() {
    let body = r#"{"message":"Forbidden"}"#;
    let err = map_copilot_error(403, body);
    assert_eq!(err.code, ErrorCode::BackendAuthFailed);
}

#[test]
fn copilot_content_filter_maps_correctly() {
    let body = r#"{"error":{"type":"content_filter","message":"blocked"}}"#;
    let err = map_copilot_error(400, body);
    assert_eq!(err.code, ErrorCode::BackendContentFiltered);
}

// ===================================================================
// 11. Recovery category suggested delays are consistent
// ===================================================================

#[test]
fn rate_limit_suggested_delay_is_30s() {
    assert_eq!(
        suggested_delay(RecoveryCategory::RateLimit),
        Duration::from_secs(30)
    );
}

#[test]
fn network_transient_suggested_delay_is_2s() {
    assert_eq!(
        suggested_delay(RecoveryCategory::NetworkTransient),
        Duration::from_secs(2)
    );
}

#[test]
fn auth_errors_have_zero_delay() {
    assert_eq!(
        suggested_delay(RecoveryCategory::Authentication),
        Duration::ZERO
    );
}

// ===================================================================
// 12. Non-JSON body handling across SDKs
// ===================================================================

#[test]
fn non_json_body_falls_back_to_default_message_all_sdks() {
    let cases: Vec<(&str, fn(u16, &str) -> AbpError, &str)> = vec![
        ("openai", map_openai_error, "OpenAI API error"),
        ("anthropic", map_anthropic_error, "Anthropic API error"),
        ("gemini", map_gemini_error, "Gemini API error"),
        ("codex", map_codex_error, "Codex API error"),
        ("copilot", map_copilot_error, "Copilot API error"),
        ("kimi", map_kimi_error, "Kimi API error"),
    ];
    for (name, mapper, default_msg) in cases {
        let err = mapper(500, "Service Unavailable");
        assert_eq!(
            err.message, default_msg,
            "{name}: non-JSON body should use default message"
        );
    }
}

// ===================================================================
// 13. ErrorCode properties
// ===================================================================

#[test]
fn backend_rate_limited_is_retryable_via_error_code() {
    assert!(ErrorCode::BackendRateLimited.is_retryable());
}

#[test]
fn backend_auth_failed_is_not_retryable_via_error_code() {
    assert!(!ErrorCode::BackendAuthFailed.is_retryable());
}

#[test]
fn backend_unavailable_is_retryable_via_error_code() {
    assert!(ErrorCode::BackendUnavailable.is_retryable());
}

#[test]
fn contract_schema_violation_is_not_retryable() {
    assert!(!ErrorCode::ContractSchemaViolation.is_retryable());
}

// ===================================================================
// 14. Claude ErrorType ↔ http_status roundtrip
// ===================================================================

#[test]
fn claude_error_type_http_status_roundtrip() {
    let types = [
        (claude::ErrorType::InvalidRequestError, 400),
        (claude::ErrorType::AuthenticationError, 401),
        (claude::ErrorType::PermissionError, 403),
        (claude::ErrorType::NotFoundError, 404),
        (claude::ErrorType::RequestTooLarge, 413),
        (claude::ErrorType::RateLimitError, 429),
        (claude::ErrorType::ApiError, 500),
        (claude::ErrorType::OverloadedError, 529),
    ];
    for (et, expected_status) in types {
        assert_eq!(et.http_status(), expected_status, "{et:?}");
    }
}

// ===================================================================
// 15. Gemini is_rate_limited helper
// ===================================================================

#[test]
fn gemini_is_rate_limited_only_for_resource_exhausted() {
    let rate_limited = gemini::GeminiErrorResponse {
        error: gemini::GeminiErrorDetail {
            code: 429,
            message: "quota".into(),
            status: gemini::GeminiErrorStatus::ResourceExhausted,
            details: None,
        },
    };
    assert!(gemini::is_rate_limited(&rate_limited));

    let not_rate_limited = gemini::GeminiErrorResponse {
        error: gemini::GeminiErrorDetail {
            code: 500,
            message: "error".into(),
            status: gemini::GeminiErrorStatus::Internal,
            details: None,
        },
    };
    assert!(!gemini::is_rate_limited(&not_rate_limited));
}

// ===================================================================
// 16. Cross-SDK: openai quota exhaustion maps like rate-limit
// ===================================================================

#[test]
fn openai_insufficient_quota_maps_same_as_rate_limit() {
    let quota_body =
        r#"{"error":{"message":"You exceeded your quota","type":"insufficient_quota"}}"#;
    let quota_err = map_openai_error(402, quota_body);

    let rate_body = r#"{"error":{"message":"Rate limit","type":"tokens"}}"#;
    let rate_err = map_openai_error(429, rate_body);

    assert_eq!(quota_err.code, rate_err.code);
    assert_eq!(categorize(quota_err.code), categorize(rate_err.code));
}

// ===================================================================
// 17. Kimi-specific mappings
// ===================================================================

#[test]
fn kimi_content_filter_maps_correctly() {
    let body = r#"{"error":{"message":"blocked","type":"content_filter"}}"#;
    let err = map_kimi_error(400, body);
    assert_eq!(err.code, ErrorCode::BackendContentFiltered);
}

#[test]
fn kimi_context_length_exceeded_maps_correctly() {
    let body = r#"{"error":{"message":"too long","type":"context_length_exceeded"}}"#;
    let err = map_kimi_error(400, body);
    assert_eq!(err.code, ErrorCode::BackendContextLength);
}
