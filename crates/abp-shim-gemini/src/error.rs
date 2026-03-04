// SPDX-License-Identifier: MIT OR Apache-2.0
//! Gemini-compatible error types.
//!
//! Provides a unified error hierarchy covering API errors, request validation,
//! streaming parse failures, and conversions.

use serde::{Deserialize, Serialize};

use crate::types::GeminiErrorResponse;

// ── Error code ──────────────────────────────────────────────────────────

/// Gemini API error status codes.
///
/// Maps to the `status` field returned by the Gemini REST API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// The request was invalid (HTTP 400).
    InvalidArgument,
    /// The API key is missing or invalid (HTTP 401/403).
    PermissionDenied,
    /// The requested resource was not found (HTTP 404).
    NotFound,
    /// Too many requests (HTTP 429).
    ResourceExhausted,
    /// Internal server error (HTTP 500).
    Internal,
    /// Service temporarily unavailable (HTTP 503).
    Unavailable,
    /// Request deadline exceeded (HTTP 504).
    DeadlineExceeded,
}

impl ErrorCode {
    /// Parse an error code from a Gemini API status string.
    #[must_use]
    pub fn from_status(s: &str) -> Option<Self> {
        match s {
            "INVALID_ARGUMENT" => Some(Self::InvalidArgument),
            "PERMISSION_DENIED" => Some(Self::PermissionDenied),
            "NOT_FOUND" => Some(Self::NotFound),
            "RESOURCE_EXHAUSTED" => Some(Self::ResourceExhausted),
            "INTERNAL" => Some(Self::Internal),
            "UNAVAILABLE" => Some(Self::Unavailable),
            "DEADLINE_EXCEEDED" => Some(Self::DeadlineExceeded),
            _ => None,
        }
    }

    /// Return the typical HTTP status code for this error code.
    #[must_use]
    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidArgument => 400,
            Self::PermissionDenied => 403,
            Self::NotFound => 404,
            Self::ResourceExhausted => 429,
            Self::Internal => 500,
            Self::Unavailable => 503,
            Self::DeadlineExceeded => 504,
        }
    }
}

// ── GeminiError ─────────────────────────────────────────────────────────

/// Unified error type for the Gemini shim.
#[derive(Debug, thiserror::Error)]
pub enum GeminiError {
    /// Request conversion / validation failed.
    #[error("request conversion error: {0}")]
    RequestConversion(String),
    /// Response conversion failed.
    #[error("response conversion error: {0}")]
    ResponseConversion(String),
    /// The backend returned a failure outcome.
    #[error("backend error: {0}")]
    BackendError(String),
    /// Serialization / deserialization error.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    /// HTTP-level API error with optional parsed detail.
    #[error("api error (status {status}): {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Human-readable message.
        message: String,
        /// Parsed error code, if available.
        code: Option<ErrorCode>,
    },
    /// Streaming parse error.
    #[error("stream parse error: {0}")]
    StreamParse(String),
}

impl GeminiError {
    /// Try to extract the [`ErrorCode`] from this error.
    #[must_use]
    pub fn error_code(&self) -> Option<ErrorCode> {
        match self {
            Self::Api { code, .. } => *code,
            _ => None,
        }
    }

