// SPDX-License-Identifier: MIT OR Apache-2.0
//! Builder pattern for configuring a sidecar process.
//!
//! [`SidecarBuilder`] collects identity, capabilities, and a run handler,
//! then produces a [`SidecarRuntime`] that manages the JSONL protocol
//! lifecycle automatically.
//!
//! # Examples
//!
//! ```
//! use abp_sidecar_sdk::builder::SidecarBuilder;
//! use abp_core::{Capability, SupportLevel};
//!
//! let builder = SidecarBuilder::new("my-sidecar")
//!     .version("1.0.0")
//!     .capability(Capability::Streaming, SupportLevel::Native)
//!     .capability(Capability::ToolUse, SupportLevel::Native);
//!
//! assert_eq!(builder.name(), "my-sidecar");
//! assert_eq!(builder.backend_version(), Some("1.0.0"));
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use abp_core::{
    BackendIdentity, Capability, CapabilityManifest, ExecutionMode, Receipt, SupportLevel,
    WorkOrder,
};

use crate::emitter::EventEmitter;
use crate::runtime::SidecarRuntime;

/// The result type returned by a run handler.
pub type RunResult = Result<Receipt, SidecarError>;

/// A boxed, pinned, `Send` future — the return type of run handlers.
pub type BoxRunFuture = Pin<Box<dyn Future<Output = RunResult> + Send>>;

/// A run handler function: receives a work order and an event emitter,
/// returns a future that resolves to a [`Receipt`] or error.
pub type RunHandler = Arc<dyn Fn(WorkOrder, EventEmitter) -> BoxRunFuture + Send + Sync + 'static>;

/// Errors that can occur during sidecar operation.
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    /// I/O error during protocol communication.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The sidecar was built without a run handler.
    #[error("no run handler configured")]
    NoHandler,

    /// Protocol-level error (malformed JSONL, etc.).
    #[error("protocol error: {0}")]
    Protocol(String),

    /// The run handler returned an error.
    #[error("handler error: {0}")]
    Handler(String),
}

/// Builder for configuring and constructing a [`SidecarRuntime`].
///
/// # Examples
///
/// ```
/// use abp_sidecar_sdk::builder::SidecarBuilder;
/// use abp_core::{Capability, SupportLevel};
///
/// let builder = SidecarBuilder::new("test")
///     .version("0.1.0")
///     .adapter_version("0.2.0")
///     .capability(Capability::Streaming, SupportLevel::Native);
///
/// assert_eq!(builder.name(), "test");
/// ```
#[derive(Clone)]
pub struct SidecarBuilder {
    name: String,
    version: Option<String>,
    adapter_version: Option<String>,
    capabilities: CapabilityManifest,
    mode: ExecutionMode,
    handler: Option<RunHandler>,
}

impl std::fmt::Debug for SidecarBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidecarBuilder")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("adapter_version", &self.adapter_version)
            .field("capabilities", &self.capabilities)
            .field("mode", &self.mode)
            .field("handler", &self.handler.as_ref().map(|_| "..."))
            .finish()
    }
}

impl SidecarBuilder {
    /// Create a new builder with the given sidecar name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            adapter_version: None,
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
            handler: None,
        }
    }

    /// Set the backend version string.
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set the adapter version string.
    #[must_use]
    pub fn adapter_version(mut self, version: impl Into<String>) -> Self {
        self.adapter_version = Some(version.into());
        self
    }

    /// Register a capability with its support level.
    #[must_use]
    pub fn capability(mut self, cap: Capability, level: SupportLevel) -> Self {
        self.capabilities.insert(cap, level);
        self
    }

    /// Replace the entire capability manifest.
    #[must_use]
    pub fn capabilities(mut self, caps: CapabilityManifest) -> Self {
        self.capabilities = caps;
        self
    }

    /// Set the execution mode.
    #[must_use]
    pub fn mode(mut self, mode: ExecutionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Register the run handler that will process incoming work orders.
    ///
    /// The handler receives the [`WorkOrder`] and an [`EventEmitter`] for
    /// streaming events back, and must return a [`Receipt`] on success.
    #[must_use]
    pub fn on_run<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(WorkOrder, EventEmitter) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = RunResult> + Send + 'static,
    {
        self.handler = Some(Arc::new(move |wo, emitter| Box::pin(handler(wo, emitter))));
        self
    }

    /// Build the [`SidecarRuntime`].
    ///
    /// # Errors
    ///
    /// Returns [`SidecarError::NoHandler`] if no run handler was registered.
    pub fn build(self) -> Result<SidecarRuntime, SidecarError> {
        let handler = self.handler.ok_or(SidecarError::NoHandler)?;

        let identity = BackendIdentity {
            id: self.name.clone(),
            backend_version: self.version.clone(),
            adapter_version: self.adapter_version.clone(),
        };

        Ok(SidecarRuntime::new(
            identity,
            self.capabilities,
            self.mode,
            handler,
        ))
    }

    /// The configured sidecar name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The configured backend version.
    #[must_use]
    pub fn backend_version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The configured adapter version.
    #[must_use]
    pub fn adapter_version_str(&self) -> Option<&str> {
        self.adapter_version.as_deref()
    }

    /// The current capability manifest.
    #[must_use]
    pub fn capability_manifest(&self) -> &CapabilityManifest {
        &self.capabilities
    }

    /// The configured execution mode.
    #[must_use]
    pub fn execution_mode(&self) -> ExecutionMode {
        self.mode
    }

    /// Whether a run handler has been set.
    #[must_use]
    pub fn has_handler(&self) -> bool {
        self.handler.is_some()
    }

    /// Build the [`BackendIdentity`] from the current configuration.
    #[must_use]
    pub fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: self.version.clone(),
            adapter_version: self.adapter_version.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults() {
        let b = SidecarBuilder::new("test");
        assert_eq!(b.name(), "test");
        assert_eq!(b.backend_version(), None);
        assert_eq!(b.adapter_version_str(), None);
        assert!(b.capability_manifest().is_empty());
        assert_eq!(b.execution_mode(), ExecutionMode::Mapped);
        assert!(!b.has_handler());
    }

    #[test]
    fn builder_debug_impl() {
        let b = SidecarBuilder::new("test");
        let debug = format!("{b:?}");
        assert!(debug.contains("SidecarBuilder"));
        assert!(debug.contains("test"));
    }
}
