// SPDX-License-Identifier: MIT OR Apache-2.0
//! codex-bridge
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Codex Responses API bridge for Agent Backplane.
//!
//! Provides translation between OpenAI Codex Responses API types and the
//! vendor-agnostic Intermediate Representation (IR) defined in `abp-dialect`.

/// Translation between Codex Responses API types and `abp-dialect` IR (feature-gated on `ir`).
pub mod ir_translate;
