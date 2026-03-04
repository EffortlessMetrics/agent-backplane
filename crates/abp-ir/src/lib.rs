// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![deny(unsafe_code)]
//! Re-exports of the cross-dialect IR types from [`abp_core::ir`].
//!
//! This crate provides a focused entry point for the intermediate
//! representation layer, keeping the heavy property-based test suite
//! isolated from `abp-core` itself.
//!
//! In addition to the core IR types it provides:
//!
//! - **[`normalize`]** — pure normalization passes (dedup system messages,
//!   trim text, merge adjacent blocks, strip metadata, extract system, etc.)
//! - **[`lower`]** — lowering functions that transform normalized IR into
//!   vendor-specific request formats (OpenAI, Claude, Gemini, and friends).

pub use abp_core::ir::*;

/// Conversation normalization passes.
pub mod normalize;

/// Lowering functions from IR to vendor-specific formats.
pub mod lower;
