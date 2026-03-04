#![allow(dead_code, unused_imports)]
//! Vendor-specific API error wrappers with structured metadata.
//!
//! Each vendor variant captures status code, message, optional retry-after
//! header, and request-id for diagnostics and retry logic.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{AbpError, ErrorCode};

// ---------------------------------------------------------------------------
// VendorApiError
// ---------------------------------------------------------------------------

/// Vendor-specific API error with structured metadata for retry and tracing.
///
/// Every variant has a stable `code()` like `"ABP-VENDOR-001"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "vendor", rename_all = "snake_case")]
pub enum VendorApiError {
    /// Error from the OpenAI API.
    OpenAi(VendorErrorDetail),
    /// Error from the Anthropic Claude API.
    Claude(VendorErrorDetail),
    /// Error from the Google Gemini API.
    Gemini(VendorErrorDetail),
    /// Error from the OpenAI Codex API.
    Codex(VendorErrorDetail),
    /// Error from the GitHub Copilot API.
    Copilot(VendorErrorDetail),
    /// Error from the Kimi API.
    Kimi(VendorErrorDetail),
}

/// Structured detail common to all vendor API errors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VendorErrorDetail {
    /// HTTP status code returned by the vendor.
    pub status_code: u16,
    /// Human-readable error message from the vendor.
    pub message: String,
    /// Value of the `Retry-After` header, if present (seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
    /// Vendor-assigned request ID for tracing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl VendorErrorDetail {
    /// Create a new detail with the given status and message.
    pub fn new(status_code: u16, message: impl Into<String>) -> Self {
        Self {
            status_code,
            message: message.into(),
            retry_after_secs: None,
            request_id: None,
        }
    }

    /// Set the retry-after value in seconds.
    pub fn with_retry_after(mut self, secs: u64) -> Self {
        self.retry_after_secs = Some(secs);
        self
    }

    /// Set the request ID.
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }
}

impl VendorApiError {
    /// Stable error code string.
    pub fn code(&self) -> &'static str {
        match self {
            Self::OpenAi(_) => "ABP-VENDOR-001",
            Self::Claude(_) => "ABP-VENDOR-002",
            Self::Gemini(_) => "ABP-VENDOR-003",
            Self::Codex(_) => "ABP-VENDOR-004",
            Self::Copilot(_) => "ABP-VENDOR-005",
            Self::Kimi(_) => "ABP-VENDOR-006",
        }
    }

    /// The vendor name as a string.
    pub fn vendor_name(&self) -> &'static str {
        match self {
            Self::OpenAi(_) => "openai",
            Self::Claude(_) => "claude",
            Self::Gemini(_) => "gemini",
            Self::Codex(_) => "codex",
            Self::Copilot(_) => "copilot",
            Self::Kimi(_) => "kimi",
        }
    }

    /// Access the inner detail.
    pub fn detail(&self) -> &VendorErrorDetail {
        match self {
            Self::OpenAi(d)
            | Self::Claude(d)
            | Self::Gemini(d)
            | Self::Codex(d)
            | Self::Copilot(d)
            | Self::Kimi(d) => d,
        }
    }

    /// Whether the vendor error suggests the request is retryable.
    pub fn is_retryable(&self) -> bool {
        let d = self.detail();
        matches!(d.status_code, 429 | 500 | 502 | 503 | 504) || d.retry_after_secs.is_some()
    }

    /// Convert into a unified [`AbpError`] with full vendor context.
    pub fn into_abp_error(self) -> AbpError {
        let d = self.detail();
        let error_code = match d.status_code {
            401 => ErrorCode::BackendAuthFailed,
            403 => ErrorCode::PolicyDenied,
            404 => ErrorCode::BackendModelNotFound,
            429 => ErrorCode::BackendRateLimited,
            408 | 504 => ErrorCode::BackendTimeout,
            500..=599 => ErrorCode::BackendUnavailable,
            _ => ErrorCode::Internal,
        };

        let mut err = AbpError::new(error_code, self.to_string())
            .with_context("vendor_code", self.code())
            .with_context("vendor_name", self.vendor_name())
            .with_context("vendor_status", d.status_code);

        if let Some(ra) = d.retry_after_secs {
            err = err.with_context("retry_after_secs", ra);
        }
        if let Some(ref rid) = d.request_id {
            err = err.with_context("request_id", rid.as_str());
        }
        err
    }
}

