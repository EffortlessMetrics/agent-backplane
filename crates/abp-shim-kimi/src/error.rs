// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kimi-specific error types.
//!
//! Provides structured error handling for the Moonshot/Kimi API, including
//! HTTP status code mapping, rate-limit detection, and retryable-error
//! classification.

use serde::{Deserialize, Serialize};

// ── API error response ──────────────────────────────────────────────────

/// Structured error body returned by the Moonshot API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiErrorBody {
    /// Human-readable error message.
    pub message: String,
    /// Machine-readable error type (e.g. `"invalid_request_error"`).
    #[serde(rename = "type")]
    pub error_type: String,
    /// Optional parameter that caused the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Optional error code (e.g. `"context_length_exceeded"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Top-level error response envelope from the Moonshot API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiErrorResponse {
    /// The error payload.
    pub error: KimiErrorBody,
}

// ── Error kind enum ─────────────────────────────────────────────────────

/// Classification of Kimi API errors by HTTP status or error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KimiErrorKind {
    /// 400 — bad request / invalid parameters.
    InvalidRequest,
    /// 401 — authentication failure.
    Authentication,
    /// 403 — permission denied.
    PermissionDenied,
    /// 404 — resource not found.
    NotFound,
    /// 429 — rate limit exceeded.
    RateLimit,
    /// 5xx — server-side error.
    Server,
    /// Context length exceeded.
    ContextLengthExceeded,
    /// Unknown / unmapped error.
    Unknown,
}

impl KimiErrorKind {
    /// Classify from an HTTP status code and optional error code string.
    #[must_use]
    pub fn from_status(status: u16, code: Option<&str>) -> Self {
        if let Some("context_length_exceeded") = code {
            return Self::ContextLengthExceeded;
        }
        match status {
            400 => Self::InvalidRequest,
            401 => Self::Authentication,
            403 => Self::PermissionDenied,
            404 => Self::NotFound,
            429 => Self::RateLimit,
            500..=599 => Self::Server,
            _ => Self::Unknown,
        }
    }
}

// ── Unified shim error ──────────────────────────────────────────────────

