//! Vendor error mapping — translates vendor-specific SDK errors into ABP
//! [`ErrorCode`]s.
//!
//! Each major vendor (OpenAI, Anthropic, Gemini, etc.) exposes its own error
//! shape. This module provides [`VendorErrorMapper`] which normalises those
//! heterogeneous formats into the stable ABP error taxonomy.
//!
//! # Examples
//!
//! ```
//! use abp_error_taxonomy::mapping::{VendorError, VendorErrorMapper, VendorKind};
//! use abp_error_taxonomy::ErrorCode;
//!
//! let vendor_err = VendorError::new(VendorKind::OpenAi, 429, "rate_limit_exceeded")
//!     .with_message("Rate limit reached for gpt-4");
//!
//! let mapper = VendorErrorMapper::new();
//! let code = mapper.map_to_abp(&vendor_err);
//! assert_eq!(code, ErrorCode::BackendRateLimited);
//! ```

use crate::{AbpError, ErrorCode};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// VendorKind
// ---------------------------------------------------------------------------

/// Identifies the upstream vendor / SDK that produced an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VendorKind {
    /// OpenAI API errors (GPT, o-series, etc.).
    OpenAi,
    /// Anthropic API errors (Claude).
    Anthropic,
    /// Google Gemini API errors.
    Gemini,
    /// A vendor not covered by the built-in mappings.
    Custom,
}

// ---------------------------------------------------------------------------
// VendorError
// ---------------------------------------------------------------------------

/// A normalised representation of a vendor-specific error.
///
/// This is the input to [`VendorErrorMapper::map_to_abp`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendorError {
    /// Which vendor produced this error.
    pub vendor: VendorKind,
    /// HTTP status code (0 if not applicable).
    pub http_status: u16,
    /// Vendor-defined error type string (e.g. `"rate_limit_exceeded"`).
    pub error_type: String,
    /// Human-readable error message from the vendor.
    pub message: Option<String>,
}

impl VendorError {
    /// Create a new vendor error.
    pub fn new(vendor: VendorKind, http_status: u16, error_type: impl Into<String>) -> Self {
        Self {
            vendor,
            http_status,
            error_type: error_type.into(),
            message: None,
        }
    }

    /// Attach a human-readable message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

// ---------------------------------------------------------------------------
// VendorErrorMapper
// ---------------------------------------------------------------------------

/// Maps [`VendorError`]s to ABP [`ErrorCode`]s.
///
/// The mapper uses a two-pass strategy:
/// 1. Check vendor-specific error type strings first (most precise).
/// 2. Fall back to HTTP status code mapping (broad but universal).
#[derive(Debug, Clone, Default)]
pub struct VendorErrorMapper;

impl VendorErrorMapper {
    /// Create a new mapper.
    pub fn new() -> Self {
        Self
    }

    /// Translate a vendor error into the closest ABP error code.
    pub fn map_to_abp(&self, err: &VendorError) -> ErrorCode {
        // First: try vendor-specific error type mapping.
        if let Some(code) = self.map_by_error_type(err) {
            return code;
        }
        // Second: fall back to HTTP status code mapping.
        Self::map_by_http_status(err.http_status)
    }

    /// Convert a [`VendorError`] into a fully-contextualised [`AbpError`].
    pub fn to_abp_error(&self, err: &VendorError) -> AbpError {
        let code = self.map_to_abp(err);
        let message = err
            .message
            .clone()
            .unwrap_or_else(|| code.message().to_string());
        AbpError::new(code, message)
            .with_context("vendor", format!("{:?}", err.vendor))
            .with_context("http_status", err.http_status)
            .with_context("vendor_error_type", &err.error_type)
    }

    // -- vendor-specific type strings --------------------------------------

    fn map_by_error_type(&self, err: &VendorError) -> Option<ErrorCode> {
        match err.vendor {
            VendorKind::OpenAi => Self::map_openai_type(&err.error_type),
            VendorKind::Anthropic => Self::map_anthropic_type(&err.error_type),
            VendorKind::Gemini => Self::map_gemini_type(&err.error_type),
            VendorKind::Custom => None,
        }
    }

    fn map_openai_type(error_type: &str) -> Option<ErrorCode> {
        Some(match error_type {
            "rate_limit_exceeded" => ErrorCode::BackendRateLimited,
            "invalid_api_key" | "authentication_error" => ErrorCode::BackendAuthFailed,
            "model_not_found" => ErrorCode::BackendModelNotFound,
            "invalid_request_error" => ErrorCode::ContractSchemaViolation,
            "context_length_exceeded" => ErrorCode::ContractSchemaViolation,
            "server_error" | "engine_overloaded" => ErrorCode::BackendUnavailable,
            "timeout" => ErrorCode::BackendTimeout,
            _ => return None,
        })
    }

