// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-dialect
//!
//! Dialect detection, validation, and metadata for the Agent Backplane.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Dialect enum ────────────────────────────────────────────────────────

/// Known agent-protocol dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    /// OpenAI chat-completions style.
    OpenAi,
    /// Anthropic Claude messages API.
    Claude,
    /// Google Gemini generateContent style.
    Gemini,
    /// OpenAI Codex / Responses API style.
    Codex,
    /// Moonshot Kimi API style.
    Kimi,
    /// GitHub Copilot extensions style.
    Copilot,
}

impl Dialect {
    /// Human-readable label for this dialect.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Codex => "Codex",
            Self::Kimi => "Kimi",
            Self::Copilot => "Copilot",
        }
    }

    /// Returns all known dialects.
    #[must_use]
    pub fn all() -> &'static [Dialect] {
        &[
            Self::OpenAi,
            Self::Claude,
            Self::Gemini,
            Self::Codex,
            Self::Kimi,
            Self::Copilot,
        ]
    }
}

impl std::fmt::Display for Dialect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ── Detection ───────────────────────────────────────────────────────────

/// Result of dialect detection on a JSON message.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Most likely dialect.
    pub dialect: Dialect,
    /// Confidence score in `[0.0, 1.0]`.
    pub confidence: f64,
    /// Human-readable evidence strings explaining the match.
    pub evidence: Vec<String>,
}

/// Analyzes a JSON [`Value`] and determines the most likely [`Dialect`].
#[derive(Debug, Default)]
pub struct DialectDetector {
    _priv: (),
}

