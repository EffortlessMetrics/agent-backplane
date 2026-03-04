// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI-compatible error types for the shim layer.
//!
//! These error types mirror the real OpenAI API error responses so that
//! downstream consumers see the same error taxonomy they would get from
//! the official SDK.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

// ── Concrete error variants ─────────────────────────────────────────────

/// An API error returned by the OpenAI-compatible endpoint.
///
/// Wraps a status code and an [`ErrorBody`] that matches the OpenAI
/// `{"error": {...}}` envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ApiError {
    /// HTTP status code.
    pub status: u16,
    /// Structured error body.
    pub body: ErrorBody,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "API error (HTTP {}): {} [{}]",
            self.status, self.body.message, self.body.error_type
        )
    }
}

impl std::error::Error for ApiError {}

/// The `"error"` object inside an OpenAI error response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ErrorBody {
    /// Human-readable error message.
    pub message: String,
    /// Machine-readable error type (e.g. `"invalid_request_error"`).
    #[serde(rename = "type")]
    pub error_type: String,
    /// Parameter that caused the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Machine-readable error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

// ── Convenience constructors for common errors ──────────────────────────

/// A rate-limit error (HTTP 429).
#[derive(Debug, Clone)]
pub struct RateLimitError {
    /// The underlying API error.
    pub inner: ApiError,
}

impl RateLimitError {
    /// Create a rate-limit error with the given message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            inner: ApiError {
                status: 429,
                body: ErrorBody {
                    message: message.into(),
                    error_type: "rate_limit_error".into(),
                    param: None,
                    code: Some("rate_limit_exceeded".into()),
                },
            },
        }
    }
}

impl fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rate limit error: {}", self.inner.body.message)
    }
}

impl std::error::Error for RateLimitError {}

/// An authentication error (HTTP 401).
#[derive(Debug, Clone)]
pub struct AuthenticationError {
    /// The underlying API error.
    pub inner: ApiError,
}

impl AuthenticationError {
    /// Create an authentication error with the given message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            inner: ApiError {
                status: 401,
                body: ErrorBody {
                    message: message.into(),
                    error_type: "authentication_error".into(),
                    param: None,
                    code: Some("invalid_api_key".into()),
                },
            },
        }
    }
}

impl fmt::Display for AuthenticationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Authentication error: {}", self.inner.body.message)
    }
}

impl std::error::Error for AuthenticationError {}

/// An invalid request error (HTTP 400).
#[derive(Debug, Clone)]
pub struct InvalidRequestError {
    /// The underlying API error.
    pub inner: ApiError,
}

impl InvalidRequestError {
    /// Create an invalid-request error with the given message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            inner: ApiError {
                status: 400,
                body: ErrorBody {
                    message: message.into(),
                    error_type: "invalid_request_error".into(),
                    param: None,
                    code: None,
                },
            },
        }
    }

    /// Create an invalid-request error pointing at a specific parameter.
    #[must_use]
    pub fn with_param(message: impl Into<String>, param: impl Into<String>) -> Self {
        Self {
            inner: ApiError {
                status: 400,
                body: ErrorBody {
                    message: message.into(),
                    error_type: "invalid_request_error".into(),
                    param: Some(param.into()),
                    code: None,
                },
            },
        }
    }
}

impl fmt::Display for InvalidRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid request: {}", self.inner.body.message)
    }
}

impl std::error::Error for InvalidRequestError {}

/// A not-found error (HTTP 404), e.g. model not found.
#[derive(Debug, Clone)]
pub struct NotFoundError {
    /// The underlying API error.
    pub inner: ApiError,
}

impl NotFoundError {
    /// Create a not-found error for a missing model.
    #[must_use]
    pub fn model(model: &str) -> Self {
        Self {
            inner: ApiError {
                status: 404,
                body: ErrorBody {
                    message: format!(
                        "The model `{model}` does not exist or you do not have access to it."
                    ),
                    error_type: "invalid_request_error".into(),
                    param: Some("model".into()),
                    code: Some("model_not_found".into()),
                },
            },
        }
    }
}

impl fmt::Display for NotFoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Not found: {}", self.inner.body.message)
    }
}

impl std::error::Error for NotFoundError {}

/// A server error (HTTP 500+).
#[derive(Debug, Clone)]
pub struct ServerError {
    /// The underlying API error.
    pub inner: ApiError,
}

impl ServerError {
    /// Create a server error with the given message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            inner: ApiError {
                status: 500,
                body: ErrorBody {
                    message: message.into(),
                    error_type: "server_error".into(),
                    param: None,
                    code: None,
                },
            },
        }
    }
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Server error: {}", self.inner.body.message)
    }
}

impl std::error::Error for ServerError {}

// ── Helpers ─────────────────────────────────────────────────────────────

impl ApiError {
    /// Return `true` if this is a rate-limit error (HTTP 429).
    #[must_use]
    pub fn is_rate_limit(&self) -> bool {
        self.status == 429
    }

