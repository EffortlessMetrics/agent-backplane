// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI Codex error response types.
//!
//! The Codex API uses the same error envelope as the OpenAI REST API.
//! This module provides first-class Rust types for the error taxonomy so
//! that ABP shims can produce wire-compatible error responses.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Error response envelope
// ---------------------------------------------------------------------------

/// Top-level error response returned by the Codex / OpenAI API.
///
/// ```json
/// { "error": { "message": "...", "type": "...", "param": null, "code": "..." } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ErrorResponse {
    /// The error detail object.
    pub error: ApiError,
}

/// The inner error object inside an [`ErrorResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ApiError {
    /// Human-readable error message.
    pub message: String,
    /// Error type (e.g. `invalid_request_error`, `authentication_error`).
    #[serde(rename = "type")]
    pub error_type: String,
    /// The parameter that caused the error, if applicable.
    pub param: Option<String>,
    /// Machine-readable error code (e.g. `invalid_api_key`, `model_not_found`).
    pub code: Option<String>,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.error_type, self.message)
    }
}

impl std::error::Error for ApiError {}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for ErrorResponse {}

// ---------------------------------------------------------------------------
// Error type constants
// ---------------------------------------------------------------------------

/// Error type string for invalid request parameters.
pub const INVALID_REQUEST_ERROR: &str = "invalid_request_error";

/// Error type string for authentication failures.
pub const AUTHENTICATION_ERROR: &str = "authentication_error";

/// Error type string for permission denied.
pub const PERMISSION_ERROR: &str = "permission_error";

/// Error type string for resource not found.
pub const NOT_FOUND_ERROR: &str = "not_found_error";

/// Error type string for rate limiting.
pub const RATE_LIMIT_ERROR: &str = "rate_limit_error";

/// Error type string for server-side failures.
pub const SERVER_ERROR: &str = "server_error";

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl ApiError {
    /// Create an invalid-request error.
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: INVALID_REQUEST_ERROR.into(),
            param: None,
            code: None,
        }
    }

    /// Create an invalid-request error pinpointing a specific parameter.
    #[must_use]
    pub fn invalid_param(param: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: INVALID_REQUEST_ERROR.into(),
            param: Some(param.into()),
            code: None,
        }
    }

    /// Create an authentication error.
    #[must_use]
    pub fn authentication(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: AUTHENTICATION_ERROR.into(),
            param: None,
            code: Some("invalid_api_key".into()),
        }
    }

    /// Create a not-found error (e.g. model not found).
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: NOT_FOUND_ERROR.into(),
            param: None,
            code: Some("model_not_found".into()),
        }
    }

    /// Create a rate-limit error.
    #[must_use]
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: RATE_LIMIT_ERROR.into(),
            param: None,
            code: Some("rate_limit_exceeded".into()),
        }
    }

    /// Create a server error.
    #[must_use]
    pub fn server_error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: SERVER_ERROR.into(),
            param: None,
            code: Some("server_error".into()),
        }
    }

    /// Return `true` if the error is transient and the request may be retried.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.error_type == RATE_LIMIT_ERROR || self.error_type == SERVER_ERROR
    }

    /// Wrap this error in an [`ErrorResponse`] envelope.
    #[must_use]
    pub fn into_response(self) -> ErrorResponse {
        ErrorResponse { error: self }
    }
}

impl ErrorResponse {
    /// Create an error response from an [`ApiError`].
    #[must_use]
    pub fn new(error: ApiError) -> Self {
        Self { error }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_serde_roundtrip() {
        let err = ApiError {
            message: "Invalid model".into(),
            error_type: "invalid_request_error".into(),
            param: Some("model".into()),
            code: Some("model_not_found".into()),
        };
        let json = serde_json::to_string(&err).unwrap();
        let parsed: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, err);
    }

