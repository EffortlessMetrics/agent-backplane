// SPDX-License-Identifier: MIT OR Apache-2.0
//! Anthropic-compatible error types for the Claude shim.
//!
//! These types mirror the error shapes returned by the real Anthropic API,
//! enabling drop-in error handling compatibility.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Error kind enumeration
// ---------------------------------------------------------------------------

/// Anthropic API error type identifiers.
///
/// These correspond to the `type` field in the Anthropic error response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The request was invalid or malformed.
    InvalidRequestError,
    /// Authentication failed (bad API key).
    AuthenticationError,
    /// The account lacks permission for the requested resource.
    PermissionError,
    /// The requested resource was not found.
    NotFoundError,
    /// The request conflicts with another request.
    ConflictError,
    /// Rate limit exceeded.
    RateLimitError,
    /// Internal server error.
    ApiError,
    /// The API is temporarily overloaded.
    OverloadedError,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{self:?}"));
        f.write_str(&s)
    }
}

impl ErrorKind {
    /// Return the typical HTTP status code for this error kind.
    #[must_use]
    pub fn status_code(&self) -> u16 {
        match self {
            Self::InvalidRequestError => 400,
            Self::AuthenticationError => 401,
            Self::PermissionError => 403,
            Self::NotFoundError => 404,
            Self::ConflictError => 409,
            Self::RateLimitError => 429,
            Self::ApiError => 500,
            Self::OverloadedError => 529,
        }
    }

    /// Return `true` if this error is retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimitError | Self::ApiError | Self::OverloadedError
        )
    }
}

// ---------------------------------------------------------------------------
// Structured error response (mirrors Anthropic JSON)
// ---------------------------------------------------------------------------

/// The top-level error response body from the Anthropic API.
///
/// ```json
/// {
///   "type": "error",
///   "error": {
///     "type": "invalid_request_error",
///     "message": "max_tokens: must be positive"
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicErrorResponse {
    /// Always `"error"`.
    #[serde(rename = "type")]
    pub response_type: String,
    /// Nested error detail.
    pub error: AnthropicErrorBody,
}

/// The inner `error` object in an Anthropic error response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicErrorBody {
    /// Error type identifier.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable description.
    pub message: String,
}

impl AnthropicErrorResponse {
    /// Create a new error response.
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            response_type: "error".to_string(),
            error: AnthropicErrorBody {
                error_type: kind.to_string(),
                message: message.into(),
            },
        }
    }

    /// Create an invalid request error.
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidRequestError, message)
    }

    /// Create an authentication error.
    #[must_use]
    pub fn authentication(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::AuthenticationError, message)
    }

    /// Create a rate limit error.
    #[must_use]
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::RateLimitError, message)
    }

    /// Create an overloaded error.
    #[must_use]
    pub fn overloaded(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::OverloadedError, message)
    }

    /// Create a generic API error.
    #[must_use]
    pub fn api_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::ApiError, message)
    }
}

// ---------------------------------------------------------------------------
// Thiserror integration
// ---------------------------------------------------------------------------

/// Unified error type for the Claude shim.
#[derive(Debug, thiserror::Error)]
pub enum ClaudeShimError {
    /// Request validation failed before sending.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The Anthropic API returned a structured error.
    #[error("api error ({kind}): {message}")]
    Api {
        /// Error kind.
        kind: ErrorKind,
        /// Human-readable message.
        message: String,
        /// HTTP status code.
        status: u16,
        /// Full structured response, if available.
        response: Option<AnthropicErrorResponse>,
    },

    /// HTTP transport error.
    #[error("http error: {0}")]
    Http(String),

    /// SSE stream parse error.
    #[error("stream error: {0}")]
    Stream(String),

    /// Serialization/deserialization failure.
    #[error("serde error: {0}")]
    Serde(String),

    /// Internal conversion error.
    #[error("internal: {0}")]
    Internal(String),
}

impl ClaudeShimError {
    /// Return `true` if this is a rate-limit error (HTTP 429).
    #[must_use]
    pub fn is_rate_limit(&self) -> bool {
        matches!(
            self,
            Self::Api {
                kind: ErrorKind::RateLimitError,
                ..
            }
        )
    }

    /// Return `true` if this is an authentication error (HTTP 401).
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Self::Api {
                kind: ErrorKind::AuthenticationError,
                ..
            }
        )
    }

    /// Return `true` if this error is potentially retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Api { kind, .. } => kind.is_retryable(),
            Self::Http(_) => true,
            _ => false,
        }
    }

    /// Extract the HTTP status code, if applicable.
    #[must_use]
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Build from an HTTP status and response body string.
    #[must_use]
    pub fn from_status_and_body(status: u16, body: &str) -> Self {
        let parsed = serde_json::from_str::<AnthropicErrorResponse>(body).ok();
        let kind = match status {
            400 => ErrorKind::InvalidRequestError,
            401 => ErrorKind::AuthenticationError,
            403 => ErrorKind::PermissionError,
            404 => ErrorKind::NotFoundError,
            409 => ErrorKind::ConflictError,
            429 => ErrorKind::RateLimitError,
            529 => ErrorKind::OverloadedError,
            _ => ErrorKind::ApiError,
        };
        let message = parsed
            .as_ref()
            .map(|p| p.error.message.clone())
            .unwrap_or_else(|| body.to_string());

        Self::Api {
            kind,
            message,
            status,
            response: parsed,
        }
    }
}

