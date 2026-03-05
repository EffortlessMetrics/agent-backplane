// SPDX-License-Identifier: MIT OR Apache-2.0
//! Request fingerprinting for dialect identification.
//!
//! Provides [`DialectFingerprint`](crate::detect::DialectFingerprint) definitions and free functions
//! ([`detect_dialect`](crate::detect::detect_dialect), [`detect_from_headers`](crate::detect::detect_from_headers)) that examine raw JSON
//! requests and HTTP headers to identify which SDK dialect produced them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Dialect;

// ── Types ───────────────────────────────────────────────────────────────

/// Heuristic markers for identifying an SDK dialect from a raw request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialectFingerprint {
    /// Model-name prefixes that indicate this dialect (e.g. `"gpt-"` → OpenAI).
    pub model_prefix_patterns: Vec<String>,
    /// Top-level JSON field names characteristic of this dialect
    /// (e.g. `"messages"` → OpenAI, `"contents"` → Gemini).
    pub field_markers: Vec<String>,
    /// HTTP header key/value pairs that signal this dialect
    /// (e.g. `("anthropic-version", "")` → Claude, where an empty value
    /// means "any value").
    pub header_markers: Vec<(String, String)>,
}

/// Result of dialect detection with confidence scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialectDetectionResult {
    /// The detected dialect.
    pub dialect: Dialect,
    /// Confidence score in `[0.0, 1.0]`.
    pub confidence: f64,
    /// Human-readable evidence strings explaining the match.
    pub evidence: Vec<String>,
}

// ── Built-in fingerprints ───────────────────────────────────────────────

/// Returns the built-in [`DialectFingerprint`] table keyed by [`Dialect`].
#[must_use]
pub fn builtin_fingerprints() -> BTreeMap<Dialect, DialectFingerprint> {
    let mut m = BTreeMap::new();

    m.insert(
        Dialect::OpenAi,
        DialectFingerprint {
            model_prefix_patterns: vec![
                "gpt-".into(),
                "o1-".into(),
                "o3-".into(),
                "o4-".into(),
                "chatgpt-".into(),
            ],
            field_markers: vec![
                "messages".into(),
                "choices".into(),
                "frequency_penalty".into(),
                "presence_penalty".into(),
            ],
            header_markers: vec![
                ("authorization".into(), "Bearer ".into()),
                ("openai-organization".into(), String::new()),
                ("openai-project".into(), String::new()),
            ],
        },
    );

    m.insert(
        Dialect::Claude,
        DialectFingerprint {
            model_prefix_patterns: vec!["claude-".into()],
            field_markers: vec!["stop_reason".into(), "system".into()],
            header_markers: vec![
                ("anthropic-version".into(), String::new()),
                ("x-api-key".into(), String::new()),
            ],
        },
    );

    m.insert(
        Dialect::Gemini,
        DialectFingerprint {
            model_prefix_patterns: vec!["gemini-".into(), "models/gemini-".into()],
            field_markers: vec![
                "contents".into(),
                "candidates".into(),
                "generationConfig".into(),
                "safetySettings".into(),
                "systemInstruction".into(),
            ],
            header_markers: vec![("x-goog-api-key".into(), String::new())],
        },
    );

    m.insert(
        Dialect::Codex,
        DialectFingerprint {
            model_prefix_patterns: vec!["codex-".into()],
            field_markers: vec!["items".into(), "instructions".into()],
            header_markers: vec![],
        },
    );

    m.insert(
        Dialect::Kimi,
        DialectFingerprint {
            model_prefix_patterns: vec!["kimi".into(), "moonshot-".into()],
            field_markers: vec!["refs".into(), "search_plus".into()],
            header_markers: vec![],
        },
    );

    m.insert(
        Dialect::Copilot,
        DialectFingerprint {
            model_prefix_patterns: vec!["copilot-".into()],
            field_markers: vec![
                "references".into(),
                "confirmations".into(),
                "agent_mode".into(),
            ],
            header_markers: vec![
                ("x-github-token".into(), String::new()),
                ("copilot-integration-id".into(), String::new()),
            ],
        },
    );

    m
}