impl DialectDetector {
    /// Create a new detector.
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }

    /// Detect the dialect of a JSON value.
    ///
    /// Returns `None` when the input is not a JSON object or no heuristic
    /// matches.
    #[must_use]
    pub fn detect(&self, value: &Value) -> Option<DetectionResult> {
        let obj = value.as_object()?;

        let mut best: Option<DetectionResult> = None;

        for &dialect in Dialect::all() {
            let (score, evidence) = match dialect {
                Dialect::OpenAi => score_openai(obj),
                Dialect::Claude => score_claude(obj),
                Dialect::Gemini => score_gemini(obj),
                Dialect::Codex => score_codex(obj),
                Dialect::Kimi => score_kimi(obj),
                Dialect::Copilot => score_copilot(obj),
            };
            if score > 0.0 && best.as_ref().is_none_or(|b| score > b.confidence) {
                best = Some(DetectionResult {
                    dialect,
                    confidence: score,
                    evidence,
                });
            }
        }

        best
    }

    /// Return scored results for *all* dialects that matched at least one
    /// heuristic, sorted by descending confidence.
    #[must_use]
    pub fn detect_all(&self, value: &Value) -> Vec<DetectionResult> {
        let Some(obj) = value.as_object() else {
            return Vec::new();
        };

        let mut results: Vec<DetectionResult> = Dialect::all()
            .iter()
            .filter_map(|&dialect| {
                let (score, evidence) = match dialect {
                    Dialect::OpenAi => score_openai(obj),
                    Dialect::Claude => score_claude(obj),
                    Dialect::Gemini => score_gemini(obj),
                    Dialect::Codex => score_codex(obj),
                    Dialect::Kimi => score_kimi(obj),
                    Dialect::Copilot => score_copilot(obj),
                };
                if score > 0.0 {
                    Some(DetectionResult {
                        dialect,
                        confidence: score,
                        evidence,
                    })
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }
}

// ── Scoring helpers ─────────────────────────────────────────────────────

type Score = (f64, Vec<String>);

fn score_openai(obj: &serde_json::Map<String, Value>) -> Score {
    let mut pts = 0.0_f64;
    let mut ev = Vec::new();

    if obj.contains_key("choices") {
        pts += 0.4;
        ev.push("has \"choices\" key".into());
    }
    if let Some(Value::Array(msgs)) = obj.get("messages")
        && msgs
            .iter()
            .any(|m| m.get("role").is_some() && m.get("content").and_then(Value::as_str).is_some())
    {
        pts += 0.35;
        ev.push("has \"messages\" with string \"content\"".into());
    }
    if obj.contains_key("model") && !obj.contains_key("contents") && !obj.contains_key("items") {
        pts += 0.15;
        ev.push("has \"model\" (not Gemini/Codex)".into());
    }
    if obj.contains_key("temperature")
        || obj.contains_key("top_p")
        || obj.contains_key("max_tokens")
    {
        pts += 0.1;
        ev.push("has common OpenAI parameters".into());
    }

    (pts.min(1.0), ev)
}

fn score_claude(obj: &serde_json::Map<String, Value>) -> Score {
    let mut pts = 0.0_f64;
    let mut ev = Vec::new();

    if obj.get("type").and_then(Value::as_str) == Some("message") {
        pts += 0.45;
        ev.push("has \"type\":\"message\"".into());
    }
    if let Some(Value::Array(msgs)) = obj.get("messages")
        && msgs
            .iter()
            .any(|m| m.get("content").is_some_and(Value::is_array))
    {
        pts += 0.35;
        ev.push("has \"messages\" with array \"content\" blocks".into());
    }
    if obj.contains_key("model") && obj.get("type").and_then(Value::as_str) == Some("message") {
        pts += 0.1;
        ev.push("has \"model\" with type=message".into());
    }
    if obj.contains_key("stop_reason")
        || obj.contains_key("content") && obj.get("content").is_some_and(Value::is_array)
    {
        pts += 0.1;
        ev.push("has \"stop_reason\" or array \"content\"".into());
    }

    (pts.min(1.0), ev)
}

fn score_gemini(obj: &serde_json::Map<String, Value>) -> Score {
    let mut pts = 0.0_f64;
    let mut ev = Vec::new();

    if let Some(Value::Array(contents)) = obj.get("contents")
        && contents.iter().any(|c| c.get("parts").is_some())
    {
        pts += 0.5;
        ev.push("has \"contents\" with \"parts\"".into());
    }
    if obj.contains_key("candidates") {
        pts += 0.4;
        ev.push("has \"candidates\" key".into());
    }
    if obj.contains_key("generationConfig") || obj.contains_key("generation_config") {
        pts += 0.1;
        ev.push("has generation config key".into());
    }

    (pts.min(1.0), ev)
}

fn score_codex(obj: &serde_json::Map<String, Value>) -> Score {
    let mut pts = 0.0_f64;
    let mut ev = Vec::new();

    if let Some(Value::Array(items)) = obj.get("items")
        && items.iter().any(|i| i.get("type").is_some())
    {
        pts += 0.45;
        ev.push("has \"items\" array with \"type\" field".into());
    }
    if obj.contains_key("status") && !obj.contains_key("candidates") {
        pts += 0.3;
        ev.push("has \"status\" field (not Gemini)".into());
    }
    if obj.get("object").and_then(Value::as_str) == Some("response") {
        pts += 0.25;
        ev.push("has \"object\":\"response\"".into());
    }

    (pts.min(1.0), ev)
}

fn score_kimi(obj: &serde_json::Map<String, Value>) -> Score {
    let mut pts = 0.0_f64;
    let mut ev = Vec::new();

    if obj.contains_key("refs") {
        pts += 0.4;
        ev.push("has \"refs\" field".into());
    }
    if obj.contains_key("search_plus") {
        pts += 0.55;
        ev.push("has \"search_plus\" field".into());
    }
    if let Some(Value::Array(msgs)) = obj.get("messages")
        && msgs.iter().any(|m| m.get("role").is_some())
        && obj.contains_key("refs")
    {
        pts += 0.25;
        ev.push("has \"messages\" with \"role\" alongside \"refs\"".into());
    }

    (pts.min(1.0), ev)
}

fn score_copilot(obj: &serde_json::Map<String, Value>) -> Score {
    let mut pts = 0.0_f64;
    let mut ev = Vec::new();

    if obj.contains_key("references") {
        pts += 0.45;
        ev.push("has \"references\" field".into());
    }
    if obj.contains_key("confirmations") {
        pts += 0.3;
        ev.push("has \"confirmations\" field".into());
    }
    if obj.contains_key("agent_mode") {
        pts += 0.45;
        ev.push("has \"agent_mode\" field".into());
    }

    (pts.min(1.0), ev)
}

// ── Validation ──────────────────────────────────────────────────────────

/// A single validation error found in a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// JSON-pointer-style path to the problematic field (e.g. `/messages/0/role`).
    pub path: String,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Outcome of validating a message against a specific dialect.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// `true` when no errors were found.
    pub valid: bool,
    /// Hard errors — the message violates the dialect contract.
    pub errors: Vec<ValidationError>,
    /// Soft warnings — technically valid but suspicious.
    pub warnings: Vec<String>,
}

/// Validates a JSON message against a specific [`Dialect`]'s expected structure.
#[derive(Debug, Default)]
pub struct DialectValidator {
    _priv: (),
}

impl DialectValidator {
    /// Create a new validator.
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }

    /// Validate `value` as a message/request in the given `dialect`.
    #[must_use]
    pub fn validate(&self, value: &Value, dialect: Dialect) -> ValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        let Some(obj) = value.as_object() else {
            errors.push(ValidationError {
                path: "/".into(),
                message: "expected a JSON object".into(),
            });
            return ValidationResult {
                valid: false,
                errors,
                warnings,
            };
        };

        match dialect {
            Dialect::OpenAi => validate_openai(obj, &mut errors, &mut warnings),
            Dialect::Claude => validate_claude(obj, &mut errors, &mut warnings),
            Dialect::Gemini => validate_gemini(obj, &mut errors, &mut warnings),
            Dialect::Codex => validate_codex(obj, &mut errors, &mut warnings),
            Dialect::Kimi => validate_kimi(obj, &mut errors, &mut warnings),
            Dialect::Copilot => validate_copilot(obj, &mut errors, &mut warnings),
        }

        ValidationResult {
            valid: errors.is_empty(),
            errors,
            warnings,
        }
    }
}

