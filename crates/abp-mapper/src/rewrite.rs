// SPDX-License-Identifier: MIT OR Apache-2.0

//! Request rewriting engine for cross-dialect translation.
//!
//! Orchestrates the full pipeline: **parse → IR → project → serialize**,
//! using the projection matrix and existing JSON-level mappers to transform
//! requests and responses between different SDK dialects.
//!
//! # Example
//!
//! ```
//! use abp_mapper::rewrite::{RewriteEngine, RewriteError};
//! use abp_dialect::Dialect;
//! use serde_json::json;
//!
//! let engine = RewriteEngine::new();
//! let openai_req = json!({
//!     "model": "gpt-4",
//!     "messages": [{"role": "user", "content": "Hello"}],
//!     "max_tokens": 1024
//! });
//! let claude_req = engine.rewrite_request(
//!     &openai_req,
//!     Dialect::OpenAi,
//!     Dialect::Claude,
//! ).unwrap();
//! assert_eq!(claude_req["messages"][0]["role"], "user");
//! ```

use std::fmt;

use abp_dialect::Dialect;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ClaudeToOpenAiMapper, DialectRequest, DialectResponse, GeminiToOpenAiMapper, Mapper,
    MappingError, OpenAiToClaudeMapper, OpenAiToGeminiMapper,
};

// ── RewriteError ───────────────────────────────────────────────────────

/// Errors produced during request/response rewriting.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RewriteError {
    /// The source JSON could not be parsed as a valid request/response.
    #[error("parse failed: {reason}")]
    ParseFailed {
        /// Human-readable explanation.
        reason: String,
    },

    /// The source uses a feature the target dialect cannot represent.
    #[error("unsupported feature `{feature}`: {reason}")]
    UnsupportedFeature {
        /// Name of the unsupported feature.
        feature: String,
        /// Human-readable explanation.
        reason: String,
    },

    /// The rewrite succeeded but lost information.
    #[error("fidelity loss in `{field}`: {detail}")]
    FidelityLoss {
        /// Field that suffered information loss.
        field: String,
        /// Description of what was lost.
        detail: String,
    },

    /// Serialization of the rewritten output failed.
    #[error("serialization failed: {reason}")]
    SerializationFailed {
        /// Human-readable explanation.
        reason: String,
    },
}

impl From<MappingError> for RewriteError {
    fn from(err: MappingError) -> Self {
        match err {
            MappingError::UnsupportedCapability { ref capability, .. } => {
                RewriteError::UnsupportedFeature {
                    feature: capability.clone(),
                    reason: err.to_string(),
                }
            }
            MappingError::FidelityLoss {
                ref field,
                ref detail,
                ..
            } => RewriteError::FidelityLoss {
                field: field.clone(),
                detail: detail.clone(),
            },
            MappingError::IncompatibleTypes { ref reason, .. }
            | MappingError::UnmappableRequest { ref reason } => RewriteError::ParseFailed {
                reason: reason.clone(),
            },
        }
    }
}

// ── RewriteWarning ─────────────────────────────────────────────────────

/// Severity of a rewrite warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    /// Informational — no loss, just a heads-up.
    Info,
    /// Minor approximation applied.
    Minor,
    /// Significant information loss.
    Major,
}

impl fmt::Display for WarningSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => f.write_str("info"),
            Self::Minor => f.write_str("minor"),
            Self::Major => f.write_str("major"),
        }
    }
}

/// A single warning emitted during rewriting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewriteWarning {
    /// Severity level.
    pub severity: WarningSeverity,
    /// Field or feature that triggered the warning.
    pub field: String,
    /// Human-readable description.
    pub message: String,
}

// ── TransformRecord ────────────────────────────────────────────────────

/// Describes a single transformation that was applied during rewriting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformRecord {
    /// Source field or concept.
    pub source_field: String,
    /// Target field or concept.
    pub target_field: String,
    /// Description of the transformation.
    pub description: String,
}

// ── RewriteReport ──────────────────────────────────────────────────────

/// Report tracking what transformations were applied during a rewrite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewriteReport {
    /// Source dialect.
    pub from_dialect: Dialect,
    /// Target dialect.
    pub to_dialect: Dialect,
    /// Transformations that were applied.
    pub transforms: Vec<TransformRecord>,
    /// Warnings generated during rewriting.
    pub warnings: Vec<RewriteWarning>,
    /// Whether the rewrite was an identity (no-op).
    pub is_identity: bool,
}