    /// Return `true` if this is an authentication error (HTTP 401).
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        self.status == 401
    }

    /// Return `true` if this is a not-found error (HTTP 404).
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        self.status == 404
    }

    /// Return `true` if this is a server error (HTTP 5xx).
    #[must_use]
    pub fn is_server_error(&self) -> bool {
        self.status >= 500
    }

    /// Return `true` if the request should be retried (rate limit or server error).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.is_rate_limit() || self.is_server_error()
    }

    /// Try to parse an error from a status code and response body.
    #[must_use]
    pub fn from_response(status: u16, body: &str) -> Self {
        #[derive(Deserialize)]
        struct Envelope {
            error: ErrorBody,
        }
        if let Ok(env) = serde_json::from_str::<Envelope>(body) {
            Self {
                status,
                body: env.error,
            }
        } else {
            Self {
                status,
                body: ErrorBody {
                    message: body.to_string(),
                    error_type: "server_error".into(),
                    param: None,
                    code: None,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_display() {
        let err = ApiError {
            status: 400,
            body: ErrorBody {
                message: "bad request".into(),
                error_type: "invalid_request_error".into(),
                param: None,
                code: None,
            },
        };
        let s = err.to_string();
        assert!(s.contains("400"));
        assert!(s.contains("bad request"));
    }

    #[test]
    fn rate_limit_error_construction() {
        let err = RateLimitError::new("Too many requests");
        assert_eq!(err.inner.status, 429);
        assert_eq!(err.inner.body.error_type, "rate_limit_error");
        assert!(err.to_string().contains("Too many requests"));
    }

    #[test]
    fn authentication_error_construction() {
        let err = AuthenticationError::new("Invalid API key provided");
        assert_eq!(err.inner.status, 401);
        assert_eq!(err.inner.body.error_type, "authentication_error");
        assert_eq!(err.inner.body.code.as_deref(), Some("invalid_api_key"));
    }

    #[test]
    fn invalid_request_error_construction() {
        let err = InvalidRequestError::new("model is required");
        assert_eq!(err.inner.status, 400);
        assert_eq!(err.inner.body.error_type, "invalid_request_error");
    }

    #[test]
    fn invalid_request_error_with_param() {
        let err = InvalidRequestError::with_param("invalid temperature", "temperature");
        assert_eq!(err.inner.body.param.as_deref(), Some("temperature"));
    }

    #[test]
    fn not_found_error_model() {
        let err = NotFoundError::model("gpt-5");
        assert_eq!(err.inner.status, 404);
        assert!(err.inner.body.message.contains("gpt-5"));
        assert_eq!(err.inner.body.code.as_deref(), Some("model_not_found"));
    }

    #[test]
    fn server_error_construction() {
        let err = ServerError::new("internal failure");
        assert_eq!(err.inner.status, 500);
        assert_eq!(err.inner.body.error_type, "server_error");
    }

    #[test]
    fn api_error_is_rate_limit() {
        let err = RateLimitError::new("slow down").inner;
        assert!(err.is_rate_limit());
        assert!(!err.is_auth_error());
        assert!(err.is_retryable());
    }

    #[test]
    fn api_error_is_auth() {
        let err = AuthenticationError::new("bad key").inner;
        assert!(err.is_auth_error());
        assert!(!err.is_rate_limit());
        assert!(!err.is_retryable());
    }

    #[test]
    fn api_error_is_server_error() {
        let err = ServerError::new("oops").inner;
        assert!(err.is_server_error());
        assert!(err.is_retryable());
    }

    #[test]
    fn api_error_is_not_found() {
        let err = NotFoundError::model("gpt-99").inner;
        assert!(err.is_not_found());
        assert!(!err.is_retryable());
    }

    #[test]
    fn api_error_from_valid_json_response() {
        let body = r#"{"error":{"message":"rate limit","type":"rate_limit_error","param":null,"code":"rate_limit_exceeded"}}"#;
        let err = ApiError::from_response(429, body);
        assert_eq!(err.status, 429);
        assert_eq!(err.body.error_type, "rate_limit_error");
    }

    #[test]
    fn api_error_from_invalid_json_falls_back() {
        let err = ApiError::from_response(500, "not json");
        assert_eq!(err.status, 500);
        assert_eq!(err.body.error_type, "server_error");
        assert!(err.body.message.contains("not json"));
    }

    #[test]
    fn api_error_serde_roundtrip() {
        let err = ApiError {
            status: 429,
            body: ErrorBody {
                message: "rate limited".into(),
                error_type: "rate_limit_error".into(),
                param: None,
                code: Some("rate_limit_exceeded".into()),
            },
        };
        let json = serde_json::to_string(&err).unwrap();
        let parsed: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, err);
    }

    #[test]
    fn error_body_serde_matches_openai_format() {
        let json = r#"{"message":"invalid model","type":"invalid_request_error","param":"model","code":"model_not_found"}"#;
        let body: ErrorBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.error_type, "invalid_request_error");
        assert_eq!(body.param.as_deref(), Some("model"));
        assert_eq!(body.code.as_deref(), Some("model_not_found"));
    }
}
