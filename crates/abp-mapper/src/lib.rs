// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # Dialect mapping engine
//!
//! `abp-mapper` translates requests, responses, and streaming events between
//! different agent-SDK dialects (OpenAI, Claude, Gemini, Codex, Kimi, Copilot).
//!
//! ## Architecture
//!
//! Translation happens at two levels:
//!
//! * **JSON-level** — the [`Mapper`] trait operates on raw `serde_json::Value`
//!   payloads.  Implementations like [`OpenAiToClaudeMapper`] convert one
//!   vendor's JSON schema into another's.
//!
//! * **IR-level** — the [`IrMapper`] trait translates via the intermediate
//!   representation defined in `abp-ir`.  Use [`default_ir_mapper`] to obtain
//!   the correct mapper for a given dialect pair, or [`supported_ir_pairs`]
//!   to enumerate all implemented pairs.
//!
//! ## Projection matrix
//!
//! The [`projection`] module contains the cross-dialect mapping engine that
//! scores backend candidates and selects the best fit for a work order.
//!
//! ## Request rewriting
//!
//! The [`rewrite`] module provides a rule-driven engine for transforming
//! requests before they reach a backend, and the [`rules`] module defines
//! individual mapping rules.
//!
//! ## Fidelity and validation
//!
//! [`fidelity`] reports information-loss metrics for a mapping, and
//! [`validation`] runs a pipeline of checks to verify mapping correctness.
//!
//! ## Quick start
//!
//! ```
//! use abp_mapper::{Mapper, IdentityMapper, DialectRequest};
//! use abp_dialect::Dialect;
//! use serde_json::json;
//!
//! let mapper = IdentityMapper;
//! let req = DialectRequest {
//!     dialect: Dialect::OpenAi,
//!     body: json!({"model": "gpt-4", "messages": []}),
//! };
//! let mapped = mapper.map_request(&req).unwrap();
//! assert_eq!(mapped, req.body);
//! ```

mod claude_to_openai;
mod error;
mod factory;
mod gemini_to_openai;
mod identity;
mod ir_claude_gemini;
mod ir_claude_kimi;
mod ir_codex_claude;
mod ir_gemini_kimi;
mod ir_identity;
mod ir_mapper;
mod ir_openai_claude;
mod ir_openai_codex;
mod ir_openai_copilot;
mod ir_openai_gemini;
mod ir_openai_kimi;
mod map_error;
mod openai_to_claude;
mod openai_to_gemini;

/// Per-dialect capability descriptors.
pub mod capabilities;
/// Emulation strategies for partially-supported features.
pub mod emulation;
/// Fidelity reporting for dialect mapping.
pub mod fidelity;
/// Projection matrix — the core cross-dialect mapping engine.
pub mod projection;
/// Request rewriting engine for cross-dialect translation.
pub mod rewrite;
/// Mapping rules for dialect translation.
pub mod rules;
/// IR-level structural validation.
pub mod validate_ir;
/// Validation pipeline for mapping correctness.
pub mod validation;

pub use claude_to_openai::ClaudeToOpenAiMapper;
pub use error::MappingError;
pub use factory::{default_ir_mapper, supported_ir_pairs};
pub use gemini_to_openai::GeminiToOpenAiMapper;
pub use identity::IdentityMapper;
pub use ir_claude_gemini::ClaudeGeminiIrMapper;
pub use ir_claude_kimi::ClaudeKimiIrMapper;
pub use ir_codex_claude::CodexClaudeIrMapper;
pub use ir_gemini_kimi::GeminiKimiIrMapper;
pub use ir_identity::IrIdentityMapper;
pub use ir_mapper::IrMapper;
pub use ir_openai_claude::OpenAiClaudeIrMapper;
pub use ir_openai_codex::OpenAiCodexIrMapper;
pub use ir_openai_copilot::OpenAiCopilotIrMapper;
pub use ir_openai_gemini::OpenAiGeminiIrMapper;
pub use ir_openai_kimi::OpenAiKimiIrMapper;
pub use map_error::MapError;
pub use openai_to_claude::OpenAiToClaudeMapper;
pub use openai_to_gemini::OpenAiToGeminiMapper;

use abp_core::AgentEvent;
use abp_dialect::Dialect;

/// A dialect-specific request destined for mapping.
///
/// Wraps the source dialect tag alongside the raw JSON body so that mappers
/// can inspect the dialect without parsing the body first.
#[derive(Debug, Clone)]
pub struct DialectRequest {
    /// Source dialect that produced this request.
    pub dialect: Dialect,
    /// Raw JSON body of the request.
    pub body: serde_json::Value,
}

/// A dialect-specific response returned from mapping.
///
/// Wraps the target dialect tag alongside the mapped JSON body.
#[derive(Debug, Clone)]
pub struct DialectResponse {
    /// Target dialect this response conforms to.
    pub dialect: Dialect,
    /// Mapped JSON body.
    pub body: serde_json::Value,
}

/// Core mapping trait for dialect translation.
///
/// Each implementation handles one directional mapping (e.g. OpenAI → Claude).
/// The methods are intentionally synchronous — they perform pure data
/// transformations with no I/O.
///
/// # Examples
///
/// ```
/// use abp_mapper::{Mapper, IdentityMapper, DialectRequest};
/// use abp_dialect::Dialect;
/// use serde_json::json;
///
/// let mapper = IdentityMapper;
/// let req = DialectRequest {
///     dialect: Dialect::OpenAi,
///     body: json!({"model": "gpt-4", "messages": []}),
/// };
/// let mapped = mapper.map_request(&req).unwrap();
/// assert_eq!(mapped, req.body);
/// ```
pub trait Mapper: Send + Sync {
    /// Maps a dialect-specific request to the target JSON format.
    fn map_request(&self, from: &DialectRequest) -> Result<serde_json::Value, MappingError>;

    /// Maps a raw JSON response back into a [`DialectResponse`].
    fn map_response(&self, from: &serde_json::Value) -> Result<DialectResponse, MappingError>;

    /// Maps an [`AgentEvent`] to the target dialect's JSON event format.
    fn map_event(&self, from: &AgentEvent) -> Result<serde_json::Value, MappingError>;

    /// The source dialect this mapper reads from.
    fn source_dialect(&self) -> Dialect;

    /// The target dialect this mapper writes to.
    fn target_dialect(&self) -> Dialect;
}

#[cfg(test)]
mod ir_tests;

#[cfg(test)]
mod roundtrip_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dialect_request_debug() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({"model": "gpt-4"}),
        };
        let dbg = format!("{req:?}");
        assert!(dbg.contains("OpenAi"));
    }

    #[test]
    fn dialect_response_debug() {
        let resp = DialectResponse {
            dialect: Dialect::Claude,
            body: json!({"content": []}),
        };
        let dbg = format!("{resp:?}");
        assert!(dbg.contains("Claude"));
    }

    #[test]
    fn dialect_request_clone() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({"contents": []}),
        };
        let cloned = req.clone();
        assert_eq!(cloned.body, req.body);
    }
}
