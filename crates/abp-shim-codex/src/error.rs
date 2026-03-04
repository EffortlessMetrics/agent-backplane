// SPDX-License-Identifier: MIT OR Apache-2.0
//! Codex-compatible error types.
//!
//! Models the OpenAI Responses API error envelope and provides typed
//! error variants for common failure modes.

use serde::{Deserialize, Serialize};
use std::fmt;

// ── API error envelope ──────────────────────────────────────────────────

/// Error payload returned by the OpenAI Responses API.
///
/// Matches the JSON structure:
/// ```json
/// { "error": { "message": "...", "type": "...", "code": "..." } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiErrorBody {
    /// Human-readable error description.
    pub message: String,
    /// Machine-readable error type (e.g. `"invalid_request_error"`).
    #[serde(rename = "type")]
    pub error_type: String,
    /// Optional error code (e.g. `"model_not_found"`, `"rate_limit_exceeded"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Optional parameter that caused the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

/// Wrapper that mirrors the `{ "error": { ... } }` JSON envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiErrorEnvelope {
    /// The nested error body.
    pub error: ApiErrorBody,
}

// ── Error enum ──────────────────────────────────────────────────────────

/// Errors produced by the Codex shim.
#[derive(Debug, thiserror::Error)]
pub enum CodexError {
    /// The request was invalid (bad parameters, missing fields, etc.).
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// Authentication failed (invalid or missing API key).
    #[error("authentication error: {0}")]
    Authentication(String),

    /// The requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Rate limit exceeded.
    #[error("rate limit exceeded: {0}")]
    RateLimited(String),

    /// The model returned an error during processing.
    #[error("model error: {0}")]
    ModelError(String),

    /// Server-side error from the API.
    #[error("server error (status {status}): {message}")]
    ServerError {
        /// HTTP status code.
        status: u16,
        /// Error message.
        message: String,
    },

    /// An API error with the full structured body.
    #[error("api error: {body}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// The structured error body.
        body: ApiErrorBody,
    },

    /// HTTP transport error.
    #[error("http error: {0}")]
    Http(String),

    /// Serialization / deserialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Timeout waiting for a response.
    #[error("timeout: {0}")]
    Timeout(String),

    /// An internal processing error.
    #[error("internal error: {0}")]
    Internal(String),

    /// Stream was interrupted or closed prematurely.
    #[error("stream error: {0}")]
    StreamError(String),
}

impl CodexError {
    /// Create a [`CodexError`] from an HTTP status code and response body.
    ///
    /// Attempts to parse the body as an [`ApiErrorEnvelope`]; falls back to
    /// a [`CodexError::ServerError`] with the raw body text.
    pub fn from_status(status: u16, body: &str) -> Self {
        if let Ok(envelope) = serde_json::from_str::<ApiErrorEnvelope>(body) {
            return Self::Api {
                status,
                body: envelope.error,
            };
        }
        match status {
            401 => Self::Authentication(body.to_string()),
            404 => Self::NotFound(body.to_string()),
            429 => Self::RateLimited(body.to_string()),
            400..=499 => Self::InvalidRequest(body.to_string()),
            _ => Self::ServerError {
                status,
                message: body.to_string(),
            },
        }
    }