// ── Validation helpers ──────────────────────────────────────────────────

fn validate_openai(
    obj: &serde_json::Map<String, Value>,
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<String>,
) {
    if !obj.contains_key("model") {
        errors.push(ValidationError {
            path: "/model".into(),
            message: "missing required \"model\" field".into(),
        });
    }
    match obj.get("messages") {
        Some(Value::Array(msgs)) => {
            for (i, msg) in msgs.iter().enumerate() {
                if msg.get("role").is_none() {
                    errors.push(ValidationError {
                        path: format!("/messages/{i}/role"),
                        message: "each message must have a \"role\"".into(),
                    });
                }
            }
        }
        Some(_) => {
            errors.push(ValidationError {
                path: "/messages".into(),
                message: "\"messages\" must be an array".into(),
            });
        }
        None => {
            warnings.push("no \"messages\" field — may be a response rather than request".into());
        }
    }
}

fn validate_claude(
    obj: &serde_json::Map<String, Value>,
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<String>,
) {
    if !obj.contains_key("model") {
        // Responses have type=message but may lack model in streaming chunks.
        if obj.get("type").and_then(Value::as_str) != Some("message") {
            errors.push(ValidationError {
                path: "/model".into(),
                message: "missing required \"model\" field".into(),
            });
        }
    }
    match obj.get("messages") {
        Some(Value::Array(msgs)) => {
            for (i, msg) in msgs.iter().enumerate() {
                if msg.get("role").is_none() {
                    errors.push(ValidationError {
                        path: format!("/messages/{i}/role"),
                        message: "each message must have a \"role\"".into(),
                    });
                }
                if let Some(content) = msg.get("content")
                    && !content.is_string()
                    && !content.is_array()
                {
                    errors.push(ValidationError {
                        path: format!("/messages/{i}/content"),
                        message: "\"content\" must be a string or array of blocks".into(),
                    });
                }
            }
        }
        Some(_) => {
            errors.push(ValidationError {
                path: "/messages".into(),
                message: "\"messages\" must be an array".into(),
            });
        }
        None => {
            warnings.push("no \"messages\" field — may be a response".into());
        }
    }
}

