// SPDX-License-Identifier: MIT OR Apache-2.0
//! Builder patterns for constructing [`Envelope`] variants ergonomically.
//!
//! # Examples
//!
//! ```
//! use abp_core::{BackendIdentity, CapabilityManifest};
//! use abp_protocol::builder::EnvelopeBuilder;
//!
//! let envelope = EnvelopeBuilder::hello()
//!     .backend("my-sidecar")
//!     .version("1.0.0")
//!     .build()
//!     .unwrap();
//! ```

use std::fmt;

use abp_core::{
    AgentEvent, BackendIdentity, CapabilityManifest, ExecutionMode, Receipt, WorkOrder,
    CONTRACT_VERSION,
};

use crate::Envelope;

// ---------------------------------------------------------------------------
// BuilderError
// ---------------------------------------------------------------------------

/// Errors that can occur when building an [`Envelope`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuilderError {
    /// A required field was not set.
    MissingField(&'static str),
}

impl fmt::Display for BuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuilderError::MissingField(field) => {
                write!(f, "missing required field: {field}")
            }
        }
    }
}

impl std::error::Error for BuilderError {}

// ---------------------------------------------------------------------------
// EnvelopeBuilder (entry point)
// ---------------------------------------------------------------------------

/// Entry point for building [`Envelope`] variants.
///
/// Each method returns a specialised sub-builder for the corresponding
/// envelope variant.
pub struct EnvelopeBuilder;

impl EnvelopeBuilder {
    /// Start building a `Hello` envelope.
    ///
    /// # Examples
    ///
    /// ```
    /// # use abp_protocol::builder::EnvelopeBuilder;
    /// # use abp_protocol::Envelope;
    /// let envelope = EnvelopeBuilder::hello()
    ///     .backend("my-sidecar")
    ///     .version("1.0.0")
    ///     .build()
    ///     .unwrap();
    ///
    /// match &envelope {
    ///     Envelope::Hello { backend, .. } => assert_eq!(backend.id, "my-sidecar"),
    ///     _ => panic!("expected Hello"),
    /// }
    /// ```
    #[must_use]
    pub fn hello() -> HelloBuilder {
        HelloBuilder::default()
    }

    /// Start building a `Run` envelope with the given [`WorkOrder`].
    #[must_use]
    pub fn run(work_order: WorkOrder) -> RunBuilder {
        RunBuilder {
            ref_id: None,
            work_order,
        }
    }

    /// Start building an `Event` envelope with the given [`AgentEvent`].
    #[must_use]
    pub fn event(event: AgentEvent) -> EventBuilder {
        EventBuilder {
            ref_id: None,
            event,
        }
    }

    /// Start building a `Final` envelope with the given [`Receipt`].
    #[must_use]
    pub fn final_receipt(receipt: Receipt) -> FinalBuilder {
        FinalBuilder {
            ref_id: None,
            receipt,
        }
    }

