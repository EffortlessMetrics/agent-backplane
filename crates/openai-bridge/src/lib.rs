// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! openai-bridge
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Standalone OpenAI Chat Completions bridge built on `sidecar-kit` transport.
//!
//! # Modes
//!
//! - **Raw passthrough**: `OpenAiBridge::run_raw()` -- sends a raw vendor request,
//!   returns raw JSON events. Zero dependency on `abp-core`.
//! - **Raw mapped**: `OpenAiBridge::run_mapped_raw()` -- task string -> raw JSON events.
//! - **Normalized** (feature `normalized`): `OpenAiBridge::run_normalized()` -- maps
//!   raw JSON events to typed `AgentEvent` and `Receipt` from `abp-core`.

/// Bridge configuration types.
pub mod config;
/// Node.js and host-script discovery helpers.
pub mod discovery;
/// OpenAI Embeddings API types.
pub mod embeddings;
/// Error types for bridge operations.
pub mod error;
/// Full function / tool calling types, builders, and parallel assembly.
pub mod function_calling;
/// Translation between OpenAI API types and `abp-sdk-types` IR (feature-gated on `ir`).
pub mod ir_translate;
/// Normalized event mapping (feature-gated).
pub mod normalized;
/// OpenAI Chat Completions API types (request, response, streaming, tool calls).
pub mod openai_types;
/// Raw passthrough and mapped-mode run functions.
pub mod raw;
/// SSE stream parser for OpenAI streaming responses.
pub mod streaming;
/// Translation between OpenAI API types and ABP IR (feature-gated on `normalized`).
pub mod translate;

pub use config::OpenAiBridgeConfig;
pub use error::BridgeError;
pub use raw::RunOptions;
pub use sidecar_kit::RawRun;

/// Main bridge handle.
pub struct OpenAiBridge {
    config: OpenAiBridgeConfig,
}

impl OpenAiBridge {
    /// Create a new bridge handle from the given configuration.
    pub fn new(config: OpenAiBridgeConfig) -> Self {
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
impl OpenAiBridge {
    /// Normalized mode: maps raw events to typed AgentEvent and Receipt.
    pub async fn run_normalized(
        &self,
        task: &str,
        opts: RunOptions,
    ) -> Result<normalized::NormalizedRun, BridgeError> {
        normalized::run_normalized(&self.config, task, opts).await
    }
}
