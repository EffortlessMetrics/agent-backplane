// SPDX-License-Identifier: MIT OR Apache-2.0
//! Error types matching the Google Gemini REST API error format.
//!
//! When the Gemini API returns a non-2xx response, the body carries an
//! [`GeminiErrorResponse`] wrapping a [`GeminiErrorDetail`]. These types
//! model that wire format so callers can deserialize error bodies directly.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Top-level error response
// ---------------------------------------------------------------------------

/// Top-level error response body from the Gemini API.
///
/// Wraps a single [`GeminiErrorDetail`] under the `"error"` key.
///
/// ```json
/// {
///   "error": {
///     "code": 400,
///     "message": "Invalid value ...",
///     "status": "INVALID_ARGUMENT"
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiErrorResponse {
    /// The error details.
    pub error: GeminiErrorDetail,
}

impl fmt::Display for GeminiErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for GeminiErrorResponse {}

// ---------------------------------------------------------------------------
// Error detail
// ---------------------------------------------------------------------------

/// Structured error detail returned by the Gemini API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiErrorDetail {
    /// HTTP status code (e.g. `400`, `404`, `429`, `500`).
    pub code: u16,

    /// Human-readable error message.
    pub message: String,

    /// gRPC-style status string (e.g. `"INVALID_ARGUMENT"`).
    pub status: GeminiErrorStatus,

    /// Optional structured details about the error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<serde_json::Value>>,
}

impl fmt::Display for GeminiErrorDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.status, self.message)
    }
}

impl std::error::Error for GeminiErrorDetail {}

// ---------------------------------------------------------------------------
// Error status codes
// ---------------------------------------------------------------------------

/// gRPC-style status codes used by the Gemini API.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeminiErrorStatus {
    /// The request was malformed or contained invalid parameters.
    InvalidArgument,
    /// The requested resource was not found.
    NotFound,
    /// The caller does not have permission to perform the operation.
    PermissionDenied,
    /// The API key is missing or invalid.
    Unauthenticated,
    /// Rate limit or quota exceeded.
    ResourceExhausted,
    /// The request was aborted (e.g. due to a conflict).
    Aborted,
    /// The operation is not implemented or not supported.
    Unimplemented,
    /// An internal server error occurred.
    Internal,
    /// The service is temporarily unavailable.
    Unavailable,
    /// The operation timed out.
    DeadlineExceeded,
    /// A precondition for the request was not met.
    FailedPrecondition,
    /// The operation was cancelled.
    Cancelled,
    /// Unknown error.
    Unknown,
}

impl fmt::Display for GeminiErrorStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::NotFound => "NOT_FOUND",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::Aborted => "ABORTED",
            Self::Unimplemented => "UNIMPLEMENTED",
            Self::Internal => "INTERNAL",
            Self::Unavailable => "UNAVAILABLE",
            Self::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Self::FailedPrecondition => "FAILED_PRECONDITION",
            Self::Cancelled => "CANCELLED",
            Self::Unknown => "UNKNOWN",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// Field violation (common details payload)
// ---------------------------------------------------------------------------

/// A field violation inside a `BadRequest` error detail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldViolation {
    /// The field path that caused the violation (e.g. `"contents[0].parts"`).
    pub field: String,
    /// A description of the violation.
    pub description: String,
}

