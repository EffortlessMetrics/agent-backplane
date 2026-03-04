// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! gemini-bridge
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![allow(dead_code, unused_imports)]
//!
//! Standalone Gemini SDK bridge built on `sidecar-kit` transport.
//!
//! # Modes
//!
//! - **Raw passthrough**: sends a raw vendor request, returns raw JSON events.
//!   Zero dependency on `abp-core`.
//! - **Normalized** (feature `normalized`): maps raw JSON events to typed
//!   `WorkOrder`, `Receipt`, and streaming events from `abp-core`.

/// Bridge configuration types.
pub mod config;
/// Error types for bridge operations.
pub mod error;
/// Extended function calling helpers (builders, validation, extraction).
pub mod function_calling;
/// Google Gemini GenerateContent API types (request, response, streaming, content blocks).
pub mod gemini_types;
/// Extended multimodal content types (Blob, FileData, VideoMetadata).
pub mod multimodal;
/// Extended safety helpers (profiles, analysis, typed block reasons).
pub mod safety;
/// Translation between Gemini API types and ABP contract types (feature-gated on `normalized`).
pub mod translate;

pub use config::GeminiBridgeConfig;
pub use error::BridgeError;

/// Main bridge handle.
pub struct GeminiBridge {
    config: GeminiBridgeConfig,
}

impl GeminiBridge {
    /// Create a new bridge handle from the given configuration.
    pub fn new(config: GeminiBridgeConfig) -> Self {
        Self { config }
    }

    /// Access the bridge configuration.
    pub fn config(&self) -> &GeminiBridgeConfig {
        &self.config
    }
}
