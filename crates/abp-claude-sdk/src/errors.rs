// SPDX-License-Identifier: MIT OR Apache-2.0
//! Anthropic API error types matching the official error format.
//!
//! The Anthropic API returns errors as JSON objects with a typed `error`
//! field. This module provides first-class Rust types for all documented
//! error kinds, enabling pattern matching on error categories.
//!
//! Reference: <https://docs.anthropic.com/en/api/errors>

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Error type enum
// ---------------------------------------------------------------------------

/// Typed error categories returned by the Anthropic API.
///
/// Each variant maps to a specific HTTP status code and error condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    /// 400 — The request body is invalid or malformed.
    InvalidRequestError,
    /// 401 — Authentication credentials are missing or invalid.
    AuthenticationError,
    /// 403 — The API key lacks permission for the requested resource.
    PermissionError,
    /// 404 — The requested resource was not found.
    NotFoundError,
    /// 409 — The request conflicts with another request.
    RequestTooLarge,
    /// 429 — Rate limit exceeded; back off and retry.
    RateLimitError,
    /// 500 — An unexpected internal server error occurred.
    ApiError,
    /// 529 — The API is temporarily overloaded.
    OverloadedError,
}

impl ErrorType {
    /// Return the HTTP status code typically associated with this error type.
    #[must_use]
    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidRequestError => 400,
            Self::AuthenticationError => 401,
            Self::PermissionError => 403,
            Self::NotFoundError => 404,
            Self::RequestTooLarge => 413,
            Self::RateLimitError => 429,
            Self::ApiError => 500,
            Self::OverloadedError => 529,
        }
    }

    /// Return the canonical string representation of this error type.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequestError => "invalid_request_error",
            Self::AuthenticationError => "authentication_error",
            Self::PermissionError => "permission_error",
            Self::NotFoundError => "not_found_error",
            Self::RequestTooLarge => "request_too_large",
            Self::RateLimitError => "rate_limit_error",
            Self::ApiError => "api_error",
            Self::OverloadedError => "overloaded_error",
        }
    }

    /// Returns `true` if the error is transient and the request may be retried.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimitError | Self::OverloadedError | Self::ApiError
        )
    }
}

impl fmt::Display for ErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Error detail
// ---------------------------------------------------------------------------

/// Detailed error object inside an Anthropic error response.
///
/// This is the `error` field within an [`ErrorResponse`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorDetail {
    /// Typed error category.
    #[serde(rename = "type")]
    pub error_type: ErrorType,
    /// Human-readable error message.
    pub message: String,
}

impl fmt::Display for ErrorDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.error_type, self.message)
    }
}

// ---------------------------------------------------------------------------
// Top-level error response
// ---------------------------------------------------------------------------

/// Top-level error response from the Anthropic API.
///
/// Matches the JSON envelope `{"type": "error", "error": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorResponse {
    /// Object type — always `"error"`.
    #[serde(rename = "type")]
    pub response_type: String,
    /// The error details.
    pub error: ErrorDetail,
}

impl ErrorResponse {
    /// Create a new error response with the given type and message.
    #[must_use]
    pub fn new(error_type: ErrorType, message: impl Into<String>) -> Self {
        Self {
            response_type: "error".into(),
            error: ErrorDetail {
                error_type,
                message: message.into(),
            },
        }
    }