/// A `BadRequest` details entry containing field violations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BadRequestDetail {
    /// The type URL for this detail.
    #[serde(rename = "@type")]
    pub type_url: String,
    /// Individual field violations.
    pub field_violations: Vec<FieldViolation>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` if the error indicates a rate-limit / quota issue.
#[must_use]
pub fn is_rate_limited(err: &GeminiErrorResponse) -> bool {
    err.error.status == GeminiErrorStatus::ResourceExhausted
}

/// Returns `true` if the error is transient and the request may be retried.
#[must_use]
pub fn is_retryable(err: &GeminiErrorResponse) -> bool {
    matches!(
        err.error.status,
        GeminiErrorStatus::ResourceExhausted
            | GeminiErrorStatus::Unavailable
            | GeminiErrorStatus::DeadlineExceeded
            | GeminiErrorStatus::Internal
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_response_serde_roundtrip() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 400,
                message: "Invalid value".into(),
                status: GeminiErrorStatus::InvalidArgument,
                details: None,
            },
        };
        let json = serde_json::to_string(&err).unwrap();
        let parsed: GeminiErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(err, parsed);
    }

    #[test]
    fn error_response_deserialize_from_api_json() {
        let json = r#"{
            "error": {
                "code": 400,
                "message": "Invalid value at 'contents[0].parts'",
                "status": "INVALID_ARGUMENT"
            }
        }"#;
        let err: GeminiErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code, 400);
        assert_eq!(err.error.status, GeminiErrorStatus::InvalidArgument);
        assert!(err.error.message.contains("Invalid value"));
    }

    #[test]
    fn error_response_with_details() {
        let json = r#"{
            "error": {
                "code": 400,
                "message": "Bad request",
                "status": "INVALID_ARGUMENT",
                "details": [
                    {
                        "@type": "type.googleapis.com/google.rpc.BadRequest",
                        "fieldViolations": [
                            {"field": "contents", "description": "missing"}
                        ]
                    }
                ]
            }
        }"#;
        let err: GeminiErrorResponse = serde_json::from_str(json).unwrap();
        assert!(err.error.details.is_some());
        assert_eq!(err.error.details.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn error_status_all_variants_serde() {
        let variants = [
            (GeminiErrorStatus::InvalidArgument, "INVALID_ARGUMENT"),
            (GeminiErrorStatus::NotFound, "NOT_FOUND"),
            (GeminiErrorStatus::PermissionDenied, "PERMISSION_DENIED"),
            (GeminiErrorStatus::Unauthenticated, "UNAUTHENTICATED"),
            (GeminiErrorStatus::ResourceExhausted, "RESOURCE_EXHAUSTED"),
            (GeminiErrorStatus::Aborted, "ABORTED"),
            (GeminiErrorStatus::Unimplemented, "UNIMPLEMENTED"),
            (GeminiErrorStatus::Internal, "INTERNAL"),
            (GeminiErrorStatus::Unavailable, "UNAVAILABLE"),
            (GeminiErrorStatus::DeadlineExceeded, "DEADLINE_EXCEEDED"),
            (GeminiErrorStatus::FailedPrecondition, "FAILED_PRECONDITION"),
            (GeminiErrorStatus::Cancelled, "CANCELLED"),
            (GeminiErrorStatus::Unknown, "UNKNOWN"),
        ];
        for (variant, expected) in &variants {
            let json = serde_json::to_value(variant).unwrap();
            assert_eq!(json.as_str().unwrap(), *expected);
            let back: GeminiErrorStatus = serde_json::from_value(json).unwrap();
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn error_display_includes_code_status_message() {
        let err = GeminiErrorDetail {
            code: 429,
            message: "Quota exceeded".into(),
            status: GeminiErrorStatus::ResourceExhausted,
            details: None,
        };
        let display = format!("{err}");
        assert!(display.contains("429"));
        assert!(display.contains("RESOURCE_EXHAUSTED"));
        assert!(display.contains("Quota exceeded"));
    }

    #[test]
    fn error_response_display() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 500,
                message: "Internal error".into(),
                status: GeminiErrorStatus::Internal,
                details: None,
            },
        };
        let display = format!("{err}");
        assert!(display.contains("500"));
        assert!(display.contains("Internal error"));
    }

    #[test]
    fn is_rate_limited_true_for_resource_exhausted() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 429,
                message: "Rate limit".into(),
                status: GeminiErrorStatus::ResourceExhausted,
                details: None,
            },
        };
        assert!(is_rate_limited(&err));
    }

    #[test]
    fn is_rate_limited_false_for_other_status() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 400,
                message: "Bad request".into(),
                status: GeminiErrorStatus::InvalidArgument,
                details: None,
            },
        };
        assert!(!is_rate_limited(&err));
    }

    #[test]
    fn is_retryable_for_transient_errors() {
        for status in [
            GeminiErrorStatus::ResourceExhausted,
            GeminiErrorStatus::Unavailable,
            GeminiErrorStatus::DeadlineExceeded,
            GeminiErrorStatus::Internal,
        ] {
            let err = GeminiErrorResponse {
                error: GeminiErrorDetail {
                    code: 500,
                    message: "error".into(),
                    status,
                    details: None,
                },
            };
            assert!(is_retryable(&err), "{status:?} should be retryable");
        }
    }

    #[test]
    fn is_not_retryable_for_permanent_errors() {
        for status in [
            GeminiErrorStatus::InvalidArgument,
            GeminiErrorStatus::NotFound,
            GeminiErrorStatus::PermissionDenied,
            GeminiErrorStatus::Unauthenticated,
        ] {
            let err = GeminiErrorResponse {
                error: GeminiErrorDetail {
                    code: 400,
                    message: "error".into(),
                    status,
                    details: None,
                },
            };
            assert!(!is_retryable(&err), "{status:?} should not be retryable");
        }
    }

    #[test]
    fn field_violation_serde_roundtrip() {
        let fv = FieldViolation {
            field: "contents[0].parts".into(),
            description: "cannot be empty".into(),
        };
        let json = serde_json::to_string(&fv).unwrap();
        let parsed: FieldViolation = serde_json::from_str(&json).unwrap();
        assert_eq!(fv, parsed);
    }

    #[test]
    fn bad_request_detail_serde_roundtrip() {
        let detail = BadRequestDetail {
            type_url: "type.googleapis.com/google.rpc.BadRequest".into(),
            field_violations: vec![FieldViolation {
                field: "model".into(),
                description: "required".into(),
            }],
        };
        let json = serde_json::to_string(&detail).unwrap();
        let parsed: BadRequestDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(detail, parsed);
    }

    #[test]
    fn deserialize_not_found_error() {
        let json = r#"{
            "error": {
                "code": 404,
                "message": "Model not found",
                "status": "NOT_FOUND"
            }
        }"#;
        let err: GeminiErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code, 404);
        assert_eq!(err.error.status, GeminiErrorStatus::NotFound);
    }

    #[test]
    fn deserialize_auth_error() {
        let json = r#"{
            "error": {
                "code": 401,
                "message": "API key not valid",
                "status": "UNAUTHENTICATED"
            }
        }"#;
        let err: GeminiErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code, 401);
        assert_eq!(err.error.status, GeminiErrorStatus::Unauthenticated);
    }
}
