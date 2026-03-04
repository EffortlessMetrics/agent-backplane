#![allow(dead_code, unused_imports)]
//! Vendor-specific error mapping — translates HTTP status + response body from
//! OpenAI, Anthropic, and Gemini into [`AbpError`]s while preserving the
//! original vendor details in structured context.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{AbpError, ErrorCode};

/// Captured vendor-specific error details before translation into the ABP
/// error taxonomy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VendorError {
    /// Name of the vendor (e.g. `"openai"`, `"anthropic"`, `"gemini"`).
    pub vendor: String,
    /// HTTP status code returned by the vendor API.
    pub status: u16,
    /// Raw response body (may be JSON or plain text).
    pub body: String,
    /// Optional vendor-specific error code extracted from the body.
    pub vendor_code: Option<String>,
    /// Optional vendor-specific error message extracted from the body.
    pub vendor_message: Option<String>,
}

impl VendorError {
    /// Create a new [`VendorError`] with the given vendor name, status, and body.
    pub fn new(vendor: impl Into<String>, status: u16, body: impl Into<String>) -> Self {
        Self {
            vendor: vendor.into(),
            status,
            body: body.into(),
            vendor_code: None,
            vendor_message: None,
        }
    }

    /// Set the vendor-specific error code.
    pub fn with_vendor_code(mut self, code: impl Into<String>) -> Self {
        self.vendor_code = Some(code.into());
        self
    }