// ── Detection functions ─────────────────────────────────────────────────

/// Examines a raw JSON request and returns the most likely [`Dialect`].
///
/// Combines fingerprint-based scoring (model prefixes, field markers) with
/// deeper structural analysis of array contents and nested fields for
/// higher-accuracy detection.
///
/// Returns `None` when the input is not a JSON object or no fingerprint
/// matched with positive confidence.
#[must_use]
pub fn detect_dialect(request_json: &Value) -> Option<DialectDetectionResult> {
    let obj = request_json.as_object()?;
    let fingerprints = builtin_fingerprints();

    let mut best: Option<DialectDetectionResult> = None;

    for (&dialect, fp) in &fingerprints {
        let (fp_score, mut evidence) = score_fingerprint(obj, fp);
        let (struct_score, struct_ev) = score_structure(dialect, obj);
        evidence.extend(struct_ev);
        let score = (fp_score + struct_score).min(1.0);
        if score > 0.0 && best.as_ref().is_none_or(|b| score > b.confidence) {
            best = Some(DialectDetectionResult {
                dialect,
                confidence: score,
                evidence,
            });
        }
    }

    best
}

/// Examines HTTP headers and returns the most likely [`Dialect`].
///
/// Header keys in the map should be lowercase. Returns `None` when no
/// header fingerprint matched.
#[must_use]
pub fn detect_from_headers(headers: &BTreeMap<String, String>) -> Option<DialectDetectionResult> {
    let fingerprints = builtin_fingerprints();

    let mut best: Option<DialectDetectionResult> = None;

    for (&dialect, fp) in &fingerprints {
        let (score, evidence) = score_headers(headers, &fp.header_markers);
        if score > 0.0 && best.as_ref().is_none_or(|b| score > b.confidence) {
            best = Some(DialectDetectionResult {
                dialect,
                confidence: score.min(1.0),
                evidence,
            });
        }
    }

    best
}

// ── Scoring helpers ─────────────────────────────────────────────────────

/// Score a JSON object against a single fingerprint.
fn score_fingerprint(
    obj: &serde_json::Map<String, Value>,
    fp: &DialectFingerprint,
) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut evidence = Vec::new();

    // Model prefix check — strongest single signal.
    if let Some(model) = obj.get("model").and_then(Value::as_str) {
        let model_lower = model.to_lowercase();
        for prefix in &fp.model_prefix_patterns {
            if model_lower.starts_with(&prefix.to_lowercase()) {
                score += 0.45;
                evidence.push(format!("model \"{model}\" matches prefix \"{prefix}\""));
                break;
            }
        }
    }

    // Field marker check — each unique hit adds confidence.
    let mut field_hits = 0u32;
    for field in &fp.field_markers {
        if obj.contains_key(field.as_str()) {
            field_hits += 1;
            evidence.push(format!("has field \"{field}\""));
        }
    }
    // Diminishing returns: first field is 0.25, each additional +0.10.
    if field_hits > 0 {
        score += 0.25 + (field_hits.saturating_sub(1) as f64) * 0.10;
    }

    (score, evidence)
}

/// Score HTTP headers against a set of header markers.
fn score_headers(
    headers: &BTreeMap<String, String>,
    markers: &[(String, String)],
) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut evidence = Vec::new();

    for (key, expected_prefix) in markers {
        let key_lower = key.to_lowercase();
        if let Some(val) = headers.get(&key_lower) {
            if expected_prefix.is_empty() || val.starts_with(expected_prefix.as_str()) {
                score += 0.40;
                evidence.push(format!("header \"{key_lower}\" present"));
            }
        }
    }

    (score, evidence)
}

// ── Structural scoring ──────────────────────────────────────────────────