    #[test]
    fn error_response_serde_roundtrip() {
        let resp = ErrorResponse {
            error: ApiError {
                message: "Unauthorized".into(),
                error_type: "authentication_error".into(),
                param: None,
                code: Some("invalid_api_key".into()),
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: ErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn error_response_matches_openai_json_format() {
        let resp = ApiError::invalid_request("You must provide a model parameter").into_response();
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""error""#));
        assert!(json.contains(r#""message""#));
        assert!(json.contains(r#""type""#));
    }

    #[test]
    fn deserialize_from_codex_json() {
        let json = r#"{
            "error": {
                "message": "Incorrect API key provided: sk-...xxxx.",
                "type": "authentication_error",
                "param": null,
                "code": "invalid_api_key"
            }
        }"#;
        let resp: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.error_type, "authentication_error");
        assert_eq!(resp.error.code.as_deref(), Some("invalid_api_key"));
        assert!(resp.error.param.is_none());
    }

    #[test]
    fn deserialize_rate_limit_error() {
        let json = r#"{
            "error": {
                "message": "Rate limit reached for codex-mini-latest.",
                "type": "rate_limit_error",
                "param": null,
                "code": "rate_limit_exceeded"
            }
        }"#;
        let resp: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.error_type, "rate_limit_error");
        assert_eq!(resp.error.code.as_deref(), Some("rate_limit_exceeded"));
    }

    #[test]
    fn convenience_invalid_request() {
        let err = ApiError::invalid_request("bad request");
        assert_eq!(err.error_type, INVALID_REQUEST_ERROR);
        assert!(err.param.is_none());
    }

    #[test]
    fn convenience_invalid_param() {
        let err = ApiError::invalid_param("model", "not found");
        assert_eq!(err.error_type, INVALID_REQUEST_ERROR);
        assert_eq!(err.param.as_deref(), Some("model"));
    }

    #[test]
    fn convenience_authentication() {
        let err = ApiError::authentication("bad key");
        assert_eq!(err.error_type, AUTHENTICATION_ERROR);
        assert_eq!(err.code.as_deref(), Some("invalid_api_key"));
    }

    #[test]
    fn convenience_not_found() {
        let err = ApiError::not_found("model not found");
        assert_eq!(err.error_type, NOT_FOUND_ERROR);
        assert_eq!(err.code.as_deref(), Some("model_not_found"));
    }

    #[test]
    fn convenience_rate_limit() {
        let err = ApiError::rate_limit("too many requests");
        assert_eq!(err.error_type, RATE_LIMIT_ERROR);
        assert!(err.is_retryable());
    }

    #[test]
    fn convenience_server_error() {
        let err = ApiError::server_error("internal failure");
        assert_eq!(err.error_type, SERVER_ERROR);
        assert!(err.is_retryable());
    }

    #[test]
    fn non_retryable_errors() {
        assert!(!ApiError::invalid_request("bad").is_retryable());
        assert!(!ApiError::authentication("denied").is_retryable());
        assert!(!ApiError::not_found("gone").is_retryable());
    }

    #[test]
    fn api_error_display() {
        let err = ApiError::invalid_request("bad input");
        let display = format!("{err}");
        assert!(display.contains("invalid_request_error"));
        assert!(display.contains("bad input"));
    }

    #[test]
    fn error_response_display() {
        let resp = ApiError::authentication("unauthorized").into_response();
        let display = format!("{resp}");
        assert!(display.contains("authentication_error"));
    }

    #[test]
    fn api_error_is_std_error() {
        let err = ApiError::invalid_request("test");
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn error_response_is_std_error() {
        let resp = ApiError::invalid_request("test").into_response();
        let _: &dyn std::error::Error = &resp;
    }

    #[test]
    fn into_response_wraps_correctly() {
        let err = ApiError::not_found("gone");
        let resp = err.clone().into_response();
        assert_eq!(resp.error, err);
    }

    #[test]
    fn error_response_new_equivalent_to_into_response() {
        let err = ApiError::server_error("boom");
        let resp1 = err.clone().into_response();
        let resp2 = ErrorResponse::new(err);
        assert_eq!(resp1, resp2);
    }

    #[test]
    fn null_param_and_code_serialize_correctly() {
        let err = ApiError {
            message: "test".into(),
            error_type: "server_error".into(),
            param: None,
            code: None,
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains(r#""param":null"#));
        assert!(json.contains(r#""code":null"#));
    }
}