    /// Convenience: create an `invalid_request_error`.
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ErrorType::InvalidRequestError, message)
    }

    /// Convenience: create an `authentication_error`.
    #[must_use]
    pub fn authentication(message: impl Into<String>) -> Self {
        Self::new(ErrorType::AuthenticationError, message)
    }

    /// Convenience: create a `permission_error`.
    #[must_use]
    pub fn permission(message: impl Into<String>) -> Self {
        Self::new(ErrorType::PermissionError, message)
    }

    /// Convenience: create a `not_found_error`.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorType::NotFoundError, message)
    }

    /// Convenience: create a `rate_limit_error`.
    #[must_use]
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self::new(ErrorType::RateLimitError, message)
    }

    /// Convenience: create an `overloaded_error`.
    #[must_use]
    pub fn overloaded(message: impl Into<String>) -> Self {
        Self::new(ErrorType::OverloadedError, message)
    }

    /// Convenience: create an `api_error`.
    #[must_use]
    pub fn api_error(message: impl Into<String>) -> Self {
        Self::new(ErrorType::ApiError, message)
    }

    /// Returns `true` if the error is transient and the request may be retried.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.error.error_type.is_retryable()
    }

    /// Return the HTTP status code for this error.
    #[must_use]
    pub fn http_status(&self) -> u16 {
        self.error.error_type.http_status()
    }
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_type_serde_roundtrip() {
        let types = [
            (ErrorType::InvalidRequestError, "\"invalid_request_error\""),
            (ErrorType::AuthenticationError, "\"authentication_error\""),
            (ErrorType::PermissionError, "\"permission_error\""),
            (ErrorType::NotFoundError, "\"not_found_error\""),
            (ErrorType::RequestTooLarge, "\"request_too_large\""),
            (ErrorType::RateLimitError, "\"rate_limit_error\""),
            (ErrorType::ApiError, "\"api_error\""),
            (ErrorType::OverloadedError, "\"overloaded_error\""),
        ];
        for (et, expected_json) in types {
            let json = serde_json::to_string(&et).unwrap();
            assert_eq!(json, expected_json, "serialization mismatch for {et:?}");
            let parsed: ErrorType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, et);
        }
    }

    #[test]
    fn error_type_http_status() {
        assert_eq!(ErrorType::InvalidRequestError.http_status(), 400);
        assert_eq!(ErrorType::AuthenticationError.http_status(), 401);
        assert_eq!(ErrorType::PermissionError.http_status(), 403);
        assert_eq!(ErrorType::NotFoundError.http_status(), 404);
        assert_eq!(ErrorType::RequestTooLarge.http_status(), 413);
        assert_eq!(ErrorType::RateLimitError.http_status(), 429);
        assert_eq!(ErrorType::ApiError.http_status(), 500);
        assert_eq!(ErrorType::OverloadedError.http_status(), 529);
    }

    #[test]
    fn error_type_retryable() {
        assert!(!ErrorType::InvalidRequestError.is_retryable());
        assert!(!ErrorType::AuthenticationError.is_retryable());
        assert!(!ErrorType::PermissionError.is_retryable());
        assert!(!ErrorType::NotFoundError.is_retryable());
        assert!(ErrorType::RateLimitError.is_retryable());
        assert!(ErrorType::OverloadedError.is_retryable());
        assert!(ErrorType::ApiError.is_retryable());
    }

    #[test]
    fn error_type_as_str() {
        assert_eq!(
            ErrorType::InvalidRequestError.as_str(),
            "invalid_request_error"
        );
        assert_eq!(ErrorType::OverloadedError.as_str(), "overloaded_error");
    }

    #[test]
    fn error_type_display() {
        assert_eq!(format!("{}", ErrorType::RateLimitError), "rate_limit_error");
    }

    #[test]
    fn error_detail_serde_roundtrip() {
        let detail = ErrorDetail {
            error_type: ErrorType::InvalidRequestError,
            message: "max_tokens must be positive".into(),
        };
        let json = serde_json::to_string(&detail).unwrap();
        let parsed: ErrorDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, detail);
    }

    #[test]
    fn error_detail_display() {
        let detail = ErrorDetail {
            error_type: ErrorType::RateLimitError,
            message: "Too many requests".into(),
        };
        assert_eq!(format!("{detail}"), "rate_limit_error: Too many requests");
    }

    #[test]
    fn error_response_serde_roundtrip() {
        let resp = ErrorResponse::new(
            ErrorType::OverloadedError,
            "Anthropic's API is temporarily overloaded",
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"overloaded_error\""));
        let parsed: ErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn error_response_matches_anthropic_format() {
        // Verify the JSON structure matches Anthropic's documented format:
        // {"type": "error", "error": {"type": "...", "message": "..."}}
        let resp = ErrorResponse::invalid_request("messages: required field missing");
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["message"], "messages: required field missing");
    }

    #[test]
    fn error_response_convenience_constructors() {
        let tests: Vec<(ErrorResponse, ErrorType)> = vec![
            (
                ErrorResponse::invalid_request("bad"),
                ErrorType::InvalidRequestError,
            ),
            (
                ErrorResponse::authentication("denied"),
                ErrorType::AuthenticationError,
            ),
            (
                ErrorResponse::permission("forbidden"),
                ErrorType::PermissionError,
            ),
            (ErrorResponse::not_found("gone"), ErrorType::NotFoundError),
            (
                ErrorResponse::rate_limit("slow down"),
                ErrorType::RateLimitError,
            ),
            (
                ErrorResponse::overloaded("busy"),
                ErrorType::OverloadedError,
            ),
            (ErrorResponse::api_error("oops"), ErrorType::ApiError),
        ];
        for (resp, expected_type) in tests {
            assert_eq!(resp.error.error_type, expected_type);
            assert_eq!(resp.response_type, "error");
        }
    }

    #[test]
    fn error_response_retryable() {
        assert!(!ErrorResponse::invalid_request("bad").is_retryable());
        assert!(ErrorResponse::rate_limit("slow down").is_retryable());
        assert!(ErrorResponse::overloaded("busy").is_retryable());
    }

    #[test]
    fn error_response_http_status() {
        assert_eq!(ErrorResponse::invalid_request("x").http_status(), 400);
        assert_eq!(ErrorResponse::rate_limit("x").http_status(), 429);
        assert_eq!(ErrorResponse::overloaded("x").http_status(), 529);
    }

    #[test]
    fn error_response_display() {
        let resp = ErrorResponse::overloaded("API is busy");
        assert_eq!(format!("{resp}"), "overloaded_error: API is busy");
    }

    #[test]
    fn error_response_from_real_anthropic_json() {
        // Test parsing a real Anthropic API error response
        let json = r#"{
            "type": "error",
            "error": {
                "type": "overloaded_error",
                "message": "Overloaded"
            }
        }"#;
        let parsed: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.response_type, "error");
        assert_eq!(parsed.error.error_type, ErrorType::OverloadedError);
        assert_eq!(parsed.error.message, "Overloaded");
    }

    #[test]
    fn error_type_json_schema_generates() {
        let schema = schemars::schema_for!(ErrorType);
        let json = serde_json::to_value(&schema).unwrap();
        let s = serde_json::to_string(&json).unwrap();
        assert!(s.contains("invalid_request_error") || s.contains("oneOf") || s.contains("enum"));
    }

    #[test]
    fn error_response_json_schema_generates() {
        let schema = schemars::schema_for!(ErrorResponse);
        let json = serde_json::to_value(&schema).unwrap();
        assert!(json.get("properties").is_some() || json.get("$defs").is_some());
    }
}