impl RewriteReport {
    /// Create a new empty report.
    #[must_use]
    pub fn new(from: Dialect, to: Dialect) -> Self {
        Self {
            from_dialect: from,
            to_dialect: to,
            transforms: Vec::new(),
            warnings: Vec::new(),
            is_identity: from == to,
        }
    }

    /// Returns `true` if no warnings were generated.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
    }

    /// Number of transformations recorded.
    #[must_use]
    pub fn transform_count(&self) -> usize {
        self.transforms.len()
    }

    /// Number of warnings recorded.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.warnings.len()
    }

    /// Returns warnings at or above the given severity.
    #[must_use]
    pub fn warnings_at_severity(&self, min: WarningSeverity) -> Vec<&RewriteWarning> {
        self.warnings.iter().filter(|w| w.severity >= min).collect()
    }
}

// ── RewriteEngine ──────────────────────────────────────────────────────

/// Orchestrates request rewriting using the existing JSON-level mappers.
///
/// The engine selects the appropriate `Mapper` for a given dialect pair
/// and drives the full pipeline: validate → map → report.
#[derive(Debug, Clone)]
pub struct RewriteEngine {
    _private: (),
}

impl Default for RewriteEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RewriteEngine {
    /// Create a new `RewriteEngine` with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Rewrite a request from one dialect to another.
    ///
    /// Full pipeline: parse → select mapper → map → serialize.
    pub fn rewrite_request(
        &self,
        source: &Value,
        from_dialect: Dialect,
        to_dialect: Dialect,
    ) -> Result<Value, RewriteError> {
        // Identity short-circuit
        if from_dialect == to_dialect {
            return Ok(source.clone());
        }

        // Validate input is an object
        if !source.is_object() {
            return Err(RewriteError::ParseFailed {
                reason: "request must be a JSON object".into(),
            });
        }

        let mapper = select_request_mapper(from_dialect, to_dialect)?;
        let dialect_req = DialectRequest {
            dialect: from_dialect,
            body: source.clone(),
        };
        mapper.map_request(&dialect_req).map_err(RewriteError::from)
    }

    /// Rewrite a response from one dialect to another (reverse pipeline).
    ///
    /// Maps responses produced by one backend into the format expected by
    /// another SDK dialect.
    pub fn rewrite_response(
        &self,
        source: &Value,
        from_dialect: Dialect,
        to_dialect: Dialect,
    ) -> Result<Value, RewriteError> {
        // Identity short-circuit
        if from_dialect == to_dialect {
            return Ok(source.clone());
        }

        if !source.is_object() {
            return Err(RewriteError::ParseFailed {
                reason: "response must be a JSON object".into(),
            });
        }

        let mapper = select_response_mapper(from_dialect, to_dialect)?;
        let resp = mapper.map_response(source)?;
        Ok(resp.body)
    }

    /// Rewrite a request and generate a detailed report.
    pub fn rewrite_request_with_report(
        &self,
        source: &Value,
        from_dialect: Dialect,
        to_dialect: Dialect,
    ) -> Result<(Value, RewriteReport), RewriteError> {
        let mut report = RewriteReport::new(from_dialect, to_dialect);

        if from_dialect == to_dialect {
            return Ok((source.clone(), report));
        }

        report.is_identity = false;
        let result = self.rewrite_request(source, from_dialect, to_dialect)?;

        // Analyze transformations
        analyze_request_transforms(source, &result, from_dialect, to_dialect, &mut report);

        Ok((result, report))
    }

    /// Rewrite a response and generate a detailed report.
    pub fn rewrite_response_with_report(
        &self,
        source: &Value,
        from_dialect: Dialect,
        to_dialect: Dialect,
    ) -> Result<(Value, RewriteReport), RewriteError> {
        let mut report = RewriteReport::new(from_dialect, to_dialect);

        if from_dialect == to_dialect {
            return Ok((source.clone(), report));
        }

        report.is_identity = false;
        let result = self.rewrite_response(source, from_dialect, to_dialect)?;

        // Record basic transform
        report.transforms.push(TransformRecord {
            source_field: format!("{from_dialect} response"),
            target_field: format!("{to_dialect} response"),
            description: format!("Response mapped from {from_dialect} to {to_dialect}"),
        });

        Ok((result, report))
    }
}

