// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! copilot-bridge
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Standalone GitHub Copilot bridge built on `sidecar-kit` transport.
//!
//! Provides Copilot-specific types (references, confirmations, function
//! calling) and translation to/from the ABP intermediate representation.

/// GitHub Copilot Chat API types (request, response, streaming, references, confirmations).
pub mod copilot_types;
/// Translation between Copilot API types and `abp-sdk-types` IR (feature-gated on `ir`).
pub mod ir_translate;