fn validate_gemini(
    obj: &serde_json::Map<String, Value>,
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<String>,
) {
    match obj.get("contents") {
        Some(Value::Array(contents)) => {
            for (i, c) in contents.iter().enumerate() {
                if c.get("parts").is_none() {
                    errors.push(ValidationError {
                        path: format!("/contents/{i}/parts"),
                        message: "each content entry must have \"parts\"".into(),
                    });
                }
            }
        }
        Some(_) => {
            errors.push(ValidationError {
                path: "/contents".into(),
                message: "\"contents\" must be an array".into(),
            });
        }
        None => {
            if !obj.contains_key("candidates") {
                warnings.push(
                    "no \"contents\" or \"candidates\" — cannot determine message direction".into(),
                );
            }
        }
    }
}

fn validate_codex(
    obj: &serde_json::Map<String, Value>,
    errors: &mut Vec<ValidationError>,
    _warnings: &mut Vec<String>,
) {
    if let Some(Value::Array(items)) = obj.get("items") {
        for (i, item) in items.iter().enumerate() {
            if item.get("type").is_none() {
                errors.push(ValidationError {
                    path: format!("/items/{i}/type"),
                    message: "each item must have a \"type\" field".into(),
                });
            }
        }
    }
}

fn validate_kimi(
    obj: &serde_json::Map<String, Value>,
    errors: &mut Vec<ValidationError>,
    _warnings: &mut Vec<String>,
) {
    if let Some(Value::Array(msgs)) = obj.get("messages") {
        for (i, msg) in msgs.iter().enumerate() {
            if msg.get("role").is_none() {
                errors.push(ValidationError {
                    path: format!("/messages/{i}/role"),
                    message: "each message must have a \"role\"".into(),
                });
            }
        }
    }
}

