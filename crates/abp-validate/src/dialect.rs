// SPDX-License-Identifier: MIT OR Apache-2.0
//! Dialect-aware request and response validation.
//!
//! Bridges [`abp_dialect::Dialect`] into the ABP validation framework,
//! providing [`DialectRequestValidator`] and [`DialectResponseValidator`]
//! that check JSON payloads against dialect-specific structural rules.

use abp_dialect::Dialect;
use serde_json::Value;

use crate::{ValidationErrorKind, ValidationErrors};

/// Validates a JSON request payload against the rules of a specific [`Dialect`].
#[derive(Debug, Clone)]
pub struct DialectRequestValidator {
    dialect: Dialect,
}

impl DialectRequestValidator {
    /// Create a validator targeting the given dialect.
    #[must_use]
    pub fn new(dialect: Dialect) -> Self {
        Self { dialect }
    }

    /// The dialect this validator targets.
    #[must_use]
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Validate `value` as a request in this dialect.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationErrors`] if the payload violates the dialect's request contract.
    pub fn validate(&self, value: &Value) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        let Some(obj) = value.as_object() else {
            errs.add(
                "",
                ValidationErrorKind::InvalidFormat,
                "request must be a JSON object",
            );
            return errs.into_result();
        };

        match self.dialect {
            Dialect::OpenAi => validate_openai_request(obj, &mut errs),
            Dialect::Claude => validate_claude_request(obj, &mut errs),
            Dialect::Gemini => validate_gemini_request(obj, &mut errs),
            Dialect::Codex => validate_codex_request(obj, &mut errs),
            Dialect::Kimi => validate_kimi_request(obj, &mut errs),
            Dialect::Copilot => validate_copilot_request(obj, &mut errs),
        }

        errs.into_result()
    }
}

/// Validates a JSON response payload against the rules of a specific [`Dialect`].
#[derive(Debug, Clone)]
pub struct DialectResponseValidator {
    dialect: Dialect,
}

impl DialectResponseValidator {
    /// Create a validator targeting the given dialect.
    #[must_use]
    pub fn new(dialect: Dialect) -> Self {
        Self { dialect }
    }

    /// The dialect this validator targets.
    #[must_use]
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Validate `value` as a response in this dialect.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationErrors`] if the payload violates the dialect's response contract.
    pub fn validate(&self, value: &Value) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        let Some(obj) = value.as_object() else {
            errs.add(
                "",
                ValidationErrorKind::InvalidFormat,
                "response must be a JSON object",
            );
            return errs.into_result();
        };

        match self.dialect {
            Dialect::OpenAi => validate_openai_response(obj, &mut errs),
            Dialect::Claude => validate_claude_response(obj, &mut errs),
            Dialect::Gemini => validate_gemini_response(obj, &mut errs),
            Dialect::Codex => validate_codex_response(obj, &mut errs),
            Dialect::Kimi => validate_kimi_response(obj, &mut errs),
            Dialect::Copilot => validate_copilot_response(obj, &mut errs),
        }

        errs.into_result()
    }
}

// ── Request validators ─────────────────────────────────────────────────

fn require_field(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    errs: &mut ValidationErrors,
) -> bool {
    if obj.contains_key(field) {
        true
    } else {
        errs.add(
            field,
            ValidationErrorKind::Required,
            format!("missing required field \"{field}\""),
        );
        false
    }
}

fn require_string_field(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    errs: &mut ValidationErrors,
) {
    match obj.get(field) {
        None => {
            errs.add(
                field,
                ValidationErrorKind::Required,
                format!("missing required field \"{field}\""),
            );
        }
        Some(v) if !v.is_string() => {
            errs.add(
                field,
                ValidationErrorKind::InvalidFormat,
                format!("\"{field}\" must be a string"),
            );
        }
        _ => {}
    }
}

fn validate_messages_array(
    obj: &serde_json::Map<String, Value>,
    errs: &mut ValidationErrors,
    require_role: bool,
) {
    match obj.get("messages") {
        Some(Value::Array(msgs)) => {
            if require_role {
                for (i, msg) in msgs.iter().enumerate() {
                    if msg.get("role").is_none() {
                        errs.add(
                            format!("messages[{i}].role"),
                            ValidationErrorKind::Required,
                            format!("messages[{i}] must have a \"role\" field"),
                        );
                    }
                }
            }
        }
        Some(_) => {
            errs.add(
                "messages",
                ValidationErrorKind::InvalidFormat,
                "\"messages\" must be an array",
            );
        }
        None => {
            errs.add(
                "messages",
                ValidationErrorKind::Required,
                "missing required field \"messages\"",
            );
        }
    }
}

fn validate_openai_request(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    require_string_field(obj, "model", errs);
    validate_messages_array(obj, errs, true);
}

fn validate_claude_request(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    require_string_field(obj, "model", errs);
    validate_messages_array(obj, errs, true);

    // Claude requires max_tokens
    if !obj.contains_key("max_tokens") {
        errs.add(
            "max_tokens",
            ValidationErrorKind::Required,
            "Claude requests require \"max_tokens\"",
        );
    }

    // Claude content blocks must be string or array
    if let Some(Value::Array(msgs)) = obj.get("messages") {
        for (i, msg) in msgs.iter().enumerate() {
            if let Some(content) = msg.get("content") {
                if !content.is_string() && !content.is_array() {
                    errs.add(
                        format!("messages[{i}].content"),
                        ValidationErrorKind::InvalidFormat,
                        "Claude message content must be a string or array of blocks",
                    );
                }
            }
        }
    }
}