/// Deep structural analysis that goes beyond key-presence checks.
///
/// Examines nested array contents and field formats to provide additional
/// confidence when the request structure matches a dialect's conventions.
fn score_structure(
    dialect: crate::Dialect,
    obj: &serde_json::Map<String, Value>,
) -> (f64, Vec<String>) {
    match dialect {
        crate::Dialect::OpenAi => score_openai_structure(obj),
        crate::Dialect::Claude => score_claude_structure(obj),
        crate::Dialect::Gemini => score_gemini_structure(obj),
        crate::Dialect::Codex => score_codex_structure(obj),
        crate::Dialect::Kimi => score_kimi_structure(obj),
        crate::Dialect::Copilot => score_copilot_structure(obj),
    }
}

/// OpenAI: messages with string role fields and valid role values.
fn score_openai_structure(obj: &serde_json::Map<String, Value>) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut ev = Vec::new();
    let valid_roles = [
        "system",
        "user",
        "assistant",
        "tool",
        "function",
        "developer",
    ];

    if let Some(Value::Array(msgs)) = obj.get("messages") {
        let roles_valid = msgs.iter().all(|m| {
            m.get("role")
                .and_then(Value::as_str)
                .is_some_and(|r| valid_roles.contains(&r))
        });
        if !msgs.is_empty() && roles_valid {
            score += 0.10;
            ev.push("messages have valid OpenAI roles".into());
        }
    }

    (score, ev)
}

/// Claude: content block format (array of `{type, ...}` objects) in messages.
fn score_claude_structure(obj: &serde_json::Map<String, Value>) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut ev = Vec::new();

    // Check model prefix without going through fingerprints
    if let Some(model) = obj.get("model").and_then(Value::as_str) {
        if model.to_lowercase().starts_with("claude-") {
            score += 0.15;
            ev.push("model starts with \"claude-\"".into());
        }
    }

    // Content block format: messages[].content is an array of typed objects
    if let Some(Value::Array(msgs)) = obj.get("messages") {
        let has_content_blocks = msgs.iter().any(|m| {
            m.get("content")
                .and_then(Value::as_array)
                .is_some_and(|blocks| {
                    blocks
                        .iter()
                        .any(|b| b.get("type").and_then(Value::as_str).is_some())
                })
        });
        if has_content_blocks {
            score += 0.15;
            ev.push("messages contain typed content blocks".into());
        }
    }

    (score, ev)
}

/// Gemini: `contents[].parts` verified as arrays with content.
fn score_gemini_structure(obj: &serde_json::Map<String, Value>) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut ev = Vec::new();

    if let Some(model) = obj.get("model").and_then(Value::as_str) {
        if model.to_lowercase().starts_with("gemini-")
            || model.to_lowercase().starts_with("models/gemini-")
        {
            score += 0.15;
            ev.push("model starts with \"gemini-\"".into());
        }
    }

    if let Some(Value::Array(contents)) = obj.get("contents") {
        let has_parts_arrays = contents.iter().any(|c| {
            c.get("parts")
                .and_then(Value::as_array)
                .is_some_and(|p| !p.is_empty())
        });
        if has_parts_arrays {
            score += 0.10;
            ev.push("contents entries have non-empty parts arrays".into());
        }
    }

    (score, ev)
}

/// Codex: `instructions` field or `items` with typed entries.
fn score_codex_structure(obj: &serde_json::Map<String, Value>) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut ev = Vec::new();

    if let Some(model) = obj.get("model").and_then(Value::as_str) {
        if model.to_lowercase().starts_with("codex-") {
            score += 0.15;
            ev.push("model starts with \"codex-\"".into());
        }
    }

    if obj.contains_key("instructions") {
        score += 0.10;
        ev.push("has \"instructions\" field".into());
    }

    (score, ev)
}