    fn map_anthropic_type(error_type: &str) -> Option<ErrorCode> {
        Some(match error_type {
            "rate_limit_error" => ErrorCode::BackendRateLimited,
            "authentication_error" => ErrorCode::BackendAuthFailed,
            "not_found_error" => ErrorCode::BackendModelNotFound,
            "invalid_request_error" => ErrorCode::ContractSchemaViolation,
            "overloaded_error" => ErrorCode::BackendUnavailable,
            "api_error" => ErrorCode::Internal,
            _ => return None,
        })
    }

    fn map_gemini_type(error_type: &str) -> Option<ErrorCode> {
        Some(match error_type {
            "RESOURCE_EXHAUSTED" => ErrorCode::BackendRateLimited,
            "UNAUTHENTICATED" => ErrorCode::BackendAuthFailed,
            "NOT_FOUND" => ErrorCode::BackendModelNotFound,
            "INVALID_ARGUMENT" => ErrorCode::ContractSchemaViolation,
            "UNAVAILABLE" => ErrorCode::BackendUnavailable,
            "DEADLINE_EXCEEDED" => ErrorCode::BackendTimeout,
            "INTERNAL" => ErrorCode::Internal,
            "PERMISSION_DENIED" => ErrorCode::ExecutionPermissionDenied,
            _ => return None,
        })
    }

    // -- HTTP status fallback ----------------------------------------------

    fn map_by_http_status(status: u16) -> ErrorCode {
        match status {
            401 => ErrorCode::BackendAuthFailed,
            403 => ErrorCode::ExecutionPermissionDenied,
            404 => ErrorCode::BackendModelNotFound,
            408 => ErrorCode::BackendTimeout,
            422 => ErrorCode::ContractSchemaViolation,
            429 => ErrorCode::BackendRateLimited,
            500 => ErrorCode::Internal,
            502 | 503 => ErrorCode::BackendUnavailable,
            504 => ErrorCode::BackendTimeout,
            _ if (400..500).contains(&status) => ErrorCode::ContractSchemaViolation,
            _ if status >= 500 => ErrorCode::Internal,
            _ => ErrorCode::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_rate_limit_maps_correctly() {
        let m = VendorErrorMapper::new();
        let e = VendorError::new(VendorKind::OpenAi, 429, "rate_limit_exceeded");
        assert_eq!(m.map_to_abp(&e), ErrorCode::BackendRateLimited);
    }

    #[test]
    fn anthropic_auth_error_maps_correctly() {
        let m = VendorErrorMapper::new();
        let e = VendorError::new(VendorKind::Anthropic, 401, "authentication_error");
        assert_eq!(m.map_to_abp(&e), ErrorCode::BackendAuthFailed);
    }

    #[test]
    fn gemini_deadline_exceeded_maps_correctly() {
        let m = VendorErrorMapper::new();
        let e = VendorError::new(VendorKind::Gemini, 504, "DEADLINE_EXCEEDED");
        assert_eq!(m.map_to_abp(&e), ErrorCode::BackendTimeout);
    }

    #[test]
    fn unknown_error_type_falls_back_to_http_status() {
        let m = VendorErrorMapper::new();
        let e = VendorError::new(VendorKind::OpenAi, 503, "something_new");
        assert_eq!(m.map_to_abp(&e), ErrorCode::BackendUnavailable);
    }

    #[test]
    fn custom_vendor_always_uses_http_fallback() {
        let m = VendorErrorMapper::new();
        let e = VendorError::new(VendorKind::Custom, 401, "auth_failed");
        assert_eq!(m.map_to_abp(&e), ErrorCode::BackendAuthFailed);
    }

    #[test]
    fn to_abp_error_includes_vendor_context() {
        let m = VendorErrorMapper::new();
        let e = VendorError::new(VendorKind::OpenAi, 429, "rate_limit_exceeded")
            .with_message("slow down");
        let abp = m.to_abp_error(&e);
        assert_eq!(abp.code, ErrorCode::BackendRateLimited);
        assert_eq!(abp.message, "slow down");
        assert!(abp.context.contains_key("vendor"));
        assert!(abp.context.contains_key("http_status"));
    }
}