fn validate_gemini_request(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    match obj.get("contents") {
        Some(Value::Array(contents)) => {
            for (i, c) in contents.iter().enumerate() {
                if c.get("parts").is_none() {
                    errs.add(
                        format!("contents[{i}].parts"),
                        ValidationErrorKind::Required,
                        format!("contents[{i}] must have a \"parts\" field"),
                    );
                }
            }
        }
        Some(_) => {
            errs.add(
                "contents",
                ValidationErrorKind::InvalidFormat,
                "\"contents\" must be an array",
            );
        }
        None => {
            errs.add(
                "contents",
                ValidationErrorKind::Required,
                "Gemini requests require \"contents\"",
            );
        }
    }
}

fn validate_codex_request(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    require_string_field(obj, "model", errs);
    // Codex responses API uses "input" not "messages"
    if !obj.contains_key("input") && !obj.contains_key("messages") {
        errs.add(
            "input",
            ValidationErrorKind::Required,
            "Codex requests require \"input\" or \"messages\"",
        );
    }
}

fn validate_kimi_request(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    require_string_field(obj, "model", errs);
    validate_messages_array(obj, errs, true);
}

fn validate_copilot_request(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    validate_messages_array(obj, errs, true);
}

// ── Response validators ────────────────────────────────────────────────

fn validate_openai_response(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    // Must have "choices" or "error"
    if !obj.contains_key("choices") && !obj.contains_key("error") {
        errs.add(
            "choices",
            ValidationErrorKind::Required,
            "OpenAI response must have \"choices\" or \"error\"",
        );
    }

    if let Some(Value::Array(choices)) = obj.get("choices") {
        for (i, choice) in choices.iter().enumerate() {
            if choice.get("message").is_none() && choice.get("delta").is_none() {
                errs.add(
                    format!("choices[{i}]"),
                    ValidationErrorKind::Required,
                    format!("choices[{i}] must have \"message\" or \"delta\""),
                );
            }
        }
    }
}

fn validate_claude_response(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    // Claude responses have "type": "message" or "error"
    match obj.get("type").and_then(Value::as_str) {
        Some("message") => {
            if !obj.contains_key("content") {
                errs.add(
                    "content",
                    ValidationErrorKind::Required,
                    "Claude message response must have \"content\"",
                );
            }
            require_string_field(obj, "role", errs);
        }
        Some("error") => {
            require_field(obj, "error", errs);
        }
        Some(other) => {
            // Streaming event types are valid
            let streaming = [
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
                "ping",
            ];
            if !streaming.contains(&other) {
                errs.add(
                    "type",
                    ValidationErrorKind::InvalidFormat,
                    format!("unexpected Claude response type \"{other}\""),
                );
            }
        }
        None => {
            errs.add(
                "type",
                ValidationErrorKind::Required,
                "Claude response must have a \"type\" field",
            );
        }
    }
}

fn validate_gemini_response(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    if !obj.contains_key("candidates") && !obj.contains_key("error") {
        errs.add(
            "candidates",
            ValidationErrorKind::Required,
            "Gemini response must have \"candidates\" or \"error\"",
        );
    }

    if let Some(Value::Array(candidates)) = obj.get("candidates") {
        for (i, c) in candidates.iter().enumerate() {
            if c.get("content").is_none() {
                errs.add(
                    format!("candidates[{i}].content"),
                    ValidationErrorKind::Required,
                    format!("candidates[{i}] must have \"content\""),
                );
            }
        }
    }
}

fn validate_codex_response(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    // Codex responses API: "object": "response", "status", "output"
    if !obj.contains_key("output") && !obj.contains_key("error") {
        errs.add(
            "output",
            ValidationErrorKind::Required,
            "Codex response must have \"output\" or \"error\"",
        );
    }

    if let Some(status) = obj.get("status").and_then(Value::as_str) {
        let valid = ["completed", "failed", "cancelled", "in_progress", "queued"];
        if !valid.contains(&status) {
            errs.add(
                "status",
                ValidationErrorKind::InvalidFormat,
                format!("unexpected Codex status \"{status}\""),
            );
        }
    }
}

fn validate_kimi_response(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    // Kimi follows OpenAI-compatible format
    if !obj.contains_key("choices") && !obj.contains_key("error") {
        errs.add(
            "choices",
            ValidationErrorKind::Required,
            "Kimi response must have \"choices\" or \"error\"",
        );
    }
}

fn validate_copilot_response(obj: &serde_json::Map<String, Value>, errs: &mut ValidationErrors) {
    // Copilot follows OpenAI-compatible format
    if !obj.contains_key("choices") && !obj.contains_key("error") {
        errs.add(
            "choices",
            ValidationErrorKind::Required,
            "Copilot response must have \"choices\" or \"error\"",
        );
    }
}
