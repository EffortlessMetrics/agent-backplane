// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-mapper
//!
//! Dialect mapping engine for the Agent Backplane.
//!
//! Provides the [`Mapper`] trait and concrete implementations that translate
//! requests, responses, and events between different agent-SDK dialects
//! (OpenAI, Claude, Gemini, Codex, Kimi, Copilot).
//!
//! ## Implementations
//!
//! - [`IdentityMapper`] — passthrough mapper that performs no transformation.
//! - [`OpenAiToClaudeMapper`] — maps OpenAI chat-completions format to Claude messages API.
//! - [`ClaudeToOpenAiMapper`] — maps Claude messages API format to OpenAI chat-completions.

mod claude_to_openai;
mod error;
mod identity;
mod openai_to_claude;

pub use claude_to_openai::ClaudeToOpenAiMapper;
pub use error::MappingError;
pub use identity::IdentityMapper;
pub use openai_to_claude::OpenAiToClaudeMapper;

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
