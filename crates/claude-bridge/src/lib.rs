// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! claude-bridge
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Standalone Claude SDK bridge built on `sidecar-kit` transport.
//!
//! # Modes
//!
//! - **Raw passthrough**: `ClaudeBridge::run_raw()` -- sends a raw vendor request,
//!   returns raw JSON events. Zero dependency on `abp-core`.
//! - **Raw mapped**: `ClaudeBridge::run_mapped_raw()` -- task string -> raw JSON events.
//! - **Normalized** (feature `normalized`): `ClaudeBridge::run_normalized()` -- maps
//!   raw JSON events to typed `AgentEvent` and `Receipt` from `abp-core`.

/// Bridge configuration types.
pub mod config;
/// Node.js and host-script discovery helpers.
pub mod discovery;
/// Error types for bridge operations.
pub mod error;
/// Normalized event mapping (feature-gated).
pub mod normalized;
/// Raw passthrough and mapped-mode run functions.
pub mod raw;

pub use config::ClaudeBridgeConfig;
pub use error::BridgeError;
pub use raw::RunOptions;
pub use sidecar_kit::RawRun;

/// Main bridge handle.
pub struct ClaudeBridge {
    config: ClaudeBridgeConfig,
}

impl ClaudeBridge {
    /// Create a new bridge handle from the given configuration.
    pub fn new(config: ClaudeBridgeConfig) -> Self {
        Self { config }
    }

    /// Passthrough: sends raw vendor request, returns raw vendor events.
    pub async fn run_raw(&self, request: serde_json::Value) -> Result<RawRun, BridgeError> {
        raw::run_raw(&self.config, request).await
    }

    /// Mapped mode: task string + options -> raw JSON events.
    pub async fn run_mapped_raw(
        &self,
        task: &str,
        opts: RunOptions,
    ) -> Result<RawRun, BridgeError> {
        raw::run_mapped_raw(&self.config, task, opts).await
    }
}

// Feature-gated normalized mode
#[cfg(feature = "normalized")]
impl ClaudeBridge {
    /// Normalized mode: maps raw events to typed AgentEvent and Receipt.
    pub async fn run_normalized(
        &self,
        task: &str,
        opts: RunOptions,
    ) -> Result<normalized::NormalizedRun, BridgeError> {
        normalized::run_normalized(&self.config, task, opts).await
    }
}