    /// Whether this error is retryable (rate limits, server errors).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited(_) | Self::ServerError { .. } | Self::Timeout(_)
        )
    }

    /// The HTTP status code, if available.
    #[must_use]
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::ServerError { status, .. } | Self::Api { status, .. } => Some(*status),
            Self::Authentication(_) => Some(401),
            Self::NotFound(_) => Some(404),
            Self::RateLimited(_) => Some(429),
            _ => None,
        }
    }

    /// Convert to the API error envelope for serialization in HTTP responses.
    #[must_use]
    pub fn to_api_body(&self) -> ApiErrorBody {
        match self {
            Self::Api { body, .. } => body.clone(),
            Self::InvalidRequest(msg) => ApiErrorBody {
                message: msg.clone(),
                error_type: "invalid_request_error".into(),
                code: None,
                param: None,
            },
            Self::Authentication(msg) => ApiErrorBody {
                message: msg.clone(),
                error_type: "authentication_error".into(),
                code: Some("invalid_api_key".into()),
                param: None,
            },
            Self::RateLimited(msg) => ApiErrorBody {
                message: msg.clone(),
                error_type: "rate_limit_error".into(),
                code: Some("rate_limit_exceeded".into()),
                param: None,
            },
            Self::NotFound(msg) => ApiErrorBody {
                message: msg.clone(),
                error_type: "not_found_error".into(),
                code: Some("model_not_found".into()),
                param: None,
            },
            other => ApiErrorBody {
                message: other.to_string(),
                error_type: "server_error".into(),
                code: None,
                param: None,
            },
        }
    }
}

impl fmt::Display for ApiErrorBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.error_type, self.message)?;
        if let Some(code) = &self.code {
            write!(f, " (code: {code})")?;
        }
        Ok(())
    }
}