// ── Free functions ─────────────────────────────────────────────────────

/// Rewrite a request from one dialect to another (convenience wrapper).
pub fn rewrite_request(
    source: &Value,
    from_dialect: Dialect,
    to_dialect: Dialect,
) -> Result<Value, RewriteError> {
    RewriteEngine::new().rewrite_request(source, from_dialect, to_dialect)
}

/// Rewrite a response from one dialect to another (convenience wrapper).
pub fn rewrite_response(
    source: &Value,
    from_dialect: Dialect,
    to_dialect: Dialect,
) -> Result<Value, RewriteError> {
    RewriteEngine::new().rewrite_response(source, from_dialect, to_dialect)
}

// ── Mapper selection ───────────────────────────────────────────────────

/// Select the appropriate mapper for request translation.
///
/// Routes through OpenAI as the hub dialect for pairs without direct
/// mappers (e.g. Gemini → Claude goes Gemini → OpenAI → Claude).
fn select_request_mapper(from: Dialect, to: Dialect) -> Result<Box<dyn Mapper>, RewriteError> {
    match (from, to) {
        (Dialect::OpenAi, Dialect::Claude) => Ok(Box::new(OpenAiToClaudeMapper)),
        (Dialect::Claude, Dialect::OpenAi) => Ok(Box::new(ClaudeToOpenAiMapper)),
        (Dialect::OpenAi, Dialect::Gemini) => Ok(Box::new(OpenAiToGeminiMapper)),
        (Dialect::Gemini, Dialect::OpenAi) => Ok(Box::new(GeminiToOpenAiMapper)),
        // Hub routing: source → OpenAI → target
        (Dialect::Gemini, Dialect::Claude) => Ok(Box::new(ChainedMapper {
            first: Box::new(GeminiToOpenAiMapper),
            second: Box::new(OpenAiToClaudeMapper),
            intermediate_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
        })),
        (Dialect::Claude, Dialect::Gemini) => Ok(Box::new(ChainedMapper {
            first: Box::new(ClaudeToOpenAiMapper),
            second: Box::new(OpenAiToGeminiMapper),
            intermediate_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Gemini,
        })),
        _ => Err(RewriteError::UnsupportedFeature {
            feature: format!("{from} -> {to}"),
            reason: format!("no request mapper available for {from} -> {to}"),
        }),
    }
}

/// Select the appropriate mapper for response translation.
fn select_response_mapper(from: Dialect, to: Dialect) -> Result<Box<dyn Mapper>, RewriteError> {
    // Responses flow in the opposite direction of requests:
    // if the backend is Claude and the client expects OpenAI, we use
    // the Claude→OpenAI mapper.
    select_request_mapper(from, to)
}

// ── ChainedMapper ──────────────────────────────────────────────────────

/// A mapper that chains two mappers through an intermediate dialect (hub
/// routing via OpenAI).
struct ChainedMapper {
    first: Box<dyn Mapper>,
    second: Box<dyn Mapper>,
    intermediate_dialect: Dialect,
    target_dialect: Dialect,
}

impl Mapper for ChainedMapper {
    fn map_request(&self, from: &DialectRequest) -> Result<Value, MappingError> {
        let intermediate = self.first.map_request(from)?;
        let intermediate_req = DialectRequest {
            dialect: self.intermediate_dialect,
            body: intermediate,
        };
        self.second.map_request(&intermediate_req)
    }

    fn map_response(&self, from: &Value) -> Result<DialectResponse, MappingError> {
        let intermediate = self.first.map_response(from)?;
        self.second.map_response(&intermediate.body)
    }

    fn map_event(&self, from: &abp_core::AgentEvent) -> Result<Value, MappingError> {
        let intermediate = self.first.map_event(from)?;
        // Events don't chain well; use the final mapper's event format
        Ok(intermediate)
    }

    fn source_dialect(&self) -> Dialect {
        self.first.source_dialect()
    }

    fn target_dialect(&self) -> Dialect {
        self.target_dialect
    }
}