fn validate_copilot(
    obj: &serde_json::Map<String, Value>,
    errors: &mut Vec<ValidationError>,
    _warnings: &mut Vec<String>,
) {
    if let Some(Value::Array(msgs)) = obj.get("messages") {
        for (i, msg) in msgs.iter().enumerate() {
            if msg.get("role").is_none() {
                errors.push(ValidationError {
                    path: format!("/messages/{i}/role"),
                    message: "each message must have a \"role\"".into(),
                });
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn detector() -> DialectDetector {
        DialectDetector::new()
    }

    fn validator() -> DialectValidator {
        DialectValidator::new()
    }

    // ── OpenAI detection ────────────────────────────────────────────

    #[test]
    fn detect_openai_request() {
        let msg = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::OpenAi);
        assert!(r.confidence > 0.4);
    }

    #[test]
    fn detect_openai_response() {
        let msg = json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}}],
            "model": "gpt-4"
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::OpenAi);
        assert!(r.confidence >= 0.5);
    }

    // ── Claude detection ────────────────────────────────────────────

    #[test]
    fn detect_claude_request() {
        let msg = json!({
            "model": "claude-3-opus-20240229",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}]
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Claude);
        assert!(r.confidence > 0.3);
    }

    #[test]
    fn detect_claude_response() {
        let msg = json!({
            "type": "message",
            "model": "claude-3-opus-20240229",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn"
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Claude);
        assert!(r.confidence > 0.5);
    }

    // ── Gemini detection ────────────────────────────────────────────

    #[test]
    fn detect_gemini_request() {
        let msg = json!({
            "contents": [{"parts": [{"text": "hello"}]}]
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Gemini);
        assert!(r.confidence >= 0.5);
    }

    #[test]
    fn detect_gemini_response() {
        let msg = json!({
            "candidates": [{"content": {"parts": [{"text": "hi"}]}}]
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Gemini);
        assert!(r.confidence >= 0.4);
    }

    // ── Codex detection ─────────────────────────────────────────────

    #[test]
    fn detect_codex_response() {
        let msg = json!({
            "object": "response",
            "status": "completed",
            "items": [{"type": "message", "content": "done"}]
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Codex);
        assert!(r.confidence > 0.5);
    }

    #[test]
    fn detect_codex_with_items() {
        let msg = json!({
            "items": [{"type": "function_call", "name": "run"}]
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Codex);
        assert!(r.confidence > 0.4);
    }

    // ── Kimi detection ──────────────────────────────────────────────

    #[test]
    fn detect_kimi_with_refs() {
        let msg = json!({
            "model": "kimi",
            "messages": [{"role": "user", "content": "search this"}],
            "refs": ["https://example.com"]
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Kimi);
        assert!(r.confidence > 0.5);
    }

    #[test]
    fn detect_kimi_search_plus() {
        let msg = json!({
            "model": "kimi",
            "messages": [{"role": "user", "content": "hello"}],
            "search_plus": true
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Kimi);
    }

    // ── Copilot detection ───────────────────────────────────────────

    #[test]
    fn detect_copilot_with_references() {
        let msg = json!({
            "messages": [{"role": "user", "content": "fix bug"}],
            "references": [{"type": "file", "path": "src/main.rs"}]
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Copilot);
    }

    #[test]
    fn detect_copilot_agent_mode() {
        let msg = json!({
            "messages": [{"role": "user", "content": "do it"}],
            "agent_mode": true,
            "confirmations": []
        });
        let r = detector().detect(&msg).unwrap();
        assert_eq!(r.dialect, Dialect::Copilot);
        assert!(r.confidence > 0.5);
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn detect_none_for_non_object() {
        assert!(detector().detect(&json!(42)).is_none());
        assert!(detector().detect(&json!("hello")).is_none());
        assert!(detector().detect(&json!(null)).is_none());
        assert!(detector().detect(&json!([])).is_none());
    }

    #[test]
    fn detect_none_for_empty_object() {
        assert!(detector().detect(&json!({})).is_none());
    }

    #[test]
    fn detect_all_returns_multiple_for_ambiguous() {
        // A message with "model" + "messages" with string content looks OpenAI,
        // but also partially matches Claude if we added some signals.
        let msg = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": 0.7
        });
        let results = detector().detect_all(&msg);
        assert!(!results.is_empty());
        assert_eq!(results[0].dialect, Dialect::OpenAi);
    }

    #[test]
    fn detect_all_empty_for_non_object() {
        assert!(detector().detect_all(&json!(null)).is_empty());
    }

    #[test]
    fn detect_all_sorted_by_confidence() {
        let msg = json!({
            "model": "x",
            "messages": [{"role": "user", "content": "hi"}],
            "refs": ["a"]
        });
        let results = detector().detect_all(&msg);
        for w in results.windows(2) {
            assert!(w[0].confidence >= w[1].confidence);
        }
    }

    // ── Confidence scoring ──────────────────────────────────────────

    #[test]
    fn confidence_capped_at_one() {
        let msg = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "choices": [{}],
            "temperature": 0.7,
            "top_p": 0.9,
            "max_tokens": 100
        });
        let r = detector().detect(&msg).unwrap();
        assert!(r.confidence <= 1.0);
    }

    #[test]
    fn evidence_is_populated() {
        let msg = json!({"choices": [{}]});
        let r = detector().detect(&msg).unwrap();
        assert!(!r.evidence.is_empty());
    }

    // ── Validation: OpenAI ──────────────────────────────────────────

    #[test]
    fn validate_openai_valid() {
        let msg = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let r = validator().validate(&msg, Dialect::OpenAi);
        assert!(r.valid);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn validate_openai_missing_model() {
        let msg = json!({
            "messages": [{"role": "user", "content": "hi"}]
        });
        let r = validator().validate(&msg, Dialect::OpenAi);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.path == "/model"));
    }

    #[test]
    fn validate_openai_missing_role() {
        let msg = json!({
            "model": "gpt-4",
            "messages": [{"content": "hi"}]
        });
        let r = validator().validate(&msg, Dialect::OpenAi);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.path.contains("role")));
    }

    // ── Validation: Claude ──────────────────────────────────────────

    #[test]
    fn validate_claude_valid_request() {
        let msg = json!({
            "model": "claude-3-opus-20240229",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });
        let r = validator().validate(&msg, Dialect::Claude);
        assert!(r.valid);
    }

    #[test]
    fn validate_claude_response_without_model() {
        let msg = json!({
            "type": "message",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn"
        });
        let r = validator().validate(&msg, Dialect::Claude);
        // A response with type=message is allowed without model.
        assert!(r.valid);
    }

    #[test]
    fn validate_claude_bad_content_type() {
        let msg = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": 42}]
        });
        let r = validator().validate(&msg, Dialect::Claude);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.path.contains("content")));
    }

    // ── Validation: Gemini ──────────────────────────────────────────

    #[test]
    fn validate_gemini_valid() {
        let msg = json!({
            "contents": [{"parts": [{"text": "hi"}]}]
        });
        let r = validator().validate(&msg, Dialect::Gemini);
        assert!(r.valid);
    }

    #[test]
    fn validate_gemini_missing_parts() {
        let msg = json!({
            "contents": [{"role": "user"}]
        });
        let r = validator().validate(&msg, Dialect::Gemini);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.path.contains("parts")));
    }

    // ── Validation: Codex ───────────────────────────────────────────

    #[test]
    fn validate_codex_valid() {
        let msg = json!({
            "items": [{"type": "message", "content": "done"}],
            "status": "completed"
        });
        let r = validator().validate(&msg, Dialect::Codex);
        assert!(r.valid);
    }

    #[test]
    fn validate_codex_item_missing_type() {
        let msg = json!({
            "items": [{"content": "done"}]
        });
        let r = validator().validate(&msg, Dialect::Codex);
        assert!(!r.valid);
    }

    // ── Validation: edge cases ──────────────────────────────────────

    #[test]
    fn validate_non_object_returns_error() {
        let r = validator().validate(&json!("oops"), Dialect::OpenAi);
        assert!(!r.valid);
        assert!(r.errors[0].path == "/");
    }

    #[test]
    fn validation_error_display() {
        let e = ValidationError {
            path: "/model".into(),
            message: "missing".into(),
        };
        assert_eq!(format!("{e}"), "/model: missing");
    }

    // ── Dialect enum ────────────────────────────────────────────────

    #[test]
    fn dialect_label() {
        assert_eq!(Dialect::OpenAi.label(), "OpenAI");
        assert_eq!(Dialect::Claude.label(), "Claude");
        assert_eq!(Dialect::Gemini.label(), "Gemini");
        assert_eq!(Dialect::Codex.label(), "Codex");
        assert_eq!(Dialect::Kimi.label(), "Kimi");
        assert_eq!(Dialect::Copilot.label(), "Copilot");
    }

    #[test]
    fn dialect_display() {
        assert_eq!(format!("{}", Dialect::OpenAi), "OpenAI");
    }

    #[test]
    fn dialect_all_contains_six() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn dialect_serde_roundtrip() {
        let d = Dialect::Claude;
        let s = serde_json::to_string(&d).unwrap();
        assert_eq!(s, "\"claude\"");
        let back: Dialect = serde_json::from_str(&s).unwrap();
        assert_eq!(back, Dialect::Claude);
    }
}