    /// Return `true` if this is a rate-limit error.
    #[must_use]
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, Self::Api { status: 429, .. })
    }

    /// Return `true` if this is an authentication / permission error.
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Self::Api {
                status: 401 | 403,
                ..
            }
        )
    }

    /// Return `true` if the error is retryable (rate-limit, unavailable, or deadline).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Api { status, .. } => matches!(status, 429 | 503 | 504),
            _ => false,
        }
    }

    /// Construct an API error from a [`GeminiErrorResponse`].
    #[must_use]
    pub fn from_error_response(resp: &GeminiErrorResponse) -> Self {
        let code = resp
            .error
            .status
            .as_deref()
            .and_then(ErrorCode::from_status);
        Self::Api {
            status: resp.error.code,
            message: resp.error.message.clone(),
            code,
        }
    }

    /// Attempt to parse a JSON body into a structured API error.
    ///
    /// Falls back to a plain error with the raw body if parsing fails.
    #[must_use]
    pub fn from_api_body(status: u16, body: &str) -> Self {
        if let Some(parsed) = GeminiErrorResponse::parse(body) {
            Self::from_error_response(&parsed)
        } else {
            Self::Api {
                status,
                message: body.to_string(),
                code: None,
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GeminiErrorDetail;

    #[test]
    fn error_code_from_status_all_variants() {
        assert_eq!(
            ErrorCode::from_status("INVALID_ARGUMENT"),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            ErrorCode::from_status("PERMISSION_DENIED"),
            Some(ErrorCode::PermissionDenied)
        );
        assert_eq!(
            ErrorCode::from_status("NOT_FOUND"),
            Some(ErrorCode::NotFound)
        );
        assert_eq!(
            ErrorCode::from_status("RESOURCE_EXHAUSTED"),
            Some(ErrorCode::ResourceExhausted)
        );
        assert_eq!(
            ErrorCode::from_status("INTERNAL"),
            Some(ErrorCode::Internal)
        );
        assert_eq!(
            ErrorCode::from_status("UNAVAILABLE"),
            Some(ErrorCode::Unavailable)
        );
        assert_eq!(
            ErrorCode::from_status("DEADLINE_EXCEEDED"),
            Some(ErrorCode::DeadlineExceeded)
        );
        assert_eq!(ErrorCode::from_status("UNKNOWN"), None);
    }

    #[test]
    fn error_code_http_status_mapping() {
        assert_eq!(ErrorCode::InvalidArgument.http_status(), 400);
        assert_eq!(ErrorCode::PermissionDenied.http_status(), 403);
        assert_eq!(ErrorCode::NotFound.http_status(), 404);
        assert_eq!(ErrorCode::ResourceExhausted.http_status(), 429);
        assert_eq!(ErrorCode::Internal.http_status(), 500);
        assert_eq!(ErrorCode::Unavailable.http_status(), 503);
        assert_eq!(ErrorCode::DeadlineExceeded.http_status(), 504);
    }

    #[test]
    fn error_code_serde_roundtrip() {
        let code = ErrorCode::ResourceExhausted;
        let json = serde_json::to_string(&code).unwrap();
        assert_eq!(json, "\"RESOURCE_EXHAUSTED\"");
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code);
    }

    #[test]
    fn gemini_error_display_api() {
        let err = GeminiError::Api {
            status: 429,
            message: "rate limited".into(),
            code: Some(ErrorCode::ResourceExhausted),
        };
        let msg = err.to_string();
        assert!(msg.contains("429"));
        assert!(msg.contains("rate limited"));
    }

    #[test]
    fn gemini_error_display_variants() {
        let e1 = GeminiError::RequestConversion("bad field".into());
        assert!(e1.to_string().contains("bad field"));

        let e2 = GeminiError::ResponseConversion("missing candidate".into());
        assert!(e2.to_string().contains("missing candidate"));

        let e3 = GeminiError::BackendError("timeout".into());
        assert!(e3.to_string().contains("timeout"));

        let e4 = GeminiError::StreamParse("unexpected EOF".into());
        assert!(e4.to_string().contains("unexpected EOF"));
    }

    #[test]
    fn is_rate_limit() {
        let err = GeminiError::Api {
            status: 429,
            message: "quota".into(),
            code: None,
        };
        assert!(err.is_rate_limit());
        assert!(!err.is_auth_error());
    }

    #[test]
    fn is_auth_error() {
        let err403 = GeminiError::Api {
            status: 403,
            message: "forbidden".into(),
            code: None,
        };
        assert!(err403.is_auth_error());
        assert!(!err403.is_rate_limit());

        let err401 = GeminiError::Api {
            status: 401,
            message: "unauthorized".into(),
            code: None,
        };
        assert!(err401.is_auth_error());
    }

    #[test]
    fn is_retryable() {
        for status in [429, 503, 504] {
            let err = GeminiError::Api {
                status,
                message: "retry".into(),
                code: None,
            };
            assert!(err.is_retryable(), "status {status} should be retryable");
        }
        let err = GeminiError::Api {
            status: 400,
            message: "bad".into(),
            code: None,
        };
        assert!(!err.is_retryable());

        let err = GeminiError::BackendError("fail".into());
        assert!(!err.is_retryable());
    }

    #[test]
    fn error_code_accessor() {
        let err = GeminiError::Api {
            status: 400,
            message: "invalid".into(),
            code: Some(ErrorCode::InvalidArgument),
        };
        assert_eq!(err.error_code(), Some(ErrorCode::InvalidArgument));

        let err2 = GeminiError::BackendError("oops".into());
        assert_eq!(err2.error_code(), None);
    }

    #[test]
    fn from_error_response() {
        let resp = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 429,
                message: "Quota exceeded".into(),
                status: Some("RESOURCE_EXHAUSTED".into()),
            },
        };
        let err = GeminiError::from_error_response(&resp);
        assert!(err.is_rate_limit());
        assert_eq!(err.error_code(), Some(ErrorCode::ResourceExhausted));
        assert!(err.to_string().contains("Quota exceeded"));
    }

    #[test]
    fn from_api_body_valid_json() {
        let body =
            r#"{"error":{"code":400,"message":"Invalid argument","status":"INVALID_ARGUMENT"}}"#;
        let err = GeminiError::from_api_body(400, body);
        assert_eq!(err.error_code(), Some(ErrorCode::InvalidArgument));
    }

    #[test]
    fn from_api_body_invalid_json_fallback() {
        let err = GeminiError::from_api_body(500, "not json");
        match &err {
            GeminiError::Api {
                status,
                message,
                code,
            } => {
                assert_eq!(*status, 500);
                assert_eq!(message, "not json");
                assert!(code.is_none());
            }
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn serde_error_conversion() {
        let bad: Result<serde_json::Value, _> = serde_json::from_str("{bad}");
        let err: GeminiError = bad.unwrap_err().into();
        assert!(matches!(err, GeminiError::Serde(_)));
        assert!(err.to_string().contains("serde error"));
    }
}