impl From<serde_json::Error> for ClaudeShimError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_display() {
        assert_eq!(
            ErrorKind::InvalidRequestError.to_string(),
            "invalid_request_error"
        );
        assert_eq!(
            ErrorKind::AuthenticationError.to_string(),
            "authentication_error"
        );
        assert_eq!(ErrorKind::RateLimitError.to_string(), "rate_limit_error");
        assert_eq!(ErrorKind::OverloadedError.to_string(), "overloaded_error");
        assert_eq!(ErrorKind::ApiError.to_string(), "api_error");
    }

    #[test]
    fn error_kind_status_codes() {
        assert_eq!(ErrorKind::InvalidRequestError.status_code(), 400);
        assert_eq!(ErrorKind::AuthenticationError.status_code(), 401);
        assert_eq!(ErrorKind::PermissionError.status_code(), 403);
        assert_eq!(ErrorKind::NotFoundError.status_code(), 404);
        assert_eq!(ErrorKind::RateLimitError.status_code(), 429);
        assert_eq!(ErrorKind::OverloadedError.status_code(), 529);
    }

    #[test]
    fn error_kind_retryable() {
        assert!(ErrorKind::RateLimitError.is_retryable());
        assert!(ErrorKind::ApiError.is_retryable());
        assert!(ErrorKind::OverloadedError.is_retryable());
        assert!(!ErrorKind::InvalidRequestError.is_retryable());
        assert!(!ErrorKind::AuthenticationError.is_retryable());
    }

    #[test]
    fn error_response_serde_roundtrip() {
        let resp = AnthropicErrorResponse::invalid_request("max_tokens must be positive");
        let json = serde_json::to_string(&resp).unwrap();
        let back: AnthropicErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
        assert_eq!(back.response_type, "error");
        assert_eq!(back.error.error_type, "invalid_request_error");
    }

    #[test]
    fn error_response_json_shape() {
        let resp = AnthropicErrorResponse::rate_limit("Too many requests");
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "rate_limit_error");
        assert_eq!(json["error"]["message"], "Too many requests");
    }

    #[test]
    fn error_response_constructors() {
        let _ = AnthropicErrorResponse::authentication("Invalid API key");
        let _ = AnthropicErrorResponse::overloaded("Server busy");
        let _ = AnthropicErrorResponse::api_error("Internal failure");
    }

    #[test]
    fn shim_error_is_rate_limit() {
        let err = ClaudeShimError::Api {
            kind: ErrorKind::RateLimitError,
            message: "Too many requests".into(),
            status: 429,
            response: None,
        };
        assert!(err.is_rate_limit());
        assert!(!err.is_auth_error());
        assert!(err.is_retryable());
        assert_eq!(err.status_code(), Some(429));
    }

    #[test]
    fn shim_error_is_auth() {
        let err = ClaudeShimError::Api {
            kind: ErrorKind::AuthenticationError,
            message: "Bad key".into(),
            status: 401,
            response: None,
        };
        assert!(err.is_auth_error());
        assert!(!err.is_rate_limit());
        assert!(!err.is_retryable());
    }

    #[test]
    fn shim_error_from_status_and_body() {
        let body =
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited"}}"#;
        let err = ClaudeShimError::from_status_and_body(429, body);
        assert!(err.is_rate_limit());
        assert_eq!(err.status_code(), Some(429));
        if let ClaudeShimError::Api { response, .. } = &err {
            assert!(response.is_some());
        }
    }

    #[test]
    fn shim_error_from_status_unparseable_body() {
        let err = ClaudeShimError::from_status_and_body(500, "internal server error");
        assert!(err.is_retryable());
        if let ClaudeShimError::Api {
            message, response, ..
        } = &err
        {
            assert_eq!(message, "internal server error");
            assert!(response.is_none());
        }
    }

    #[test]
    fn shim_error_display() {
        let err = ClaudeShimError::InvalidRequest("messages cannot be empty".into());
        assert!(err.to_string().contains("messages cannot be empty"));
    }

    #[test]
    fn shim_error_http_is_retryable() {
        let err = ClaudeShimError::Http("connection reset".into());
        assert!(err.is_retryable());
        assert_eq!(err.status_code(), None);
    }

    #[test]
    fn shim_error_from_serde_error() {
        let bad_json = "not json";
        let serde_err = serde_json::from_str::<serde_json::Value>(bad_json).unwrap_err();
        let err: ClaudeShimError = serde_err.into();
        assert!(matches!(err, ClaudeShimError::Serde(_)));
        assert!(!err.is_retryable());
    }

    #[test]
    fn error_kind_serde_roundtrip() {
        let kinds = vec![
            ErrorKind::InvalidRequestError,
            ErrorKind::AuthenticationError,
            ErrorKind::PermissionError,
            ErrorKind::NotFoundError,
            ErrorKind::ConflictError,
            ErrorKind::RateLimitError,
            ErrorKind::ApiError,
            ErrorKind::OverloadedError,
        ];
        for kind in &kinds {
            let json = serde_json::to_string(kind).unwrap();
            let back: ErrorKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, back);
        }
    }
}
