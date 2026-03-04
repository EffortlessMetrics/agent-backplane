// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! kimi-bridge
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Standalone Kimi SDK bridge built on `sidecar-kit` transport.
//!
//! # Features
//!
//! - **`ir`**: Enables translation between Kimi API types and `abp-dialect` IR.

/// Translation between Kimi API types and `abp-dialect` IR (feature-gated on `ir`).
pub mod ir_translate;
/// Kimi Chat Completions API types (request, response, streaming, tool calls).
pub mod kimi_types;