    /// Set the vendor-specific error message.
    pub fn with_vendor_message(mut self, message: impl Into<String>) -> Self {
        self.vendor_message = Some(message.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn attach_vendor_context(err: AbpError, ve: &VendorError) -> AbpError {
    let mut err = err
        .with_context("vendor", &ve.vendor)
        .with_context("vendor_status", ve.status);
    if let Some(ref vc) = ve.vendor_code {
        err = err.with_context("vendor_code", vc.as_str());
    }
    if let Some(ref vm) = ve.vendor_message {
        err = err.with_context("vendor_message", vm.as_str());
    }
    err.with_context("vendor_body", &ve.body)
}

fn error_code_for_status(status: u16) -> ErrorCode {
    match status {
        401 => ErrorCode::BackendAuthFailed,
        403 => ErrorCode::PolicyDenied,
        404 => ErrorCode::BackendModelNotFound,
        429 => ErrorCode::BackendRateLimited,
        408 | 504 => ErrorCode::BackendTimeout,
        500..=599 => ErrorCode::BackendUnavailable,
        _ => ErrorCode::Internal,
    }
}

/// Extract a string field from a JSON body, returning `None` on parse failure.
fn json_field(body: &str, field: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get(field)?.as_str().map(String::from))
}

/// Extract a nested string field like `error.type` from a JSON body.
fn json_nested(body: &str, outer: &str, inner: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get(outer)?.get(inner)?.as_str().map(String::from))
}

// ---------------------------------------------------------------------------
// OpenAI
// ---------------------------------------------------------------------------

/// Map an OpenAI API error response into an [`AbpError`].
///
/// OpenAI errors typically look like:
/// ```json
/// { "error": { "message": "...", "type": "...", "code": "..." } }
/// ```
pub fn map_openai_error(status: u16, body: &str) -> AbpError {
    let vendor_code = json_nested(body, "error", "type");
    let vendor_message = json_nested(body, "error", "message");

    let code = match (status, vendor_code.as_deref()) {
        (401, _) => ErrorCode::BackendAuthFailed,
        (429, _) => ErrorCode::BackendRateLimited,
        (404, _) => ErrorCode::BackendModelNotFound,
        (_, Some("insufficient_quota")) => ErrorCode::BackendRateLimited,
        (_, Some("invalid_request_error")) => ErrorCode::ContractSchemaViolation,
        _ => error_code_for_status(status),
    };

    let message = vendor_message
        .as_deref()
        .unwrap_or("OpenAI API error")
        .to_string();

    let mut ve = VendorError::new("openai", status, body);
    if let Some(ref vc) = vendor_code {
        ve = ve.with_vendor_code(vc.as_str());
    }
    if let Some(ref vm) = vendor_message {
        ve = ve.with_vendor_message(vm.as_str());
    }

    attach_vendor_context(AbpError::new(code, message), &ve)
}

// ---------------------------------------------------------------------------
// Anthropic
// ---------------------------------------------------------------------------

/// Map an Anthropic API error response into an [`AbpError`].
///
/// Anthropic errors typically look like:
/// ```json
/// { "type": "error", "error": { "type": "...", "message": "..." } }
/// ```
pub fn map_anthropic_error(status: u16, body: &str) -> AbpError {
    let vendor_code = json_nested(body, "error", "type");
    let vendor_message = json_nested(body, "error", "message");

    let code = match (status, vendor_code.as_deref()) {
        (401, _) => ErrorCode::BackendAuthFailed,
        (429, _) => ErrorCode::BackendRateLimited,
        (_, Some("authentication_error")) => ErrorCode::BackendAuthFailed,
        (_, Some("rate_limit_error")) => ErrorCode::BackendRateLimited,
        (_, Some("not_found_error")) => ErrorCode::BackendModelNotFound,
        (_, Some("overloaded_error")) => ErrorCode::BackendUnavailable,
        (_, Some("invalid_request_error")) => ErrorCode::ContractSchemaViolation,
        _ => error_code_for_status(status),
    };

    let message = vendor_message
        .as_deref()
        .unwrap_or("Anthropic API error")
        .to_string();

    let mut ve = VendorError::new("anthropic", status, body);
    if let Some(ref vc) = vendor_code {
        ve = ve.with_vendor_code(vc.as_str());
    }
    if let Some(ref vm) = vendor_message {
        ve = ve.with_vendor_message(vm.as_str());
    }

    attach_vendor_context(AbpError::new(code, message), &ve)
}

// ---------------------------------------------------------------------------
// Gemini
// ---------------------------------------------------------------------------

/// Map a Gemini API error response into an [`AbpError`].
///
/// Gemini errors typically look like:
/// ```json
/// { "error": { "code": 429, "message": "...", "status": "RESOURCE_EXHAUSTED" } }
/// ```
pub fn map_gemini_error(status: u16, body: &str) -> AbpError {
    let vendor_status_str = json_nested(body, "error", "status");
    let vendor_message = json_nested(body, "error", "message");

    let code = match (status, vendor_status_str.as_deref()) {
        (401, _) | (_, Some("UNAUTHENTICATED")) => ErrorCode::BackendAuthFailed,
        (429, _) | (_, Some("RESOURCE_EXHAUSTED")) => ErrorCode::BackendRateLimited,
        (404, _) | (_, Some("NOT_FOUND")) => ErrorCode::BackendModelNotFound,
        (_, Some("PERMISSION_DENIED")) => ErrorCode::PolicyDenied,
        (_, Some("INVALID_ARGUMENT")) => ErrorCode::ContractSchemaViolation,
        _ => error_code_for_status(status),
    };

    let message = vendor_message
        .as_deref()
        .unwrap_or("Gemini API error")
        .to_string();

    let mut ve = VendorError::new("gemini", status, body);
    if let Some(ref vc) = vendor_status_str {
        ve = ve.with_vendor_code(vc.as_str());
    }
    if let Some(ref vm) = vendor_message {
        ve = ve.with_vendor_message(vm.as_str());
    }

    attach_vendor_context(AbpError::new(code, message), &ve)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- VendorError construction --------------------------------------

    #[test]
    fn vendor_error_new() {
        let ve = VendorError::new("openai", 429, r#"{"error":"rate limit"}"#);
        assert_eq!(ve.vendor, "openai");
        assert_eq!(ve.status, 429);
        assert!(ve.vendor_code.is_none());
        assert!(ve.vendor_message.is_none());
    }

    #[test]
    fn vendor_error_with_fields() {
        let ve = VendorError::new("anthropic", 401, "body")
            .with_vendor_code("authentication_error")
            .with_vendor_message("invalid api key");
        assert_eq!(ve.vendor_code.as_deref(), Some("authentication_error"));
        assert_eq!(ve.vendor_message.as_deref(), Some("invalid api key"));
    }

    #[test]
    fn vendor_error_serde_roundtrip() {
        let ve = VendorError::new("gemini", 500, "err")
            .with_vendor_code("INTERNAL")
            .with_vendor_message("server error");
        let json = serde_json::to_string(&ve).unwrap();
        let back: VendorError = serde_json::from_str(&json).unwrap();
        assert_eq!(ve, back);
    }

    // -- OpenAI mapping ------------------------------------------------

    #[test]
    fn openai_401_auth_failed() {
        let body = r#"{"error":{"message":"Incorrect API key","type":"invalid_api_key","code":"invalid_api_key"}}"#;
        let err = map_openai_error(401, body);
        assert_eq!(err.code, ErrorCode::BackendAuthFailed);
        assert!(err.context.contains_key("vendor"));
        assert_eq!(err.context["vendor"], serde_json::json!("openai"));
    }

    #[test]
    fn openai_429_rate_limited() {
        let body = r#"{"error":{"message":"Rate limit reached","type":"tokens","code":null}}"#;
        let err = map_openai_error(429, body);
        assert_eq!(err.code, ErrorCode::BackendRateLimited);
    }

    #[test]
    fn openai_404_model_not_found() {
        let body = r#"{"error":{"message":"model not found","type":"not_found"}}"#;
        let err = map_openai_error(404, body);
        assert_eq!(err.code, ErrorCode::BackendModelNotFound);
    }

    #[test]
    fn openai_insufficient_quota() {
        let body = r#"{"error":{"message":"You exceeded your quota","type":"insufficient_quota"}}"#;
        let err = map_openai_error(402, body);
        assert_eq!(err.code, ErrorCode::BackendRateLimited);
    }

    #[test]
    fn openai_invalid_request() {
        let body = r#"{"error":{"message":"bad request","type":"invalid_request_error"}}"#;
        let err = map_openai_error(400, body);
        assert_eq!(err.code, ErrorCode::ContractSchemaViolation);
    }

    #[test]
    fn openai_500_server_error() {
        let body = r#"{"error":{"message":"Internal error","type":"server_error"}}"#;
        let err = map_openai_error(500, body);
        assert_eq!(err.code, ErrorCode::BackendUnavailable);
    }

    #[test]
    fn openai_preserves_vendor_body() {
        let body = r#"{"error":{"message":"test","type":"test_type"}}"#;
        let err = map_openai_error(500, body);
        assert_eq!(err.context["vendor_body"], serde_json::json!(body));
    }

    #[test]
    fn openai_non_json_body() {
        let err = map_openai_error(503, "Service Unavailable");
        assert_eq!(err.code, ErrorCode::BackendUnavailable);
        assert_eq!(err.message, "OpenAI API error");
    }

    // -- Anthropic mapping ---------------------------------------------

    #[test]
    fn anthropic_401_auth_failed() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let err = map_anthropic_error(401, body);
        assert_eq!(err.code, ErrorCode::BackendAuthFailed);
        assert_eq!(err.context["vendor"], serde_json::json!("anthropic"));
    }

    #[test]
    fn anthropic_rate_limit_by_type() {
        let body =
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"rate limited"}}"#;
        let err = map_anthropic_error(429, body);
        assert_eq!(err.code, ErrorCode::BackendRateLimited);
    }

    #[test]
    fn anthropic_not_found() {
        let body =
            r#"{"type":"error","error":{"type":"not_found_error","message":"model not found"}}"#;
        let err = map_anthropic_error(404, body);
        assert_eq!(err.code, ErrorCode::BackendModelNotFound);
    }

    #[test]
    fn anthropic_overloaded() {
        let body = r#"{"type":"error","error":{"type":"overloaded_error","message":"overloaded"}}"#;
        let err = map_anthropic_error(529, body);
        assert_eq!(err.code, ErrorCode::BackendUnavailable);
    }

    #[test]
    fn anthropic_invalid_request() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad"}}"#;
        let err = map_anthropic_error(400, body);
        assert_eq!(err.code, ErrorCode::ContractSchemaViolation);
    }

    #[test]
    fn anthropic_non_json_body() {
        let err = map_anthropic_error(502, "Bad Gateway");
        assert_eq!(err.code, ErrorCode::BackendUnavailable);
        assert_eq!(err.message, "Anthropic API error");
    }

    // -- Gemini mapping ------------------------------------------------

    #[test]
    fn gemini_401_unauthenticated() {
        let body =
            r#"{"error":{"code":401,"message":"API key invalid","status":"UNAUTHENTICATED"}}"#;
        let err = map_gemini_error(401, body);
        assert_eq!(err.code, ErrorCode::BackendAuthFailed);
        assert_eq!(err.context["vendor"], serde_json::json!("gemini"));
    }

    #[test]
    fn gemini_429_resource_exhausted() {
        let body =
            r#"{"error":{"code":429,"message":"Quota exceeded","status":"RESOURCE_EXHAUSTED"}}"#;
        let err = map_gemini_error(429, body);
        assert_eq!(err.code, ErrorCode::BackendRateLimited);
    }

    #[test]
    fn gemini_404_not_found() {
        let body = r#"{"error":{"code":404,"message":"Model not found","status":"NOT_FOUND"}}"#;
        let err = map_gemini_error(404, body);
        assert_eq!(err.code, ErrorCode::BackendModelNotFound);
    }

    #[test]
    fn gemini_permission_denied() {
        let body = r#"{"error":{"code":403,"message":"Forbidden","status":"PERMISSION_DENIED"}}"#;
        let err = map_gemini_error(403, body);
        assert_eq!(err.code, ErrorCode::PolicyDenied);
    }

    #[test]
    fn gemini_invalid_argument() {
        let body = r#"{"error":{"code":400,"message":"Bad input","status":"INVALID_ARGUMENT"}}"#;
        let err = map_gemini_error(400, body);
        assert_eq!(err.code, ErrorCode::ContractSchemaViolation);
    }

    #[test]
    fn gemini_500_server_error() {
        let body = r#"{"error":{"code":500,"message":"Internal","status":"INTERNAL"}}"#;
        let err = map_gemini_error(500, body);
        assert_eq!(err.code, ErrorCode::BackendUnavailable);
    }

    #[test]
    fn gemini_non_json_body() {
        let err = map_gemini_error(503, "unavailable");
        assert_eq!(err.code, ErrorCode::BackendUnavailable);
        assert_eq!(err.message, "Gemini API error");
    }

    // -- Edge cases ----------------------------------------------------

    #[test]
    fn empty_body() {
        let err = map_openai_error(500, "");
        assert_eq!(err.code, ErrorCode::BackendUnavailable);

        let err = map_anthropic_error(500, "");
        assert_eq!(err.code, ErrorCode::BackendUnavailable);

        let err = map_gemini_error(500, "");
        assert_eq!(err.code, ErrorCode::BackendUnavailable);
    }

    #[test]
    fn unknown_status_code() {
        let err = map_openai_error(418, "I'm a teapot");
        assert_eq!(err.code, ErrorCode::Internal);
    }

    #[test]
    fn timeout_status_codes() {
        let err = map_openai_error(408, "timeout");
        assert_eq!(err.code, ErrorCode::BackendTimeout);
        let err = map_openai_error(504, "gateway timeout");
        assert_eq!(err.code, ErrorCode::BackendTimeout);
    }
}