    /// Start building a `Fatal` envelope with the given error message.
    #[must_use]
    pub fn fatal(message: &str) -> FatalBuilder {
        FatalBuilder {
            ref_id: None,
            code: None,
            message: message.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// HelloBuilder
// ---------------------------------------------------------------------------

/// Builder for the `Hello` envelope variant.
#[derive(Debug, Default)]
pub struct HelloBuilder {
    backend_id: Option<String>,
    backend_version: Option<String>,
    adapter_version: Option<String>,
    capabilities: Option<CapabilityManifest>,
    mode: Option<ExecutionMode>,
}

impl HelloBuilder {
    /// Set the backend identifier (required).
    #[must_use]
    pub fn backend(mut self, id: impl Into<String>) -> Self {
        self.backend_id = Some(id.into());
        self
    }

    /// Set the backend runtime version.
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.backend_version = Some(version.into());
        self
    }

    /// Set the adapter version.
    #[must_use]
    pub fn adapter_version(mut self, version: impl Into<String>) -> Self {
        self.adapter_version = Some(version.into());
        self
    }

    /// Set the capability manifest. Defaults to an empty manifest.
    #[must_use]
    pub fn capabilities(mut self, caps: CapabilityManifest) -> Self {
        self.capabilities = Some(caps);
        self
    }

    /// Set the execution mode. Defaults to [`ExecutionMode::Mapped`].
    #[must_use]
    pub fn mode(mut self, mode: ExecutionMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Build the `Hello` [`Envelope`].
    ///
    /// # Errors
    ///
    /// Returns [`BuilderError::MissingField`] if `backend` was not set.
    pub fn build(self) -> Result<Envelope, BuilderError> {
        let backend_id = self
            .backend_id
            .ok_or(BuilderError::MissingField("backend"))?;

        Ok(Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend: BackendIdentity {
                id: backend_id,
                backend_version: self.backend_version,
                adapter_version: self.adapter_version,
            },
            capabilities: self.capabilities.unwrap_or_default(),
            mode: self.mode.unwrap_or_default(),
        })
    }
}

// ---------------------------------------------------------------------------
// RunBuilder
// ---------------------------------------------------------------------------

/// Builder for the `Run` envelope variant.
#[derive(Debug)]
pub struct RunBuilder {
    ref_id: Option<String>,
    work_order: WorkOrder,
}

impl RunBuilder {
    /// Override the run id. Defaults to the work order's id.
    #[must_use]
    pub fn ref_id(mut self, id: impl Into<String>) -> Self {
        self.ref_id = Some(id.into());
        self
    }

    /// Build the `Run` [`Envelope`].
    ///
    /// Uses the work order's `id` as the envelope `id` unless overridden
    /// via [`ref_id`](Self::ref_id).
    ///
    /// # Errors
    ///
    /// This builder currently always succeeds, but returns `Result` for
    /// API consistency.
    pub fn build(self) -> Result<Envelope, BuilderError> {
        let id = self
            .ref_id
            .unwrap_or_else(|| self.work_order.id.to_string());
        Ok(Envelope::Run {
            id,
            work_order: self.work_order,
        })
    }
}

// ---------------------------------------------------------------------------
// EventBuilder
// ---------------------------------------------------------------------------

/// Builder for the `Event` envelope variant.
#[derive(Debug)]
pub struct EventBuilder {
    ref_id: Option<String>,
    event: AgentEvent,
}

impl EventBuilder {
    /// Set the reference id that correlates this event to a run (required).
    #[must_use]
    pub fn ref_id(mut self, id: impl Into<String>) -> Self {
        self.ref_id = Some(id.into());
        self
    }

    /// Build the `Event` [`Envelope`].
    ///
    /// # Errors
    ///
    /// Returns [`BuilderError::MissingField`] if `ref_id` was not set.
    pub fn build(self) -> Result<Envelope, BuilderError> {
        let ref_id = self
            .ref_id
            .ok_or(BuilderError::MissingField("ref_id"))?;

        Ok(Envelope::Event {
            ref_id,
            event: self.event,
        })
    }
}

// ---------------------------------------------------------------------------
// FinalBuilder
// ---------------------------------------------------------------------------

/// Builder for the `Final` envelope variant.
#[derive(Debug)]
pub struct FinalBuilder {
    ref_id: Option<String>,
    receipt: Receipt,
}

impl FinalBuilder {
    /// Set the reference id that correlates this receipt to a run (required).
    #[must_use]
    pub fn ref_id(mut self, id: impl Into<String>) -> Self {
        self.ref_id = Some(id.into());
        self
    }

    /// Build the `Final` [`Envelope`].
    ///
    /// # Errors
    ///
    /// Returns [`BuilderError::MissingField`] if `ref_id` was not set.
    pub fn build(self) -> Result<Envelope, BuilderError> {
        let ref_id = self
            .ref_id
            .ok_or(BuilderError::MissingField("ref_id"))?;

        Ok(Envelope::Final {
            ref_id,
            receipt: self.receipt,
        })
    }
}

// ---------------------------------------------------------------------------
// FatalBuilder
// ---------------------------------------------------------------------------

/// Builder for the `Fatal` envelope variant.
#[derive(Debug)]
pub struct FatalBuilder {
    ref_id: Option<String>,
    #[allow(dead_code)]
    code: Option<String>,
    message: String,
}

impl FatalBuilder {
    /// Optionally associate this fatal error with a specific run.
    #[must_use]
    pub fn ref_id(mut self, id: impl Into<String>) -> Self {
        self.ref_id = Some(id.into());
        self
    }

    /// Set an optional error code for programmatic handling.
    #[must_use]
    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Build the `Fatal` [`Envelope`].
    ///
    /// # Errors
    ///
    /// This builder currently always succeeds, but returns `Result` for
    /// API consistency.
    pub fn build(self) -> Result<Envelope, BuilderError> {
        Ok(Envelope::Fatal {
            ref_id: self.ref_id,
            error: self.message,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_builder_missing_backend() {
        let err = EnvelopeBuilder::hello().build().unwrap_err();
        assert_eq!(err, BuilderError::MissingField("backend"));
    }

    #[test]
    fn hello_builder_minimal() {
        let env = EnvelopeBuilder::hello()
            .backend("test")
            .build()
            .unwrap();
        match env {
            Envelope::Hello {
                backend, mode, capabilities, ..
            } => {
                assert_eq!(backend.id, "test");
                assert_eq!(mode, ExecutionMode::Mapped);
                assert!(capabilities.is_empty());
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn hello_builder_all_fields() {
        let env = EnvelopeBuilder::hello()
            .backend("sidecar")
            .version("2.0")
            .adapter_version("1.0")
            .mode(ExecutionMode::Passthrough)
            .capabilities(CapabilityManifest::new())
            .build()
            .unwrap();
        match env {
            Envelope::Hello {
                backend,
                mode,
                contract_version,
                ..
            } => {
                assert_eq!(backend.id, "sidecar");
                assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
                assert_eq!(backend.adapter_version.as_deref(), Some("1.0"));
                assert_eq!(mode, ExecutionMode::Passthrough);
                assert_eq!(contract_version, CONTRACT_VERSION);
            }
            _ => panic!("expected Hello"),
        }
    }
}