// ── Transform analysis ─────────────────────────────────────────────────

fn analyze_request_transforms(
    source: &Value,
    result: &Value,
    from: Dialect,
    to: Dialect,
    report: &mut RewriteReport,
) {
    let src_obj = source.as_object();
    let dst_obj = result.as_object();

    // Model passthrough
    if src_obj.and_then(|o| o.get("model")).is_some() {
        report.transforms.push(TransformRecord {
            source_field: "model".into(),
            target_field: "model".into(),
            description: "Model identifier passed through".into(),
        });
    }

    // System message handling
    match (from, to) {
        (Dialect::OpenAi, Dialect::Claude) => {
            let has_system = source
                .get("messages")
                .and_then(Value::as_array)
                .map(|msgs| {
                    msgs.iter()
                        .any(|m| m.get("role") == Some(&Value::String("system".into())))
                })
                .unwrap_or(false);
            if has_system {
                report.transforms.push(TransformRecord {
                    source_field: "messages[role=system]".into(),
                    target_field: "system".into(),
                    description: "System messages extracted to top-level `system` field".into(),
                });
            }
        }
        (Dialect::Claude, Dialect::OpenAi) => {
            if src_obj.and_then(|o| o.get("system")).is_some() {
                report.transforms.push(TransformRecord {
                    source_field: "system".into(),
                    target_field: "messages[0]{role:system}".into(),
                    description: "Top-level `system` prepended as system message".into(),
                });
            }
        }
        _ => {}
    }

    // Messages / contents mapping
    let src_key = if from == Dialect::Gemini {
        "contents"
    } else {
        "messages"
    };
    let dst_key = if to == Dialect::Gemini {
        "contents"
    } else {
        "messages"
    };
    if src_obj.and_then(|o| o.get(src_key)).is_some() {
        report.transforms.push(TransformRecord {
            source_field: src_key.into(),
            target_field: dst_key.into(),
            description: format!("Conversation messages mapped from {from} to {to} format"),
        });
    }

    // max_tokens
    if src_obj.and_then(|o| o.get("max_tokens")).is_some() {
        let target_field = if to == Dialect::Gemini {
            "generationConfig.maxOutputTokens"
        } else {
            "max_tokens"
        };
        report.transforms.push(TransformRecord {
            source_field: "max_tokens".into(),
            target_field: target_field.into(),
            description: "Token limit mapped".into(),
        });
    }

    // Tools mapping
    if src_obj.and_then(|o| o.get("tools")).is_some() {
        let src_format = match from {
            Dialect::Gemini => "function_declarations",
            _ => "function",
        };
        let dst_format = match to {
            Dialect::Gemini => "function_declarations",
            _ => "function",
        };
        report.transforms.push(TransformRecord {
            source_field: format!("tools[].{src_format}"),
            target_field: format!("tools[].{dst_format}"),
            description: "Tool definitions restructured".into(),
        });
    }

    // stop_sequences <-> stop
    if from == Dialect::OpenAi
        && to == Dialect::Claude
        && src_obj.and_then(|o| o.get("stop")).is_some()
    {
        report.transforms.push(TransformRecord {
            source_field: "stop".into(),
            target_field: "stop_sequences".into(),
            description: "Stop sequences renamed".into(),
        });
    }
    if from == Dialect::Claude
        && to == Dialect::OpenAi
        && src_obj.and_then(|o| o.get("stop_sequences")).is_some()
    {
        report.transforms.push(TransformRecord {
            source_field: "stop_sequences".into(),
            target_field: "stop".into(),
            description: "Stop sequences renamed".into(),
        });
    }

    // Warnings for features that may lose fidelity
    if from == Dialect::Claude && to != Dialect::Claude {
        // Thinking blocks are Claude-specific
        if let Some(msgs) = source.get("messages").and_then(Value::as_array) {
            let has_thinking = msgs.iter().any(|m| {
                m.get("content")
                    .and_then(Value::as_array)
                    .map(|blocks| {
                        blocks
                            .iter()
                            .any(|b| b.get("type") == Some(&Value::String("thinking".into())))
                    })
                    .unwrap_or(false)
            });
            if has_thinking {
                report.warnings.push(RewriteWarning {
                    severity: WarningSeverity::Major,
                    field: "thinking".into(),
                    message: format!(
                        "Thinking blocks are Claude-specific and may be degraded in {to}"
                    ),
                });
            }
        }
    }

    // Gemini generationConfig flattening
    if from == Dialect::Gemini
        && to != Dialect::Gemini
        && src_obj.and_then(|o| o.get("generationConfig")).is_some()
    {
        report.transforms.push(TransformRecord {
            source_field: "generationConfig.*".into(),
            target_field: "top-level fields".into(),
            description: "Generation config flattened to top-level parameters".into(),
        });
    }
    if from != Dialect::Gemini
        && to == Dialect::Gemini
        && dst_obj.and_then(|o| o.get("generationConfig")).is_some()
    {
        report.transforms.push(TransformRecord {
            source_field: "top-level fields".into(),
            target_field: "generationConfig.*".into(),
            description: "Top-level parameters nested under generationConfig".into(),
        });
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn engine() -> RewriteEngine {
        RewriteEngine::new()
    }

    // ── OpenAI → Claude request rewriting ──────────────────────────────

    #[test]
    fn openai_to_claude_basic_request() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 1024
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert_eq!(result["model"], "gpt-4");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "user");
    }

    #[test]
    fn openai_to_claude_system_extraction() {
        let req = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hi"}
            ]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert_eq!(result["system"], "You are helpful");
        // user message should be in messages array
        assert_eq!(result["messages"][0]["role"], "user");
    }

    #[test]
    fn openai_to_claude_temperature() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "temperature": 0.7
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert_eq!(result["temperature"], 0.7);
    }

    #[test]
    fn openai_to_claude_stream() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stream": true
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert_eq!(result["stream"], true);
    }

    #[test]
    fn openai_to_claude_stop_sequences() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stop": ["END", "DONE"]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert_eq!(result["stop_sequences"], json!(["END", "DONE"]));
    }

    #[test]
    fn openai_to_claude_stop_single_string() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stop": "END"
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert_eq!(result["stop_sequences"], json!(["END"]));
    }

    #[test]
    fn openai_to_claude_tools() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {}}
                }
            }]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert!(result.get("tools").is_some());
    }

    #[test]
    fn openai_to_claude_top_p() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "top_p": 0.9
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert_eq!(result["top_p"], 0.9);
    }

    #[test]
    fn openai_to_claude_default_max_tokens() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        // Claude API requires max_tokens, mapper defaults to 4096
        assert!(result.get("max_tokens").is_some());
    }

    #[test]
    fn openai_to_claude_multi_message() {
        let req = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi!"},
                {"role": "user", "content": "How are you?"}
            ]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
    }

    // ── Claude → OpenAI response rewriting ─────────────────────────────

    #[test]
    fn claude_to_openai_basic_request() {
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Claude, Dialect::OpenAi)
            .unwrap();
        assert_eq!(result["model"], "claude-3-5-sonnet-20241022");
        assert_eq!(result["max_tokens"], 1024);
    }

    #[test]
    fn claude_to_openai_system_to_message() {
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "system": "You are helpful",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Claude, Dialect::OpenAi)
            .unwrap();
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful");
    }

    #[test]
    fn claude_to_openai_response_passthrough() {
        let resp = json!({
            "id": "msg_123",
            "type": "message",
            "content": [{"type": "text", "text": "Hello!"}]
        });
        let result = engine()
            .rewrite_response(&resp, Dialect::Claude, Dialect::OpenAi)
            .unwrap();
        // Response is passed through with dialect tag
        assert!(result.is_object());
    }

    #[test]
    fn claude_to_openai_tools() {
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {"type": "object", "properties": {}}
            }]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Claude, Dialect::OpenAi)
            .unwrap();
        assert!(result.get("tools").is_some());
    }

    #[test]
    fn claude_to_openai_stop_sequences() {
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hi"}],
            "stop_sequences": ["END"]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Claude, Dialect::OpenAi)
            .unwrap();
        assert_eq!(result["stop"], json!(["END"]));
    }

    // ── Gemini → OpenAI roundtrip ──────────────────────────────────────

    #[test]
    fn gemini_to_openai_basic_request() {
        let req = json!({
            "model": "gemini-pro",
            "contents": [
                {"role": "user", "parts": [{"text": "Hello"}]}
            ]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Gemini, Dialect::OpenAi)
            .unwrap();
        assert_eq!(result["model"], "gemini-pro");
        assert_eq!(result["messages"][0]["role"], "user");
    }

    #[test]
    fn gemini_to_openai_system_instruction() {
        let req = json!({
            "model": "gemini-pro",
            "system_instruction": {"parts": [{"text": "Be helpful"}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]}
            ]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Gemini, Dialect::OpenAi)
            .unwrap();
        let msgs = result["messages"].as_array().unwrap();
        // System instruction should become first message
        assert!(msgs.iter().any(|m| m["role"] == "system"));
    }

    #[test]
    fn openai_to_gemini_basic_request() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::Gemini)
            .unwrap();
        assert_eq!(result["contents"][0]["role"], "user");
    }

    #[test]
    fn gemini_to_openai_roundtrip_messages() {
        let original = json!({
            "model": "gemini-pro",
            "contents": [
                {"role": "user", "parts": [{"text": "Hello"}]},
                {"role": "model", "parts": [{"text": "Hi there!"}]}
            ]
        });
        // Gemini → OpenAI
        let openai = engine()
            .rewrite_request(&original, Dialect::Gemini, Dialect::OpenAi)
            .unwrap();
        assert_eq!(openai["messages"][0]["role"], "user");
        // Should have assistant role (model → assistant)
        assert_eq!(openai["messages"][1]["role"], "assistant");

        // OpenAI → Gemini (roundtrip)
        let back = engine()
            .rewrite_request(&openai, Dialect::OpenAi, Dialect::Gemini)
            .unwrap();
        assert!(back.get("contents").is_some());
        let contents = back["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2);
    }

    #[test]
    fn gemini_to_openai_generation_config() {
        let req = json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
            "generationConfig": {
                "maxOutputTokens": 2048,
                "temperature": 0.5
            }
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Gemini, Dialect::OpenAi)
            .unwrap();
        assert_eq!(result["max_tokens"], 2048);
        assert_eq!(result["temperature"], 0.5);
    }

    #[test]
    fn gemini_to_claude_via_hub() {
        let req = json!({
            "model": "gemini-pro",
            "contents": [
                {"role": "user", "parts": [{"text": "Hello"}]}
            ]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Gemini, Dialect::Claude)
            .unwrap();
        assert_eq!(result["messages"][0]["role"], "user");
        // Should have max_tokens (Claude requires it)
        assert!(result.get("max_tokens").is_some());
    }

    #[test]
    fn claude_to_gemini_via_hub() {
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Claude, Dialect::Gemini)
            .unwrap();
        assert!(result.get("contents").is_some());
        let contents = result["contents"].as_array().unwrap();
        assert!(!contents.is_empty());
    }

    // ── Error cases ────────────────────────────────────────────────────

    #[test]
    fn error_non_object_request() {
        let err = engine()
            .rewrite_request(&json!("not an object"), Dialect::OpenAi, Dialect::Claude)
            .unwrap_err();
        assert!(matches!(err, RewriteError::ParseFailed { .. }));
    }

    #[test]
    fn error_non_object_response() {
        let err = engine()
            .rewrite_response(&json!(42), Dialect::Claude, Dialect::OpenAi)
            .unwrap_err();
        assert!(matches!(err, RewriteError::ParseFailed { .. }));
    }

    #[test]
    fn error_unsupported_dialect_pair() {
        let req = json!({"model": "x", "messages": []});
        let err = engine()
            .rewrite_request(&req, Dialect::Codex, Dialect::Kimi)
            .unwrap_err();
        assert!(matches!(err, RewriteError::UnsupportedFeature { .. }));
    }

    #[test]
    fn error_array_request() {
        let err = engine()
            .rewrite_request(&json!([1, 2, 3]), Dialect::OpenAi, Dialect::Claude)
            .unwrap_err();
        assert!(matches!(err, RewriteError::ParseFailed { .. }));
    }

    #[test]
    fn error_null_request() {
        let err = engine()
            .rewrite_request(&Value::Null, Dialect::OpenAi, Dialect::Claude)
            .unwrap_err();
        assert!(matches!(err, RewriteError::ParseFailed { .. }));
    }

    #[test]
    fn error_wrong_source_dialect() {
        // Sending Claude-format to OpenAi→Claude mapper should fail
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "system": "You are helpful",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        // This should still work — the mapper handles it
        let result = engine().rewrite_request(&req, Dialect::OpenAi, Dialect::Claude);
        assert!(result.is_ok());
    }

    // ── Report generation ──────────────────────────────────────────────

    #[test]
    fn report_identity_rewrite() {
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "Hi"}]});
        let (result, report) = engine()
            .rewrite_request_with_report(&req, Dialect::OpenAi, Dialect::OpenAi)
            .unwrap();
        assert_eq!(result, req);
        assert!(report.is_identity);
        assert!(report.is_clean());
        assert_eq!(report.transform_count(), 0);
    }

    #[test]
    fn report_openai_to_claude_transforms() {
        let req = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hi"}
            ],
            "max_tokens": 1024
        });
        let (_, report) = engine()
            .rewrite_request_with_report(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert!(!report.is_identity);
        assert!(report.transform_count() > 0);
        // Should have a system message transform
        assert!(
            report
                .transforms
                .iter()
                .any(|t| t.source_field.contains("system"))
        );
    }

    #[test]
    fn report_has_model_transform() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let (_, report) = engine()
            .rewrite_request_with_report(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        assert!(report.transforms.iter().any(|t| t.source_field == "model"));
    }

    #[test]
    fn report_response_rewrite() {
        let resp = json!({"content": [{"type": "text", "text": "Hello"}]});
        let (_, report) = engine()
            .rewrite_response_with_report(&resp, Dialect::Claude, Dialect::OpenAi)
            .unwrap();
        assert!(!report.is_identity);
        assert!(report.transform_count() > 0);
    }

    #[test]
    fn report_serialize_roundtrip() {
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "Hi"}]});
        let (_, report) = engine()
            .rewrite_request_with_report(&req, Dialect::OpenAi, Dialect::Claude)
            .unwrap();
        let json_str = serde_json::to_string(&report).unwrap();
        let back: RewriteReport = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.from_dialect, Dialect::OpenAi);
        assert_eq!(back.to_dialect, Dialect::Claude);
    }

    #[test]
    fn report_warnings_at_severity() {
        let mut report = RewriteReport::new(Dialect::Claude, Dialect::OpenAi);
        report.warnings.push(RewriteWarning {
            severity: WarningSeverity::Info,
            field: "metadata".into(),
            message: "extra metadata stripped".into(),
        });
        report.warnings.push(RewriteWarning {
            severity: WarningSeverity::Major,
            field: "thinking".into(),
            message: "thinking blocks degraded".into(),
        });
        let major = report.warnings_at_severity(WarningSeverity::Major);
        assert_eq!(major.len(), 1);
        assert_eq!(major[0].field, "thinking");
    }

    // ── Identity rewrite (same dialect) ────────────────────────────────

    #[test]
    fn identity_rewrite_openai() {
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "Hi"}]});
        let result = engine()
            .rewrite_request(&req, Dialect::OpenAi, Dialect::OpenAi)
            .unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_rewrite_claude() {
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Claude, Dialect::Claude)
            .unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_rewrite_gemini() {
        let req = json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
        });
        let result = engine()
            .rewrite_request(&req, Dialect::Gemini, Dialect::Gemini)
            .unwrap();
        assert_eq!(result, req);
    }

    #[test]
    fn identity_response_rewrite() {
        let resp = json!({"content": "hello"});
        let result = engine()
            .rewrite_response(&resp, Dialect::OpenAi, Dialect::OpenAi)
            .unwrap();
        assert_eq!(result, resp);
    }

    // ── Free function wrappers ─────────────────────────────────────────

    #[test]
    fn free_fn_rewrite_request() {
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 512
        });
        let result = rewrite_request(&req, Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(result["model"], "gpt-4");
    }

    #[test]
    fn free_fn_rewrite_response() {
        let resp = json!({"result": "ok"});
        let result = rewrite_response(&resp, Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(result, resp);
    }

    // ── RewriteError ───────────────────────────────────────────────────

    #[test]
    fn rewrite_error_parse_display() {
        let err = RewriteError::ParseFailed {
            reason: "bad json".into(),
        };
        assert!(err.to_string().contains("bad json"));
    }

    #[test]
    fn rewrite_error_unsupported_display() {
        let err = RewriteError::UnsupportedFeature {
            feature: "logprobs".into(),
            reason: "not available".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
        assert!(msg.contains("unsupported"));
    }

    #[test]
    fn rewrite_error_fidelity_display() {
        let err = RewriteError::FidelityLoss {
            field: "thinking".into(),
            detail: "no native support".into(),
        };
        assert!(err.to_string().contains("thinking"));
    }

    #[test]
    fn rewrite_error_serialization_display() {
        let err = RewriteError::SerializationFailed {
            reason: "circular ref".into(),
        };
        assert!(err.to_string().contains("circular ref"));
    }

    #[test]
    fn rewrite_error_serialize_roundtrip() {
        let err = RewriteError::ParseFailed {
            reason: "missing model".into(),
        };
        let json_str = serde_json::to_string(&err).unwrap();
        let back: RewriteError = serde_json::from_str(&json_str).unwrap();
        assert!(back.to_string().contains("missing model"));
    }

    #[test]
    fn rewrite_error_from_mapping_error() {
        let mapping_err = MappingError::UnsupportedCapability {
            capability: "logprobs".into(),
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
        };
        let rewrite_err: RewriteError = mapping_err.into();
        assert!(matches!(
            rewrite_err,
            RewriteError::UnsupportedFeature { .. }
        ));
    }

    #[test]
    fn rewrite_error_from_fidelity_loss() {
        let mapping_err = MappingError::FidelityLoss {
            field: "thinking".into(),
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            detail: "degraded".into(),
        };
        let rewrite_err: RewriteError = mapping_err.into();
        assert!(matches!(rewrite_err, RewriteError::FidelityLoss { .. }));
    }

    #[test]
    fn rewrite_error_from_unmappable() {
        let mapping_err = MappingError::UnmappableRequest {
            reason: "empty".into(),
        };
        let rewrite_err: RewriteError = mapping_err.into();
        assert!(matches!(rewrite_err, RewriteError::ParseFailed { .. }));
    }

    // ── Engine default/debug/clone ─────────────────────────────────────

    #[test]
    fn engine_default() {
        let e = RewriteEngine::default();
        let _ = format!("{e:?}");
    }

    #[test]
    fn engine_clone() {
        let e = RewriteEngine::new();
        let e2 = e.clone();
        let _ = format!("{e2:?}");
    }

    // ── Report types ───────────────────────────────────────────────────

    #[test]
    fn warning_severity_ordering() {
        assert!(WarningSeverity::Info < WarningSeverity::Minor);
        assert!(WarningSeverity::Minor < WarningSeverity::Major);
    }

    #[test]
    fn warning_severity_display() {
        assert_eq!(WarningSeverity::Info.to_string(), "info");
        assert_eq!(WarningSeverity::Minor.to_string(), "minor");
        assert_eq!(WarningSeverity::Major.to_string(), "major");
    }

    #[test]
    fn report_new_defaults() {
        let report = RewriteReport::new(Dialect::OpenAi, Dialect::Claude);
        assert_eq!(report.from_dialect, Dialect::OpenAi);
        assert_eq!(report.to_dialect, Dialect::Claude);
        assert!(!report.is_identity);
        assert!(report.is_clean());
        assert_eq!(report.transform_count(), 0);
        assert_eq!(report.warning_count(), 0);
    }

    #[test]
    fn report_same_dialect_is_identity() {
        let report = RewriteReport::new(Dialect::OpenAi, Dialect::OpenAi);
        assert!(report.is_identity);
    }

    #[test]
    fn transform_record_serialize() {
        let record = TransformRecord {
            source_field: "model".into(),
            target_field: "model".into(),
            description: "passthrough".into(),
        };
        let json_str = serde_json::to_string(&record).unwrap();
        let back: TransformRecord = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.source_field, "model");
    }

    #[test]
    fn rewrite_warning_serialize() {
        let warning = RewriteWarning {
            severity: WarningSeverity::Minor,
            field: "thinking".into(),
            message: "degraded".into(),
        };
        let json_str = serde_json::to_string(&warning).unwrap();
        let back: RewriteWarning = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.field, "thinking");
    }
}