/// Errors produced by the Kimi shim.
#[derive(Debug, thiserror::Error)]
pub enum KimiShimError {
    /// Structured API error with status code.
    #[error("kimi api error (status {status}): {body}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Raw response body.
        body: String,
        /// Parsed error body, if available.
        parsed: Option<KimiErrorBody>,
        /// Classified error kind.
        kind: KimiErrorKind,
    },
    /// HTTP transport error.
    #[error("http error: {0}")]
    Http(String),
    /// The request was invalid before sending.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// Serialization / deserialization error.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    /// Internal processing error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl KimiShimError {
    /// Create an API error from status code and body string.
    #[must_use]
    pub fn from_status_and_body(status: u16, body: String) -> Self {
        let parsed = serde_json::from_str::<KimiErrorResponse>(&body)
            .ok()
            .map(|r| r.error);
        let code = parsed.as_ref().and_then(|p| p.code.as_deref());
        let kind = KimiErrorKind::from_status(status, code);
        Self::Api {
            status,
            body,
            parsed,
            kind,
        }
    }

    /// The HTTP status code, if this is an API error.
    #[must_use]
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// The classified error kind, if this is an API error.
    #[must_use]
    pub fn error_kind(&self) -> Option<KimiErrorKind> {
        match self {
            Self::Api { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// Returns `true` if this is a rate-limit error (HTTP 429).
    #[must_use]
    pub fn is_rate_limit(&self) -> bool {
        matches!(
            self,
            Self::Api {
                kind: KimiErrorKind::RateLimit,
                ..
            }
        )
    }

    /// Returns `true` if this is an authentication error (HTTP 401).
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Self::Api {
                kind: KimiErrorKind::Authentication,
                ..
            }
        )
    }

    /// Returns `true` if this is a context-length-exceeded error.
    #[must_use]
    pub fn is_context_length_exceeded(&self) -> bool {
        matches!(
            self,
            Self::Api {
                kind: KimiErrorKind::ContextLengthExceeded,
                ..
            }
        )
    }

    /// Returns `true` if the error is likely retryable (rate limit or server error).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Api {
                kind: KimiErrorKind::RateLimit | KimiErrorKind::Server,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn error_kind_from_status_400() {
        assert_eq!(
            KimiErrorKind::from_status(400, None),
            KimiErrorKind::InvalidRequest
        );
    }

    #[test]
    fn error_kind_from_status_401() {
        assert_eq!(
            KimiErrorKind::from_status(401, None),
            KimiErrorKind::Authentication
        );
    }

    #[test]
    fn error_kind_from_status_403() {
        assert_eq!(
            KimiErrorKind::from_status(403, None),
            KimiErrorKind::PermissionDenied
        );
    }

    #[test]
    fn error_kind_from_status_404() {
        assert_eq!(
            KimiErrorKind::from_status(404, None),
            KimiErrorKind::NotFound
        );
    }

    #[test]
    fn error_kind_from_status_429() {
        assert_eq!(
            KimiErrorKind::from_status(429, None),
            KimiErrorKind::RateLimit
        );
    }

    #[test]
    fn error_kind_from_status_500() {
        assert_eq!(KimiErrorKind::from_status(500, None), KimiErrorKind::Server);
    }

    #[test]
    fn error_kind_from_status_503() {
        assert_eq!(KimiErrorKind::from_status(503, None), KimiErrorKind::Server);
    }

    #[test]
    fn error_kind_context_length_exceeded() {
        assert_eq!(
            KimiErrorKind::from_status(400, Some("context_length_exceeded")),
            KimiErrorKind::ContextLengthExceeded
        );
    }

    #[test]
    fn error_kind_unknown_status() {
        assert_eq!(
            KimiErrorKind::from_status(418, None),
            KimiErrorKind::Unknown
        );
    }

    #[test]
    fn from_status_and_body_parses_structured_error() {
        let body = json!({
            "error": {
                "message": "Rate limit reached",
                "type": "rate_limit_error",
                "param": null,
                "code": "rate_limit"
            }
        })
        .to_string();

        let err = KimiShimError::from_status_and_body(429, body);
        assert!(err.is_rate_limit());
        assert!(err.is_retryable());
        assert!(!err.is_auth_error());
        assert_eq!(err.status_code(), Some(429));

        if let KimiShimError::Api { parsed, .. } = &err {
            let p = parsed.as_ref().unwrap();
            assert_eq!(p.message, "Rate limit reached");
            assert_eq!(p.error_type, "rate_limit_error");
        } else {
            panic!("expected Api variant");
        }
    }

    #[test]
    fn from_status_and_body_handles_unparseable_body() {
        let err = KimiShimError::from_status_and_body(500, "internal error".into());
        assert!(err.is_retryable());
        assert!(!err.is_rate_limit());
        assert_eq!(err.error_kind(), Some(KimiErrorKind::Server));
        if let KimiShimError::Api { parsed, .. } = &err {
            assert!(parsed.is_none());
        }
    }

    #[test]
    fn auth_error_detection() {
        let body = json!({
            "error": {
                "message": "Invalid API key",
                "type": "authentication_error"
            }
        })
        .to_string();
        let err = KimiShimError::from_status_and_body(401, body);
        assert!(err.is_auth_error());
        assert!(!err.is_retryable());
    }

    #[test]
    fn context_length_exceeded_detection() {
        let body = json!({
            "error": {
                "message": "This model's maximum context length is 8192 tokens",
                "type": "invalid_request_error",
                "code": "context_length_exceeded"
            }
        })
        .to_string();
        let err = KimiShimError::from_status_and_body(400, body);
        assert!(err.is_context_length_exceeded());
        assert!(!err.is_retryable());
    }

    #[test]
    fn error_display_contains_status() {
        let err = KimiShimError::from_status_and_body(429, "too many requests".into());
        let msg = err.to_string();
        assert!(msg.contains("429"));
        assert!(msg.contains("too many requests"));
    }

    #[test]
    fn invalid_request_error_display() {
        let err = KimiShimError::InvalidRequest("missing model field".into());
        assert!(err.to_string().contains("missing model field"));
        assert_eq!(err.status_code(), None);
    }

    #[test]
    fn internal_error_not_retryable() {
        let err = KimiShimError::Internal("bug".into());
        assert!(!err.is_retryable());
        assert!(!err.is_rate_limit());
    }

    #[test]
    fn serde_error_from_json() {
        let result: Result<KimiErrorResponse, _> = serde_json::from_str("not json");
        let err = KimiShimError::from(result.unwrap_err());
        assert!(matches!(err, KimiShimError::Serde(_)));
    }

    #[test]
    fn error_body_serde_roundtrip() {
        let body = KimiErrorBody {
            message: "Bad request".into(),
            error_type: "invalid_request_error".into(),
            param: Some("temperature".into()),
            code: Some("invalid_value".into()),
        };
        let json = serde_json::to_string(&body).unwrap();
        let parsed: KimiErrorBody = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, body);
    }

    #[test]
    fn error_body_optional_fields_absent() {
        let json_str = r#"{"message":"error","type":"server_error"}"#;
        let body: KimiErrorBody = serde_json::from_str(json_str).unwrap();
        assert!(body.param.is_none());
        assert!(body.code.is_none());
    }

    #[test]
    fn error_response_envelope_roundtrip() {
        let resp = KimiErrorResponse {
            error: KimiErrorBody {
                message: "Not found".into(),
                error_type: "not_found_error".into(),
                param: None,
                code: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: KimiErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }
}