/// Kimi: `moonshot-` model prefix or search-related options.
fn score_kimi_structure(obj: &serde_json::Map<String, Value>) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut ev = Vec::new();

    if let Some(model) = obj.get("model").and_then(Value::as_str) {
        if model.to_lowercase().starts_with("moonshot-") {
            score += 0.15;
            ev.push("model starts with \"moonshot-\"".into());
        }
    }

    if obj.get("search_plus").is_some_and(Value::is_boolean) {
        score += 0.05;
        ev.push("\"search_plus\" is a boolean".into());
    }

    (score, ev)
}

/// Copilot: `references` array with typed items, or `agent_mode` flag.
fn score_copilot_structure(obj: &serde_json::Map<String, Value>) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut ev = Vec::new();

    if let Some(Value::Array(refs)) = obj.get("references") {
        let has_typed_refs = refs
            .iter()
            .any(|r| r.get("type").and_then(Value::as_str).is_some());
        if has_typed_refs {
            score += 0.10;
            ev.push("references contain typed entries".into());
        }
    }

    (score, ev)
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_fingerprints_cover_all_dialects() {
        let fps = builtin_fingerprints();
        for d in Dialect::all() {
            assert!(fps.contains_key(d), "missing fingerprint for {d:?}");
        }
    }

    #[test]
    fn detect_returns_none_for_non_object() {
        assert!(detect_dialect(&json!(42)).is_none());
        assert!(detect_dialect(&json!("hello")).is_none());
        assert!(detect_dialect(&json!(null)).is_none());
    }

    #[test]
    fn detect_returns_none_for_empty_object() {
        assert!(detect_dialect(&json!({})).is_none());
    }

    #[test]
    fn headers_returns_none_for_empty() {
        assert!(detect_from_headers(&BTreeMap::new()).is_none());
    }

    // ── Structural scoring tests ────────────────────────────────────

    #[test]
    fn openai_structural_boost_from_valid_roles() {
        let plain = json!({"messages": [{"role": "user", "content": "hi"}]});
        let r = detect_dialect(&plain).expect("should detect");
        assert_eq!(r.dialect, Dialect::OpenAi);
        assert!(r.evidence.iter().any(|e| e.contains("roles")));
    }

    #[test]
    fn claude_structural_boost_from_content_blocks() {
        let v = json!({
            "model": "claude-3-opus",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });
        let r = detect_dialect(&v).expect("should detect");
        assert_eq!(r.dialect, Dialect::Claude);
        assert!(r.evidence.iter().any(|e| e.contains("content blocks")));
    }

    #[test]
    fn gemini_structural_boost_from_parts_arrays() {
        let v = json!({"model": "gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
        let r = detect_dialect(&v).expect("should detect");
        assert_eq!(r.dialect, Dialect::Gemini);
        assert!(r.evidence.iter().any(|e| e.contains("parts arrays")));
    }

    #[test]
    fn codex_structural_boost_from_instructions() {
        let v = json!({"model": "codex-mini", "instructions": "fix the bug", "items": [{"type": "message"}]});
        let r = detect_dialect(&v).expect("should detect");
        assert_eq!(r.dialect, Dialect::Codex);
        assert!(r.evidence.iter().any(|e| e.contains("instructions")));
    }

    #[test]
    fn kimi_structural_boost_from_moonshot_prefix() {
        let v = json!({"model": "moonshot-v1-32k", "refs": ["doc"]});
        let r = detect_dialect(&v).expect("should detect");
        assert_eq!(r.dialect, Dialect::Kimi);
        assert!(r.evidence.iter().any(|e| e.contains("moonshot-")));
    }

    #[test]
    fn copilot_structural_boost_from_typed_references() {
        let v = json!({"references": [{"type": "file", "path": "src/main.rs"}]});
        let r = detect_dialect(&v).expect("should detect");
        assert_eq!(r.dialect, Dialect::Copilot);
        assert!(r.evidence.iter().any(|e| e.contains("typed entries")));
    }
}
