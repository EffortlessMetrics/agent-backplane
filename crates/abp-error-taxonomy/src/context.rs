//! Error context enrichment — typed helpers for attaching structured metadata
//! to [`AbpError`]s.
//!
//! While [`AbpError::with_context`] accepts arbitrary key-value pairs, this
//! module provides [`ErrorContextBuilder`] with named setters for the metadata
//! fields that ABP commonly needs, ensuring consistency across SDK shims.
//!
//! # Examples
//!
//! ```
//! use abp_error_taxonomy::context::ErrorContextBuilder;
//! use abp_error_taxonomy::{AbpError, ErrorCode};
//!
//! let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30 s");
//! let enriched = ErrorContextBuilder::from_error(err)
//!     .backend("openai")
//!     .request_id("req-abc-123")
//!     .model("gpt-4")
//!     .elapsed_ms(30_000)
//!     .build();
//!
//! assert_eq!(enriched.context["backend"], serde_json::json!("openai"));
//! assert_eq!(enriched.context["request_id"], serde_json::json!("req-abc-123"));
//! ```

use crate::AbpError;
use serde::Serialize;

// ---------------------------------------------------------------------------
// ErrorContextBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for enriching an [`AbpError`] with structured metadata.
///
/// Wraps an existing error and provides typed setters for common ABP context
/// fields.  Call [`build`](Self::build) to get the enriched error back.
pub struct ErrorContextBuilder {
    inner: AbpError,
}

impl ErrorContextBuilder {
    /// Start enrichment from an existing error.
    pub fn from_error(err: AbpError) -> Self {
        Self { inner: err }
    }

    /// Name of the backend that produced the error.
    pub fn backend(mut self, name: impl Into<String>) -> Self {
        self.inner = self.inner.with_context("backend", name.into());
        self
    }

    /// Vendor-assigned request identifier for correlation.
    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.inner = self.inner.with_context("request_id", id.into());
        self
    }

    /// Model identifier involved in the error.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.inner = self.inner.with_context("model", model.into());
        self
    }

    /// Wall-clock time in milliseconds the operation took before failing.
    pub fn elapsed_ms(mut self, ms: u64) -> Self {
        self.inner = self.inner.with_context("elapsed_ms", ms);
        self
    }

    /// Number of retry attempts that preceded this failure.
    pub fn retry_count(mut self, count: u32) -> Self {
        self.inner = self.inner.with_context("retry_count", count);
        self
    }

    /// HTTP status code returned by the vendor, if applicable.
    pub fn http_status(mut self, status: u16) -> Self {
        self.inner = self.inner.with_context("http_status", status);
        self
    }

    /// Sidecar process identifier.
    pub fn sidecar_pid(mut self, pid: u32) -> Self {
        self.inner = self.inner.with_context("sidecar_pid", pid);
        self
    }

    /// The work order identifier that was being executed.
    pub fn work_order_id(mut self, id: impl Into<String>) -> Self {
        self.inner = self.inner.with_context("work_order_id", id.into());
        self
    }

    /// Attach an arbitrary typed context value.
    pub fn custom(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        self.inner = self.inner.with_context(key, value);
        self
    }

    /// Consume the builder and return the enriched error.
    pub fn build(self) -> AbpError {
        self.inner
    }
}

// ---------------------------------------------------------------------------
// Convenience extension
// ---------------------------------------------------------------------------

/// Extension trait that adds `.enrich()` to [`AbpError`] for ergonomic
/// context enrichment.
pub trait EnrichError {
    /// Start enriching this error with structured metadata.
    fn enrich(self) -> ErrorContextBuilder;
}

impl EnrichError for AbpError {
    fn enrich(self) -> ErrorContextBuilder {
        ErrorContextBuilder::from_error(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    #[test]
    fn builder_attaches_all_typed_fields() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
        let enriched = ErrorContextBuilder::from_error(err)
            .backend("openai")
            .request_id("req-1")
            .model("gpt-4")
            .elapsed_ms(5000)
            .retry_count(3)
            .http_status(504)
            .sidecar_pid(1234)
            .work_order_id("wo-99")
            .build();

        assert_eq!(enriched.context["backend"], serde_json::json!("openai"));
        assert_eq!(enriched.context["request_id"], serde_json::json!("req-1"));
        assert_eq!(enriched.context["model"], serde_json::json!("gpt-4"));
        assert_eq!(enriched.context["elapsed_ms"], serde_json::json!(5000));
        assert_eq!(enriched.context["retry_count"], serde_json::json!(3));
        assert_eq!(enriched.context["http_status"], serde_json::json!(504));
        assert_eq!(enriched.context["sidecar_pid"], serde_json::json!(1234));
        assert_eq!(
            enriched.context["work_order_id"],
            serde_json::json!("wo-99")
        );
    }

    #[test]
    fn enrich_extension_trait_works() {
        use super::EnrichError;
        let err = AbpError::new(ErrorCode::Internal, "oops")
            .enrich()
            .backend("mock")
            .build();
        assert_eq!(err.context["backend"], serde_json::json!("mock"));
    }

    #[test]
    fn custom_key_value_works() {
        let err = AbpError::new(ErrorCode::Internal, "oops");
        let enriched = ErrorContextBuilder::from_error(err)
            .custom("extra", vec!["a", "b"])
            .build();
        assert_eq!(enriched.context["extra"], serde_json::json!(["a", "b"]));
    }
}