impl fmt::Display for VendorApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.detail();
        write!(
            f,
            "[{}] {} API error (HTTP {}): {}",
            self.code(),
            self.vendor_name(),
            d.status_code,
            d.message
        )
    }
}

impl std::error::Error for VendorApiError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn openai_err() -> VendorApiError {
        VendorApiError::OpenAi(VendorErrorDetail::new(429, "rate limited"))
    }

    fn claude_err() -> VendorApiError {
        VendorApiError::Claude(VendorErrorDetail::new(401, "invalid key"))
    }

    fn gemini_err() -> VendorApiError {
        VendorApiError::Gemini(VendorErrorDetail::new(503, "overloaded").with_retry_after(30))
    }

    fn codex_err() -> VendorApiError {
        VendorApiError::Codex(
            VendorErrorDetail::new(500, "internal error").with_request_id("req-abc-123"),
        )
    }

    fn copilot_err() -> VendorApiError {
        VendorApiError::Copilot(VendorErrorDetail::new(404, "model not found"))
    }

    fn kimi_err() -> VendorApiError {
        VendorApiError::Kimi(VendorErrorDetail::new(408, "timeout"))
    }

    // -- code() -------------------------------------------------------

    #[test]
    fn code_openai() {
        assert_eq!(openai_err().code(), "ABP-VENDOR-001");
    }

    #[test]
    fn code_claude() {
        assert_eq!(claude_err().code(), "ABP-VENDOR-002");
    }

    #[test]
    fn code_gemini() {
        assert_eq!(gemini_err().code(), "ABP-VENDOR-003");
    }

    #[test]
    fn code_codex() {
        assert_eq!(codex_err().code(), "ABP-VENDOR-004");
    }

    #[test]
    fn code_copilot() {
        assert_eq!(copilot_err().code(), "ABP-VENDOR-005");
    }

    #[test]
    fn code_kimi() {
        assert_eq!(kimi_err().code(), "ABP-VENDOR-006");
    }

    // -- vendor_name() ------------------------------------------------

    #[test]
    fn vendor_names() {
        assert_eq!(openai_err().vendor_name(), "openai");
        assert_eq!(claude_err().vendor_name(), "claude");
        assert_eq!(gemini_err().vendor_name(), "gemini");
        assert_eq!(codex_err().vendor_name(), "codex");
        assert_eq!(copilot_err().vendor_name(), "copilot");
        assert_eq!(kimi_err().vendor_name(), "kimi");
    }

    // -- is_retryable() -----------------------------------------------

    #[test]
    fn retryable_429() {
        assert!(openai_err().is_retryable());
    }

    #[test]
    fn retryable_503_with_retry_after() {
        assert!(gemini_err().is_retryable());
    }

    #[test]
    fn retryable_500() {
        assert!(codex_err().is_retryable());
    }

    #[test]
    fn not_retryable_401() {
        assert!(!claude_err().is_retryable());
    }

    #[test]
    fn not_retryable_404() {
        assert!(!copilot_err().is_retryable());
    }

    // -- Display ------------------------------------------------------

    #[test]
    fn display_openai() {
        let s = openai_err().to_string();
        assert!(s.contains("ABP-VENDOR-001"));
        assert!(s.contains("openai"));
        assert!(s.contains("429"));
        assert!(s.contains("rate limited"));
    }

    #[test]
    fn display_claude() {
        let s = claude_err().to_string();
        assert!(s.contains("claude"));
        assert!(s.contains("401"));
    }

    // -- serde roundtrip ----------------------------------------------

    #[test]
    fn serde_roundtrip_openai() {
        let e = openai_err();
        let json = serde_json::to_string(&e).unwrap();
        let back: VendorApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_claude() {
        let e = claude_err();
        let json = serde_json::to_string(&e).unwrap();
        let back: VendorApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_gemini() {
        let e = gemini_err();
        let json = serde_json::to_string(&e).unwrap();
        let back: VendorApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_codex() {
        let e = codex_err();
        let json = serde_json::to_string(&e).unwrap();
        let back: VendorApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_copilot() {
        let e = copilot_err();
        let json = serde_json::to_string(&e).unwrap();
        let back: VendorApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn serde_roundtrip_kimi() {
        let e = kimi_err();
        let json = serde_json::to_string(&e).unwrap();
        let back: VendorApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    // -- into_abp_error -----------------------------------------------

    #[test]
    fn into_abp_error_429() {
        let abp = openai_err().into_abp_error();
        assert_eq!(abp.code, ErrorCode::BackendRateLimited);
        assert_eq!(
            abp.context["vendor_code"],
            serde_json::json!("ABP-VENDOR-001")
        );
        assert_eq!(abp.context["vendor_name"], serde_json::json!("openai"));
    }

    #[test]
    fn into_abp_error_401() {
        let abp = claude_err().into_abp_error();
        assert_eq!(abp.code, ErrorCode::BackendAuthFailed);
    }

    #[test]
    fn into_abp_error_503() {
        let abp = gemini_err().into_abp_error();
        assert_eq!(abp.code, ErrorCode::BackendUnavailable);
        assert_eq!(abp.context["retry_after_secs"], serde_json::json!(30));
    }

    #[test]
    fn into_abp_error_500() {
        let abp = codex_err().into_abp_error();
        assert_eq!(abp.code, ErrorCode::BackendUnavailable);
        assert_eq!(abp.context["request_id"], serde_json::json!("req-abc-123"));
    }

    #[test]
    fn into_abp_error_404() {
        let abp = copilot_err().into_abp_error();
        assert_eq!(abp.code, ErrorCode::BackendModelNotFound);
    }

    #[test]
    fn into_abp_error_408() {
        let abp = kimi_err().into_abp_error();
        assert_eq!(abp.code, ErrorCode::BackendTimeout);
    }

    // -- detail access ------------------------------------------------

    #[test]
    fn detail_access() {
        let e = gemini_err();
        assert_eq!(e.detail().status_code, 503);
        assert_eq!(e.detail().retry_after_secs, Some(30));
    }

    // -- VendorErrorDetail builder ------------------------------------

    #[test]
    fn detail_builder() {
        let d = VendorErrorDetail::new(500, "oops")
            .with_retry_after(60)
            .with_request_id("req-xyz");
        assert_eq!(d.status_code, 500);
        assert_eq!(d.message, "oops");
        assert_eq!(d.retry_after_secs, Some(60));
        assert_eq!(d.request_id.as_deref(), Some("req-xyz"));
    }

    #[test]
    fn detail_serde_roundtrip() {
        let d = VendorErrorDetail::new(429, "rate limited")
            .with_retry_after(10)
            .with_request_id("abc");
        let json = serde_json::to_string(&d).unwrap();
        let back: VendorErrorDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn std_error_impl() {
        let e = openai_err();
        let dyn_err: &dyn std::error::Error = &e;
        assert!(dyn_err.source().is_none());
    }

    #[test]
    fn all_codes_unique() {
        let codes = [
            openai_err().code(),
            claude_err().code(),
            gemini_err().code(),
            codex_err().code(),
            copilot_err().code(),
            kimi_err().code(),
        ];
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len(), "duplicate vendor codes found");
    }
}