/// Result alias for Codex shim operations.
pub type CodexResult<T> = std::result::Result<T, CodexError>;

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_body_display() {
        let body = ApiErrorBody {
            message: "Invalid model".into(),
            error_type: "invalid_request_error".into(),
            code: Some("model_not_found".into()),
            param: None,
        };
        let s = body.to_string();
        assert!(s.contains("invalid_request_error"));
        assert!(s.contains("Invalid model"));
        assert!(s.contains("model_not_found"));
    }

    #[test]
    fn api_error_body_display_no_code() {
        let body = ApiErrorBody {
            message: "Something went wrong".into(),
            error_type: "server_error".into(),
            code: None,
            param: None,
        };
        let s = body.to_string();
        assert!(s.contains("server_error"));
        assert!(!s.contains("code:"));
    }

    #[test]
    fn from_status_parses_envelope() {
        let json = r#"{"error":{"message":"Rate limit","type":"rate_limit_error","code":"rate_limit_exceeded"}}"#;
        let err = CodexError::from_status(429, json);
        match &err {
            CodexError::Api { status, body } => {
                assert_eq!(*status, 429);
                assert_eq!(body.error_type, "rate_limit_error");
                assert_eq!(body.code.as_deref(), Some("rate_limit_exceeded"));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn from_status_fallback_401() {
        let err = CodexError::from_status(401, "Unauthorized");
        assert!(matches!(err, CodexError::Authentication(_)));
    }

    #[test]
    fn from_status_fallback_404() {
        let err = CodexError::from_status(404, "Not found");
        assert!(matches!(err, CodexError::NotFound(_)));
    }

    #[test]
    fn from_status_fallback_429() {
        let err = CodexError::from_status(429, "Too many requests");
        assert!(matches!(err, CodexError::RateLimited(_)));
    }

    #[test]
    fn from_status_fallback_400() {
        let err = CodexError::from_status(400, "Bad request");
        assert!(matches!(err, CodexError::InvalidRequest(_)));
    }

    #[test]
    fn from_status_fallback_500() {
        let err = CodexError::from_status(500, "Internal error");
        match &err {
            CodexError::ServerError { status, message } => {
                assert_eq!(*status, 500);
                assert_eq!(message, "Internal error");
            }
            other => panic!("expected ServerError, got {other:?}"),
        }
    }

    #[test]
    fn is_retryable_rate_limit() {
        let err = CodexError::RateLimited("slow down".into());
        assert!(err.is_retryable());
    }

    #[test]
    fn is_retryable_server_error() {
        let err = CodexError::ServerError {
            status: 503,
            message: "unavailable".into(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn is_retryable_timeout() {
        let err = CodexError::Timeout("timed out".into());
        assert!(err.is_retryable());
    }

    #[test]
    fn is_not_retryable_invalid_request() {
        let err = CodexError::InvalidRequest("bad params".into());
        assert!(!err.is_retryable());
    }

    #[test]
    fn is_not_retryable_auth() {
        let err = CodexError::Authentication("bad key".into());
        assert!(!err.is_retryable());
    }

    #[test]
    fn status_code_mapping() {
        assert_eq!(
            CodexError::Authentication("x".into()).status_code(),
            Some(401)
        );
        assert_eq!(CodexError::NotFound("x".into()).status_code(), Some(404));
        assert_eq!(CodexError::RateLimited("x".into()).status_code(), Some(429));
        assert_eq!(
            CodexError::ServerError {
                status: 502,
                message: "x".into()
            }
            .status_code(),
            Some(502)
        );
        assert_eq!(CodexError::Internal("x".into()).status_code(), None);
    }

    #[test]
    fn to_api_body_invalid_request() {
        let err = CodexError::InvalidRequest("bad param".into());
        let body = err.to_api_body();
        assert_eq!(body.error_type, "invalid_request_error");
        assert_eq!(body.message, "bad param");
    }

    #[test]
    fn to_api_body_authentication() {
        let err = CodexError::Authentication("invalid key".into());
        let body = err.to_api_body();
        assert_eq!(body.error_type, "authentication_error");
        assert_eq!(body.code.as_deref(), Some("invalid_api_key"));
    }

    #[test]
    fn to_api_body_rate_limited() {
        let err = CodexError::RateLimited("too fast".into());
        let body = err.to_api_body();
        assert_eq!(body.error_type, "rate_limit_error");
        assert_eq!(body.code.as_deref(), Some("rate_limit_exceeded"));
    }

    #[test]
    fn to_api_body_not_found() {
        let err = CodexError::NotFound("no such model".into());
        let body = err.to_api_body();
        assert_eq!(body.error_type, "not_found_error");
    }

    #[test]
    fn to_api_body_passthrough() {
        let original = ApiErrorBody {
            message: "original".into(),
            error_type: "custom".into(),
            code: Some("custom_code".into()),
            param: Some("model".into()),
        };
        let err = CodexError::Api {
            status: 422,
            body: original.clone(),
        };
        let body = err.to_api_body();
        assert_eq!(body, original);
    }

    #[test]
    fn api_error_body_serde_roundtrip() {
        let body = ApiErrorBody {
            message: "test".into(),
            error_type: "test_error".into(),
            code: Some("test_code".into()),
            param: Some("model".into()),
        };
        let json = serde_json::to_string(&body).unwrap();
        let decoded: ApiErrorBody = serde_json::from_str(&json).unwrap();
        assert_eq!(body, decoded);
    }

    #[test]
    fn api_error_envelope_serde_roundtrip() {
        let envelope = ApiErrorEnvelope {
            error: ApiErrorBody {
                message: "test".into(),
                error_type: "invalid_request_error".into(),
                code: None,
                param: None,
            },
        };
        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("\"error\""));
        let decoded: ApiErrorEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope, decoded);
    }

    #[test]
    fn codex_error_display() {
        let err = CodexError::InvalidRequest("missing model".into());
        assert!(err.to_string().contains("missing model"));

        let err = CodexError::ServerError {
            status: 500,
            message: "oops".into(),
        };
        assert!(err.to_string().contains("500"));
        assert!(err.to_string().contains("oops"));
    }

    #[test]
    fn codex_error_from_serde_error() {
        let serde_err = serde_json::from_str::<String>("not json").unwrap_err();
        let err = CodexError::from(serde_err);
        assert!(matches!(err, CodexError::Serialization(_)));
    }

    #[test]
    fn stream_error_variant() {
        let err = CodexError::StreamError("connection reset".into());
        assert!(err.to_string().contains("connection reset"));
        assert!(!err.is_retryable());
    }

    #[test]
    fn model_error_variant() {
        let err = CodexError::ModelError("context too long".into());
        assert!(err.to_string().contains("context too long"));
        assert!(!err.is_retryable());
        assert_eq!(err.status_code(), None);
    }
}
